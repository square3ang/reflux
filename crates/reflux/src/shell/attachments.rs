//! Message attachments (`channel/components/embeds/attachments/*`): the media mosaic
//! (`AttachmentMosaic`, `AttachmentLayoutGrid`, `getMosaicTileBox`), media tiles with spoiler and
//! mature-media blur (`MatureMediaBlurOverlay`), inline video and audio players, file cards
//! (`AttachmentFile`) and text previews (`TextualAttachmentPreview`).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use reflux_model::{Attachment, AttachmentFlags, Channel, ChannelType, Message, RelationshipType, Snowflake};
use reflux_store::Store;
use rune::{Color, El, FileDialogOptions, Player, PlayerState, Tx, col, el, icon, rich, row, text};

use super::lightbox::{self, Item};
use super::{ShellAccess, ShellState};
use crate::icons;
use crate::media::Load;
use crate::theme::{Palette, dim, font};
use crate::ui::{over, rgba};
use crate::i18n::{t, tf};

/// Media UI state: the lightbox, revealed spoilers, the clip playing inline, invite previews.
#[derive(Default)]
pub struct MediaUi {
    pub viewer: Option<lightbox::Viewer>,
    /// Keys of spoilers and mature media the user revealed.
    pub revealed: HashSet<String>,
    /// `MatureMediaGateDetailsModal`'s reason, while it's open.
    pub mature_details: Option<Gate>,
    /// The clip playing inline, by its tile key.
    pub playing: Option<(String, Player)>,
    /// The same through GStreamer, where Rune's player can't decode (`clip::native`).
    pub gst: Option<super::clip::GstClip>,
    /// The voice message whose volume popout shows, and when it closes (200 ms after the
    /// pointer leaves, `MediaVerticalVolumeControl`).
    pub volume_pop: Option<(String, Option<std::time::Instant>)>,
    /// gifv embeds playing in place.
    pub gifv: super::gifv::Gifv,
    /// Invite previews by code (`InviteEmbed`).
    pub invites: HashMap<String, super::invite_embed::Preview>,
    /// Mature channels the user chose to see this session (`MatureContentChannelGate`).
    pub mature_ok: HashSet<Snowflake>,
    /// Discovery's `age_policy` regions (country, optional region).
    pub age_geos: Vec<(String, Option<String>)>,
    /// Discovery's `hidden_built_in_guild_badges`.
    pub hidden_badges: Vec<String>,
    /// Discovery's custom guild badges, built-in badge replacement images and badge version.
    pub guild_badges: Vec<reflux_model::GuildBadge>,
    pub badge_images: reflux_model::BuiltInGuildBadgeImages,
    pub badges_version: Option<String>,
    /// The open `BadgeListModal`'s badges.
    pub badge_list: Option<Vec<super::badges::Item>>,
    /// The (guild, channel) of the invite card whose message menu is open.
    pub invite_menu: (Option<Snowflake>, Option<Snowflake>),
    /// The attachment a message menu was opened on (`MediaMenuData`'s items).
    pub media_menu: Option<MenuMedia>,
    /// Text previews shown expanded.
    pub expanded: HashSet<String>,
    /// The URL waiting on the external link warning, and its "skip next time" box.
    pub link_warning: Option<(String, bool)>,
    /// The user's `trusted_domains`, mirrored each frame for handlers.
    pub trusted: Vec<String>,
    /// The attachment waiting on "Delete attachment": (channel, message, attachment).
    pub confirm_delete: Option<(Snowflake, Snowflake, Snowflake)>,
}

/// What the media items of a message menu act on.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuMedia {
    pub url: String,
    pub proxy: String,
    pub filename: String,
    pub kind: MediaKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Gif,
    Video,
}

impl MenuMedia {
    pub fn of(a: &Attachment) -> Option<Self> {
        let url = a.url.clone().filter(|_| !a.expired)?;
        let kind = if a.content_type.starts_with("video/") {
            MediaKind::Video
        } else if is_animated(&a.content_type, a.flags) {
            MediaKind::Gif
        } else {
            MediaKind::Image
        };
        Some(MenuMedia { proxy: a.proxy_url.clone().unwrap_or_else(|| url.clone()), url, filename: a.filename.clone(), kind })
    }

    /// An embed's image, GIF or video; the file name is the URL's last segment.
    pub fn embed(url: &str, proxy: Option<&str>, video: bool, animated: bool) -> Self {
        let kind = if video { MediaKind::Video } else if animated { MediaKind::Gif } else { MediaKind::Image };
        let filename = url.split(['?', '#']).next().unwrap_or_default().rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("media").to_owned();
        MenuMedia { url: url.to_owned(), proxy: proxy.unwrap_or(url).to_owned(), filename, kind }
    }

    /// (copy, download, copy link, open link) labels by kind (`getCopyLabel` …).
    fn labels(&self) -> [&'static str; 4] {
        match self.kind {
            MediaKind::Image => ["Copy image", "Download image", "Copy image link", "Open image link"],
            MediaKind::Gif => ["Copy GIF", "Download GIF", "Copy GIF link", "Open GIF link"],
            MediaKind::Video => ["Copy video", "Download video", "Copy video link", "Open video link"],
        }
    }
}

/// `MediaMenuData`'s last group: copy (pictures only; the clipboard takes no video), download,
/// copy link, open link.
pub fn menu_items<S: 'static>(st: &ShellState, acc: ShellAccess<S>) -> Vec<crate::ui::MenuEntry<S>> {
    use crate::ui::{MenuEntry, MenuItem};
    let Some(m) = st.media_ui.media_menu.clone() else { return Vec::new() };
    let [copy, download, copy_link, open_link] = m.labels().map(t);
    let mut out = Vec::new();
    if m.kind != MediaKind::Video {
        let png = format!("{}{}format=png", m.proxy, if m.proxy.contains('?') { '&' } else { '?' });
        out.push(MenuEntry::Item(MenuItem::new("media-copy", copy).icon(&icons::COPY_FILL).on_select(move |s: &mut S| acc(s).media.copy_image(png.clone()))));
    }
    let (url, name) = (m.url.clone(), m.filename.clone());
    if crate::look::with(|l| l.media_download_button) {
        out.push(MenuEntry::Item(MenuItem::new("media-download", download).icon(&icons::DOWNLOAD_SIMPLE_FILL).on_select_tx(move |_: &mut S, tx: &mut Tx| save_as(acc, url.clone(), name.clone(), tx))));
    }
    let url = m.url.clone();
    out.push(MenuEntry::Item(MenuItem::new("media-copy-link", copy_link).icon(&icons::LINK_FILL).on_select_tx(move |_: &mut S, tx: &mut Tx| tx.copy(url.clone()))));
    let url = m.url;
    out.push(MenuEntry::Item(MenuItem::new("media-open-link", open_link).icon(&icons::ARROW_SQUARE_OUT).on_select_tx(move |_: &mut S, tx: &mut Tx| {
        let _ = tx.open_url(&url);
    })));
    out
}

// ---- Sensitive media ----------------------------------------------------------------------------

/// `resolveEffectiveChannelMatureContent`: the channel's override, its category's, else the guild's.
pub fn channel_mature(store: &Store, c: &Channel) -> bool {
    let parent = c.parent_id.and_then(|p| store.channels.get(&p));
    c.nsfw_override
        .or_else(|| parent.and_then(|p| p.nsfw_override))
        .unwrap_or_else(|| c.guild_id.and_then(|g| store.guilds.get(&g)).and_then(|g| g.info.as_ref()).is_some_and(|i| i.nsfw))
}

/// `getSensitiveFilterLevel`: 0 show, 1 blur, 2 block, for flagged media in a channel.
pub fn filter_level(store: &Store, channel: Snowflake) -> u8 {
    let Some(mut c) = store.channels.get(&channel) else { return 0 };
    if c.kind.is_thread()
        && let Some(parent) = c.parent_id.and_then(|p| store.channels.get(&p))
    {
        c = parent;
    }
    let s = &store.settings;
    match c.kind {
        ChannelType::DM => {
            let friend = c.recipients.first().and_then(|u| store.relationships.get(&u.id)).is_some_and(|r| r.kind == RelationshipType::FRIEND);
            if friend { s.sensitive_content_friend_dm_filter } else { s.sensitive_content_non_friend_dm_filter }
        }
        ChannelType::GROUP_DM => s.sensitive_content_non_friend_dm_filter,
        _ if c.guild_id.is_some() && channel_mature(store, c) => 0,
        _ if c.guild_id.is_some() => s.sensitive_content_guild_filter,
        _ => 0,
    }
}

fn flagged(nsfw: bool, flags: AttachmentFlags) -> bool {
    nsfw || flags.contains(AttachmentFlags::CONTAINS_EXPLICIT_MEDIA)
}

/// `GuildMatureContentAgree.getGateReason` for a mature channel the user can't see yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// The user's region checks age and they haven't (`GEO_RESTRICTED`).
    Geo,
    /// The account may not see mature content (`MATURE_CONTENT_CHECK_REQUIRED`).
    Check,
    /// They haven't agreed to this scope yet (`CONSENT_REQUIRED`).
    Consent,
}

/// The gate for `c`, if any: the geo and account checks first, then an agreement to the
/// channel, its category or the guild (kept per account, like Fluxer's synced agreements).
pub fn gate_reason(store: &Store, st: &ShellState, c: &Channel) -> Option<Gate> {
    let g = c.guild_id?;
    if let Some(mock) = st.prefs.dev.mature_gate() {
        return Some(mock);
    }
    if !channel_mature(store, c) {
        return None;
    }
    let me = store.me.as_ref();
    if let Some((country, region)) = geo(st)
        && st.media_ui.age_geos.iter().any(|(cc, rc)| *cc == country && rc.as_ref().is_none_or(|r| Some(r) == region.as_ref()))
        && !me.is_some_and(|u| u.age_verified_adult)
    {
        return Some(Gate::Geo);
    }
    if me.is_some_and(|u| !u.nsfw_allowed) {
        return Some(Gate::Check);
    }
    let agreed = crate::nav::mine(st).mature_agreed;
    let ok = |id: Snowflake| st.media_ui.mature_ok.contains(&id) || agreed.contains(&id);
    (!(ok(c.id) || c.parent_id.is_some_and(ok) || ok(g))).then_some(Gate::Consent)
}

/// `useMatureMedia`'s gate reason for mature media in the open channel, when it's a geo or
/// account gate (consent gates the channel instead).
fn media_gate(store: &Store, st: &ShellState) -> Option<Gate> {
    match st.prefs.dev.mock_mature_media_gate_reason.as_str() {
        "geo_restricted" => return Some(Gate::Geo),
        "none" => {}
        _ => return Some(Gate::Check),
    }
    let c = store.channels.get(&st.channel?)?;
    gate_reason(store, st, c).filter(|g| *g != Gate::Consent)
}

/// `MatureMediaGateDetailsModal`.
pub fn details_modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let gate = st.media_ui.mature_details?;
    let close = move |s: &mut S| acc(s).media_ui.mature_details = None;
    let para = |s: String| text(s).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap();
    let modal = crate::ui::Modal::new(t("Mature media"), close);
    Some(match gate {
        Gate::Geo => {
            let region = geo(st).map_or_else(|| region_display_name(None, None), |(c, r)| region_display_name(Some(&c), r.as_deref()));
            let ok = crate::ui::Button::new(t("Close")).key("mature-details-close").on_press(close).el(p);
            modal.body(para(tf("Due to mature content laws in {regionName}, this media is not available from your current region.", &[("regionName", &region)]))).footer(ok).el(p)
        }
        _ => {
            let url = st.marketing_base.as_ref().map(|b| format!("{}/help/change-date-of-birth", b.trim_end_matches('/')));
            let body = match url {
                Some(url) => {
                    let (s, range) = crate::i18n::tf_marked("This mature media is not available to your account. <0>Learn more</0>", &[]);
                    let spans = vec![rune::Span::new(range.start, range.end).color(p.text_link)];
                    rich(s, spans).link(rune::Link::new(range, url)).on_link(|_: &mut S, url: &str, tx: &mut rune::Tx| drop(tx.open_url(url))).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap()
                }
                None => para(t("This mature media is not available to your account.").into()),
            };
            modal.body(body).el(p)
        }
    })
}

/// The client's `GET /ip` country and region (`GeoIP`), once loaded.
fn geo(st: &ShellState) -> Option<(String, Option<String>)> {
    // `mockInUK` (`getEffectiveMatureContentGeoContext`).
    if st.prefs.dev.mock_in_uk {
        return Some(("GB".into(), None));
    }
    let Some(super::expr::Fetched::Done(v)) = st.expr.fetched.get(GEO) else { return None };
    let s = |k: &str| v[k].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    Some((s("countryCode")?, s("regionCode")))
}

/// `UserGeo.getRegionDisplayName`: the country's localized name ("your region" without one), with
/// the state first for the US.
fn region_display_name(country: Option<&str>, region: Option<&str>) -> String {
    let Some(c) = country else { return t("your region").into() };
    let name = crate::i18n::region_name_in(crate::i18n::locale(), c);
    match (c, region) {
        ("US", Some(r)) => format!("{}, {}", us_state(r), name.as_deref().unwrap_or("United States")),
        _ => name.unwrap_or_else(|| c.to_string()),
    }
}

/// `US_STATE_DESCRIPTORS`: a state's name, else its code.
fn us_state(code: &str) -> String {
    const STATES: [(&str, &str); 51] = [
        ("AL", "Alabama"), ("AK", "Alaska"), ("AZ", "Arizona"), ("AR", "Arkansas"), ("CA", "California"), ("CO", "Colorado"), ("CT", "Connecticut"),
        ("DE", "Delaware"), ("FL", "Florida"), ("GA", "Georgia"), ("HI", "Hawaii"), ("ID", "Idaho"), ("IL", "Illinois"), ("IN", "Indiana"), ("IA", "Iowa"),
        ("KS", "Kansas"), ("KY", "Kentucky"), ("LA", "Louisiana"), ("ME", "Maine"), ("MD", "Maryland"), ("MA", "Massachusetts"), ("MI", "Michigan"),
        ("MN", "Minnesota"), ("MS", "Mississippi"), ("MO", "Missouri"), ("MT", "Montana"), ("NE", "Nebraska"), ("NV", "Nevada"), ("NH", "New Hampshire"),
        ("NJ", "New Jersey"), ("NM", "New Mexico"), ("NY", "New York"), ("NC", "North Carolina"), ("ND", "North Dakota"), ("OH", "Ohio"), ("OK", "Oklahoma"),
        ("OR", "Oregon"), ("PA", "Pennsylvania"), ("RI", "Rhode Island"), ("SC", "South Carolina"), ("SD", "South Dakota"), ("TN", "Tennessee"), ("TX", "Texas"),
        ("UT", "Utah"), ("VT", "Vermont"), ("VA", "Virginia"), ("WA", "Washington"), ("WV", "West Virginia"), ("WI", "Wisconsin"), ("WY", "Wyoming"),
        ("DC", "District of Columbia"),
    ];
    STATES.iter().find(|(c, _)| *c == code).map_or_else(|| code.to_string(), |(_, n)| t(n).to_string())
}

/// `expr.fetched` key of the `/ip` lookup.
pub const GEO: &str = "geoip";

/// `MatureContentChannelGate`: the reason's title and text; for consent, the scope's warning
/// and a danger "Proceed" that remembers the agreement.
pub fn mature_gate<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let reason = gate_reason(store, st, c)?;
    let parent = c.parent_id.and_then(|p| store.channels.get(&p)).filter(|p| p.kind == ChannelType::GUILD_CATEGORY);
    let (scope, scope_id) = if c.nsfw_override.is_some() {
        ("channel", c.id)
    } else if let Some(cat) = parent.filter(|p| p.nsfw_override.is_some()) {
        ("category", cat.id)
    } else {
        ("community", c.guild_id.unwrap_or(c.id))
    };
    let (mut learn_more, mut link_range) = (None::<String>, 0..0);
    let (title, body, button) = match reason {
        Gate::Geo => {
            let region = geo(st).map_or_else(|| region_display_name(None, None), |(c, r)| region_display_name(Some(&c), r.as_deref()));
            (t("Mature content").to_string(), tf("Due to mature content laws in {regionName}, mature content is not available from here.", &[("regionName", &region)]), None)
        }
        // With a help center, the text ends in a "Learn more" link (`HelpCenterArticleSlug.ChangeDateOfBirth`).
        Gate::Check => {
            let (title, linked, plain) = match scope {
                "community" => ("Mature community", "This mature community is not available to your account. <0>Learn more</0>", "This mature community is not available to your account."),
                "category" => ("Mature category", "This mature category is not available to your account. <0>Learn more</0>", "This mature category is not available to your account."),
                _ => ("Mature channel", "This mature channel is not available to your account. <0>Learn more</0>", "This mature channel is not available to your account."),
            };
            learn_more = st.marketing_base.as_ref().map(|b| format!("{}/help/change-date-of-birth", b.trim_end_matches('/')));
            let body = if learn_more.is_some() { crate::i18n::tf_marked(linked, &[]) } else { (t(plain).to_string(), 0..0) };
            link_range = body.1;
            (t(title).to_string(), body.0, None)
        }
        Gate::Consent => {
            let proceed = crate::ui::Button::new(t("Proceed"))
                .variant(crate::ui::ButtonVariant::Danger)
                .key("mature-proceed")
                .on_press(move |s: &mut S| {
                    let st = acc(s);
                    st.media_ui.mature_ok.insert(scope_id);
                    crate::nav::update_mine(st, |a| a.mature_agreed.push(scope_id));
                })
                .el(p);
            (t("Mature content").into(), format!("This {scope} is marked for mature content and may contain material that may be inappropriate for some users."), Some(proceed))
        }
    };
    Some(
        row().w_full().grow(1.0).min_h(0.0).center().fill(p.background_secondary_lighter).child(
            col()
                .padding(32.0)
                .items_center()
                .child(text(title).font(font(24.0, 600.0).lh(32.0)).color(p.text_primary).mb(12.0))
                .child(match learn_more {
                    Some(url) => {
                        let spans = vec![rune::Span::new(link_range.start, link_range.end).color(p.text_link)];
                        rich(body, spans).link(rune::Link::new(link_range, url)).on_link(|_: &mut S, url: &str, tx: &mut rune::Tx| drop(tx.open_url(url)))
                    }
                    None => text(body),
                }
                .font(font(16.0, 400.0).lh(24.0))
                .color(p.text_primary_muted)
                .wrap()
                .align(rune::Align::Center)
                .max_w(448.0)
                .mb(16.0))
                .children(button),
        ),
    )
}

pub fn is_media(a: &Attachment) -> bool {
    a.width.is_some_and(|w| w > 0) && a.height.is_some_and(|h| h > 0) && (a.content_type.starts_with("image/") || a.content_type.starts_with("video/"))
}

fn is_spoiler(a: &Attachment) -> bool {
    a.flags.contains(AttachmentFlags::IS_SPOILER) || a.filename.starts_with("SPOILER_")
}

fn is_animated(content_type: &str, flags: AttachmentFlags) -> bool {
    content_type == "image/gif" || flags.contains(AttachmentFlags::IS_ANIMATED)
}

/// A lightbox item for an attachment.
pub fn item(a: &Attachment) -> Item {
    Item {
        url: a.url.clone().unwrap_or_default(),
        proxy: a.proxy_url.clone().or_else(|| a.url.clone()).unwrap_or_default(),
        w: a.width.unwrap_or(0),
        h: a.height.unwrap_or(0),
        name: a.title.clone().unwrap_or_else(|| a.filename.clone()),
        size: a.size,
        alt: a.description.clone().filter(|d| !d.trim().is_empty()),
        video: a.content_type.starts_with("video/"),
        animated: is_animated(&a.content_type, a.flags),
    }
}

// ---- Mosaic ------------------------------------------------------------------------------------

/// `MOSAIC_TILE_SLOTS`: (columns, span, aspect w, aspect h) of tile `i` of `n`.
fn slot(n: usize, i: usize) -> (f32, f32, f32, f32) {
    match (n, i) {
        (2, _) => (2.0, 1.0, 1.0, 1.0),
        (3, 0) => (3.0, 2.0, 1.0, 1.0),
        (3, _) => (3.0, 1.0, 1.0, 1.0),
        (4, _) => (2.0, 1.0, 3.0, 2.0),
        (5, 0 | 1) => (6.0, 3.0, 3.0, 2.0),
        (5, _) => (6.0, 2.0, 1.0, 1.0),
        (6 | 9, _) => (3.0, 1.0, 1.0, 1.0),
        (7 | 10, 0) => (1.0, 1.0, 16.0, 9.0),
        (7 | 10, _) => (3.0, 1.0, 1.0, 1.0),
        (8, 0 | 1) => (2.0, 1.0, 3.0, 2.0),
        (8, _) => (3.0, 1.0, 1.0, 1.0),
        _ => (2.0, 1.0, 3.0, 2.0),
    }
}

const GAP: f32 = 4.0;
/// `getMosaicMediaDimensions().maxWidth`.
const MOSAIC_W: f32 = 550.0;

thread_local! {
    /// The width media may take while a narrow card (pins, search) builds its message.
    static MAX_W: std::cell::Cell<f32> = const { std::cell::Cell::new(f32::INFINITY) };
}

/// Builds `f` with media scaled down to fit `w` (`max-width: 100%` in a narrow card).
pub fn with_max_w<R>(w: f32, f: impl FnOnce() -> R) -> R {
    let old = MAX_W.replace(w.max(1.0));
    let r = f();
    MAX_W.set(old);
    r
}

/// A drawn size scaled down to the current cap, keeping its ratio.
fn capped((w, h): (f32, f32)) -> (f32, f32) {
    let k = (MAX_W.get() / w).min(1.0);
    ((w * k).round().max(1.0), (h * k).round().max(1.0))
}

/// `getMosaicTileBox`.
pub fn tile_box(n: usize, i: usize, width: f32) -> (f32, f32) {
    let (cols, span, aw, ah) = slot(n, i);
    let track = (width - (cols - 1.0) * GAP) / cols;
    let w = (track * span + (span - 1.0) * GAP).max(1.0);
    (w, (w * ah / aw).max(1.0))
}

/// Tiles per row of `AttachmentLayoutGrid` (3 is a 2-row span plus a column, handled apart).
fn rows(n: usize) -> Vec<usize> {
    match n {
        2 => vec![2],
        4 => vec![2, 2],
        5 => vec![2, 3],
        6 => vec![3, 3],
        7 => vec![1, 3, 3],
        8 => vec![2, 3, 3],
        9 => vec![3, 3, 3],
        10 => vec![1, 3, 3, 3],
        n => vec![2; n.div_ceil(2)],
    }
}

/// One media tile's look and what pressing it does.
pub struct Tile<'a> {
    pub key: String,
    pub proxy: Option<&'a str>,
    pub url: &'a str,
    pub size: (f32, f32),
    pub video: bool,
    pub animated: bool,
    pub alt: Option<&'a str>,
    pub spoiler: bool,
    /// Flagged media the filter blurs.
    pub mature: bool,
    /// The message attachment it shows (channel, message, attachment): the hover toolbar's
    /// Download and Add to saved media.
    pub saved: Option<(Snowflake, Snowflake, &'a Attachment)>,
    /// The hover toolbar offers Delete ([`can_delete_attachment`]).
    pub deletable: bool,
    /// Right-click opens this message's menu with the media items.
    pub menu: Option<(Snowflake, MenuMedia)>,
}

/// The message's media attachments (minus blocked ones) as one picture or the mosaic, then
/// its other attachments as players, previews or file cards.
pub fn attachments<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> Vec<El> {
    let level = filter_level(store, m.channel_id);
    // `inlineAttachmentMedia` off: images and videos are file cards like other files.
    let inline = store.settings.inline_attachment_media;
    let shown = |a: &&Attachment| inline && is_media(a);
    let media: Vec<&Attachment> = m.attachments.iter().filter(|a| shown(a) && !(flagged(a.nsfw, a.flags) && level == 2)).collect();
    // `render_spoilers` 2 ("in channels I moderate") shows them where I can manage messages.
    let unspoil = store.settings.render_spoilers == 2 && store.permissions(m.channel_id).contains(reflux_model::Permissions::MANAGE_MESSAGES);
    let items: Rc<Vec<Item>> = Rc::new(media.iter().map(|a| item(a)).collect());
    let hovered = st.hovered_msg() == Some(m.id);
    let deletable = can_delete_attachment(store, m.author.id, m.channel_id);
    let mut out = Vec::new();
    let mosaic_w = MOSAIC_W.min(MAX_W.get());
    let tile = |i: usize, size: (f32, f32)| {
        let a = media[i];
        let t = Tile {
            key: format!("att-{}", a.id),
            proxy: a.proxy_url.as_deref().or(a.url.as_deref()),
            url: a.url.as_deref().unwrap_or_default(),
            size,
            video: a.content_type.starts_with("video/"),
            animated: is_animated(&a.content_type, a.flags),
            alt: a.description.as_deref(),
            spoiler: is_spoiler(a) && !unspoil,
            mature: (flagged(a.nsfw, a.flags) && level == 1) || st.prefs.dev.mature_media(),
            saved: Some((m.channel_id, m.id, a)),
            deletable,
            menu: MenuMedia::of(a).map(|mm| (m.id, mm)),
        };
        let (items, msg) = (items.clone(), (m.channel_id, m.id));
        media_tile(store, st, t, hovered, media.len() > 1, p, acc, Rc::new(move |s: &mut S| lightbox::open(acc(s), (*items).clone(), i, Some(msg))))
    };
    match media.len() {
        0 => {}
        1 => {
            let (w, h) = (media[0].width.unwrap_or(1), media[0].height.unwrap_or(1));
            out.push(tile(0, capped(super::chat::media_size(w, h))).key("media"));
        }
        3 => {
            let side = col().gap(GAP).child(tile(1, tile_box(3, 1, mosaic_w))).child(tile(2, tile_box(3, 2, mosaic_w)));
            out.push(row().gap(GAP).items_start().key("media").child(tile(0, tile_box(3, 0, mosaic_w))).child(side));
        }
        n => {
            let mut grid = col().gap(GAP);
            let mut i = 0;
            for per in rows(n) {
                let mut r = row().gap(GAP);
                for _ in 0..per.min(n - i) {
                    r = r.child(tile(i, tile_box(n, i, mosaic_w)));
                    i += 1;
                }
                grid = grid.child(r);
            }
            out.push(grid.key("media"));
        }
    }
    if !media.is_empty()
        && let Some(f) = expiry_footnote(&media, st.now_ms, p)
    {
        out.push(f.key("media-expiry"));
    }
    for a in m.attachments.iter().filter(|a| !shown(a)) {
        let footnote = expiry_footnote(&[a], st.now_ms, p);
        let card = if a.waveform.is_some() && a.url.is_some() {
            super::voice_message::player(st, a, p, acc)
        } else if a.content_type.starts_with("audio/") && a.url.is_some() {
            audio_player(st, &format!("audio-{}", a.id), a, p, acc)
        } else if is_textual(a) {
            text_preview(st, a, p, acc)
        } else {
            file_card(a, p, acc)
        };
        out.push(match footnote {
            Some(f) => col().items_start().child(card.key("card")).child(f.key("expiry")),
            None => card,
        }
        .key(format!("att-{}", a.id)));
    }
    out
}

/// `ExpiryFootnote` under a mosaic (`AttachmentMosaic`: earliest, or "between" two dates) or a
/// single attachment: 10.4px `text-tertiary`, 0.35rem above.
// shortcut: plain text; Fluxer links it to the attachment-expiry help article.
fn expiry_footnote(atts: &[&Attachment], now_ms: u64, p: &Palette) -> Option<El> {
    if !crate::look::with(|l| l.media_expiry_indicator) {
        return None;
    }
    let now = chrono::DateTime::from_timestamp_millis(now_ms as i64).unwrap_or_default();
    let mut dates = Vec::new();
    let mut expired = false;
    for a in atts {
        let Some(d) = a.expires_at.as_deref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|d| d.to_utc()).or(a.expired.then_some(now)) else { continue };
        expired |= a.expired || d <= now;
        dates.push(d);
    }
    let (first, last) = (dates.iter().min()?, dates.iter().max()?);
    let fmt = |d: &chrono::DateTime<chrono::Utc>| crate::desktop::time::medium_date(d.with_timezone(&chrono::Local).date_naive(), crate::i18n::locale());
    let (a, b) = (fmt(first), fmt(last));
    let label = match (a != b, expired) {
        (true, true) => tf("Expired between {earliest} and {latest}", &[("earliest", &a), ("latest", &b)]),
        (true, false) => tf("Expires between {earliest} and {latest}", &[("earliest", &a), ("latest", &b)]),
        (false, true) => tf("Expired on {earliest}", &[("earliest", &a)]),
        (false, false) => tf("Expires on {earliest}", &[("earliest", &a)]),
    };
    Some(text(label).font(font(10.4, 400.0).lh(12.48)).color(p.text_tertiary).mt(5.6))
}

/// A picture (or a video's poster) at its drawn size, with the GIF, ALT, play, spoiler and
/// mature overlays Fluxer draws on it. `cover`: a mosaic tile, cropped to its box.
#[allow(clippy::too_many_arguments)]
pub fn media_tile<S: 'static>(store: &Store, st: &ShellState, t: Tile, hovered: bool, cover: bool, p: &Palette, acc: ShellAccess<S>, open: Rc<dyn Fn(&mut S)>) -> El {
    let (w, h) = t.size;
    if t.video
        && let Some((_, clip)) = st.media_ui.playing.as_ref().filter(|(k, _)| *k == t.key)
    {
        return video_player(clip, &t.key, t.url, t.size, p, acc, Some(open));
    }
    if t.video
        && let Some(c) = super::clip::get(st, &t.key)
    {
        let full: Rc<dyn Fn(&mut S)> = Rc::new(move |s: &mut S| {
            acc(s).media_ui.gst = None;
            open(s)
        });
        return super::clip::video(c, (w, h), p, acc, Some(full));
    }
    let ui = &st.media_ui;
    // `render_spoilers` 0 shows spoilers outright; 1 blurs them; 2 too, outside the channels I
    // moderate (cleared by the caller).
    let hidden_spoiler = t.spoiler && store.settings.render_spoilers != 0 && !ui.revealed.contains(&t.key);
    let hidden_mature = t.mature && !ui.revealed.contains(&format!("{}-mature", t.key));
    let hidden = hidden_spoiler || hidden_mature;
    // Reduced motion pauses autoplay (`resolveMotionPreferencesModel`).
    let auto = store.settings.gif_auto_play && !crate::look::reduced_motion();
    let animate = t.animated && !t.video && !hidden && (auto || hovered);
    let load = match t.proxy {
        Some(u) if animate => st.media.get(&crate::media::fit_url_animated(u, w, st.media.scale())),
        Some(u) => st.media.get(&crate::media::fit_url(u, w, st.media.scale())),
        None => Load::Failed,
    };
    let mut pic = match load {
        Load::Failed => row().size(w, h).center().radius(dim::MEDIA_BORDER_RADIUS).fill(p.background_secondary_alt).child(icon(&icons::IMAGE_FILL, 32.0).color(p.text_tertiary_muted)),
        Load::Ready(px) if !cover => el().size(w, h).radius(dim::MEDIA_BORDER_RADIUS).pixels_fit(&px, rune::SurfaceFit::Fit),
        l => super::picture(l, w, h, dim::MEDIA_BORDER_RADIUS, p),
    };
    if hidden_spoiler {
        pic = pic.blur(44.0);
    } else if hidden_mature {
        pic = pic.blur(12.0).opacity(0.15);
    }
    let mut tile = el().size(w, h).radius(dim::MEDIA_BORDER_RADIUS).clip_rounded(dim::MEDIA_BORDER_RADIUS).fill(p.background_secondary).child(pic);
    let fill = || el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0));
    if hidden_spoiler {
        let key = t.key.clone();
        let pill = text(crate::i18n::t("Spoiler").to_uppercase()).font(font(15.0, 600.0).lh(20.0)).color(Color::WHITE);
        tile = tile.child(
            fill()
                .fill(rgba(255, 255, 255, 0.1))
                .cursor(rune::Cursor::Pointer)
                .on_press(move |s: &mut S| {
                    acc(s).media_ui.revealed.insert(key.clone());
                })
                .child(row().w_full().h_full().center().child(row().py(6.0).px(12.0).radius(20.0).fill(rgba(0, 0, 0, 0.6)).hover(rgba(0, 0, 0, 0.8)).child(pill))),
        );
        return tile.role("button").label(crate::i18n::t("Spoiler"));
    }
    if hidden_mature {
        let key = format!("{}-mature", t.key);
        // A geo or account gate can't be revealed: Reveal explains it (`MatureMediaBlurOverlay`).
        if let Some(gate) = media_gate(store, st) {
            let reveal = row()
                .py(8.0)
                .px(12.0)
                .radius(6.0)
                .fill(p.background_modifier_accent)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .child(text(crate::i18n::t("Reveal")).font(font(12.0, 600.0).lh(12.0)).color(p.text_primary))
                .on_press(move |s: &mut S| acc(s).media_ui.mature_details = Some(gate));
            let reveal = super::badges::tip(format!("{key}-details"), super::badges::label_tip(crate::i18n::t("Click for details"), p), 240.0, reveal, acc);
            return tile.child(fill().fill(p.spoiler_overlay_color).backdrop_blur(12.0)).child(fill().center().child(reveal));
        }
        let reveal = row()
            .py(8.0)
            .px(12.0)
            .radius(6.0)
            .fill(p.background_modifier_accent)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(crate::i18n::t("Click to reveal"))
            .child(text(crate::i18n::t("Reveal")).font(font(12.0, 600.0).lh(12.0)).color(p.text_primary))
            .on_press(move |s: &mut S| {
                acc(s).media_ui.revealed.insert(key.clone());
            });
        return tile.child(fill().fill(p.spoiler_overlay_color).backdrop_blur(12.0)).child(fill().center().child(reveal));
    }
    tile = tile.cursor(rune::Cursor::Pointer).role("button").label(t.alt.unwrap_or("Image").to_owned());
    if t.video {
        // `.playButton`: 56px black circle; pressing plays in place.
        let (key, url) = (t.key.clone(), t.url.to_owned());
        tile = tile.child(fill().center().key("play").child(
            row()
                .size(56.0, 56.0)
                .center()
                .radius(28.0)
                .fill(rgba(0, 0, 0, 0.75))
                .child(icon(&icons::PLAY_FILL, 28.0).color(p.text_on_dark))
                .on_press(move |s: &mut S| super::clip::start(acc(s), key.clone(), url.clone(), false)),
        ));
    } else if t.animated && !auto && !hovered && crate::look::with(|l| l.media_gif_indicator) {
        // `GifIndicator` while the GIF waits for a hover.
        tile = tile.child(row().absolute(8.0, 8.0).py(4.0).px(8.0).radius(4.0).fill(rgba(0, 0, 0, 0.6)).key("gif").child(text("GIF").font(font(14.0, 600.0).lh(14.0)).color(p.text_on_dark)));
    }
    if let Some(alt) = t.alt.map(str::trim).filter(|a| !a.is_empty()) {
        // `AltTextBadge`: bottom-right, 8.8px caps on near-black.
        tile = tile.child(
            el().pin(None, None, Some(6.0), Some(6.0))
                .key("alt")
                .child(row().py(1.0).px(3.0).radius(3.0).fill(rgba(0, 0, 0, 0.95)).child(text("ALT").font(font(8.8, 600.0).lh(12.0)).color(Color::WHITE)))
                .label(alt.to_owned()),
        );
    }
    if let Some(bar) = media_tools(st, &t, p, acc) {
        tile = tile.hover_group().child(bar.key("tools"));
    }
    // Right-click: the message menu plus the media items.
    if let Some((msg, media)) = t.menu {
        tile = tile.on_secondary_press(move |s: &mut S, at, _| {
            let st = acc(s);
            st.popups.open_at(&format!("msg-menu-{msg}"), [at.x, at.y]);
            st.active_msg = Some(msg);
            st.media_ui.invite_menu = (None, None);
            st.media_ui.media_menu = Some(media.clone());
        });
    }
    tile.on_press(move |s: &mut S| open(s))
}

/// `canDeleteAttachmentUtil`: your own message, outside communities that disabled sending.
pub fn can_delete_attachment(store: &Store, m_author: Snowflake, channel: Snowflake) -> bool {
    let guild = store
        .channels
        .get(&channel)
        .and_then(|c| c.guild_id)
        .and_then(|g| store.guilds.get(&g))
        .and_then(|g| g.info.as_ref());
    m_author == store.my_id()
        && !guild.is_some_and(|g| {
            g.disabled_operations
                .contains(reflux_model::DisabledOperations::SEND_MESSAGE)
        })
}

/// `useDeleteAttachment`: Shift deletes at once, otherwise the confirmation opens.
fn request_delete(st: &mut ShellState, target: (Snowflake, Snowflake, Snowflake)) {
    if st.shift_held {
        delete_attachment(st, target);
    } else {
        st.media_ui.confirm_delete = Some(target);
    }
}

fn delete_attachment(st: &mut ShellState, (c, m, a): (Snowflake, Snowflake, Snowflake)) {
    st.media_ui.confirm_delete = None;
    super::expr::send(
        st,
        reqwest::Method::DELETE,
        format!("/channels/{c}/messages/{m}/attachments/{a}"),
        None,
    );
}

/// The "Delete attachment" `ConfirmModal`.
pub fn delete_modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    use crate::ui::{Button, ButtonVariant, Modal};
    let target = st.media_ui.confirm_delete?;
    let close = move |s: &mut S| acc(s).media_ui.confirm_delete = None;
    let body = text(crate::i18n::t("Are you sure you want to delete this attachment? This action cannot be undone and will remove the attachment from this message."))
        .font(font(14.0, 400.0).lh(20.0))
        .color(p.text_primary_muted)
        .wrap();
    let footer = row()
        .gap(8.0)
        .child(
            Button::new(crate::i18n::t("Cancel"))
                .variant(ButtonVariant::Secondary)
                .on_press(close)
                .el(p),
        )
        .child(
            Button::new(crate::i18n::t("Delete attachment"))
                .variant(ButtonVariant::Danger)
                .key("attachment-delete")
                .on_press(move |s: &mut S| delete_attachment(acc(s), target))
                .el(p),
        );
    Some(
        Modal::new(crate::i18n::t("Delete attachment"), close)
            .body(body)
            .footer(footer)
            .el(p),
    )
}

/// `MediaContainer`'s hover toolbar: Delete (own messages), Download, then the saved media star
/// (`useMediaFavorite`: a saved one is removed at once, otherwise `AddFavoriteMemeModal` opens).
/// Hidden on media under 110×50 (`shouldShowOverlays`).
fn media_tools<S: 'static>(
    st: &ShellState,
    t: &Tile,
    p: &Palette,
    acc: ShellAccess<S>,
) -> Option<El> {
    use super::expr::{Fetched, Tool, hover_tools, tool};
    let (channel, message, a) = t.saved?;
    if t.size.0 < 110.0 || t.size.1 < 50.0 {
        return None;
    }
    let saved = match (&a.content_hash, st.expr.fetched.get("memes")) {
        (Some(h), Some(Fetched::Done(v))) => v.as_array().and_then(|l| l.iter().find(|m| m["content_hash"].as_str() == Some(h.as_str()))).and_then(|m| m["id"].as_str()).map(str::to_owned),
        _ => None,
    };
    let (url, name) = (a.url.clone().unwrap_or_default(), a.filename.clone());
    let what = if t.video { "Download video" } else if t.animated { "Download GIF" } else { "Download image" };
    let download = tool(&icons::DOWNLOAD_SIMPLE_FILL, crate::i18n::t(what), 20.0, Tool::Plain, p, move |_: &mut S, tx: &mut Tx| save_as(acc, url.clone(), name.clone(), tx));
    let att = a.clone();
    let star = match saved {
        Some(id) => tool(&icons::STAR_FILL, crate::i18n::t("Remove from favorites"), 20.0, Tool::Active, p, move |s: &mut S, _: &mut Tx| {
            let st = acc(s);
            if let Some(Fetched::Done(serde_json::Value::Array(l))) = st.expr.fetched.get_mut("memes") {
                l.retain(|m| m["id"].as_str() != Some(id.as_str()));
            }
            st.social(reqwest::Method::DELETE, format!("/users/@me/memes/{id}"), None, crate::social::Done::Toast(crate::i18n::t("Removed from saved media").into()));
        }),
        None => tool(&icons::STAR_BOLD, crate::i18n::t("Add to favorites"), 20.0, Tool::Plain, p, move |s: &mut S, _: &mut Tx| crate::social::open_add_meme(acc(s), channel, message, &att)),
    };
    let mut tools = Vec::new();
    if crate::look::with(|l| l.media_favorite_button) {
        tools.push(star);
    }
    if crate::look::with(|l| l.media_download_button) {
        tools.insert(0, download);
    }
    if t.deletable && crate::look::with(|l| l.media_delete_button) {
        let target = (channel, message, a.id);
        tools.insert(
            0,
            tool(
                &icons::TRASH_BOLD,
                crate::i18n::t("Delete"),
                20.0,
                Tool::Danger,
                p,
                move |s: &mut S, _: &mut Tx| request_delete(acc(s), target),
            ),
        );
    }
    (!tools.is_empty()).then(|| hover_tools(tools, p))
}

/// Starts `clip` as the one clip playing inline (pausing the last).
pub fn play(st: &mut ShellState, key: String, clip: Player) {
    if let Some((_, old)) = st.media_ui.playing.take() {
        old.pause();
    }
    clip.play();
    st.media_ui.playing = Some((key, clip));
}

/// `m:ss` (`h:mm:ss` past an hour).
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// Small white HUD button (`VideoPlayer` controls).
fn hud_button<S: 'static>(glyph: &'static rune::Icon, label: &str, f: impl Fn(&mut S) + 'static) -> El {
    row()
        .size(32.0, 32.0)
        .center()
        .radius(6.0)
        .hover(rgba(255, 255, 255, 0.14))
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_owned())
        .child(icon(glyph, 20.0).color(Color::WHITE))
        .on_press(f)
}

/// A video at `size`: the clip, and Fluxer's control bar (play, time, seek, mute, and full
/// screen when `open` opens the media viewer). Platforms without a player offer the browser.
#[allow(clippy::too_many_arguments)]
pub fn video_player<S: 'static>(clip: &Player, key: &str, url: &str, size: (f32, f32), p: &Palette, acc: ShellAccess<S>, open: Option<super::clip::Opener<S>>) -> El {
    let (w, h) = size;
    let frame = el().size(w, h).radius(dim::MEDIA_BORDER_RADIUS).clip_rounded(dim::MEDIA_BORDER_RADIUS).fill(Color::BLACK);
    if let PlayerState::Failed(err) = clip.state() {
        let url = url.to_owned();
        let open_btn = row()
            .py(8.0)
            .px(12.0)
            .gap(6.0)
            .radius(6.0)
            .fill(rgba(255, 255, 255, 0.14))
            .hover(rgba(255, 255, 255, 0.22))
            .cursor(rune::Cursor::Pointer)
            .child(icon(&icons::ARROW_SQUARE_OUT_FILL, 16.0).color(Color::WHITE))
            .child(text(crate::i18n::t("Open in browser")).font(font(13.0, 600.0).lh(16.0)).color(Color::WHITE))
            .on_press_tx(move |_: &mut S, tx: &mut Tx| {
                let _ = tx.open_url(&url);
            });
        return frame.child(
            col()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .center()
                .gap(12.0)
                .padding(16.0)
                .key("failed")
                .child(text(err).font(font(13.0, 500.0).lh(18.0)).color(rgba(255, 255, 255, 0.8)).wrap().align(rune::Align::Center))
                .child(open_btn),
        );
    }
    let playing = clip.state() == PlayerState::Playing;
    let dur = clip.duration().unwrap_or(0.0);
    let (c1, c2, c3) = (clip.clone(), clip.clone(), clip.clone());
    let seek = crate::ui::Slider::new(format!("{key}-seek"), clip.time() as f32, 0.0, dur.max(0.01) as f32)
        .mini()
        .on_change(move |_: &mut S, v| c2.seek(v as f64))
        .el(p);
    let mut bar = row()
        .pin(Some(0.0), None, Some(0.0), Some(0.0))
        .h(40.0)
        .px(4.0)
        .gap(4.0)
        .center_y()
        .fill(rgba(0, 0, 0, 0.6))
        .child(hud_button(if playing { &icons::PAUSE_FILL } else { &icons::PLAY_FILL }, if playing { crate::i18n::t("Pause") } else { crate::i18n::t("Play") }, move |_: &mut S| {
            if c1.state() == PlayerState::Playing { c1.pause() } else { c1.play() }
        }))
        .child(text(format!("{} / {}", clock(clip.time()), clock(dur))).font(font(12.0, 500.0).mono().lh(16.0)).color(Color::WHITE))
        .child(el().grow(1.0).min_w(0.0).px(8.0).child(seek))
        .child(hud_button(if clip.muted() { &icons::SPEAKER_X_FILL } else { &icons::SPEAKER_HIGH_FILL }, crate::i18n::t("Mute"), move |_: &mut S| c3.set_muted(!c3.muted())));
    if let Some(open) = open {
        bar = bar.child(hud_button(&icons::CORNERS_OUT_FILL, "Full screen", move |s: &mut S| {
            acc(s).media_ui.playing = None;
            open(s)
        }));
    }
    frame.child(el().w_full().h_full().video(clip, rune::SurfaceFit::Fit).key("video")).child(bar)
}

// ---- Audio ---------------------------------------------------------------------------------------

/// The file name and its extension apart (`splitFilename`).
fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    }
}

fn name_line(name: &str, size: f32, weight: f32, lh: f32, p: &Palette) -> El {
    let (stem, ext) = split_name(name);
    let line = format!("{stem}{ext}");
    let spans = vec![rune::text::Span::new(stem.len(), line.len()).color(p.text_tertiary)];
    rich(line, spans).font(font(size, weight).lh(lh)).color(p.text_primary).truncate()
}

/// `InlineAudioPlayer`: play button, name and size, then the seek bar and time.
pub fn audio_player<S: 'static>(st: &ShellState, key: &str, a: &Attachment, p: &Palette, acc: ShellAccess<S>) -> El {
    let key = key.to_owned();
    let clip = st.media_ui.playing.as_ref().filter(|(k, _)| *k == key).map(|(_, c)| c.clone());
    let gst = super::clip::get(st, &key);
    let playing = clip.as_ref().is_some_and(|c| c.state() == PlayerState::Playing) || gst.is_some_and(|c| c.player.playing);
    let failed = clip.as_ref().and_then(|c| if let PlayerState::Failed(e) = c.state() { Some(e) } else { None }).or(gst.filter(|c| c.failed).map(|_| "This file can't be played.".to_string()));
    let url = a.url.clone().unwrap_or_default();
    let toggle = {
        let (key, url, clip, has_gst) = (key.clone(), url.clone(), clip.clone(), gst.is_some());
        move |s: &mut S| match &clip {
            Some(c) if c.state() == PlayerState::Playing => c.pause(),
            Some(c) => c.play(),
            None if has_gst => super::clip::toggle(acc, &key, s),
            None => super::clip::start(acc(s), key.clone(), url.clone(), true),
        }
    };
    let button = row()
        .size(36.0, 36.0)
        .center()
        .radius(18.0)
        .fill(p.brand_primary)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(if playing { t("Pause") } else { t("Play") })
        .child(icon(if playing { &icons::PAUSE_FILL } else { &icons::PLAY_FILL }, 16.0).color(p.text_on_brand_primary))
        .on_press(toggle);
    let size = super::upload::format_size(a.size);
    let info = col().grow(1.0).min_w(0.0).gap(2.0).child(name_line(a.title.as_deref().unwrap_or(&a.filename), 14.0, 500.0, 18.2, p)).child(text(size).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary));
    let dur = clip.as_ref().and_then(|c| c.duration()).or(gst.and_then(|c| c.player.duration())).or(a.duration).unwrap_or(0.0);
    let now = clip.as_ref().map(|c| c.time()).or(gst.and_then(|c| c.player.position())).unwrap_or(0.0);
    let seek = {
        let (clip, k, has_gst) = (clip.clone(), key.clone(), gst.is_some());
        crate::ui::Slider::new(format!("{key}-seek"), now as f32, 0.0, dur.max(0.01) as f32)
            .mini()
            .disabled(clip.is_none() && !has_gst)
            .on_change(move |s: &mut S, v| match &clip {
                Some(c) => c.seek(v as f64),
                None => super::clip::seek(acc, &k, s, v as f64),
            })
            .el(p)
    };
    let mut c = col()
        // Narrows with the chat (`width: 100%`), as master's file card fix found live.
        .w_full()
        .max_w(400.0)
        .min_w(0.0)
        .padding(12.0)
        .gap(8.0)
        .radius(8.0)
        .fill(p.background_secondary)
        .border(1.0, over(p.background_modifier_accent, p.background_secondary))
        .child(row().w_full().gap(12.0).center_y().child(button).child(info).child(download_button(a, 32.0, p, acc)))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(el().grow(1.0).min_w(120.0).child(seek))
                .child(text(format!("{} / {}", clock(now), clock(dur))).font(font(11.0, 400.0).mono().lh(11.0)).color(p.text_tertiary).min_w(70.0).align(rune::Align::End)),
        );
    if let Some(e) = failed {
        c = c.child(text(e).font(font(12.0, 400.0).lh(16.0)).color(p.status_danger).wrap());
    }
    c
}

// ---- Files -------------------------------------------------------------------------------------------

/// `getFileTypeIcon` by extension.
fn file_icon(name: &str) -> &'static rune::Icon {
    let ext = name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "bmp" | "ico" => &icons::FILE_IMAGE_FILL,
        "pdf" => &icons::FILE_PDF_FILL,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" => &icons::FILE_ARCHIVE_FILL,
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "wma" => &icons::FILE_AUDIO_FILL,
        "mp4" | "webm" | "mov" | "avi" | "mkv" | "flv" | "wmv" => &icons::FILE_VIDEO_FILL,
        "js" | "jsx" | "ts" | "tsx" | "py" | "java" | "c" | "cpp" | "h" | "css" | "html" | "json" | "xml" | "yml" | "yaml" | "sh" | "go" | "rs" | "rb" | "php" => &icons::FILE_CODE_FILL,
        "xls" | "xlsx" | "csv" => &icons::FILE_XLS_FILL,
        "ppt" | "pptx" => &icons::FILE_PPT_FILL,
        _ => &icons::FILE_TEXT_FILL,
    }
}

/// Asks where to save `url` (named `name`), then downloads it there (`FileDownloads`).
pub fn save_as<S: 'static>(acc: ShellAccess<S>, url: String, name: String, tx: &mut Tx) {
    let options = FileDialogOptions { title: "Save file".into(), file_name: Some(name), ..Default::default() };
    tx.save_file(options, move |s: &mut S, r, _| match r {
        Ok(Some(path)) => acc(s).media.download(url, path),
        Ok(None) => {}
        Err(e) => eprintln!("reflux: save dialog: {e}"),
    });
}

/// `.downloadButton`: brand square with the download arrow; disabled once expired.
fn download_button<S: 'static>(a: &Attachment, size: f32, p: &Palette, acc: ShellAccess<S>) -> El {
    let b = row().size(size, size).center().radius(8.0);
    let url = a.proxy_url.clone().or_else(|| a.url.clone());
    match url.filter(|_| !a.expired) {
        None => {
            let b = b.fill(p.brand_primary).opacity(0.4);
            if crate::look::with(|l| l.media_expiry_indicator) {
                b.label(t("Attachment expired")).child(icon(&icons::WARNING_CIRCLE_BOLD, 20.0).color(p.text_on_brand_primary))
            } else {
                b.child(icon(&icons::DOWNLOAD_SIMPLE_BOLD, 20.0).color(p.text_on_brand_primary))
            }
        }
        Some(url) => {
            let name = a.filename.clone();
            b.fill(p.brand_primary)
                .hover(p.brand_secondary)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Download"))
                .child(icon(&icons::DOWNLOAD_SIMPLE_BOLD, 20.0).color(p.text_on_brand_primary))
                .on_press_tx(move |_: &mut S, tx: &mut Tx| save_as(acc, url.clone(), name.clone(), tx))
        }
    }
}

/// `AttachmentFile`: type icon, name (extension dimmed), size, download.
fn file_card<S: 'static>(a: &Attachment, p: &Palette, acc: ShellAccess<S>) -> El {
    let name = a.title.clone().unwrap_or_else(|| a.filename.clone());
    row()
        // Narrows with the chat (`width: 100%`), as master's file card fix found live.
        .w_full()
        .max_w(400.0)
        .min_w(0.0)
        .padding(12.0)
        .gap(12.0)
        .center_y()
        .radius(8.0)
        .fill(p.background_secondary)
        .border(1.0, over(p.background_modifier_accent, p.background_secondary))
        .child(row().size(48.0, 48.0).center().radius(8.0).fill(p.background_tertiary).child(icon(file_icon(&name), 32.0).color(p.text_tertiary)))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .child(name_line(&name, 14.0, 600.0, 20.0, p))
                .child(text(super::upload::format_size(a.size)).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary)),
        )
        .child(download_button(a, 40.0, p, acc))
        .role("group")
}

/// `isTextualAttachment` + `shouldPreviewAttachment` (128 KiB).
fn is_textual(a: &Attachment) -> bool {
    const EXT: [&str; 40] = [
        "txt", "md", "markdown", "log", "json", "js", "jsx", "ts", "tsx", "py", "java", "c", "cpp", "h", "cs", "css", "scss", "sass", "html", "htm", "xml", "yml", "yaml", "sh", "bash",
        "ps1", "go", "rb", "php", "rust", "dockerfile", "sql", "ini", "cfg", "conf", "csv", "rs", "toml", "kt", "swift",
    ];
    let t = a.content_type.to_ascii_lowercase();
    let t = t.split(';').next().unwrap_or_default().trim();
    let mime = ["text/", "application/json", "application/ld+json", "application/xml", "application/javascript"].iter().any(|m| t.starts_with(m))
        || ["application/x-sh", "application/x-shellscript", "application/x-bash", "application/sql", "application/x-yaml", "package/manifest+json"].contains(&t);
    let ext = a.filename.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    a.url.is_some() && !a.expired && a.size <= 128 * 1024 && (mime || (a.filename.contains('.') && EXT.contains(&ext.as_str())))
}

/// `TextualAttachmentPreview`: six highlighted lines (up to 100 expanded), then a footer with
/// the name, size, expand and download.
fn text_preview<S: 'static>(st: &ShellState, a: &Attachment, p: &Palette, acc: ShellAccess<S>) -> El {
    let key = format!("text-{}", a.id);
    let open = st.media_ui.expanded.contains(&key);
    let url = a.url.clone().unwrap_or_default();
    let lines = if open { 100 } else { 6 };
    let (lh, pad) = (16.0, 10.0);
    let body = match st.media.text(&url) {
        None => row().w_full().h(6.0 * lh).center().key("loading").child(crate::ui::spinner(p.text_tertiary)),
        Some(Err(_)) => row()
            .w_full()
            .gap(8.0)
            .key("failed")
            .child(icon(&icons::WARNING_CIRCLE_BOLD, 16.0).color(p.status_danger))
            .child(text("Failed to load preview").font(font(13.0, 400.0).lh(18.0)).color(p.status_danger)),
        Some(Ok(t)) => {
            let shown: String = t.split_inclusive('\n').take(lines).collect();
            let shown = shown.trim_end_matches('\n').to_owned();
            let ext = a.filename.rsplit('.').next().unwrap_or_default();
            let spans = super::highlight::spans(ext, &shown, crate::ui::is_light(p));
            rich(shown, spans).font(font(12.0, 400.0).mono().lh(lh)).color(p.text_code).selectable(true).key("text")
        }
    };
    let surface = col().w_full().min_h(6.0 * lh + 2.0 * pad + 8.0).max_h(500.0).padding(pad).clip_rounded(0.0).child(body);
    let k2 = key.clone();
    let expand = row()
        .size(32.0, 32.0)
        .center()
        .radius(10.0)
        .fill(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(if open { t("Collapse") } else { t("Expand") })
        .child(icon(if open { &icons::CARET_UP_BOLD } else { &icons::CARET_DOWN_BOLD }, 16.0).color(p.text_secondary))
        .on_press(move |s: &mut S| {
            let e = &mut acc(s).media_ui.expanded;
            if !e.remove(&k2) {
                e.insert(k2.clone());
            }
        });
    let footer = row()
        .w_full()
        .py(8.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .fill(p.background_secondary)
        .child(expand)
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.8)
                .child(text(a.filename.clone()).font(font(13.0, 500.0).lh(16.25)).color(p.text_primary).truncate())
                .child(text(super::upload::format_size(a.size)).font(font(12.0, 400.0).lh(15.0)).color(p.text_tertiary)),
        )
        .child(download_button(a, 32.0, p, acc));
    col()
        .w_full()
        .max_w(550.0)
        .radius(6.0)
        .clip_rounded(6.0)
        .fill(p.background_secondary)
        .border(1.0, over(p.code_block_border, p.background_secondary))
        .child(surface)
        .child(el().w_full().h(1.0).fill(over(p.background_modifier_accent, p.background_secondary)))
        .child(footer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_scale_media_down_to_fit() {
        assert_eq!(capped((550.0, 309.0)), (550.0, 309.0));
        assert_eq!(with_max_w(275.0, || capped((550.0, 310.0))), (275.0, 155.0));
        assert_eq!(with_max_w(275.0, || capped((200.0, 100.0))), (200.0, 100.0));
        // Restored afterwards.
        assert_eq!(capped((550.0, 309.0)), (550.0, 309.0));
    }

    #[test]
    fn region_names_like_user_geo() {
        assert_eq!(super::region_display_name(None, None), "your region");
        assert_eq!(super::region_display_name(Some("GB"), Some("ENG")), "United Kingdom");
        assert_eq!(super::region_display_name(Some("US"), Some("TX")), "Texas, United States");
        assert_eq!(super::region_display_name(Some("US"), Some("ZZ")), "ZZ, United States");
        assert_eq!(crate::i18n::region_name_in("ko", "kr").as_deref(), Some("대한민국"));
        assert_eq!(crate::i18n::region_name_in("ko", "123"), None);
    }

    #[test]
    fn media_menu_kinds_and_expiry() {
        let a = |ty: &str| Attachment { filename: "f".into(), content_type: ty.into(), url: Some("https://x/f".into()), ..Default::default() };
        assert_eq!(MenuMedia::of(&a("image/png")).map(|m| m.kind), Some(MediaKind::Image));
        assert_eq!(MenuMedia::of(&a("image/gif")).map(|m| m.kind), Some(MediaKind::Gif));
        assert_eq!(MenuMedia::of(&a("video/mp4")).map(|m| m.kind), Some(MediaKind::Video));
        assert_eq!(MenuMedia::of(&a("image/png")).map(|m| m.proxy), Some("https://x/f".into()));
        assert_eq!(MenuMedia::of(&Attachment { expired: true, ..a("image/png") }), None);
        let e = MenuMedia::embed("https://x/y/cat.gif?w=1", None, false, true);
        assert_eq!((e.filename.as_str(), e.proxy.as_str(), e.kind), ("cat.gif", "https://x/y/cat.gif?w=1", MediaKind::Gif));
    }

    #[test]
    fn mosaic_tiles_follow_fluxer_slots() {
        // Two squares side by side: (550 - 4) / 2.
        assert_eq!(tile_box(2, 0, 550.0), (273.0, 273.0));
        // Three: the first spans two of three tracks and is as tall as the stacked pair.
        let (w0, h0) = tile_box(3, 0, 550.0);
        let (_, h1) = tile_box(3, 1, 550.0);
        assert_eq!(w0, h0);
        assert!((h1 * 2.0 + GAP - h0).abs() < 0.01);
        // Seven: a 16:9 hero over a 3×2 grid.
        assert_eq!(tile_box(7, 0, 550.0), (550.0, 550.0 * 9.0 / 16.0));
        assert_eq!(rows(10).iter().sum::<usize>(), 10);
        assert_eq!(rows(5), [2, 3]);
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(clock(65.4), "1:05");
        assert_eq!(split_name("a.tar.gz"), ("a.tar", ".gz"));
        assert_eq!(split_name(".env"), (".env", ""));
    }

    #[test]
    fn textual_attachments_need_a_url_and_a_text_type() {
        let a = |name: &str, ty: &str, size: u64| Attachment { filename: name.into(), content_type: ty.into(), size, url: Some("u".into()), ..Default::default() };
        assert!(is_textual(&a("notes.txt", "text/plain", 10)));
        assert!(is_textual(&a("main.rs", "application/octet-stream", 10)));
        assert!(!is_textual(&a("big.txt", "text/plain", 200 * 1024)));
        assert!(!is_textual(&a("x.bin", "application/octet-stream", 10)));
        assert!(!is_textual(&Attachment { url: None, ..a("n.txt", "text/plain", 1) }));
    }
}
