//! Threads (`features/threads`): the sidebar list under a parent (`SidebarThreadList`), the chip
//! under a starter message (`MessageThreadChip`), the side panel with its create pane
//! (`ThreadSidePanel`, `ThreadCreatePane`, `ThreadComposerArea`), the header's thread browser
//! (`ThreadBrowser`), the thread menu (`useThreadMenuData`) and settings (`ThreadSettingsModal`).
// shortcut: the panel isn't resizable (fixed 450, the default width), the browser has no name
// search, and the member list in the panel is a plain popout list.

use std::collections::HashMap;

use reflux_model::{Channel, ChannelType, Message, NotificationLevel, Snowflake, ThreadMemberFlags};
use reflux_store::Store;
use reflux_store::threads::{self as rules, CreateKind};
use rune::text::Span;
use rune::{Color, El, ScrollSpec, TextEdit, col, el, icon, rich, row, text};

use super::{Intent, ShellAccess, ShellState, avatar, format_relative, pops};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::overlay::{anchor, popout_shadow};
use crate::ui::{Button, ButtonSize, ButtonVariant, Input, Menu, MenuEntry, MenuItem, Modal, Placement, RadioGroup, Switch, mention_badge, over, scroller, spinner};
use crate::i18n::t;

/// The header's thread browser popout.
pub const BROWSER: &str = "header-threads";
/// The thread context menu (opened at the pointer) and the panel's "…" menu.
pub const MENU: &str = "thread-menu";
const MEMBERS: &str = "thread-members";
/// `THREAD_PANEL_DEFAULT_WIDTH`.
pub const PANEL_WIDTH: f32 = 450.0;
/// `THREAD_NAME_MAX_LENGTH`.
const NAME_MAX: usize = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tab {
    #[default]
    Active,
    Archived,
    ArchivedPrivate,
}

/// What the side panel shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Panel {
    Thread(Snowflake),
    /// The new-thread pane, from a message or standalone.
    Create { parent: Snowflake, message: Option<Snowflake> },
}

/// A loaded page list of archived threads.
#[derive(Clone, Debug, Default)]
pub struct Archived {
    pub ids: Vec<Snowflake>,
    pub has_more: bool,
    pub loading: bool,
    pub failed: bool,
}

/// The confirmation waiting on the user.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Confirm {
    Delete(Snowflake),
    LeavePrivate(Snowflake),
}

/// The settings modal's draft.
pub struct Settings {
    pub thread: Snowflake,
    pub name: TextEdit,
    pub auto_archive: u32,
    pub invitable: bool,
}

/// UI state of threads and forums.
pub struct Threads {
    pub panel: Option<Panel>,
    /// The create pane's name field, its error and the private toggle.
    pub name: TextEdit,
    pub name_error: Option<String>,
    pub private: bool,
    /// The panel's composer and its message list position.
    pub composer: TextEdit,
    pub uploads: Vec<super::upload::Upload>,
    pub at_bottom: bool,
    pub rows_below: usize,
    pub list_fits: bool,
    pub tab: Tab,
    pub archived: HashMap<(Snowflake, Tab), Archived>,
    /// The thread the open menu is about.
    pub menu: Option<Snowflake>,
    pub settings: Option<Settings>,
    pub confirm: Option<Confirm>,
    pub forum: super::forum::Forum,
}

impl Default for Threads {
    fn default() -> Self {
        Self {
            panel: None,
            name: TextEdit::new(""),
            name_error: None,
            private: false,
            composer: TextEdit::new(""),
            uploads: Vec::new(),
            at_bottom: true,
            rows_below: 0,
            list_fits: true,
            tab: Tab::Active,
            archived: HashMap::new(),
            menu: None,
            settings: None,
            confirm: None,
            forum: Default::default(),
        }
    }
}

/// Thread requests the runtime runs (`app/threads.rs`).
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// The panel opened a thread: load its messages and subscribe to its member list.
    Open(Snowflake),
    /// Start a thread (from `message`, or standalone) and send `content` as its first message.
    Create { parent: Snowflake, message: Option<Snowflake>, name: String, private: bool, content: String },
    /// A forum post: the thread and its first message in one request.
    Post { forum: Snowflake, name: String, content: String, tags: Vec<Snowflake>, attachments: Vec<super::upload::Upload> },
    /// `PATCH /channels/{id}` (threads and forum channels).
    Patch { channel: Snowflake, body: serde_json::Value },
    Delete(Snowflake),
    Join(Snowflake),
    Leave(Snowflake),
    /// `PATCH /channels/{id}/thread-members/@me/settings`.
    MemberSettings { thread: Snowflake, body: serde_json::Value },
    /// A page of archived threads for the browser (or a forum's older posts).
    LoadArchived { parent: Snowflake, tab: Tab, more: bool },
    /// The first messages of forum posts (`post-data`).
    PostData { forum: Snowflake, ids: Vec<Snowflake> },
}

fn op(st: &mut ShellState, o: Op) {
    st.intents.push(Intent::Thread(o));
}

// ---- Navigation --------------------------------------------------------------------------------

/// Opens a thread in the side panel next to its parent (`openThread`).
pub fn open(st: &mut ShellState, store_parent: Option<Snowflake>, thread: Snowflake) {
    if st.threads.panel != Some(Panel::Thread(thread)) {
        reset_pane(&mut st.threads);
    }
    // The panel shows next to its parent; open the parent if needed.
    if let Some(parent) = store_parent.filter(|p| st.channel != Some(*p)) {
        st.select_channel(parent);
    }
    st.threads.panel = Some(Panel::Thread(thread));
    st.popups.close();
    op(st, Op::Open(thread));
}

/// The thread as the main view (`openThreadFullView`).
pub fn open_full(st: &mut ShellState, thread: Snowflake) {
    st.threads.panel = None;
    st.popups.close();
    st.select_channel(thread);
}

/// The new-thread pane for a parent, from a message or standalone (`openCreateThread`).
pub fn start_create(st: &mut ShellState, parent: Snowflake, message: Option<Snowflake>) {
    reset_pane(&mut st.threads);
    (st.threads.name, st.threads.name_error, st.threads.private) = (TextEdit::new(""), None, false);
    st.threads.name.focused = true;
    st.composer.focused = false;
    st.threads.panel = Some(Panel::Create { parent, message });
    st.popups.close();
}

pub fn close(st: &mut ShellState) {
    st.threads.panel = None;
}

fn reset_pane(t: &mut Threads) {
    t.composer = TextEdit::new("");
    t.uploads.clear();
    (t.at_bottom, t.rows_below, t.list_fits) = (true, 0, true);
}

/// Opens a thread the way its trigger asks: Shift (or a thread that isn't under the open
/// channel) opens it in full view (`openThreadFromEvent`).
fn open_from_event(st: &mut ShellState, t: &Channel, full: bool) {
    if full {
        open_full(st, t.id);
    } else {
        open(st, t.parent_id, t.id);
    }
}

/// Enter in the create pane: names the thread (the source message text when left empty) and
/// starts it with the composer's text.
pub(super) fn submit_create(st: &mut ShellState, suggested: &str) {
    let Some(Panel::Create { parent, message }) = st.threads.panel else { return };
    let typed = st.threads.name.text.trim();
    let name: String = if typed.is_empty() { suggested } else { typed }.chars().take(NAME_MAX).collect();
    if name.is_empty() {
        st.threads.name_error = Some(t("Give the thread a name before you send.").into());
        return;
    }
    let content = super::autocomplete::apply_command(crate::compose::serialize(&st.threads.composer).trim());
    let private = st.threads.private && message.is_none();
    op(st, Op::Create { parent, message, name, private, content });
    st.threads.composer.clear();
    st.threads.name = TextEdit::new("");
}

/// The text a new thread from `m` is named after by default (its content on one line).
/// Markdown is rendered to plaintext first, like `ThreadCreatePane.tsx`.
pub(super) fn suggested_name(store: &Store, guild: Option<Snowflake>, m: Option<&Message>) -> String {
    let Some(m) = m.filter(|m| !m.content.is_empty()) else { return String::new() };
    let mut o = reflux_markdown::PlaintextOptions::default();
    for u in &m.mentions {
        o.users.insert(u.id.to_string(), store.display_name(guild, u.id));
    }
    for c in &m.mention_channels {
        o.channels.insert(c.id.to_string(), c.name.clone());
    }
    let nodes = reflux_markdown::parse(&m.content, &reflux_markdown::Options { flags: reflux_markdown::presets::STANDARD, emoji: super::emoji::context(&m.content) });
    let t = reflux_markdown::render_ast_to_plaintext(&nodes, &o);
    t.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(NAME_MAX).collect()
}

// ---- Sidebar -----------------------------------------------------------------------------------

/// The joined (and open) threads under a channel row, joined to it by a spine
/// (`SidebarThreadList.module.css`).
pub fn sidebar_rows<S: 'static>(store: &Store, st: &ShellState, parent: &Channel, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let selected = st.channel.filter(|c| store.channels.get(c).is_some_and(|c| c.kind.is_thread()));
    let threads = store.sidebar_threads(parent.id, selected);
    if threads.is_empty() {
        return None;
    }
    let n = threads.len();
    let spine_c = p.interactive_muted;
    let mut list = col().w_full().pt(1.0);
    for (i, t) in threads.into_iter().enumerate() {
        let is_sel = selected == Some(t.id);
        let muted = store.is_thread_muted(t);
        let mentions = store.mention_count(t.id);
        let unread = t.member.is_some() && !muted && store.has_unread(t.id);
        let fg = if is_sel {
            p.surface_interactive_selected_color
        } else if unread || mentions > 0 {
            p.text_secondary
        } else {
            p.text_tertiary_muted
        };
        let id = t.id;
        let mut item = row()
            .grow(1.0)
            .min_w(0.0)
            .ml(8.0)
            .pl(6.0)
            .pr(8.0)
            .py(6.0)
            .gap(6.0)
            .center_y()
            .radius(6.0)
            .cursor(rune::Cursor::Pointer)
            .role("link")
            .label(t.name.clone().unwrap_or_default())
            .hover_group()
            .child(text(t.name.clone().unwrap_or_default()).font(font(14.0, 500.0).lh(20.0)).color(fg).group_color(if is_sel { fg } else { p.text_chat }).truncate().grow(1.0).shrink())
            .on_press(move |s: &mut S| open_full(acc(s), id))
            .on_secondary_press(move |s: &mut S, at, _| open_menu(acc(s), id, [at.x, at.y]));
        if let Some(b) = mention_badge(mentions as u64, true, p) {
            item = item.child(b);
        }
        item = if is_sel { item.fill(p.surface_interactive_selected_bg) } else { item.hover(p.surface_interactive_hover_bg) };
        if muted && !is_sel {
            item = item.opacity(0.5);
        }
        // `.spine`: 2px left and bottom border, 6px reach, curving into the row's middle.
        let h = 1.0 + 32.0;
        let spine_h = h - (16.0 - 1.0);
        let spine = el()
            .absolute(25.0, 0.0)
            .size(8.0, spine_h)
            .clip_rounded(0.0)
            .inert()
            .child(el().absolute(0.0, -10.0).size(18.0, spine_h + 10.0).corners([0.0, 0.0, 0.0, 6.0]).border(2.0, spine_c));
        let mut r = row().w_full().pt(1.0).pl(28.0).child(spine);
        if i + 1 < n {
            r = r.child(el().absolute(25.0, 0.0).size(2.0, h).fill(spine_c).inert());
        }
        if unread && !is_sel {
            r = r.child(el().absolute(-4.0, 1.0 + 16.0 - 4.0).size(8.0, 8.0).corners([0.0, 4.0, 4.0, 0.0]).fill(p.text_primary));
        }
        list = list.child(r.child(item).key(format!("thread-{id}")));
    }
    Some(list)
}

// ---- Message chip and system messages -------------------------------------------------------------

/// `MessageThreadChip`: under a message that started a thread, its name, message count and the
/// newest message.
pub(super) fn chip<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let t = store.thread(m.id).filter(|t| t.parent_id == Some(m.channel_id))?;
    let last = t.last_message_id.and_then(|l| store.messages(t.id).iter().find(|x| x.id == l));
    let preview = match last {
        Some(l) => Some(format!("{}: {}", store.display_name(t.guild_id, l.author.id), notification_body(l))),
        None if t.message_count == 0 => Some(crate::i18n::t("There are no recent messages in this thread.").to_string()),
        None => None,
    };
    let (thread, id) = (t.clone(), t.id);
    let mut c = col()
        .max_w(480.0)
        .mt(4.0)
        .py(8.0)
        .px(12.0)
        .gap(4.0)
        .radius(8.0)
        .fill(p.background_secondary_alt)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .key(format!("thread-chip-{id}"))
        .child(
            row()
                .gap(8.0)
                .min_w(0.0)
                .center_y()
                .child(text(t.name.clone().unwrap_or_default()).font(font(14.0, 600.0).lh(18.0)).color(p.text_primary).truncate().shrink())
                .child(
                    row()
                        .gap(2.0)
                        .center_y()
                        .child(text(message_count(t.message_count)).font(font(14.0, 600.0).lh(18.0)).color(p.brand_primary_light))
                        .child(icon(&icons::CARET_RIGHT_BOLD, 12.0).color(p.brand_primary_light)),
                ),
        )
        .on_press_tx(move |s: &mut S, tx| open_from_event(acc(s), &thread, tx.mods().shift))
        .on_secondary_press(move |s: &mut S, at, _| open_menu(acc(s), id, [at.x, at.y]));
    if let Some(pv) = preview {
        c = c.child(text(pv).font(font(13.0, 400.0).lh(16.0)).color(p.text_primary_muted).truncate());
    }
    let _ = st;
    Some(c)
}

/// "{n} messages" (`MESSAGE_COUNT_DESCRIPTOR`).
pub(super) fn message_count(n: u32) -> String {
    if n == 1 { "1 message".into() } else { format!("{n} messages") }
}

/// A one-line message summary (`buildMessageNotificationBody`).
// shortcut: content as typed (no markdown stripping); attachment-only messages say so.
pub(super) fn notification_body(m: &Message) -> String {
    if !m.content.is_empty() {
        return m.content.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    match m.attachments.len() {
        0 => String::new(),
        1 => t("Sent an attachment").into(),
        n => format!("Sent {n} attachments"),
    }
}

/// `ThreadCreatedMessage`: "{name} started a thread {thread}. See all threads."
// shortcut: the punctuation around the links is approximated.
pub(super) fn created_message<S: 'static>(store: &Store, m: &Message, name: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let thread = m.message_reference.as_ref().and_then(|r| store.thread(r.channel_id)).cloned();
    let tname = thread.as_ref().and_then(|t| t.name.clone()).unwrap_or_else(|| m.content.clone());
    let line = format!("{name} started a thread {tname}. See all threads.");
    let (a, b) = (name.len() + " started a thread ".len(), name.len() + " started a thread ".len() + tname.len());
    let see = (b + 2, line.len() - 1);
    let spans = vec![
        Span::new(0, name.len()).weight(500.0).color(p.text_primary),
        Span::new(a, b).weight(600.0).color(p.text_primary),
        Span::new(see.0, see.1).weight(500.0).color(p.text_primary),
    ];
    let body = rich(line, spans).font(font(16.0, 400.0).lh(dim::MESSAGE_LINE_HEIGHT)).color(p.text_chat_muted).wrap().grow(1.0).shrink();
    let open_t = thread.clone();
    let row_ = row()
        .w_full()
        .pr(dim::CHAT_HORIZONTAL_PADDING_DEFAULT + dim::MESSAGE_ACTION_BAR_OFFSET)
        .py(dim::MESSAGE_SPACING_Y)
        .items_start()
        .hover(p.background_modifier_hover)
        .child(el().w(dim::CHAT_HORIZONTAL_PADDING_DEFAULT))
        .child(row().w(dim::MESSAGE_AVATAR_SIZE).h(dim::MESSAGE_LINE_HEIGHT).justify_end().child(icon(&icons::CHATS_BOLD, dim::SYSTEM_MESSAGE_ICON_SIZE).color(p.text_primary_muted).opacity(0.6)))
        .child(el().w(dim::MESSAGE_GUTTER))
        .child(body);
    // shortcut: the whole line opens the thread (or the browser when it isn't loaded), rather
    // than the two links on their own.
    row_.cursor(rune::Cursor::Pointer).on_press_tx(move |s: &mut S, tx| {
        let st = acc(s);
        match &open_t {
            Some(t) => open_from_event(st, t, tx.mods().shift),
            None => browse(st),
        }
    })
}

/// `ThreadStarterMessage`: the parent message a thread started from, at the top of the thread.
pub(super) fn starter_message<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> El {
    let r = m.message_reference.as_ref();
    let loaded = r.and_then(|r| store.messages(r.channel_id).iter().find(|x| Some(x.id) == r.message_id));
    let starter = match (loaded, &m.referenced_message) {
        (Some(x), _) => Some(x.clone()),
        (None, Some(Some(x))) => Some((**x).clone()),
        _ => None,
    };
    match starter {
        Some(s) => {
            let md = super::markdown::Md::new(store, &st.media, store.channels.get(&s.channel_id).and_then(|c| c.guild_id), p, format!("starter-m{}", s.id));
            // `<Divider spacing={8}>` (ChannelDivider `.container`/`.line`) under the starter.
            let line = row()
                .w_full()
                .h(20.0)
                .my(8.0)
                .px(dim::CHAT_HORIZONTAL_PADDING_DEFAULT + 20.0)
                .center_y()
                .child(el().grow(1.0).h(2.0).fill(p.background_modifier_accent).opacity(0.3));
            col().w_full().child(col().w_full().inert().child(super::chat::message_preview(store, st, &s, &md, p, acc))).child(line)
        }
        None => row()
            .w_full()
            .py(8.0)
            .px(16.0)
            .gap(8.0)
            .center_y()
            .child(icon(&icons::TRASH, 16.0).color(p.text_tertiary))
            .child(text(t("Original message was deleted.")).font(font(14.0, 400.0).italic().lh(20.0)).color(p.text_tertiary)),
    }
}

// ---- Header -------------------------------------------------------------------------------------

/// `ChannelHeaderIcon`.
fn header_icon(i: &'static rune::Icon, label: &str, selected: bool, p: &Palette) -> El {
    row()
        .size(dim::CHANNEL_HEADER_ACTION_SIZE, dim::CHANNEL_HEADER_ACTION_SIZE)
        .center()
        .radius(dim::RADIUS_FULL)
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_string())
        .child(icon(i, dim::CHANNEL_HEADER_ACTION_ICON_SIZE).color(if selected { p.text_primary } else { p.text_primary_muted }).group_color(p.text_primary))
}

/// The channel whose threads the header's Threads button browses: a text or announcement
/// channel, or a thread's parent.
fn browser_parent<'a>(store: &'a Store, c: &'a Channel) -> Option<&'a Channel> {
    let parent = if c.kind.is_thread() { c.parent_id.and_then(|p| store.channels.get(&p))? } else { c };
    matches!(parent.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT).then_some(parent)
}

/// The header's Threads button (`ThreadsButton`), for text and announcement channels and threads.
pub(super) fn header_button<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    browser_parent(store, c)?;
    Some(header_icon(&icons::CHATS, t("Threads"), st.popups.is_open(BROWSER), p).key(BROWSER).on_press(move |s: &mut S| {
        let st = acc(s);
        st.popups.toggle(BROWSER);
        st.threads.tab = Tab::Active;
    }))
}

/// Opens the thread browser ("See all threads").
fn browse(st: &mut ShellState) {
    st.popups.open = Some(BROWSER.into());
    st.popups.at = None;
    st.threads.tab = Tab::Active;
}

/// `ThreadParentCrumb`: the parent's icon and name, then a caret, before a thread's own name.
pub(super) fn parent_crumb<S: 'static>(store: &Store, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let parent = c.parent_id.and_then(|id| store.channels.get(&id)).filter(|_| c.kind.is_thread())?;
    let id = parent.id;
    Some(
        row()
            .gap(6.0)
            .min_w(0.0)
            .max_w(320.0)
            .shrink()
            .center_y()
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .on_press(move |s: &mut S| acc(s).select_channel(id))
            .child(icon(super::channel_icon(parent.kind), dim::CHANNEL_HEADER_ICON_SIZE).color(p.text_secondary).group_color(p.text_primary))
            .child(text(parent.name.clone().unwrap_or_default()).font(font(16.0, 600.0).lh(24.0)).color(p.text_secondary).group_color(p.text_primary).truncate().shrink())
            .child(icon(&icons::CARET_RIGHT_BOLD, 14.0).color(p.text_tertiary).mx(4.0)),
    )
}

/// The thread header's bell (`ThreadNotificationSettingsButton`): the notification submenu.
pub(super) fn notification_button<S: 'static>(st: &ShellState, t: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let id = t.id;
    let open = st.popups.is_open(MENU) && st.threads.menu == Some(id) && st.popups.at.is_none();
    header_icon(&icons::BELL, "Notification Settings", open, p).key(MENU).on_press(move |s: &mut S| {
        let st = acc(s);
        st.threads.menu = Some(id);
        st.popups.at = None;
        st.popups.toggle(MENU);
    })
}

// ---- Side panel ---------------------------------------------------------------------------------

/// The side panel next to `parent`, if one is open for it (`ThreadSidePanel`).
pub(super) fn panel<S: 'static>(store: &Store, st: &ShellState, parent: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> Option<El> {
    let (title, body, thread) = match st.threads.panel? {
        Panel::Thread(id) => {
            let t = store.thread(id).filter(|t| t.parent_id == Some(parent.id))?;
            let mut body = col().w_full().grow(1.0).min_h(0.0).child(super::chat::messages(store, st, t, p, acc, layer, true));
            body = body.children(composer_area(store, st, t, true, p, acc));
            (t.name.clone().unwrap_or_default(), body, Some(t))
        }
        Panel::Create { parent: pid, message } if pid == parent.id => (t("New thread").to_string(), create_pane(store, st, parent, message, p, acc), None),
        Panel::Create { .. } => return None,
    };
    let forum_post = thread.is_some_and(|t| t.parent_id.and_then(|p| store.channels.get(&p)).is_some_and(|p| rules::is_forum(p.kind)));
    let mut actions = row().gap(dim::CHANNEL_HEADER_ACTIONS_GAP).center_y();
    if let Some(t) = thread {
        let id = t.id;
        if !forum_post {
            actions = actions
                .child(header_icon(&icons::USERS, crate::i18n::t("Thread members"), st.popups.is_open(MEMBERS), p).key(MEMBERS).on_press(move |s: &mut S| acc(s).popups.toggle(MEMBERS)))
                .child(header_icon(&icons::ARROWS_OUT_SIMPLE, crate::i18n::t("Open in full view"), false, p).on_press(move |s: &mut S| open_full(acc(s), id)));
        }
        actions = actions.child(header_icon(&icons::DOTS_THREE_BOLD, crate::i18n::t("Thread settings"), false, p).key(MENU).on_press(move |s: &mut S| {
            let st = acc(s);
            st.threads.menu = Some(id);
            st.popups.at = None;
            st.popups.open = Some(MENU.into());
        }));
        if st.popups.is_open(MEMBERS) {
            layer.push(members_popout(store, st, t, p, acc));
        }
    }
    actions = actions.child(header_icon(&icons::X_BOLD, t("Close"), false, p).on_press(move |s: &mut S| close(acc(s))));
    let divider = over(p.user_area_divider_color, p.background_secondary_lighter);
    let mut header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::CHANNEL_HEADER_PADDING_INLINE)
        .gap(dim::CHANNEL_HEADER_NAME_GAP)
        .center_y();
    if !forum_post {
        header = header.child(icon(&icons::CHATS, dim::CHANNEL_HEADER_ICON_SIZE).color(p.text_primary_muted));
    }
    header = header
        .child(text(title).font(font(16.0, 500.0).lh(24.0)).color(p.text_primary).truncate().grow(1.0).shrink())
        .child(actions)
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(divider));
    Some(
        col()
            .w(PANEL_WIDTH)
            .h_full()
            .min_h(0.0)
            .fill(p.background_secondary_lighter)
            .key("thread-panel")
            .child(header)
            .child(body)
            .child(el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w(1.0).fill(divider)),
    )
}

/// The composer of a thread with its notices (`ThreadComposerArea`): locked threads only take
/// moderators; closed ones say a message opens them; unjoined ones offer Join.
pub(super) fn composer_area<S: 'static>(store: &Store, st: &ShellState, t: &Channel, pane: bool, p: &Palette, acc: ShellAccess<S>) -> Vec<El> {
    let r = store.thread_rights(t);
    let notice = |glyph: &'static rune::Icon, line: &str, button: Option<El>| {
        row()
            .mx(16.0)
            .mb(8.0)
            .py(10.0)
            .px(12.0)
            .gap(12.0)
            .center_y()
            .radius(8.0)
            .fill(p.background_secondary_alt)
            .child(icon(glyph, 20.0).color(p.text_tertiary))
            .child(text(line.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap().grow(1.0).shrink().min_w(0.0))
            .children(button)
    };
    if rules::is_locked(t) && !r.send {
        // `BarrierBase`: the composer's place, with a lock.
        return vec![
            col().w_full().px(dim::FOOTER_BOX_INSET).pb(dim::COMPOSER_BOX_INSET).child(
                row()
                    .w_full()
                    .min_h(dim::FOOTER_BOX_HEIGHT)
                    .px(16.0)
                    .gap(8.0)
                    .center()
                    .radius(dim::FOOTER_BOX_RADIUS)
                    .fill(p.background_textarea)
                    .child(icon(&icons::LOCK_SIMPLE, 20.0).color(p.text_primary_muted))
                    .child(text(crate::i18n::t("This thread is locked. Only moderators can send messages.")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary_muted)),
            ),
        ];
    }
    let mut out = Vec::new();
    let post = t.parent_id.and_then(|p| store.channels.get(&p)).is_some_and(|p| rules::is_forum(p.kind));
    if rules::is_archived(t) {
        out.push(notice(&icons::ARCHIVE, crate::i18n::t("This thread is closed. Sending a message will open it again."), None));
    } else if !post && !r.member && r.join {
        let id = t.id;
        let join = Button::new(crate::i18n::t("Join thread")).size(ButtonSize::Small).key(format!("thread-join-{id}")).on_press(move |s: &mut S| op(acc(s), Op::Join(id))).el(p);
        out.push(notice(&icons::CHATS, crate::i18n::t("Join this thread to follow it in your channel list and get notifications."), Some(join)));
    }
    out.push(super::chat::composer(store, st, t, p, acc, if pane { super::chat::Slot::Pane } else { super::chat::Slot::Main }));
    out
}

/// `ThreadCreatePane`: an icon, the name field, the private toggle and the source message over
/// a composer whose Enter starts the thread.
fn create_pane<S: 'static>(store: &Store, st: &ShellState, parent: &Channel, message: Option<Snowflake>, p: &Palette, acc: ShellAccess<S>) -> El {
    let source = message.and_then(|m| store.messages(parent.id).iter().find(|x| x.id == m));
    let suggested = suggested_name(store, parent.guild_id, source);
    let can_private = message.is_none() && parent.kind == ChannelType::GUILD_TEXT && store.can_create_thread(parent, CreateKind::Private);
    let can_public = store.can_create_thread(parent, if message.is_some() { CreateKind::FromMessage } else { CreateKind::Public });
    // shortcut: Rune wraps a long placeholder even in a single-line field; cut it near the
    // field's width instead of clipping it like an `<input>`.
    let placeholder = if suggested.is_empty() { "New thread".to_string() } else { suggested.chars().take(48).collect() };
    let mut body = col()
        .w_full()
        .gap(16.0)
        .padding(16.0)
        .child(row().size(64.0, 64.0).center().radius(32.0).fill(p.background_modifier_accent).child(icon(&icons::CHATS, 32.0).color(p.text_primary)))
        .child(
            Input::new("thread-name", &st.threads.name, move |s: &mut S| &mut acc(s).threads.name)
                .label(t("Thread name"))
                .placeholder(&placeholder)
                .error(st.threads.name_error.as_deref())
                .el(p),
        );
    if can_private && can_public {
        body = body.child(
            Switch::new(st.threads.private)
                .label(t("Private thread"))
                .description(t("Only people you invite and moderators can see this thread."))
                .on_toggle(move |s: &mut S| acc(s).threads.private ^= true)
                .el(p),
        );
    } else if can_private {
        // Only private is allowed: it is what gets created.
        let _ = 0;
    }
    if let Some(m) = source {
        let md = super::markdown::Md::new(store, &st.media, parent.guild_id, p, format!("create-src-m{}", m.id));
        body = body.child(
            row()
                .w_full()
                .py(4.0)
                .radius(4.0)
                .child(el().w(4.0).self_stretch().corners([4.0, 0.0, 0.0, 4.0]).fill(p.background_modifier_accent))
                .child(col().grow(1.0).min_w(0.0).inert().child(super::chat::message_preview(store, st, m, &md, p, acc))),
        );
    }
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: true }).justify_end().child(body);
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(p.composer_surface_color)
        .child(scroller(viewport, true, p))
        .child(super::chat::composer(store, st, parent, p, acc, super::chat::Slot::Create))
}

/// The panel's member list: who joined, with their avatars.
fn members_popout<S: 'static>(store: &Store, st: &ShellState, t: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let ids: Vec<Snowflake> = store.thread_members.get(&t.id).map_or_else(Vec::new, |l| l.iter().filter_map(|m| m.user_id).collect());
    let mut list = col().w_full().px(8.0);
    for u in ids {
        list = list.child(
            row()
                .w_full()
                .h(42.0)
                .px(8.0)
                .gap(12.0)
                .center_y()
                .radius(4.0)
                .hover(p.background_modifier_hover)
                .child(avatar(store, st, u, 32.0, Some(super::presence(store, u)), p.background_secondary, p))
                .child(text(store.display_name(t.guild_id, u)).font(font(16.0, 500.0).lh(20.0)).color(p.text_primary).truncate().shrink()),
        );
    }
    let bg = p.background_secondary;
    let surface = col()
        .w(264.0)
        .max_h(480.0)
        .py(8.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .clip_rounded(8.0)
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .role("dialog")
        .label(crate::i18n::t("Thread members"))
        .child(scroller(col().w_full().scroll(ScrollSpec { stick_bottom: false }).child(list), false, p))
        .modal(true, move |s: &mut S| acc(s).popups.close());
    anchor(surface, MEMBERS, Placement::BottomEnd, 8.0)
}

/// `ThreadMembersPanel`: a full-view thread's member list, its members split online/offline and
/// sorted by name.
// shortcut: hoisted-role groups aren't split out; everyone online lands in "Online".
pub(super) fn members_column<S: 'static>(store: &Store, st: &ShellState, t: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let gid = t.guild_id.unwrap_or_default();
    let owner = store.guilds.get(&gid).and_then(|g| g.info.as_ref()).map(|i| i.owner_id);
    let mut ids: Vec<Snowflake> = store.thread_members.get(&t.id).map_or_else(Vec::new, |l| l.iter().filter_map(|m| m.user_id).collect());
    ids.sort_by_cached_key(|u| store.display_name(Some(gid), *u).to_lowercase());
    let (offline, online): (Vec<_>, Vec<_>) = ids.into_iter().partition(|u| super::presence(store, *u) == crate::ui::Status::Offline);
    let mut body = col().w_full().pt(dim::MEMBER_LIST_SCROLLER_PADDING_BLOCK_START).px(dim::MEMBER_LIST_SCROLLER_PADDING_INLINE).pb(dim::MEMBER_LIST_SCROLLER_PADDING_BLOCK_END);
    let mut first = true;
    for (label, users) in [("Online", online), ("Offline", offline)] {
        if users.is_empty() {
            continue;
        }
        body = body.child(
            row()
                .w_full()
                .pt(12.0 + if first { 0.0 } else { dim::MEMBER_LIST_GROUP_SPACER_HEIGHT })
                .pb(4.0)
                .px(8.0)
                .child(text(format!("{label} — {}", users.len())).font(font(14.0, 600.0).lh(16.0)).color(p.text_primary_muted).truncate()),
        );
        first = false;
        for u in users {
            body = body.child(col().w_full().pb(dim::MEMBER_LIST_ROW_GAP).child(super::members::member(store, st, gid, u, owner == Some(u), p, acc)).key(format!("thread-member-{u}")));
        }
    }
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(body.key(format!("thread-members-{}", t.id)));
    col().w(dim::LAYOUT_MEMBER_LIST_WIDTH).h_full().min_h(0.0).fill(p.background_secondary_lighter).child(scroller(viewport, false, p))
}

// ---- Browser ------------------------------------------------------------------------------------

/// `ThreadBrowser`: the parent's threads by tab, newest activity first.
pub(super) fn browser<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.popups.is_open(BROWSER) {
        return;
    }
    let Some(parent) = browser_parent(store, c) else { return };
    let pid = parent.id;
    let tab = st.threads.tab;
    let bg = p.background_secondary;
    let border = over(p.background_modifier_accent, bg);
    let mut header = row()
        .w_full()
        .min_h(48.0)
        .py(8.0)
        .pl(16.0)
        .pr(12.0)
        .gap(8.0)
        .center_y()
        .child(icon(&icons::CHATS, 20.0).color(p.text_primary_muted))
        .child(text(t("Threads")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).grow(1.0));
    if store.can_create_thread(parent, CreateKind::Public) || store.can_create_thread(parent, CreateKind::Private) {
        header = header.child(Button::new(t("Create thread")).size(ButtonSize::Small).key("thread-browser-create").on_press(move |s: &mut S| start_create(acc(s), pid, None)).el(p));
    }
    header = header.child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(border));
    let mut tabs = vec![(Tab::Active, "Active"), (Tab::Archived, "Closed")];
    if parent.kind == ChannelType::GUILD_TEXT {
        tabs.push((Tab::ArchivedPrivate, t("Closed private")));
    }
    let mut tab_list = row().w_full().padding(3.0).radius(10.0).fill(p.background_tertiary);
    for (t, label) in tabs {
        let on = t == tab;
        let mut b = row()
            .grow(1.0)
            .py(6.0)
            .px(12.0)
            .center()
            .radius(8.0)
            .cursor(rune::Cursor::Pointer)
            .role("tab")
            .child(text(label).font(font(14.0, 600.0).lh(18.0)).color(if on { p.text_primary } else { p.text_secondary }))
            .key(format!("thread-tab-{label}"))
            .on_press(move |s: &mut S| {
                let st = acc(s);
                st.threads.tab = t;
                if t != Tab::Active && !st.threads.archived.contains_key(&(pid, t)) {
                    st.threads.archived.entry((pid, t)).or_default().loading = true;
                    op(st, Op::LoadArchived { parent: pid, tab: t, more: false });
                }
            });
        if on {
            b = b.fill(p.background_secondary).box_shadow(rune::Shadow { x: 0.0, y: 1.0, blur: 2.0, spread: 0.0, color: Color { a: 0.08, ..Color::BLACK }, inset: false });
        }
        tab_list = tab_list.child(b);
    }
    let tabs_el = col().w_full().pt(12.0).child(col().w_full().pt(4.0).px(16.0).pb(8.0).child(tab_list));
    let mut list = col().w_full().px(8.0).pb(12.0);
    let item = |t: &Channel| browser_item(st, t, p, acc);
    match tab {
        Tab::Active => {
            let mut active: Vec<&Channel> = store.threads(pid).into_iter().filter(|t| !rules::is_archived(t)).collect();
            active.sort_by_key(|t| std::cmp::Reverse(rules::last_activity(t)));
            let (joined, other): (Vec<&Channel>, Vec<&Channel>) = active.into_iter().partition(|t| t.member.is_some());
            if joined.is_empty() && other.is_empty() {
                list = list.child(empty(p));
            }
            for (title, group) in [("Joined", joined), ("Other active threads", other)] {
                if group.is_empty() {
                    continue;
                }
                list = list.child(text(title).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted).pt(12.0).px(8.0).pb(4.0));
                for t in group {
                    list = list.child(item(t));
                }
            }
        }
        _ => {
            let a = st.threads.archived.get(&(pid, tab)).cloned().unwrap_or_default();
            let threads: Vec<&Channel> = a.ids.iter().filter_map(|id| store.thread(*id)).filter(|t| (tab == Tab::ArchivedPrivate) == (t.kind == ChannelType::PRIVATE_THREAD)).collect();
            if threads.is_empty() && !a.loading {
                list = list.child(empty(p));
            }
            for t in threads {
                list = list.child(item(t));
            }
            if a.loading {
                list = list.child(row().w_full().padding(12.0).center().child(spinner(p.text_primary_muted)));
            } else if a.has_more {
                list = list.child(row().w_full().padding(12.0).center().child(
                    Button::new(t("Load more"))
                        .variant(ButtonVariant::Secondary)
                        .size(ButtonSize::Small)
                        .key("thread-browser-more")
                        .on_press(move |s: &mut S| {
                            let st = acc(s);
                            st.threads.archived.entry((pid, tab)).or_default().loading = true;
                            op(st, Op::LoadArchived { parent: pid, tab, more: true });
                        })
                        .el(p),
                ));
            }
        }
    }
    let body = scroller(col().w_full().h_full().scroll(ScrollSpec { stick_bottom: false }).child(list), false, p);
    let surface = col()
        .size(480.0, 576.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, border)
        .clip_rounded(8.0)
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .role("dialog")
        .label(t("Threads"))
        .child(header)
        .child(tabs_el)
        .child(col().w_full().grow(1.0).min_h(0.0).child(body))
        .modal(true, move |s: &mut S| acc(s).popups.close());
    layer.push(anchor(surface, BROWSER, Placement::BottomEnd, 8.0));
}

/// `ThreadBrowserItem`: name, then (lock,) message count and last activity.
fn browser_item<S: 'static>(st: &ShellState, t: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut meta = row().gap(6.0).center_y();
    if t.kind == ChannelType::PRIVATE_THREAD {
        meta = meta.child(icon(&icons::LOCK_SIMPLE, 14.0).color(p.text_primary_muted));
    }
    let small = font(13.0, 400.0).lh(16.0);
    meta = meta
        .child(text(message_count(t.message_count)).font(small).color(p.text_primary_muted))
        .child(text(format_relative(rules::last_activity(t).timestamp_ms(), st.now_ms)).font(small).color(p.text_primary_muted));
    let (thread, id) = (t.clone(), t.id);
    col()
        .w_full()
        .padding(8.0)
        .gap(2.0)
        .radius(6.0)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .key(format!("thread-item-{id}"))
        .child(text(t.name.clone().unwrap_or_default()).font(font(15.0, 600.0).lh(20.0)).color(p.text_primary).truncate())
        .child(meta)
        .on_press_tx(move |s: &mut S, tx| open_from_event(acc(s), &thread, tx.mods().shift))
        .on_secondary_press(move |s: &mut S, at, _| open_menu(acc(s), id, [at.x, at.y]))
}

fn empty(p: &Palette) -> El {
    col()
        .w_full()
        .min_h(360.0)
        .padding(32.0)
        .gap(8.0)
        .center()
        .child(icon(&icons::CHATS, 48.0).color(p.text_tertiary))
        .child(text(t("There are no threads")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary))
        .child(
            text(t("Stay focused on a conversation with a thread, a temporary text channel."))
                .font(font(14.0, 400.0).lh(20.0))
                .color(p.text_primary_muted)
                .wrap()
                .align(rune::Align::Center),
        )
}

// ---- Menu, settings, confirmations --------------------------------------------------------------

/// Opens the thread menu at a point.
pub fn open_menu(st: &mut ShellState, thread: Snowflake, at: [f32; 2]) {
    st.threads.menu = Some(thread);
    st.popups.open_at(MENU, at);
}

/// The notification choices (`buildThreadNotificationItems`).
fn notification_items<S: 'static>(store: &Store, t: &Channel, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let cur = store.thread_notify_setting(t);
    let id = t.id;
    [
        (NotificationLevel::INHERIT, "Use channel default", ThreadMemberFlags::empty()),
        (NotificationLevel::ALL_MESSAGES, "All messages", ThreadMemberFlags::ALL_MESSAGES),
        (NotificationLevel::ONLY_MENTIONS, "Only @mentions", ThreadMemberFlags::ONLY_MENTIONS),
        (NotificationLevel::NO_MESSAGES, "Nothing", ThreadMemberFlags::NO_MESSAGES),
    ]
    .into_iter()
    .map(|(level, label, flag)| {
        MenuEntry::Item(MenuItem::new(format!("thread-notify-{}", level.0), label).radio(cur == level).on_select(move |s: &mut S| {
            op(acc(s), Op::MemberSettings { thread: id, body: serde_json::json!({ "flags": flag.bits() }) })
        }))
    })
    .collect()
}

/// `ThreadContextMenu` (`useThreadMenuData`).
fn menu<S: 'static>(store: &Store, st: &ShellState, t: &Channel, acc: ShellAccess<S>) -> Menu<S> {
    let id = t.id;
    let r = store.thread_rights(t);
    let mut m = Menu::new(MENU).placement(Placement::BottomEnd);
    // The header bell shows the notification choices alone.
    if st.popups.at.is_none() && !st.threads.panel.is_some_and(|p| p == Panel::Thread(id)) {
        for e in notification_items(store, t, acc) {
            if let MenuEntry::Item(i) = e {
                m = m.item(i);
            }
        }
        if r.member {
            let muted = t.member.as_ref().is_some_and(|x| x.muted);
            m = m.separator().item(MenuItem::new("thread-mute", if muted { crate::i18n::t("Unmute thread") } else { crate::i18n::t("Mute thread") }).on_select(move |s: &mut S| {
                op(acc(s), Op::MemberSettings { thread: id, body: if muted { serde_json::json!({ "muted": false }) } else { serde_json::json!({ "muted": true, "mute_config": null }) } })
            }));
        }
        return m;
    }
    if store.has_unread(id) || store.mention_count(id) > 0 {
        let last = t.last_message_id.unwrap_or(id);
        m = m.item(MenuItem::new("thread-read", "Mark as read").icon(&icons::ENVELOPE_OPEN).on_select(move |s: &mut S| acc(s).intents.push(Intent::MarkRead(vec![(id, last)])))).separator();
    }
    let parent = t.parent_id;
    if st.threads.panel != Some(Panel::Thread(id)) && st.channel != Some(id) {
        m = m.item(MenuItem::new("thread-open", crate::i18n::t("Open thread")).icon(&icons::ARROW_SQUARE_OUT).on_select(move |s: &mut S| open(acc(s), parent, id)));
    }
    if st.channel != Some(id) {
        m = m.item(MenuItem::new("thread-full", crate::i18n::t("Open in full view")).icon(&icons::ARROWS_OUT_SIMPLE).on_select(move |s: &mut S| open_full(acc(s), id)));
    }
    m = m.separator();
    if r.member {
        let muted = t.member.as_ref().is_some_and(|x| x.muted);
        m = m
            .item(MenuItem::new("thread-mute", if muted { crate::i18n::t("Unmute thread") } else { crate::i18n::t("Mute thread") }).icon(&icons::BELL_SLASH).on_select(move |s: &mut S| {
                op(acc(s), Op::MemberSettings { thread: id, body: if muted { serde_json::json!({ "muted": false }) } else { serde_json::json!({ "muted": true, "mute_config": null }) } })
            }))
            .submenu("thread-notify", crate::i18n::t("Notification settings"), notification_items(store, t, acc))
            .separator();
    }
    if r.edit {
        let meta = t.thread_metadata.clone().unwrap_or_default();
        let (tname, auto, invitable) = (t.name.clone().unwrap_or_default(), meta.auto_archive_duration.max(60), meta.invitable.unwrap_or(true));
        m = m.item(MenuItem::new("thread-edit", crate::i18n::t("Edit thread")).icon(&icons::PENCIL_SIMPLE).on_select(move |s: &mut S| {
            let st = acc(s);
            st.popups.close();
            st.threads.settings = Some(Settings { thread: id, name: TextEdit::new(tname.clone()), auto_archive: auto, invitable });
        }));
    }
    let archived = rules::is_archived(t);
    if if archived { r.unarchive } else { r.archive } {
        m = m.item(MenuItem::new("thread-archive", if archived { crate::i18n::t("Open thread again") } else { crate::i18n::t("Close thread") }).icon(&icons::ARCHIVE).on_select(move |s: &mut S| {
            op(acc(s), Op::Patch { channel: id, body: serde_json::json!({ "archived": !archived }) })
        }));
    }
    if r.lock {
        let locked = rules::is_locked(t);
        m = m.item(MenuItem::new("thread-lock", if locked { crate::i18n::t("Unlock thread") } else { crate::i18n::t("Lock thread") }).icon(if locked { &icons::LOCK_SIMPLE_OPEN } else { &icons::LOCK_SIMPLE }).on_select(move |s: &mut S| {
            op(acc(s), Op::Patch { channel: id, body: serde_json::json!({ "locked": !locked }) })
        }));
    }
    if !r.member && r.join {
        m = m.item(MenuItem::new("thread-join", crate::i18n::t("Join thread")).icon(&icons::SIGN_IN).on_select(move |s: &mut S| op(acc(s), Op::Join(id))));
    }
    if r.member && r.leave {
        let private = t.kind == ChannelType::PRIVATE_THREAD;
        let mut item = MenuItem::new("thread-leave", crate::i18n::t("Leave thread")).icon(&icons::SIGN_OUT).on_select(move |s: &mut S| {
            let st = acc(s);
            if private {
                st.threads.confirm = Some(Confirm::LeavePrivate(id));
            } else {
                op(st, Op::Leave(id));
            }
        });
        if private {
            item = item.danger();
        }
        m = m.item(item);
    }
    if r.delete {
        m = m.item(MenuItem::new("thread-delete", crate::i18n::t("Delete thread")).icon(&icons::TRASH).danger().on_select(move |s: &mut S| acc(s).threads.confirm = Some(Confirm::Delete(id))));
    }
    m = m.separator();
    let link = format!("{}/channels/{}/{}", st.web_base.trim_end_matches('/'), t.guild_id.map_or("@me".into(), |g| g.to_string()), id);
    m = m.item(MenuItem::new("thread-link", crate::i18n::t("Copy link")).icon(&icons::LINK_BOLD).on_select_tx(move |_: &mut S, tx| tx.copy(link.clone())));
    if store.settings.developer_mode {
        m = m.item(MenuItem::new("thread-id", crate::i18n::t("Copy thread ID")).icon(&icons::SNOWFLAKE_FILL).on_select_tx(move |_: &mut S, tx| tx.copy(id.to_string())));
    }
    m
}

/// The thread menu, the settings modal and confirmations; and the browser when `c` has one.
pub(super) fn layer<S: 'static>(store: &Store, st: &ShellState, c: Option<&Channel>, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if let Some(c) = c {
        browser(store, st, c, p, acc, layer);
    }
    if let Some(t) = st.threads.menu.and_then(|id| store.thread(id))
        && st.popups.is_open(MENU)
    {
        menu(store, st, t, acc).el(&st.popups, pops(acc), p, layer);
    }
    if let Some(s) = &st.threads.settings
        && let Some(t) = store.thread(s.thread)
    {
        layer.push(settings_modal(s, t, p, acc));
    }
    match st.threads.confirm {
        Some(Confirm::Delete(id)) => {
            let name = store.channels.get(&id).and_then(|t| t.name.clone()).unwrap_or_default();
            layer.push(confirm(p, acc, t("Delete thread"), &crate::i18n::tf("Are you sure you want to delete {channelLabel}? This cannot be undone.", &[("channelLabel", &name.to_string())]), t("Delete thread"), Op::Delete(id)));
        }
        Some(Confirm::LeavePrivate(id)) => {
            let name = store.channels.get(&id).and_then(|t| t.name.clone()).unwrap_or_default();
            let body = crate::i18n::tf("Leave {threadName}? It's private, so you can only come back if someone adds you again.", &[("threadName", &name.to_string())]);
            layer.push(confirm(p, acc, t("Leave private thread"), &body, t("Leave thread"), Op::Leave(id)));
        }
        None => {}
    }
}

fn confirm<S: 'static>(p: &Palette, acc: ShellAccess<S>, title: &str, body: &str, primary: &str, then: Op) -> El {
    let close = move |s: &mut S| acc(s).threads.confirm = None;
    Modal::new(title, close)
        .body(text(body.to_string()).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("thread-confirm-cancel").on_press(close).el(p))
                .child(
                    Button::new(primary)
                        .variant(ButtonVariant::Danger)
                        .key("thread-confirm-ok")
                        .on_press(move |s: &mut S| {
                            let st = acc(s);
                            st.threads.confirm = None;
                            if let Op::Delete(id) = then
                                && st.threads.panel == Some(Panel::Thread(id))
                            {
                                st.threads.panel = None;
                            }
                            op(st, then.clone());
                        })
                        .el(p),
                ),
        )
        .el(p)
}

/// `ThreadSettingsModal`: name, hide after inactivity and (private threads) invites.
// shortcut: no slowmode control.
fn settings_modal<S: 'static>(s: &Settings, t: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).threads.settings = None;
    let opts: [(&str, &str, Option<&str>); 4] = [("60", "1 hour", None), ("1440", "24 hours", None), ("4320", "3 days", None), ("10080", "1 week", None)];
    let cur = s.auto_archive.to_string();
    fn draft<S>(acc: ShellAccess<S>, s: &mut S) -> Option<&mut Settings> {
        acc(s).threads.settings.as_mut()
    }
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(
            Input::new("thread-settings-name", &s.name, move |s: &mut S| {
                let st = acc(s);
                &mut st.threads.settings.get_or_insert_with(|| Settings { thread: Snowflake(0), name: TextEdit::new(""), auto_archive: 4320, invitable: true }).name
            })
            .label(crate::i18n::t("Thread name"))
            .el(p),
        )
        .child(text(crate::i18n::t("Hide after inactivity")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary))
        .child(
            RadioGroup::new(&opts, Some(&cur))
                .on_change(move |s: &mut S, v| {
                    if let Some(x) = draft(acc, s) {
                        x.auto_archive = v.parse().unwrap_or(4320);
                    }
                })
                .el(p),
        );
    let private = t.kind == ChannelType::PRIVATE_THREAD;
    if private {
        body = body.child(Switch::new(s.invitable).label(crate::i18n::t("Allow anyone to invite")).on_toggle(move |s: &mut S| {
            if let Some(x) = draft(acc, s) {
                x.invitable ^= true;
            }
        }).el(p));
    }
    let id = t.id;
    Modal::new(crate::i18n::t("Thread settings"), close)
        .body(body)
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(crate::i18n::t("Cancel")).variant(ButtonVariant::Secondary).key("thread-settings-cancel").on_press(close).el(p))
                .child(
                    Button::new(crate::i18n::t("Save"))
                        .key("thread-settings-save")
                        .on_press(move |s: &mut S| {
                            let st = acc(s);
                            let Some(x) = st.threads.settings.take() else { return };
                            let mut body = serde_json::json!({ "name": x.name.text.trim(), "auto_archive_duration": x.auto_archive });
                            if private {
                                body["invitable"] = x.invitable.into();
                            }
                            op(st, Op::Patch { channel: id, body });
                        })
                        .el(p),
                ),
        )
        .el(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_names_from_the_source_and_requires_a_name() {
        let mut st = ShellState::default();
        start_create(&mut st, Snowflake(10), Some(Snowflake(20)));
        st.threads.composer = TextEdit::new("first!");
        submit_create(&mut st, "");
        assert_eq!(st.threads.name_error.as_deref(), Some("Give the thread a name before you send."));
        assert!(st.intents.is_empty());
        let m = Message { content: "  hello\n  world ".into(), ..Default::default() };
        submit_create(&mut st, &suggested_name(&Store::default(), None, Some(&m)));
        assert_eq!(
            st.intents,
            [Intent::Thread(Op::Create { parent: Snowflake(10), message: Some(Snowflake(20)), name: "hello world".into(), private: false, content: "first!".into() })]
        );
        assert!(st.threads.composer.text.is_empty());
    }

    #[test]
    fn opening_a_thread_selects_its_parent_and_loads_it() {
        let mut st = ShellState::default();
        open(&mut st, Some(Snowflake(10)), Snowflake(30));
        assert_eq!(st.channel, Some(Snowflake(10)));
        assert_eq!(st.threads.panel, Some(Panel::Thread(Snowflake(30))));
        assert!(st.intents.contains(&Intent::Thread(Op::Open(Snowflake(30)))));
        open_full(&mut st, Snowflake(30));
        assert_eq!((st.channel, st.threads.panel), (Some(Snowflake(30)), None));
    }
}
