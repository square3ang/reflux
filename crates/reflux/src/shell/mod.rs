//! The logged-in app: guild rail, channel sidebar, chat and member list, drawn from a
//! [`Store`] and a small [`ShellState`] (`fluxer_app/src/features/app/components/layout/
//! {GuildsLayout,OutlineFrame}.module.css`, `channel/components/ChannelIndexPage.module.css`).
//!
//! Views are pure; what needs the network goes out as an [`Intent`] queued on
//! [`ShellState::intents`] for the runtime to drain.

pub mod active_now;
pub mod attachments;
mod birthday;
mod autocomplete;
mod chat;
pub mod debug;
mod compact;
pub(crate) mod embeds;
pub(crate) mod highlight;
pub mod invite_embed;
pub mod lightbox;
mod links;
pub mod demo;
pub(crate) mod emoji;
pub(crate) mod role_icon;
pub mod expr;
pub(crate) mod friends;
mod guilds;
mod markdown;
mod members;
pub mod members_page;
mod menus;
pub mod pins;
pub(crate) mod profile;
pub mod search;
mod search_query;
mod sidebar;
pub mod status;
pub mod switcher;
pub mod forum;
pub mod threads;
pub mod upload;
pub mod voice;
pub mod call;
pub mod clip;
pub mod gifv;
pub mod voice_message;
pub mod system_msg;
pub mod queue;

pub use friends::FriendsTab;
pub use members::ranges as member_ranges;
pub(crate) use chat::{channel_title, format_relative};
pub(crate) use guilds::{acronym, folder_glyph, guild_face};

use std::collections::HashSet;

use reflux_model::{Channel, ChannelType, ReactionEmoji, Snowflake, Status as PresenceStatus};
use reflux_store::Store;
use rune::{Color, El, Icon, Image, ImageCache, TextEdit, Waker, col, el, row};

use crate::media::{self, Load};
use crate::theme::{Palette, dim};
use crate::ui::{Avatar, Placement, PopupAccess, Popups, Status, over, popup_access, rgb};
use crate::i18n::t;

/// What the shell asks the runtime to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    /// `None` = home (DMs). The shell already picked the channel it shows.
    SelectGuild(Option<Snowflake>),
    /// Fetch its newest messages if none are loaded; ack it.
    SelectChannel(Snowflake),
    /// `reply`: the message replied to and whether to ping its author.
    SendMessage { channel: Snowflake, content: String, reply: Option<(Snowflake, bool)>, attachments: Vec<upload::Upload> },
    /// Send a failed message again / drop it (`MessageRetryUtils`, `deleteLocal`).
    RetrySend { channel: Snowflake, nonce: Snowflake },
    DropLocal { channel: Snowflake, nonce: Snowflake },
    /// The list reached its oldest loaded message: fetch the page before it.
    LoadOlder { channel: Snowflake },
    /// The list reached its newest loaded message while in history: fetch the page after it.
    LoadNewer { channel: Snowflake },
    /// Show a message: scroll to it if loaded, else load the page around it first.
    JumpTo { channel: Snowflake, message: Snowflake },
    /// Back to the newest messages (reloading them if the window is in history).
    JumpToPresent(Snowflake),
    /// Fetch the channel's pins (`more`: the next page).
    LoadPins { channel: Snowflake, more: bool },
    /// `POST /search/messages` with this body; results land in `search.results`.
    Search(serde_json::Value),
    /// The user typed in the composer (already throttled by the shell).
    Typing(Snowflake),
    /// Move the read marker back so `message` and everything after it is unread.
    MarkUnread { channel: Snowflake, message: Snowflake },
    React { channel: Snowflake, message: Snowflake, emoji: ReactionEmoji, add: bool },
    Edit { channel: Snowflake, message: Snowflake, content: String },
    Delete { channel: Snowflake, message: Snowflake },
    Pin { channel: Snowflake, message: Snowflake, pin: bool },
    /// A partial user settings object: apply it now and `PATCH /users/@me/settings`.
    PatchSettings(serde_json::Value),
    Logout,
    /// Op 4 into the channel, then connect LiveKit on VOICE_SERVER_UPDATE.
    JoinVoice { guild: Option<Snowflake>, channel: Snowflake },
    LeaveVoice,
    /// `voice.self_mute` / `self_deaf` changed: tell the gateway and the call.
    VoiceSelf,
    ToggleScreen,
    /// Show this user's screen share (`None` stops).
    Watch(Option<Snowflake>),
    /// List microphones and speakers into `voice.devices`.
    LoadDevices,
    /// `voice.input` / `voice.output` changed.
    VoiceDevices,
    /// Camera, calls, settings, mic test, voice messages (`voice::Op`).
    Voice(voice::Op),
    /// Open (creating it if needed) the DM with a user and show it.
    OpenDm(Snowflake),
    /// `startCallWithUser`: open the DM with a user and start (or confirm) a voice call there.
    DmCall(Snowflake),
    /// `InviteToCommunityMenuItem`: send a user an invite link to `guild` (via `channel`, or
    /// the vanity link) in their DM.
    InviteToGuild { user: Snowflake, guild: Snowflake, channel: Snowflake },
    /// Close a DM or leave a group DM.
    CloseDm(Snowflake),
    /// Send a friend request to, or accept one from, a user.
    AddFriend(Snowflake),
    /// Friend request by `username` or `username#0000`.
    FriendRequest(String),
    /// Remove a friend, cancel or ignore a request, or unblock.
    RemoveRelationship(Snowflake),
    Block(Snowflake),
    SetNote { user: Snowflake, note: String },
    LeaveGuild(Snowflake),
    /// (channel, its last message) pairs to ack.
    MarkRead(Vec<(Snowflake, Snowflake)>),
    /// `POST /auth/verify/resend` from the email verification nagbar.
    ResendVerification,
    /// `PUT /users/@me/date-of-birth` from the birthday gate.
    SetDateOfBirth(String),
    /// A community management request (`crate::guild`).
    Api(crate::guild::Call),
    /// A partial user guild settings object: apply it now and
    /// `PATCH /users/@me/guilds/{guild}/settings` (`None` = DMs).
    PatchGuildSettings { guild: Option<Snowflake>, patch: serde_json::Value },
    /// Save changed local desktop preferences.
    Prefs(Box<crate::desktop::Prefs>),
    /// Switch the interface language (a `crate::i18n::LOCALES` code).
    SetLocale(&'static str),
    /// `GET /invites/{code}` into `media_ui.invites`.
    FetchInvite(String),
    /// Accounts, profiles and sudo (`crate::settings::Op`).
    Account(crate::settings::Op),
    /// A thread or forum request (`threads::Op`).
    Thread(threads::Op),
    /// A REST call whose answer lands in `expr.fetched[key]` (an empty key drops it).
    Fetch { key: String, method: reqwest::Method, path: String, body: Option<serde_json::Value> },
    /// A message carrying a sticker or a saved meme instead of text.
    SendExtra { channel: Snowflake, sticker: Option<Snowflake>, meme: Option<Snowflake> },
    Desktop(crate::desktop::Request),
    /// A people / moderation request (`crate::social`).
    Social(crate::social::Req),
}

/// `EmailVerificationNagbar`: neutral tone (#4e5058 / white), 36 tall, 14px/600, centred message
/// and a super-compact inverted button, gap 12.
fn verify_nagbar<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let me = store.me.as_ref().filter(|u| !u.verified && u.email.is_some())?;
    let name = me.base.global_name.clone().unwrap_or_else(|| me.base.username.clone());
    let (message, button) = if me.email_bounced {
        (crate::i18n::tf("Hey {displayName}, we can't deliver email to your address. Add a working email to verify your account.", &[("displayName", &name.to_string())]), None)
    } else {
        let label = if st.verify_sent { "Email sent" } else { t("Resend email") };
        let b = crate::ui::Button::new(label)
            .variant(crate::ui::ButtonVariant::Inverted)
            .size(crate::ui::ButtonSize::SuperCompact)
            .disabled(st.verify_sent)
            .key("nagbar-verify-resend")
            .on_press(move |s: &mut S| {
                let sh = acc(s);
                sh.verify_sent = true;
                sh.intents.push(Intent::ResendVerification);
            })
            .el(p);
        (crate::i18n::tf("Hey {displayName}, please confirm your email address.", &[("displayName", &name.to_string())]), Some(b))
    };
    Some(
        row()
            .w_full()
            .min_h(36.0)
            .py(4.0)
            .px(8.0)
            .gap(12.0)
            .center()
            .fill(crate::ui::rgb(0x4e, 0x50, 0x58))
            .child(rune::text(message).font(crate::theme::font(14.0, 600.0).lh(20.0)).color(rune::Color::WHITE).wrap().align(rune::Align::Center))
            .children(button)
            .role("status"),
    )
}

/// What the open context menu or profile popout is about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// `guild`: the member's guild (roles, nick); `dm`: the DM row the menu came from.
    User { user: Snowflake, guild: Option<Snowflake>, dm: Option<Snowflake> },
    Guild(Snowflake),
    Channel(Snowflake),
}

/// The composer's "Replying to …" target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplyTo {
    pub channel: Snowflake,
    pub message: Snowflake,
    /// Ping the author (the bar's @ ON/OFF toggle).
    pub mention: bool,
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
    /// Avatars, icons, attachments and emoji from the media proxy.
    pub media: crate::media::Media,
    pub intents: Vec<Intent>,
    /// The verification nagbar's "Resend email" was used this session.
    pub verify_sent: bool,
    pub birthday: birthday::Gate,
    /// Context menu, "more" menu and emoji picker (one at a time), and tooltips.
    pub popups: Popups,
    /// The message under the pointer (shows its action bar).
    pub hover_msg: Option<Snowflake>,
    /// Shift is down (the action bar's Shift to expand).
    pub shift_held: bool,
    /// The message the open menu or picker belongs to (keeps its action bar up).
    pub active_msg: Option<Snowflake>,
    pub reply: Option<ReplyTo>,
    /// The message being edited inline and its draft.
    pub editing: Option<(Snowflake, TextEdit)>,
    /// The message waiting on the delete confirmation.
    pub confirm_delete: Option<Snowflake>,
    /// The expression pickers, who-reacted and the expression info card.
    pub expr: expr::Expr,
    /// The instance's GIF provider name (`Klipy`); empty when GIFs are off.
    pub gif_provider: String,
    /// When the last typing notice went out, and for which channel (`TypingSendThrottle`).
    pub typing_sent: Option<(Snowflake, u64)>,
    /// The message list shows its newest row (read states may be acked).
    pub at_bottom: bool,
    /// Where the red NEW line stays after the channel was acked while open (Fluxer's sticky
    /// unread marker), and channels marked unread by hand, which aren't acked again until
    /// they're reopened.
    pub sticky_unread: Option<(Snowflake, Snowflake)>,
    pub hold_ack: Option<Snowflake>,
    /// Instance web app origin, for message links (`https://web.fluxer.app`).
    pub web_base: String,
    /// The user settings modal, when open.
    pub settings: Option<crate::settings::SettingsUi>,
    /// The instance shows Billing tabs (`shouldShowPremiumFeatures`).
    pub premium: bool,
    /// The instance has personal notes (the DM list's Notes row).
    pub personal_notes: bool,
    pub friends_tab: FriendsTab,
    pub friends_query: TextEdit,
    pub add_friend: TextEdit,
    /// The last Add Friend result: `Ok` = sent, `Err` = what went wrong.
    pub add_friend_result: Option<Result<(), String>>,
    /// The context menu's (`ctx-menu`) or profile popout's (`profile`) subject.
    pub target: Option<Target>,
    /// The element the profile popout hangs off, and on which side.
    pub profile_anchor: Option<(String, Placement)>,
    /// The profile popout's note field.
    pub note: TextEdit,
    /// The guild waiting on the leave confirmation.
    pub confirm_leave: Option<Snowflake>,
    /// Member list rows on screen (drives the lazy list subscription).
    pub member_rows: std::ops::Range<usize>,
    pub voice: voice::Voice,
    /// The open community management dialog and its form.
    pub dialog: Option<crate::guild::Dialog>,
    pub form: crate::guild::Form,
    /// Invite link origin (`discovery.endpoints.invite`, e.g. `https://fluxer.gg`).
    pub invite_base: String,
    /// Gift links (`endpoints.gift`).
    pub gift_base: String,
    /// The local desktop preferences, mirrored for the settings views.
    pub prefs: crate::desktop::Prefs,
    /// Nagbars, toasts and the spelling menu (`crate::desktop::nagbar`).
    pub desktop: crate::desktop::ShellView,
    /// Files waiting in the composer's tray, and the last tray ID given out.
    pub uploads: Vec<upload::Upload>,
    /// `ChannelSticker.pendingSticker`: (channel, sticker) staged to go with the next message.
    pub pending_sticker: Option<(Snowflake, Snowflake)>,
    pub upload_seq: u64,
    /// The tray file whose alt text is being edited.
    pub upload_edit: Option<(u64, TextEdit)>,
    /// Upload progress (0..1) of optimistic messages, by their nonce ID.
    pub progress: std::collections::HashMap<Snowflake, f32>,
    /// Text sends that failed, by their nonce ID (drawn red with Retry / Delete).
    pub failed: std::collections::HashMap<Snowflake, queue::Failed>,
    /// An error dialog: (title, body).
    pub notice: Option<(String, String)>,
    /// The account's resolved instance limits.
    pub limits: upload::Limits,
    /// A file drag is over the chat.
    pub drop_hover: bool,
    /// Loaded pins by channel, and the pin waiting on the unpin confirmation.
    pub pins: std::collections::HashMap<Snowflake, pins::Pins>,
    pub confirm_unpin: Option<(Snowflake, Snowflake)>,
    pub search: search::Search,
    /// A handler asked for a search (run where the store is at hand).
    pub search_requested: bool,
    /// The message jumped to, drawn highlighted until another jump or channel.
    pub highlight: Option<Snowflake>,
    /// A key to scroll into view on the next frame (`msg-{id}`, or `msgs-end`).
    pub scroll_to: Option<String>,
    /// The last scroll request, sent once more on the next frame: a list that mounts in the
    /// request's frame isn't laid out when `poll` runs, and Rune drops a reveal it can't place.
    pub scroll_again: Option<String>,
    /// The whole message list showed on the last frame (it's bottom-aligned only then).
    pub list_fits: bool,
    /// Rows between the last one on screen and the end of the list.
    pub rows_below: usize,
    pub switcher: Option<switcher::Switcher>,
    /// The highlighted autocomplete row, and the trigger word dismissed with Escape.
    pub ac_index: usize,
    pub ac_dismissed: Option<(usize, String)>,
    /// When slowmode lets the user send again, by channel (unix ms).
    pub cooldowns: std::collections::HashMap<Snowflake, u64>,
    /// Lightbox, spoilers, inline players, invite previews, link warning.
    pub media_ui: attachments::MediaUi,
    /// Sudo prompt, fetched profiles, account modals and the account switcher.
    pub account: crate::settings::AccountUi,
    /// Thread panel, browser and forum state.
    pub threads: threads::Threads,
    pub social: crate::social::Social,
    /// Rail folders and drag and drop, favorites, keybinds and other navigation extras.
    pub nav: crate::nav::Nav,
    /// Developer-mode debug modal.
    pub debug: Option<debug::Debug>,
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
            shift_held: false,
            default_avatars: Vec::new(),
            media: Default::default(),
            intents: Vec::new(),
            verify_sent: false,
            birthday: Default::default(),
            popups: Popups::default(),
            hover_msg: None,
            active_msg: None,
            reply: None,
            editing: None,
            confirm_delete: None,
            expr: expr::Expr::default(),
            gif_provider: String::new(),
            typing_sent: None,
            at_bottom: true,
            sticky_unread: None,
            hold_ack: None,
            web_base: String::new(),
            settings: None,
            premium: false,
            personal_notes: true,
            friends_tab: FriendsTab::default(),
            friends_query: TextEdit::new(""),
            add_friend: TextEdit::new(""),
            add_friend_result: None,
            target: None,
            profile_anchor: None,
            note: TextEdit::new(""),
            confirm_leave: None,
            member_rows: 0..0,
            voice: voice::Voice::default(),
            dialog: None,
            form: Default::default(),
            invite_base: String::new(),
            gift_base: String::new(),
            prefs: Default::default(),
            desktop: Default::default(),
            uploads: Vec::new(),
            pending_sticker: None,
            upload_seq: 0,
            upload_edit: None,
            progress: Default::default(),
            failed: Default::default(),
            notice: None,
            limits: Default::default(),
            drop_hover: false,
            pins: Default::default(),
            confirm_unpin: None,
            search: Default::default(),
            search_requested: false,
            highlight: None,
            scroll_to: None,
            scroll_again: None,
            list_fits: true,
            rows_below: 0,
            switcher: None,
            ac_index: 0,
            ac_dismissed: None,
            cooldowns: Default::default(),
            media_ui: Default::default(),
            account: Default::default(),
            threads: Default::default(),
            social: crate::social::Social::new(),
            nav: Default::default(),
            debug: None,
        }
    }
}

pub type ShellAccess<S> = fn(&mut S) -> &mut ShellState;

fn pops<S: 'static>(acc: ShellAccess<S>) -> impl PopupAccess<S> {
    popup_access(move |s: &mut S| &mut acc(s).popups)
}

impl ShellState {
    pub fn select_guild(&mut self, guild: Option<Snowflake>, channel: Option<Snowflake>) {
        self.nav.favorites = false;
        self.guild = guild;
        self.social.discovery = false;
        self.social.members = None;
        self.intents.push(Intent::SelectGuild(guild));
        match channel {
            Some(c) => self.select_channel(c),
            None => self.channel = None,
        }
    }

    /// Opens the context menu for `t` at a window point.
    pub fn open_menu(&mut self, t: Target, at: [f32; 2]) {
        self.popups.open_at("ctx-menu", at);
        self.target = Some(t);
    }

    /// [`Self::open_menu`] for a user; "View profile" then opens next to the element keyed `anchor`.
    pub fn open_user_menu(&mut self, t: Target, at: [f32; 2], anchor: String, placement: Placement) {
        self.open_menu(t, at);
        self.profile_anchor = Some((anchor, placement));
    }

    /// Opens a user's profile popout next to the element keyed `anchor`.
    pub fn open_profile(&mut self, user: Snowflake, guild: Option<Snowflake>, note: String, anchor: String, placement: Placement) {
        self.popups.close();
        self.popups.open = Some("profile".into());
        self.target = Some(Target::User { user, guild, dm: None });
        self.profile_anchor = Some((anchor, placement));
        self.note = TextEdit::new(note);
        self.fetch_profile(user, guild);
    }

    /// Loads a user's full profile once (`UserProfileCommands.fetch`).
    pub fn fetch_profile(&mut self, user: Snowflake, guild: Option<Snowflake>) {
        if !self.account.profiles.contains_key(&(user, guild)) {
            self.intents.push(Intent::Account(crate::settings::Op::FetchProfile { user, guild }));
        }
    }

    pub fn select_channel(&mut self, channel: Snowflake) {
        if self.channel != Some(channel) {
            self.composer = TextEdit::new("");
            (self.reply, self.editing, self.sticky_unread, self.hold_ack, self.highlight) = (None, None, None, None, None);
            self.uploads.clear();
            self.at_bottom = true;
            self.rows_below = 0;
        }
        self.channel = Some(channel);
        self.social.discovery = false;
        self.social.members = None;
        self.voice.call_view = false;
        self.intents.push(Intent::SelectChannel(channel));
    }

    /// Opens the quick switcher, or closes it.
    pub fn toggle_switcher(&mut self) {
        self.switcher = if self.switcher.is_some() { None } else { Some(switcher::Switcher::new()) };
    }
}

/// Goes to a message: opens its channel if needed, then scrolls to it (loading around it when
/// it isn't loaded) and highlights it.
pub fn jump(st: &mut ShellState, guild: Option<Snowflake>, channel: Snowflake, message: Snowflake) {
    if st.channel != Some(channel) {
        if st.guild != guild {
            st.guild = guild;
            st.intents.push(Intent::SelectGuild(guild));
        }
        st.select_channel(channel);
    }
    st.highlight = Some(message);
    st.intents.push(Intent::JumpTo { channel, message });
}

/// Once a frame, where the store and a `Tx` are at hand: runs a requested search, settles jumps
/// to loaded messages and scrolls to a requested row. Runs after the runtime drained intents.
pub fn poll_shell(store: &Store, st: &mut ShellState, tx: &mut rune::Tx) -> bool {
    let mut changed = expr::poll(store, st, tx);
    changed |= crate::nav::poll(store, st);
    changed |= members_page::poll(st, tx);
    changed |= crate::guild::poll_overwrite_search(store, st, tx);
    changed |= crate::settings::search::poll(store, st);
    changed |= crate::settings::dev::poll(st);
    if std::mem::take(&mut st.search_requested) {
        search::run(store, st, st.search.page.max(1));
        changed = true;
    }
    changed |= media_poll(store, st, tx);
    // A jump to a loaded message only scrolls; the runtime loads the others first.
    st.intents.retain(|i| match i {
        Intent::JumpTo { channel, message } if store.messages(*channel).iter().any(|m| m.id == *message) => {
            st.scroll_to = Some(format!("msg-{message}"));
            false
        }
        _ => true,
    });
    // A jumped-to message lands centred (`scrollToMessage`); the end just comes into view.
    let again = st.scroll_again.take();
    let Some(key) = st.scroll_to.take().or(again.clone()) else { return changed };
    if key.starts_with("msg-") {
        tx.scroll_center(key.clone());
    } else {
        tx.scroll_into_view(key.clone());
    }
    if again.is_none() {
        st.scroll_again = Some(key);
        tx.wake_after(std::time::Duration::ZERO);
    }
    true
}

/// Media upkeep: mirrors trusted domains, fetches linked invites, schedules the next animation
/// frame and player tick, and puts a copied image on the clipboard.
fn media_poll(store: &Store, st: &mut ShellState, tx: &mut rune::Tx) -> bool {
    if st.media_ui.trusted != store.settings.trusted_domains {
        st.media_ui.trusted = store.settings.trusted_domains.clone();
    }
    let mut changed = invite_embed::request(store, st);
    changed |= clip::poll(st, tx);
    changed |= gifv::poll(st, tx);
    changed |= voice_message::poll_volume(st);
    changed |= crate::settings::theme_folder::poll(st, tx);
    // Saved media, once a session, so message media know which are saved (`FavoriteMemes`).
    if store.me.is_some() && !st.expr.fetched.contains_key("memes") {
        expr::fetch(st, "memes", "/users/@me/memes");
    }
    if st.voice.voice_message.as_ref().is_some_and(|vm| vm.playhead().is_some()) {
        tx.wake_after(std::time::Duration::from_millis(33));
        changed = true;
    }
    if let Some(d) = st.media.take_next_frame() {
        tx.wake_after(d);
        changed = true;
    }
    let playing = st.media_ui.playing.as_ref().is_some_and(|(_, c)| matches!(c.state(), rune::PlayerState::Playing | rune::PlayerState::Loading));
    if playing {
        tx.wake_after(std::time::Duration::from_millis(250));
    }
    if let Some(png) = st.media.take_clip() {
        match rune::ClipboardImage::from_png(png) {
            Some(img) => tx.copy_rich(rune::ClipboardContent::default().with_image(img)),
            None => eprintln!("reflux: copy image: not a PNG"),
        }
        changed = true;
    }
    changed
}

/// A message link (`https://{webapp}/channels/{guild or @me}/{channel}/{message}`) on this
/// instance: (guild, channel, message).
pub fn parse_message_link(web_base: &str, url: &str) -> Option<(Option<Snowflake>, Snowflake, Snowflake)> {
    let base = web_base.trim_end_matches('/');
    let rest = url.strip_prefix(base).filter(|_| !base.is_empty())?.strip_prefix("/channels/")?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let g = parts.next()?;
    let c: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let guild = if g == "@me" { None } else { Some(Snowflake(g.parse().ok()?)) };
    Some((guild, Snowflake(c), Snowflake(m)))
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
    // `WatchStreamOverlay` in full-window mode.
    if st.voice.stream_fullscreen
        && let Some(v) = voice::viewer(store, st, p, acc)
    {
        return rune_kit::root(v, Vec::new());
    }
    let divider = over(p.user_area_divider_color, p.background_secondary);
    // Personal notes: a channel whose ID is the user's own, which READY may leave out.
    let notes = Channel { id: store.my_id(), kind: ChannelType::DM_PERSONAL_NOTES, name: Some("Personal Notes".into()), ..Default::default() };
    let channel = st.channel.and_then(|c| store.channels.get(&c).or((c == notes.id).then_some(&notes)));
    let mut layer = Vec::new();
    let side = match st.guild {
        _ if st.nav.favorites => crate::nav::favorites::sidebar(store, st, p, acc),
        Some(g) => sidebar::guild(store, st, g, p, acc),
        None => sidebar::home(store, st, p, acc),
    };
    let main = match (st.guild, channel) {
        _ if st.voice.watching.is_some() => voice::viewer(store, st, p, acc).unwrap_or_else(el),
        (None, None) if st.social.discovery => crate::social::discovery_page(st, p, acc, &mut layer),
        _ if st.voice.call_view && st.voice.channel.is_some_and(|(g, _)| g.is_some()) => call::view(store, st, p, acc).unwrap_or_else(el),
        (Some(_), _) if st.social.members.as_ref().is_some_and(|m| Some(m.guild) == st.guild) => members_page::view(store, st, p, acc, &mut layer).unwrap_or_else(el),
        (None, None) => row().grow(1.0).min_w(0.0).h_full().child(friends::view(store, st, p, acc, &mut layer)).children(active_now::view(store, st, p, acc)),
        _ => chat::main(store, st, channel, p, acc, &mut layer),
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
            .child(main)
            .child(el().pin(Some(dim::LAYOUT_SIDEBAR_WIDTH), Some(0.0), None, Some(0.0)).w(1.0).fill(divider)),
    );
    let body = row()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(p.background_secondary)
        .child(guilds::rail(store, st, p, acc))
        .child(frame)
        .child(sidebar::user_area(store, st, p, acc));
    let body = col().w_full().h_full().children(crate::desktop::nagbar::bars(st, p, acc)).children(crate::nav::terms::nagbar(store, st, p, acc)).children(crate::desktop::notices::instance(st, p, acc)).children(verify_nagbar(store, st, p, acc)).children(crate::desktop::notices::premium(store, st, p, acc)).children(crate::social::preview_nagbar(store, st, p, acc)).child(body);
    crate::compose::floating_toolbar(st, p, acc, &mut layer);
    menus::layer(store, st, p, acc, &mut layer);
    status::menu(store, st, p, acc, &mut layer);
    expr::layer(store, st, p, acc, &mut layer);
    crate::desktop::nagbar::layer(st, p, acc, &mut layer);
    crate::guild::layer(store, st, p, acc, &mut layer);
    crate::social::layer(store, st, p, acc, &mut layer);
    crate::nav::layer(store, st, p, acc, &mut layer);
    if let Some(ui) = &st.settings {
        crate::settings::view(store, st, ui, p, st.premium, acc, &mut layer);
    }
    crate::settings::view_shell_modals(store, st, p, acc, &mut layer);
    layer.extend(crate::settings::sudo::view(st, p, acc));
    layer.extend(upload::edit_modal(st, p, acc));
    layer.extend(switcher::view(store, st, p, acc));
    layer.extend(upload::notice_modal(st, p, acc));
    layer.extend(lightbox::view(st, p, acc));
    layer.extend(links::modal(st, p, acc));
    layer.extend(debug::modal(store, st, p, acc));
    call::modals(store, st, p, acc, &mut layer);
    call::details(st, p, acc, &mut layer);
    call::tile_menu(store, st, p, acc, &mut layer);
    layer.extend(voice_message::modal(st, p, acc));
    voice_message::volume_layer(st, p, acc, &mut layer);
    layer.extend(call::incoming(store, st, p, acc));
    birthday::view(store, st, p, acc, &mut layer);
    let body = body.on_key_up(move |s: &mut S, k: &rune::KeyEvent, _| {
        let sh = acc(s);
        let key = format!("{:?}", k.key);
        if sh.voice.ptt_held && sh.voice.settings.ptt_key.as_deref() == Some(key.as_str()) {
            sh.voice.ptt_held = false;
            sh.intents.push(Intent::Voice(voice::Op::PushToTalk(false)));
            return true;
        }
        false
    });
    rune_kit::root(body, layer)
}

// ---- Shared bits -------------------------------------------------------------------------------

/// A message drawn on its own as a group head, for surfaces outside the shell (inbox, forward).
pub(crate) fn preview<S: 'static>(store: &Store, st: &ShellState, m: &reflux_model::Message, key: String, p: &Palette, acc: ShellAccess<S>) -> El {
    let md = markdown::Md::new(store, &st.media, store.channels.get(&m.channel_id).and_then(|c| c.guild_id), p, key);
    chat::message_preview(store, st, m, &md, p, acc)
}

/// Markdown text (a channel topic) as a wrapped block.
pub(crate) fn rich_text(store: &Store, st: &ShellState, guild: Option<Snowflake>, t: &str, key: &str, p: &Palette) -> El {
    markdown::inline_text(t, &markdown::Md::new(store, &st.media, guild, p, key.to_string()), crate::theme::font(16.0, 400.0).lh(22.0), p.text_primary).w_full()
}

/// Sends the composer after the @everyone confirmation.
pub(crate) fn send_confirmed(st: &mut ShellState, channel: Snowflake) {
    chat::send_now(st, channel);
}

/// Height the user area covers at the bottom of the rail and sidebar (`--layout-user-area-height`
/// + `--user-area-box-inset-block-end` + `--layout-voice-connection-height`).
fn user_area_overlay(st: &ShellState) -> f32 {
    dim::LAYOUT_USER_AREA_HEIGHT + dim::FOOTER_BOX_INSET + 1.0 + voice::panel_height(&st.voice)
}

/// A role colour (`0xRRGGBB`).
pub(crate) fn role_color(c: u32) -> Color {
    rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

pub(crate) fn presence(store: &Store, user: Snowflake) -> Status {
    let s = store.presences.get(&user).map(|p| p.status);
    match s {
        Some(PresenceStatus::Online) => Status::Online,
        Some(PresenceStatus::Idle) => Status::Idle,
        Some(PresenceStatus::Dnd) => Status::Dnd,
        _ => Status::Offline,
    }
}

/// A user's avatar from the media proxy: the skeleton while it loads, the default avatar
/// (`id % 6`) without a hash or when it fails. In a community, the member's own avatar there
/// wins (`getGuildMemberAvatarURL`).
pub(crate) fn avatar(store: &Store, st: &ShellState, user: Snowflake, size: f32, status: Option<Status>, surface: Color, p: &Palette) -> El {
    avatar_anim(store, st, user, size, status, surface, false, p)
}

/// [`avatar`], animated (an `a_` hash) when `animate` (`shouldAnimate`: the message is hovered).
#[allow(clippy::too_many_arguments)]
pub(crate) fn avatar_anim(store: &Store, st: &ShellState, user: Snowflake, size: f32, status: Option<Status>, surface: Color, animate: bool, p: &Palette) -> El {
    let hash = store.users.get(&user).map(|u| &u.avatar).or(store.me.as_ref().filter(|m| m.id == user).map(|m| &m.avatar));
    let default = || st.default_avatars.get((user.0 % 6) as usize).map(|i| i.pixels().clone());
    let member = st.guild.and_then(|g| Some((g, store.member(g, user)?.avatar.as_deref()?)));
    let load = match member {
        Some((g, h)) => Some(st.media.asset(|b, s| media::animate(media::member_avatar_url(b, g, user, h, size, s), h, animate))),
        None => hash.and_then(|h| h.as_deref()).map(|h| st.media.asset(|b, s| media::animate(media::avatar_url(b, user, h, size, s), h, animate))),
    };
    let px = match load {
        Some(Load::Ready(px)) => Some(px),
        Some(Load::Pending) => None,
        Some(Load::Failed) | None => default(),
    };
    Avatar::new(size).pixels(px.as_ref()).status(status).surface(surface).el(p)
}

/// `GroupDMAvatar` without an icon: two or three recipients clustered in the circle, each later
/// one cut out of the earlier by a 2px ring of `surface`.
// shortcut: group DM icons aren't loaded; the recipient cluster always shows.
pub(crate) fn group_avatar(store: &Store, st: &ShellState, c: &Channel, size: f32, surface: Color, p: &Palette) -> El {
    let me = store.my_id();
    let mut ids: Vec<Snowflake> = c.recipients.iter().map(|u| u.id).filter(|&u| u != me).take(3).collect();
    if ids.len() == 1 {
        ids.push(me);
    }
    let n = ids.len();
    let cluster = if n == 3 { size.min(32.0) } else { size };
    // `getAvatarPosition`: (top, left, size).
    let at = |i: usize| -> (f32, f32, f32) {
        let (ratio, inset) = match n {
            2 => (0.7, (cluster * 0.06).min(cluster * 0.7 * 0.18)),
            3 => (0.68, (cluster * 0.04).min(cluster * 0.68 * 0.12)),
            _ => return (0.0, 0.0, cluster),
        };
        let a = cluster * ratio;
        match (n, i) {
            (_, 0) => (inset, 0.0, a),
            (2, _) => (cluster - a - inset, cluster - a, a),
            (_, 1) => (inset, cluster - a, a),
            _ => (cluster - a - inset, (cluster - a) / 2.0, a),
        }
    };
    let mut e = el().size(cluster, cluster);
    for (i, u) in ids.into_iter().enumerate() {
        let (top, left, a) = at(i);
        let face = avatar(store, st, u, a, None, surface, p);
        e = e.child(if i == 0 { face.absolute(left, top) } else { el().absolute(left - 2.0, top - 2.0).padding(2.0).radius(a).fill(surface).child(face) });
    }
    e
}

/// A remote picture at a fixed size, cropped to cover it; `background-secondary-alt` until
/// it loads (and if it fails).
pub(crate) fn picture(load: Load, w: f32, h: f32, radius: f32, p: &Palette) -> El {
    let e = el().size(w, h).radius(radius);
    match load {
        Load::Ready(px) => e.pixels_fit(&px, rune::SurfaceFit::Fill),
        Load::Pending | Load::Failed => e.fill(p.background_secondary_alt),
    }
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
pub(crate) fn channel_icon(kind: ChannelType) -> &'static Icon {
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
