//! The logged-in app: guild rail, channel sidebar, chat and member list, drawn from a
//! [`Store`] and a small [`ShellState`] (`fluxer_app/src/features/app/components/layout/
//! {GuildsLayout,OutlineFrame}.module.css`, `channel/components/ChannelIndexPage.module.css`).
//!
//! Views are pure; what needs the network goes out as an [`Intent`] queued on
//! [`ShellState::intents`] for the runtime to drain.

mod chat;
pub mod demo;
mod guilds;
mod markdown;
mod members;
mod sidebar;

use std::collections::HashSet;

use reflux_model::{ChannelType, Snowflake, Status as PresenceStatus};
use reflux_store::Store;
use rune::{Color, El, Icon, Image, ImageCache, TextEdit, Waker, col, el, row};

use crate::theme::{Palette, dim};
use crate::ui::{Avatar, Status, over, rgb};

/// What the shell asks the runtime to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    /// `None` = home (DMs). The shell already picked the channel it shows.
    SelectGuild(Option<Snowflake>),
    /// Fetch its newest messages if none are loaded; ack it.
    SelectChannel(Snowflake),
    SendMessage { channel: Snowflake, content: String },
    /// The list reached its oldest loaded message: fetch the page before it.
    LoadOlder { channel: Snowflake },
}

/// UI-only state of the shell.
pub struct ShellState {
    /// `None` = home (DMs).
    pub guild: Option<Snowflake>,
    pub channel: Option<Snowflake>,
    pub composer: TextEdit,
    pub collapsed: HashSet<Snowflake>,
    pub member_list: bool,
    /// The guild rail item under the pointer (`Some(None)` = the home button).
    pub hover_guild: Option<Option<Snowflake>>,
    /// Unix ms the views format timestamps against ("Today at …"); the runtime refreshes it.
    pub now_ms: u64,
    /// `assets/media/images/avatars/{0..5}.png`, picked by user ID % 6.
    pub default_avatars: Vec<Image>,
    pub intents: Vec<Intent>,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            guild: None,
            channel: None,
            composer: TextEdit::new(""),
            collapsed: HashSet::new(),
            member_list: true,
            hover_guild: None,
            now_ms: 0,
            default_avatars: Vec::new(),
            intents: Vec::new(),
        }
    }
}

pub type ShellAccess<S> = fn(&mut S) -> &mut ShellState;

impl ShellState {
    pub fn select_guild(&mut self, guild: Option<Snowflake>, channel: Option<Snowflake>) {
        self.guild = guild;
        self.intents.push(Intent::SelectGuild(guild));
        if let Some(c) = channel {
            self.select_channel(c);
        }
    }

    pub fn select_channel(&mut self, channel: Snowflake) {
        if self.channel != Some(channel) {
            self.composer = TextEdit::new("");
        }
        self.channel = Some(channel);
        self.intents.push(Intent::SelectChannel(channel));
    }
}

/// Decodes the six default avatars (they are tiny; waits for them).
pub fn load_default_avatars(waker: Waker) -> Vec<Image> {
    const PNGS: [&[u8]; 6] = [
        include_bytes!("../../../../assets/media/images/avatars/0.png"),
        include_bytes!("../../../../assets/media/images/avatars/1.png"),
        include_bytes!("../../../../assets/media/images/avatars/2.png"),
        include_bytes!("../../../../assets/media/images/avatars/3.png"),
        include_bytes!("../../../../assets/media/images/avatars/4.png"),
        include_bytes!("../../../../assets/media/images/avatars/5.png"),
    ];
    let cache = ImageCache::new(waker);
    PNGS.iter()
        .map(|b| {
            let img = cache.load(*b);
            let _ = img.wait();
            img
        })
        .collect()
}

/// The first channel of a guild the sidebar would show (Fluxer opens it when no channel was
/// remembered).
pub fn first_channel(store: &Store, guild: Snowflake) -> Option<Snowflake> {
    store.channel_groups(guild).iter().flat_map(|g| g.text.iter()).find(|c| c.kind != ChannelType::GUILD_LINK).map(|c| c.id)
}

/// The whole logged-in screen.
pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let divider = over(p.user_area_divider_color, p.background_secondary);
    let channel = st.channel.and_then(|c| store.channels.get(&c));
    let side = match st.guild {
        Some(g) => sidebar::guild(store, st, g, p, acc),
        None => sidebar::home(store, st, p, acc),
    };
    // `OutlineFrame`: 1px divider-coloured border (no top on the web/system-titlebar build), the
    // sidebar, and a 1px divider over the main column's first pixel.
    let frame = col().grow(1.0).min_w(0.0).h_full().pl(1.0).pr(1.0).pb(1.0).fill(divider).child(
        row()
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .fill(p.background_secondary_lighter)
            .child(side)
            .child(chat::main(store, st, channel, p, acc))
            .child(el().pin(Some(dim::LAYOUT_SIDEBAR_WIDTH), Some(0.0), None, Some(0.0)).w(1.0).fill(divider)),
    );
    row()
        .w_full()
        .h_full()
        .fill(p.background_secondary)
        .child(guilds::rail(store, st, p, acc))
        .child(frame)
        .child(sidebar::user_area(store, st, p))
}

// ---- Shared bits -------------------------------------------------------------------------------

/// Height the user area covers at the bottom of the rail and sidebar (`--layout-user-area-height`
/// + `--user-area-box-inset-block-end`).
const USER_AREA_OVERLAY: f32 = dim::LAYOUT_USER_AREA_HEIGHT + dim::FOOTER_BOX_INSET + 1.0;

/// A role colour (`0xRRGGBB`).
fn role_color(c: u32) -> Color {
    rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn presence(store: &Store, user: Snowflake) -> Status {
    let s = store.presences.get(&user).map(|p| p.status);
    match s {
        Some(PresenceStatus::Online) => Status::Online,
        Some(PresenceStatus::Idle) => Status::Idle,
        Some(PresenceStatus::Dnd) => Status::Dnd,
        _ => Status::Offline,
    }
}

/// An avatar: the default image for now (custom avatars need the media proxy).
// shortcut: custom avatar hashes are ignored; load them through the runtime's image cache.
fn avatar(st: &ShellState, user: Snowflake, size: f32, status: Option<Status>, surface: Color, p: &Palette) -> El {
    let px = st.default_avatars.get((user.0 % 6) as usize).map(|i| i.pixels());
    Avatar::new(size).pixels(px).status(status).surface(surface).el(p)
}

macro_rules! fluxer_icon {
    ($name:ident, $id:literal, $body:literal) => {
        static $name: Icon = Icon { name: $id, view: 256.0, body: $body };
    };
}

// Fluxer's own channel glyphs (`features/ui/components/icons/*ChannelIcon.tsx`).
fluxer_icon!(TEXT_CHANNEL, "fluxer-text-channel", r#"<path fill="currentColor" fill-rule="evenodd" d="M164.193 37.8536c1.186-6.5205 7.433-10.8456 13.953-9.6602 6.521 1.1856 10.846 7.4327 9.661 13.9531L180.197 84H224c6.627 0 12 5.3726 12 12 0 6.627-5.373 12-12 12h-48.167l-7.272 40H208c6.627 0 12 5.373 12 12s-5.373 12-12 12h-43.803l-8.39 46.147c-1.186 6.52-7.433 10.845-13.953 9.66-6.521-1.186-10.846-7.433-9.661-13.953l7.610-41.854h-39.606l-8.3904 46.147c-1.1856 6.52-7.4327 10.845-13.9531 9.66-6.5204-1.186-10.8455-7.433-9.6601-13.953L75.8027 172H32c-6.6274 0-11.9999-5.373-12-12 0-6.627 5.3726-12 12-12h48.167l7.2725-40H48c-6.6274 0-11.9999-5.373-12-12 0-6.6274 5.3726-12 12-12h43.8027l8.3903-46.1464c1.186-6.5205 7.433-10.8456 13.953-9.6602 6.521 1.1856 10.846 7.4327 9.661 13.9531L116.197 84h39.606zM104.561 148h39.606l7.272-40h-39.606z"/>"#);
fluxer_icon!(VOICE_CHANNEL, "fluxer-voice-channel", r#"<path fill="currentColor" fill-rule="evenodd" d="M116.693 32.0001C126.763 21.9201 144 29.056 144 43.3067V212.693c0 14.251-17.237 21.387-27.317 11.307l-48.0003-48H48.0958c-12.1705 0-24.725-7.094-28.3623-20.32-2.4426-8.821-3.7334-18.101-3.7334-27.68-.0115-9.351 1.2371-18.662 3.7119-27.68 3.648-13.237 16.2033-20.3199 28.374-20.3199h20.6074zm87.19 20.124c2.12 0 4.153.8417 5.653 2.3398 40.619 40.608 40.619 106.4541 0 147.0721-1.516 1.413-3.522 2.182-5.595 2.146-2.072-.037-4.049-.876-5.515-2.342s-2.305-3.443-2.342-5.516c-.036-2.072.733-4.078 2.146-5.594 8.172-8.172 14.656-17.874 19.079-28.551 4.423-10.678 6.699-22.122 6.699-33.679s-2.276-23.001-6.699-33.6786c-4.423-10.6774-10.907-20.3791-19.079-28.5508-1.499-1.5-2.34-3.5333-2.34-5.6533 0-2.1201.841-4.1534 2.34-5.6534 1.5-1.4981 3.533-2.3398 5.653-2.3398m-28.282 28.2724c1.051.0001 2.091.2071 3.062.6094.971.4024 1.853.992 2.596 1.7354 5.944 5.9431 10.659 12.9991 13.876 20.7647 3.217 7.765 4.873 16.089 4.873 24.494 0 8.406-1.656 16.729-4.873 24.494-3.217 7.766-7.932 14.822-13.876 20.765-1.51 1.456-3.531 2.261-5.628 2.242-2.098-.019-4.104-.861-5.587-2.345s-2.323-3.491-2.34-5.589c-.017-2.097.79-4.117 2.248-5.626 4.457-4.457 7.993-9.748 10.405-15.572 2.413-5.823 3.655-12.065 3.655-18.369 0-6.303-1.242-12.545-3.655-18.369-2.412-5.824-5.948-11.1151-10.405-15.5723-1.498-1.5-2.34-3.5334-2.34-5.6534 0-2.1199.842-4.1533 2.34-5.6533l-.011-.0107c.743-.7433 1.625-1.333 2.596-1.7354.971-.4023 2.013-.6094 3.064-.6094"/>"#);
fluxer_icon!(ANNOUNCEMENT_CHANNEL, "fluxer-announcement-channel", r#"<path fill="currentColor" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="24" d="M36 96H96L212 40V216L96 160H36ZM68 160L84 212"/>"#);
fluxer_icon!(FORUM_CHANNEL, "fluxer-forum-channel", r#"<path fill="currentColor" d="M169.57,72.59A80,80,0,0,0,16,104v64a16,16,0,0,0,16,16H86.67A80.15,80.15,0,0,0,160,232h64a16,16,0,0,0,16-16V152A80,80,0,0,0,169.57,72.59ZM224,216H160a64.14,64.14,0,0,1-55.68-32.43A79.93,79.93,0,0,0,174.7,89.71,64,64,0,0,1,224,152Z"/>"#);
fluxer_icon!(LINK_CHANNEL, "fluxer-link-channel", r#"<path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="24" d="m108.71 197.23-5.11 5.11c-4.3345 4.335-9.481 7.773-15.1453 10.117S76.7195 216.005 70.5894 216s-12.1991-1.218-17.8599-3.57c-5.6607-2.353-10.802-5.799-15.1297-10.14-8.7201-8.751-13.6115-20.604-13.6003-32.958.0113-12.353 4.9242-24.198 13.6603-32.932l34.74-34.74c4.3292-4.3306 9.4692-7.7658 15.1262-10.1095 5.6571-2.3437 11.7204-3.55 17.844-3.55 6.123 0 12.186 1.2063 17.844 3.55 5.657 2.3437 10.797 5.7789 15.126 10.1095 6.16 6.132 10.47 13.874 12.44 22.34m-3.49-65.2301 5.11-5.11c4.329-4.3305 9.469-7.7657 15.126-10.1094s11.72-3.55 17.844-3.55c6.123 0 12.186 1.2063 17.844 3.55 5.657 2.3437 10.797 5.7789 15.126 10.1094 4.33 4.3292 7.765 9.4691 10.109 15.1262s3.55 11.7204 3.55 17.8437c0 6.1234-1.206 12.1868-3.55 17.8442-2.344 5.657-5.779 10.797-10.109 15.126l-34.74 34.74c-4.335 4.335-9.481 7.773-15.145 10.117-5.665 2.344-11.735 3.548-17.866 3.543-6.13-.005-12.199-1.218-17.859-3.57-5.661-2.353-10.802-5.799-15.13-10.14-6.125-6.13-10.413-13.851-12.38-22.29"/>"#);

/// `getIcon` in `channel/utils/ChannelUtils.tsx` (the unlocked, non-mature variants).
fn channel_icon(kind: ChannelType) -> &'static Icon {
    match kind {
        ChannelType::GUILD_VOICE => &VOICE_CHANNEL,
        ChannelType::GUILD_ANNOUNCEMENT => &ANNOUNCEMENT_CHANNEL,
        ChannelType::GUILD_FORUM => &FORUM_CHANNEL,
        ChannelType::GUILD_LINK => &LINK_CHANNEL,
        ChannelType::GUILD_MEDIA => &crate::icons::IMAGES_SQUARE,
        ChannelType::DM_PERSONAL_NOTES => &crate::icons::NOTE_PENCIL_BOLD,
        t if t.is_thread() => &crate::icons::CHATS,
        _ => &TEXT_CHANNEL,
    }
}
