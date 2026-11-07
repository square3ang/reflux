//! The expression picker (`ExpressionPickerPopout`): GIFs, saved media, stickers and emoji
//! behind one set of tabs, opened from the composer's buttons, and its emoji-only form for
//! reactions (`EmojiPickerPopout`). Also who reacted (`ReactionTooltip`,
//! `MessageReactionsModal`) and the expression info card (`ExpressionInfoPopout`).
//!
//! Frecency, favorites, collapsed categories, the skin tone and favorite GIFs live in
//! `~/.config/reflux/expressions.json` and sync through `synced_preferences` ([`sync`]).

mod emoji;
pub mod gif;
pub mod image_search;
mod info;
mod memes;
mod reactions;
mod sticker;
mod sync;
pub use sticker::pending as pending_sticker;

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use reflux_model::{Channel, Permissions, Snowflake};
use reflux_store::Store;
use rune::{Align, Color, El, Key, KeyEvent, Shadow, Side, TextEdit, Tx, col, el, icon, row, text, text_input};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Intent, ShellAccess, ShellState};
use crate::icons;
use crate::media::Load;
use crate::theme::{Palette, font};
use crate::ui::{mix, over, scroller};
use crate::i18n::t;

pub use info::{Info, open_emoji_info, open_sticker_info};
pub use memes::{Tool, hover_tools, on_meme, tool};
pub use reactions::{message_menu_items, pill_hover, reactor_tip_key};

/// The composer's picker in [`crate::ui::Popups`]; the reaction picker is `emoji-{message}`.
pub const POPOUT: &str = "expr-picker";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    Gifs,
    Memes,
    Stickers,
    #[default]
    Emojis,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Gifs, Tab::Memes, Tab::Stickers, Tab::Emojis];
    fn label(self) -> &'static str {
        match self {
            Tab::Gifs => t("GIFs"),
            Tab::Memes => t("Media"),
            Tab::Stickers => t("Stickers"),
            Tab::Emojis => t("Emojis"),
        }
    }
}

/// What the open picker picks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum For {
    Composer(Snowflake),
    React { channel: Snowflake, message: Snowflake },
    /// A settings form's emoji field: the pick goes into `form.data[key]` as
    /// `{"id": custom ID or null, "name": surrogate or custom name}`, anchored under `pick-{key}`.
    Form { channel: Snowflake, key: &'static str },
    /// The custom status modal's emoji (`CustomStatusModal`, emoji tab only).
    Status,
}

impl For {
    fn channel(self) -> Snowflake {
        match self {
            For::Composer(c) | For::React { channel: c, .. } | For::Form { channel: c, .. } => c,
            For::Status => Snowflake(0),
        }
    }
}

/// A [`Intent::Fetch`] answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Fetched {
    Loading,
    Done(Value),
    Failed(String),
}

/// `UsageEntry`: uses and the last one (unix ms).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub count: u32,
    pub last_used: u64,
}

/// What `expressions.json` keeps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    /// 0 = default, 1..=5 = 🏻..🏿.
    pub skin_tone: u8,
    pub emoji_usage: BTreeMap<String, Usage>,
    pub favorite_emojis: Vec<String>,
    pub sticker_usage: BTreeMap<String, Usage>,
    pub favorite_stickers: Vec<String>,
    /// Picker categories folded shut (`favorites`, `frequently-used`, a guild ID, a unicode
    /// category key; stickers prefix theirs with `sticker:`).
    pub collapsed: Vec<String>,
    /// Starred GIF objects, newest first.
    pub favorite_gifs: Vec<Value>,
}

/// `USAGE_FRECENCY_HALF_LIFE_MS`, `MAX_TRACKED_USAGE_KEYS`, `MAX_FRECENT_EMOJIS`.
const HALF_LIFE_MS: f64 = 7.0 * 86_400_000.0;
const MAX_TRACKED: usize = 200;
const MAX_FRECENT: usize = 42;
const MAX_FAVORITES: usize = 500;

/// `usageFrecencyScore`: log2(count + 1), halved every week since the last use.
fn score(u: &Usage, now: u64) -> f64 {
    ((u.count as f64) + 1.0).log2() * 0.5f64.powf(now.saturating_sub(u.last_used) as f64 / HALF_LIFE_MS)
}

/// `rankUsageMap`: best first; ties by recency, then count, then key.
fn ranked(usage: &BTreeMap<String, Usage>, now: u64) -> Vec<&str> {
    let mut v: Vec<(&String, &Usage)> = usage.iter().collect();
    v.sort_by(|a, b| {
        score(b.1, now).total_cmp(&score(a.1, now)).then(b.1.last_used.cmp(&a.1.last_used)).then(b.1.count.cmp(&a.1.count)).then(a.0.cmp(b.0))
    });
    v.into_iter().map(|(k, _)| k.as_str()).collect()
}

/// `bumpUsageEntry`, pruning to the best `MAX_TRACKED_USAGE_KEYS`.
fn bump(usage: &mut BTreeMap<String, Usage>, key: &str, now: u64) {
    let e = usage.entry(key.to_string()).or_default();
    e.count = (e.count + 1).min(100_000);
    e.last_used = now;
    if usage.len() > MAX_TRACKED {
        let keep: Vec<String> = ranked(usage, now).into_iter().take(MAX_TRACKED).map(str::to_string).collect();
        usage.retain(|k, _| keep.contains(k));
    }
}

fn toggle_in(list: &mut Vec<String>, key: &str, max: usize) {
    if let Some(i) = list.iter().position(|k| k == key) {
        list.remove(i);
    } else if list.len() < max {
        list.push(key.to_string());
    }
}

fn saved_path() -> Option<std::path::PathBuf> {
    Some(crate::session::dir()?.join("expressions.json"))
}

pub fn load() -> Saved {
    saved_path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// The GIF tab's page.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum GifView {
    #[default]
    Categories,
    Trending,
    Favorites,
    /// A category's or the typed search's results.
    Search(String),
}

/// UI state of the expression surfaces.
#[derive(Default)]
pub struct Expr {
    pub target: Option<For>,
    pub tab: Tab,
    pub query: TextEdit,
    /// The keyboard selection: (item row, column).
    pub sel: Option<(usize, usize)>,
    /// The hovered item's usage key (the inspector and search placeholder show it).
    pub hover: Option<String>,
    pub skin_open: bool,
    pub saved: Saved,
    /// Write [`Saved`] back on change (off for the demo and tests).
    pub persist: bool,
    /// `Intent::Fetch` answers by key.
    pub fetched: HashMap<String, Fetched>,
    pub gif: GifView,
    /// When the query last changed (unix ms), for the GIF search debounce.
    pub edited_at: u64,
    /// A category header to bring to the top: (scroll key, header key), and once more next frame.
    pub jump: Option<(String, String)>,
    jump_again: Option<(String, String)>,
    /// The reaction pill under the pointer: (channel, message, emoji key), since when, and
    /// when the pointer left it (the tooltip lingers).
    pub tip: Option<(Snowflake, Snowflake, String)>,
    pub tip_since: u64,
    pub tip_left: Option<u64>,
    /// `MessageReactionsModal`: (channel, message, selected emoji key).
    pub reactions: Option<(Snowflake, Snowflake, String)>,
    /// "Remove all reactions" waiting on its confirmation.
    pub confirm_clear: Option<(Snowflake, Snowflake)>,
    pub info: Option<Info>,
    /// "Join community" confirmation from an info card: (guild, name, icon, expression name, sticker).
    pub join: Option<(Snowflake, String, Option<String>, String, bool)>,
    /// Meme filter: 0 all, 1 images, 2 videos.
    pub meme_filter: u8,
    /// The right-click menu over a picker cell (its own popup state, so the picker stays open)
    /// and the item it's for.
    pub menu: crate::ui::Popups,
    pub menu_item: Option<Item>,
    /// The favorite GIF import modal, while open or running.
    pub gif_import: Option<gif::GifImport>,
    /// Saved media waiting on its delete confirmation: (id, name).
    pub confirm_meme: Option<(String, String)>,
    /// The reverse image search provider picker: (image URL, engine selected).
    pub image_search: Option<(String, String)>,
    /// The web search / translation provider picker: (kind, text, engine selected).
    pub provider_picker: Option<(super::search_providers::Kind, String, String)>,
    /// The `synced_preferences` snapshot last seen or sent.
    pub synced: Option<sync::Seen>,
    /// A new reverse image search default waiting to be sent ([`image_search::set_default`]).
    pub ris_default: Option<Option<String>>,
    /// New web search and translation defaults waiting to be sent.
    pub search_default: Option<Option<String>>,
    pub translate_default: Option<Option<String>>,
}


impl Expr {
    fn save(&self) {
        if !self.persist {
            return;
        }
        let Some(p) = saved_path() else { return };
        let _ = p.parent().map(std::fs::create_dir_all);
        if let Ok(b) = serde_json::to_vec(&self.saved) {
            let _ = std::fs::write(p, b);
        }
    }

    fn reset(&mut self, target: For, tab: Tab) {
        self.target = Some(target);
        self.tab = tab;
        self.query = TextEdit::new("");
        self.query.focused = true;
        (self.sel, self.hover, self.skin_open) = (None, None, false);
        self.gif = GifView::Categories;
    }

    pub fn toggle_favorite(&mut self, key: &str) {
        if key.starts_with("sticker:") {
            toggle_in(&mut self.saved.favorite_stickers, key, MAX_FAVORITES);
        } else {
            toggle_in(&mut self.saved.favorite_emojis, key, MAX_FAVORITES);
        }
        self.save();
    }

    pub fn is_favorite(&self, key: &str) -> bool {
        self.saved.favorite_emojis.iter().chain(&self.saved.favorite_stickers).any(|k| k == key)
    }

    fn track(&mut self, key: &str, now: u64) {
        let usage = if key.starts_with("sticker:") { &mut self.saved.sticker_usage } else { &mut self.saved.emoji_usage };
        bump(usage, key, now);
        self.save();
    }
}

/// Opens the reaction picker for a message (emoji only).
pub fn open_react(st: &mut ShellState, channel: Snowflake, message: Snowflake) {
    st.popups.open = Some(format!("emoji-{message}"));
    st.active_msg = Some(message);
    st.expr.reset(For::React { channel, message }, Tab::Emojis);
}

/// The settings forms' emoji picker in [`crate::ui::Popups`].
pub const FORM_POPOUT: &str = "expr-form";

/// Opens the emoji picker for a settings form field (see [`For::Form`]); `channel` decides which
/// custom emoji may be used.
pub fn open_for_form(st: &mut ShellState, channel: Snowflake, key: &'static str) {
    st.popups.close();
    st.popups.open = Some(FORM_POPOUT.into());
    st.expr.reset(For::Form { channel, key }, Tab::Emojis);
}

/// The settings forms' picker, for the dialog layer that owns the field to draw over itself.
pub fn form_picker<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let t @ For::Form { key, .. } = st.expr.target? else { return None };
    if !st.popups.is_open(FORM_POPOUT) {
        return None;
    }
    let e = popout(store, st, t, p, acc).modal(true, move |s: &mut S| close(acc(s)));
    Some(crate::ui::overlay::anchor(e, &format!("pick-{key}"), crate::ui::Placement::BottomStart, 8.0))
}

/// A composer button: opens the picker on `tab`, switches to it, or closes it when it already
/// shows `tab` (`handleExpressionPickerTabToggle`).
pub fn toggle(st: &mut ShellState, channel: Snowflake, tab: Tab) {
    let open = st.popups.is_open(POPOUT);
    if open && st.expr.tab == tab {
        st.popups.close();
        st.composer.focused = true;
    } else if open {
        set_tab(st, tab);
    } else {
        st.popups.close();
        st.popups.open = Some(POPOUT.into());
        st.expr.reset(For::Composer(channel), tab);
        st.composer.focused = false;
        load_tab(st);
    }
}

fn set_tab(st: &mut ShellState, tab: Tab) {
    if let Some(t) = st.expr.target {
        st.expr.reset(t, tab);
        load_tab(st);
    }
}

/// Fetches what a tab shows the first time it opens.
fn load_tab(st: &mut ShellState) {
    match st.expr.tab {
        Tab::Gifs => fetch(st, "gif:featured", "/gifs/featured?locale=en-US"),
        Tab::Memes => fetch(st, "memes", "/users/@me/memes"),
        _ => {}
    }
}

/// GETs `path` into `expr.fetched[key]` unless it's loaded or loading.
pub(crate) fn fetch(st: &mut ShellState, key: &str, path: &str) {
    if matches!(st.expr.fetched.get(key), Some(Fetched::Done(_) | Fetched::Loading)) {
        return;
    }
    st.expr.fetched.insert(key.into(), Fetched::Loading);
    st.intents.push(Intent::Fetch { key: key.into(), method: reqwest::Method::GET, path: path.into(), body: None });
}

/// A request whose answer arrives over the gateway (or doesn't matter).
pub(crate) fn send(st: &mut ShellState, method: reqwest::Method, path: String, body: Option<Value>) {
    st.intents.push(Intent::Fetch { key: String::new(), method, path, body });
}

/// `encodeURIComponent` for query values.
pub(crate) fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn close(st: &mut ShellState) {
    let composer = matches!(st.expr.target, Some(For::Composer(_)));
    st.popups.close();
    st.active_msg = None;
    st.expr.target = None;
    if composer {
        st.composer.focused = true;
    }
}

/// Once a frame: the GIF search debounce and two-step category jumps.
pub fn poll(store: &Store, st: &mut ShellState, tx: &mut Tx) -> bool {
    let mut changed = reactions::poll(store, st) | sync::poll(store, st, tx);
    if let Some((_, key)) = st.expr.jump_again.take() {
        tx.scroll_key_to(key, 0.0);
        changed = true;
    }
    if let Some(j) = st.expr.jump.take() {
        tx.scroll_into_view_in(j.0.clone(), j.1.clone());
        st.expr.jump_again = Some(j);
        tx.wake_after(std::time::Duration::ZERO);
        changed = true;
    }
    if st.expr.target.is_some() && st.expr.tab == Tab::Gifs {
        changed |= gif::poll(st, tx);
    }
    changed |= gif::poll_import(st);
    changed
}

/// The overlays this module owns: the composer's picker, the reaction tooltip, the reactions
/// modal and its confirmation, and the info card.
pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if st.popups.is_open(POPOUT)
        && let Some(t @ For::Composer(_)) = st.expr.target
    {
        let e = popout(store, st, t, p, acc).modal(true, move |s: &mut S| close(acc(s)));
        layer.push(e.anchored_fit("composer-expr", Side::Top, Align::End, 8.0));
    }
    menu_layer(store, st, p, acc, layer);
    image_search::layer(st, p, acc, layer);
    super::search_providers::layer(st, p, acc, layer);
    gif::import_layer(st, p, acc, layer);
    memes::confirm_layer(st, p, acc, layer);
    reactions::layer(store, st, p, acc, layer);
    info::layer(store, st, p, acc, layer);
}

/// The reaction picker surface; the caller anchors it.
pub fn react_picker<S: 'static>(store: &Store, st: &ShellState, channel: Snowflake, message: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    popout(store, st, For::React { channel, message }, p, acc)
}

/// Opens the emoji picker for the custom status modal.
pub fn open_status(st: &mut ShellState) {
    st.popups.open = Some(POPOUT.into());
    st.expr.reset(For::Status, Tab::Emojis);
}

/// The custom status modal's picker, under the emoji button (`status-emoji`).
pub fn status_picker<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    (st.popups.is_open(POPOUT) && st.expr.target == Some(For::Status))
        .then(|| popout(store, st, For::Status, p, acc).modal(true, move |s: &mut S| close(acc(s))).anchored_fit("status-emoji", Side::Bottom, Align::Start, 8.0))
}

/// `EXPRESSION_PICKER_DEFAULT_SIZE`.
const WIDTH: f32 = 592.0;
const HEIGHT: f32 = 690.0;

/// `ExpressionPickerPopout`: header (tabs and the tab's search bar), then the tab.
fn popout<S: 'static>(store: &Store, st: &ShellState, t: For, p: &Palette, acc: ShellAccess<S>) -> El {
    let bg = p.form_surface_background;
    let tabs: Vec<Tab> = match t {
        For::React { .. } | For::Form { .. } | For::Status => vec![Tab::Emojis],
        For::Composer(_) => Tab::ALL.into_iter().filter(|&x| x != Tab::Gifs || gif::enabled(st)).collect(),
    };
    let channel = store.channels.get(&t.channel());
    let (bar, body) = match st.expr.tab {
        Tab::Gifs => gif::view(st, t, p, acc),
        Tab::Memes => memes::view(st, t, p, acc),
        Tab::Stickers => sticker::view(store, st, channel, t, p, acc),
        Tab::Emojis => emoji::view(store, st, channel, t, p, acc),
    };
    let mut header = col().w_full().padding(12.0).gap(12.0);
    if tabs.len() > 1 {
        let mut nav = row().gap(8.0).role("tablist").label(crate::i18n::t("Expression picker categories"));
        for tab in tabs {
            let on = st.expr.tab == tab;
            let mut b = row()
                .px(8.0)
                .py(4.0)
                .radius(crate::theme::dim::RADIUS_MD)
                .cursor(rune::Cursor::Pointer)
                .role("tab")
                .label(tab.label())
                .key(format!("expr-tab-{}", tab.label()))
                .child(text(tab.label()).font(font(14.0, 600.0).lh(20.0)).color(if on { p.text_primary } else { p.text_primary_muted }))
                .on_press(move |s: &mut S| set_tab(acc(s), tab));
            b = if on { b.fill(p.background_modifier_selected) } else { b.hover(p.background_modifier_hover) };
            nav = nav.child(b);
        }
        header = header.child(nav);
    }
    header = header.child(bar);
    col()
        .w(WIDTH)
        .h(HEIGHT)
        .shrink()
        .min_h(0.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .clip_rounded(8.0)
        .role("dialog")
        .label("Expression picker")
        .key("expr-picker")
        .child(header)
        .child(el().w_full().h(1.0).fill(over(p.background_modifier_hover, bg)))
        .child(col().w_full().grow(1.0).min_h(0.0).fill(p.background_primary).child(body))
}

// ---- Shared grid ---------------------------------------------------------------------------

/// One thing a grid offers.
#[derive(Clone, Debug)]
pub enum Item {
    Unicode(&'static super::emoji::Emoji),
    Emoji { guild: Snowflake, id: Snowflake, name: String, animated: bool },
    Sticker { guild: Snowflake, id: Snowflake, name: String, description: String, tags: Vec<String> },
}

impl Item {
    /// `getEmojiUsageKey`, and `sticker:` + the sticker store's `getStickerKey` (`{guild}:{id}`).
    pub fn key(&self) -> String {
        match self {
            Item::Unicode(e) => format!("unicode:{}", e.name),
            Item::Emoji { guild, id, .. } => format!("custom:{guild}:{id}"),
            Item::Sticker { guild, id, .. } => format!("sticker:{guild}:{id}"),
        }
    }
    fn guild(&self) -> Option<Snowflake> {
        match self {
            Item::Unicode(_) => None,
            Item::Emoji { guild, .. } | Item::Sticker { guild, .. } => Some(*guild),
        }
    }
    /// `allNamesString`: every `:name:` it answers to.
    fn names(&self) -> String {
        match self {
            Item::Unicode(e) => e.names.iter().map(|n| format!(":{n}:")).collect::<Vec<_>>().join(" "),
            Item::Emoji { name, .. } => format!(":{name}:"),
            Item::Sticker { name, .. } => name.clone(),
        }
    }
}

/// A picker row: a category header, or a row of items.
#[derive(Clone, Debug)]
enum Row {
    Head { cat: String, name: String, guild: Option<Snowflake> },
    Items(Vec<Item>),
}

/// One labelled group of items (`PickerSection`).
struct Section {
    cat: String,
    name: String,
    guild: Option<Snowflake>,
    items: Vec<Item>,
}

/// `buildPickerRows`: search results alone, or each section's header and (unless collapsed)
/// its items `cols` to a row.
fn build_rows(search: Option<Vec<Item>>, sections: Vec<Section>, cols: usize, collapsed: &[String], prefix: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let chunk = |rows: &mut Vec<Row>, items: Vec<Item>| rows.extend(items.chunks(cols).map(|c| Row::Items(c.to_vec())));
    if let Some(found) = search {
        chunk(&mut rows, found);
        return rows;
    }
    for s in sections.into_iter().filter(|s| !s.items.is_empty()) {
        let folded = collapsed.iter().any(|c| *c == format!("{prefix}{}", s.cat));
        rows.push(Row::Head { cat: s.cat, name: s.name, guild: s.guild });
        if !folded {
            chunk(&mut rows, s.items);
        }
    }
    rows
}

/// Favorites, then frequently used, then each guild's (current guild first, then the guild
/// list's order), then `rest`.
fn sections(store: &Store, st: &ShellState, all: &[Item], favorites: &[String], usage: &BTreeMap<String, Usage>, rest: Vec<Section>) -> Vec<Section> {
    let by_key: HashMap<String, &Item> = all.iter().map(|i| (i.key(), i)).collect();
    let favs: Vec<Item> = all.iter().filter(|i| favorites.contains(&i.key())).cloned().collect();
    let frecent: Vec<Item> = ranked(usage, st.now_ms).into_iter().filter_map(|k| by_key.get(k).map(|i| (*i).clone())).take(MAX_FRECENT).collect();
    let mut out = vec![
        Section { cat: "favorites".into(), name: "Favorited".into(), guild: None, items: favs },
        Section { cat: "frequently-used".into(), name: "Frequently used".into(), guild: None, items: frecent },
    ];
    for g in guild_order(store, st) {
        let items: Vec<Item> = all.iter().filter(|i| i.guild() == Some(g)).cloned().collect();
        let name = store.guilds.get(&g).and_then(|x| x.info.as_ref()).map(|i| i.name.clone()).unwrap_or_default();
        out.push(Section { cat: g.to_string(), name, guild: Some(g), items });
    }
    out.extend(rest);
    out
}

/// `groupByGuildInGuildListOrder`: the guild list, the selected guild first.
pub(crate) fn guild_order(store: &Store, st: &ShellState) -> Vec<Snowflake> {
    let mut ids: Vec<Snowflake> = store
        .guild_list()
        .into_iter()
        .flat_map(|i| match i {
            reflux_store::GuildListItem::Guild(g) => vec![g],
            reflux_store::GuildListItem::Folder { guilds, .. } => guilds,
        })
        .collect();
    if let Some(i) = st.guild.and_then(|g| ids.iter().position(|x| *x == g)) {
        let g = ids.remove(i);
        ids.insert(0, g);
    }
    ids
}

/// `checkEmojiAvailability` / `checkStickerAvailability`: a guild's expression works in its
/// own channels, elsewhere with the external permission and `feature_global_expressions`.
pub(crate) fn can_use(store: &Store, st: &ShellState, channel: Option<&Channel>, guild: Snowflake, external: Permissions) -> bool {
    match channel.and_then(|c| c.guild_id.map(|g| (c.id, g))) {
        Some((_, g)) if g == guild => true,
        Some((c, _)) => store.permissions(c).contains(external) && st.limits.global_expressions,
        None => st.limits.global_expressions,
    }
}

/// Geometry of a grid: cell size, row height, columns.
#[derive(Clone, Copy)]
struct Grid {
    cell: f32,
    row_h: f32,
    cols: usize,
    /// Prefix of collapse keys and element keys (`emoji` / `sticker`).
    kind: &'static str,
}

/// `PICKER_CATEGORY_HEADER_HEIGHT` + `PICKER_CATEGORY_HEADER_GAP`, `EMOJI_SECTION_GAP`.
const HEAD_H: f32 = 32.0;
const HEAD_GAP: f32 = 8.0;
const SECTION_GAP: f32 = 12.0;
/// The picker's category rail (`EXPRESSION_PICKER_CATEGORY_RAIL_WIDTH`).
const RAIL_W: f32 = 46.0;

/// Item rows only, for keyboard selection: (index into `rows`, items).
fn item_rows(rows: &[Row]) -> Vec<(usize, &Vec<Item>)> {
    rows.iter().enumerate().filter_map(|(i, r)| if let Row::Items(v) = r { Some((i, v)) } else { None }).collect()
}

/// Arrow keys move the selection over the item rows, Enter picks, Escape closes
/// (`usePickerGridKeyboard`). `pick` gets the item and whether Shift was held.
fn grid_key(st: &mut ShellState, rows: &[Row], k: &KeyEvent, tx: &mut Tx, scope: &str, pick: fn(&mut ShellState, &Item, bool, &mut Tx)) -> bool {
    let items = item_rows(rows);
    let (mut r, mut c) = st.expr.sel.unwrap_or((usize::MAX, 0));
    let len = |r: usize| items.get(r).map_or(0, |x| x.1.len());
    match &k.key {
        Key::Escape => {
            close(st);
            return true;
        }
        Key::Enter => {
            let at = if r == usize::MAX { (0, 0) } else { (r, c) };
            if let Some(it) = items.get(at.0).and_then(|x| x.1.get(at.1)).cloned() {
                pick(st, &it, k.mods.shift, tx);
            }
            return true;
        }
        Key::Down | Key::Up | Key::Left | Key::Right if !items.is_empty() => {
            if r == usize::MAX {
                (r, c) = (0, 0);
            } else {
                match &k.key {
                    Key::Down if r + 1 < items.len() => r += 1,
                    Key::Up if r > 0 => r -= 1,
                    Key::Right if c + 1 < len(r) => c += 1,
                    Key::Right if r + 1 < items.len() => (r, c) = (r + 1, 0),
                    Key::Left if c > 0 => c -= 1,
                    Key::Left if r > 0 => (r, c) = (r - 1, len(r - 1).saturating_sub(1)),
                    _ => {}
                }
                c = c.min(len(r).saturating_sub(1));
            }
            st.expr.sel = Some((r, c));
            st.expr.hover = items[r].1.get(c).map(Item::key);
            tx.scroll_into_view_in(scope.to_string(), format!("{scope}-row-{}", items[r].0));
            return true;
        }
        _ => {}
    }
    false
}

/// `PickerSearchInput`: a 36-tall field on `form-surface` darkened 15%, a magnifier inside on the
/// left and a clear button on the right. Arrow keys and Enter go to `key`.
fn search_bar<S: 'static>(st: &ShellState, placeholder: String, p: &Palette, acc: ShellAccess<S>, key: impl Fn(&mut S, &KeyEvent, &mut Tx) -> bool + 'static) -> El {
    let bg = mix(p.form_surface_background, Color::BLACK, 85.0);
    let mut field = row()
        .grow(1.0)
        .min_w(0.0)
        .min_h(36.0)
        .pl(10.0)
        .pr(8.0)
        .gap(8.0)
        .center_y()
        .radius(crate::theme::dim::RADIUS_MD)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .child(icon(&icons::MAGNIFYING_GLASS, 18.0).color(p.text_primary_muted))
        .child(
            text_input(&st.expr.query, font(16.0, 400.0).lh(20.0))
                .key("expr-search")
                .grow(1.0)
                .single_line()
                .color(p.text_primary)
                .placeholder(placeholder, p.text_tertiary)
                .caret(p.text_primary, p.text_selection)
                .on_edit(move |s: &mut S| -> &mut TextEdit {
                    let x = &mut acc(s).expr;
                    x.sel = None;
                    x.edited_at = crate::app::now_ms();
                    &mut x.query
                })
                .on_input_key(key),
        );
    if !st.expr.query.text.is_empty() {
        field = field.child(
            row()
                .padding(4.0)
                .radius(4.0)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Clear search"))
                .hover_group()
                .child(icon(&icons::X_BOLD, 16.0).color(p.text_primary_muted).group_color(p.text_primary))
                .on_press(move |s: &mut S| {
                    let x = &mut acc(s).expr;
                    x.query = TextEdit::new("");
                    x.query.focused = true;
                    x.sel = None;
                }),
        );
    }
    field
}

/// A guild's icon at `size` (its acronym on `background-secondary` without one).
fn guild_icon(store: &Store, st: &ShellState, g: Snowflake, size: f32, p: &Palette) -> El {
    let info = store.guilds.get(&g).and_then(|x| x.info.as_ref());
    guild_face(st, g, &info.map(|i| i.name.clone()).unwrap_or_default(), info.and_then(|i| i.icon.as_deref()), size, p)
}

/// A guild's icon (or acronym) from its parts, for guilds you may not be in.
fn guild_face(st: &ShellState, g: Snowflake, name: &str, icon: Option<&str>, size: f32, p: &Palette) -> El {
    match icon.map(|h| st.media.asset(|b, sc| crate::media::guild_icon_url(b, g, h, size, sc))) {
        Some(Load::Ready(px)) => el().size(size, size).radius(size * 0.3).pixels_fit(&px, rune::SurfaceFit::Fill),
        _ => row()
            .size(size, size)
            .center()
            .radius(size * 0.3)
            .fill(p.background_secondary)
            .child(text(super::acronym(name)).font(font((size * 0.4).max(7.0), 600.0)).color(p.text_primary).truncate()),
    }
}

/// `EmojiPickerCategoryList`: frequently used, each guild, then `extra` icons; 26px buttons,
/// gap 8, on `background-primary` with a 1px inset right edge.
fn rail<S: 'static>(store: &Store, st: &ShellState, rows: &[Row], scope: &'static str, extra: Vec<(String, &'static rune::Icon, &'static str)>, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut list = col().w_full().items_center().gap(8.0);
    let button = |cat: String, glyph: El, label: String| {
        let head = format!("{scope}-head-{cat}");
        row()
            .size(26.0, 26.0)
            .center()
            .radius(6.0)
            .hover(p.background_modifier_hover)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label)
            .child(glyph)
            .on_press(move |s: &mut S| acc(s).expr.jump = Some((scope.to_string(), head.clone())))
    };
    let has = |cat: &str| rows.iter().any(|r| matches!(r, Row::Head { cat: c, .. } if c == cat));
    if has("frequently-used") {
        list = list.child(button("frequently-used".into(), icon(&icons::CLOCK, 24.0).color(p.text_primary_muted).group_color(p.text_primary), "Frequently used".into()));
    }
    for r in rows {
        if let Row::Head { cat, name, guild: Some(g) } = r {
            list = list.child(button(cat.clone(), guild_icon(store, st, *g, 24.0, p), name.clone()));
        }
    }
    for (cat, glyph, label) in extra.into_iter().filter(|(c, ..)| has(c)) {
        list = list.child(button(cat, icon(glyph, 24.0).color(p.text_primary_muted).group_color(p.text_primary), label.into()));
    }
    col()
        .pin(Some(0.0), Some(0.0), None, Some(0.0))
        .w(RAIL_W)
        .py(8.0)
        .fill(p.background_primary)
        .box_shadow(Shadow { x: -1.0, y: 0.0, blur: 0.0, spread: 0.0, color: p.background_modifier_accent, inset: true })
        .child(col().w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(list))
}

/// `PickerCategoryHeaderRow`: a 32-tall button folding the section, its icon, name and caret.
/// `head`: (category, name, guild).
fn head_row<S: 'static>(store: &Store, st: &ShellState, (cat, name, guild): (&str, &str, Option<Snowflake>), g: Grid, p: &Palette, acc: ShellAccess<S>) -> El {
    let key = format!("{}{cat}", if g.kind == "sticker" { "sticker:" } else { "" });
    let folded = st.expr.saved.collapsed.contains(&key);
    let lead = match (cat, guild) {
        ("favorites", _) => Some(icon(&icons::STAR_FILL, 16.0).color(p.text_primary_muted)),
        ("frequently-used", _) => Some(icon(&icons::CLOCK_FILL, 16.0).color(p.text_primary_muted)),
        (_, Some(id)) => Some(guild_icon(store, st, id, 16.0, p)),
        _ => None,
    };
    row()
        .w_full()
        .h(HEAD_H)
        .mb(HEAD_GAP)
        .px(12.0)
        .gap(8.0)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(name.to_string())
        .children(lead)
        .child(text(name.to_string()).font(font(14.0, 700.0).lh(17.0)).color(p.text_primary_muted).truncate().shrink().min_w(0.0))
        .child(icon(&icons::CARET_DOWN_BOLD, 12.0).color(p.text_primary_muted).rotate(if folded { -std::f32::consts::FRAC_PI_2 } else { 0.0 }))
        .on_press(move |s: &mut S| {
            let x = &mut acc(s).expr;
            toggle_in(&mut x.saved.collapsed, &key, 200);
            x.save();
        })
}

/// What a lazily built cell needs.
#[derive(Clone)]
struct CellCx {
    media: crate::media::Media,
    tone: u8,
    p: Palette,
}

/// The scrolling grid: headers and item rows, `cell` drawing each item. Item rows are built
/// only when they scroll into view.
fn grid_list<S: 'static>(
    store: &Store,
    st: &ShellState,
    rows: Rc<Vec<Row>>,
    g: Grid,
    p: &Palette,
    acc: ShellAccess<S>,
    cell: fn(&CellCx, &Item, Grid, bool, ShellAccess<S>) -> El,
) -> El {
    let scope = format!("{}-grid", g.kind);
    let sel = st.expr.sel.and_then(|(r, c)| item_rows(&rows).get(r).map(|x| (x.0, c)));
    let heads: Rc<Vec<Option<El>>> = Rc::new(
        rows.iter()
            .map(|r| match r {
                Row::Head { cat, name, guild } => Some(head_row(store, st, (cat, name, *guild), g, p, acc)),
                Row::Items(_) => None,
            })
            .collect(),
    );
    let cx = CellCx { media: st.media.clone(), tone: st.expr.saved.skin_tone, p: *p };
    let n = rows.len();
    let q = st.expr.query.text.clone();
    let sc = scope.clone();
    let list = col().w_full().key(scope.clone()).growth(rune::GrowthSpec::default()).virtual_rows(
        n,
        g.row_h,
        move |_: &S, i| format!("{q}-{i}").into(),
        move |_: &S, i| {
            let e = match (&rows[i], &heads[i]) {
                (_, Some(h)) => h.clone().key(format!("{sc}-head-{}", if let Row::Head { cat, .. } = &rows[i] { cat.as_str() } else { "" })),
                (Row::Items(items), None) => row()
                    .w_full()
                    .h(g.row_h)
                    .gap(if g.kind == "sticker" { 8.0 } else { 0.0 })
                    .children(items.iter().enumerate().map(|(c, it)| cell(&cx, it, g, sel == Some((i, c)), acc))),
                _ => el(),
            };
            let gap = matches!(rows[i], Row::Items(_)) && matches!(rows.get(i + 1), Some(Row::Head { .. }));
            let e = if gap { col().w_full().pb(SECTION_GAP).child(e) } else { e };
            col().w_full().key(format!("{sc}-row-{i}")).child(e)
        },
    );
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(p.background_tertiary)
        .child(scroller(col().w_full().h_full().pt(12.0).px(8.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(list), false, p))
}

/// The empty state over the grid: a duotone sad face and a line.
fn empty_state(line: &str, sub: Option<&str>, p: &Palette) -> El {
    let fg = mix(p.text_primary_muted, Color::TRANSPARENT, 70.0);
    col()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .center()
        .gap(8.0)
        .inert()
        .child(icon(&icons::SMILEY_SAD_FILL, 42.0).color(fg).opacity(0.7))
        .child(text(line.to_string()).font(font(14.0, 400.0)).color(fg))
        .children(sub.map(|s| text(s.to_string()).font(font(12.0, 400.0)).color(fg)))
}

/// `.inspector`: 48 tall under the grid, the hovered item, its names and source guild.
fn inspector(store: &Store, item: Option<(&Item, El)>, p: &Palette) -> El {
    let mut bar = row()
        .w_full()
        .h(48.0)
        .px(16.0)
        .center_y()
        .fill(p.background_primary)
        .box_shadow(Shadow { x: 0.0, y: 1.0, blur: 0.0, spread: 0.0, color: p.background_modifier_accent, inset: true });
    if let Some((it, glyph)) = item {
        let from = it.guild().and_then(|g| store.guilds.get(&g)).and_then(|g| g.info.as_ref()).map(|i| i.name.clone());
        bar = bar.child(glyph).child(
            col()
                .ml(8.0)
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(it.names()).font(font(14.0, 600.0).lh(17.0)).color(p.text_primary).truncate())
                .children(from.map(|g| text(format!("from {g}")).font(font(12.0, 400.0).lh(15.0)).color(p.text_primary_muted).truncate())),
        );
    }
    bar
}

/// The picked item's row/column hover handler: shows it in the inspector.
/// Right-click opens `ExpressionContextMenuItems` for `item`.
fn with_menu<S: 'static>(e: El, item: &Item, acc: ShellAccess<S>) -> El {
    let it = item.clone();
    e.on_secondary_press(move |s: &mut S, at, _| {
        let x = &mut acc(s).expr;
        x.menu_item = Some(it.clone());
        x.menu.open_at("expr-menu", [at.x, at.y]);
    })
}

/// `ExpressionContextMenuItems`: Favorite, Copy ID, "Clone … to…", then the image's Copy URL,
/// Open in browser and reverse image search.
fn menu_layer<S: 'static>(
    store: &Store,
    st: &ShellState,
    p: &Palette,
    acc: ShellAccess<S>,
    layer: &mut Vec<El>,
) {
    let Some(item) = &st.expr.menu_item else {
        return;
    };
    let sticker = matches!(item, Item::Sticker { .. });
    let key = item.key();
    let fav = st.expr.is_favorite(&key);
    let base = st.media.base();
    let (id, url) = match item {
        Item::Unicode(_) => (None, None),
        Item::Emoji { id, animated, .. } => (Some(*id), Some(if *animated { crate::media::emoji_url_animated(&base, &id.to_string()) } else { crate::media::emoji_url(&base, &id.to_string()) })),
        Item::Sticker { id, .. } => (Some(*id), Some(crate::media::sticker_url(&base, *id, false))),
    };
    let label = |e: &'static str, s: &'static str| t(if sticker { s } else { e });
    let close = move |s: &mut S| acc(s).expr.menu.close();
    let mut m = crate::ui::Menu::new("expr-menu");
    {
        // Picker items are unicode or from communities you're in, so all can be favourites.
        let (k, ic) = (key.clone(), if fav { &icons::STAR_FILL } else { &icons::STAR_BOLD });
        m = m.item(crate::ui::MenuItem::new("expr-fav", if fav { label("Unfavorite emoji", "Unfavorite sticker") } else { label("Favorite emoji", "Favorite sticker") }).icon(ic).on_select(move |s: &mut S| {
            acc(s).expr.toggle_favorite(&k);
            close(s);
        }));
    }
    if let Some(id) = id {
        m = m.item(crate::ui::MenuItem::new("expr-copy-id", label("Copy emoji ID", "Copy sticker ID")).icon(&icons::CLIPBOARD_TEXT).on_select_tx(move |s: &mut S, tx: &mut Tx| {
            tx.copy(id.to_string());
            close(s);
        }));
    }
    if let (Some(id), Some(g), true) = (id, item.guild(), crate::look::with(|l| l.clone_shortcuts))
    {
        m = clone_entries(store, st, item, id, g, acc, m);
    }
    if let Some(url) = url {
        let u = url.clone();
        let search = image_search::entries(store, st, &url, acc, close);
        m = m
            .separator()
            .item(crate::ui::MenuItem::new("expr-copy-url", label("Copy emoji URL", "Copy sticker URL")).icon(&icons::LINK).on_select_tx(move |s: &mut S, tx: &mut Tx| {
                tx.copy(u.clone());
                crate::desktop::toast(&mut acc(s).desktop, "Link copied to clipboard", true);
                close(s);
            }))
            .item(crate::ui::MenuItem::new("expr-open", label("Open emoji in browser", "Open sticker in browser")).icon(&icons::ARROW_SQUARE_OUT).on_select(move |s: &mut S| {
                crate::settings::privacy::open_url(&url);
                close(s);
            }));
        for e in search {
            m = m.entry(e);
        }
    }
    m.el(&st.expr.menu, crate::ui::popup_access(move |s: &mut S| &mut acc(s).expr.menu), p, layer);
}

/// `CloneExpressionMenuItem`: when the source community allows cloning (`CLONE_EMOJI_ENABLED` /
/// `CLONE_STICKER_ENABLED`), a submenu of the other communities you manage expressions in, by
/// name, full ones disabled with "… slots full".
fn clone_entries<S: 'static>(store: &Store, st: &ShellState, item: &Item, id: Snowflake, source: Snowflake, acc: ShellAccess<S>, m: crate::ui::Menu<S>) -> crate::ui::Menu<S> {
    let sticker = matches!(item, Item::Sticker { .. });
    let feature = if sticker { "CLONE_STICKER_ENABLED" } else { "CLONE_EMOJI_ENABLED" };
    let allowed = store.guilds.get(&source).and_then(|g| g.info.as_ref()).is_some_and(|i| i.features.iter().any(|f| f == feature));
    let mut targets: Vec<(Snowflake, String)> = store
        .guilds
        .iter()
        .filter(|(g, x)| **g != source && x.info.is_some() && store.guild_permissions(**g).contains(Permissions::MANAGE_EXPRESSIONS))
        .map(|(g, x)| (*g, x.info.as_ref().map(|i| i.name.clone()).unwrap_or_default()))
        .collect();
    if !allowed || targets.is_empty() {
        return m;
    }
    targets.sort_by_key(|t| t.1.to_lowercase());
    let name = match item {
        Item::Emoji { name, .. } | Item::Sticker { name, .. } => name.clone(),
        Item::Unicode(_) => return m,
    };
    let (kind, field) = if sticker { ("stickers", "source_sticker_id") } else { ("emojis", "source_emoji_id") };
    let entries = targets
        .into_iter()
        .map(|(g, n)| {
            let done = if sticker {
                crate::i18n::tf("Cloned sticker {stickerName} to {targetName}", &[("stickerName", &name), ("targetName", &n)])
            } else {
                crate::i18n::tf("Cloned :{sourceName}: to {targetName}", &[("sourceName", &name), ("targetName", &n)])
            };
            let full = info::slots_full(store, st, g, sticker);
            let mut it = crate::ui::MenuItem::new(format!("clone-{g}"), n).disabled(full).on_select(move |s: &mut S| {
                let body = serde_json::json!({ field: id.to_string() });
                acc(s).social(reqwest::Method::POST, format!("/guilds/{g}/{kind}/clone"), Some(body), crate::social::Done::Toast(done.clone()));
            });
            if full {
                it = it.hint(if sticker { t("Sticker slots full") } else { t("Emoji slots full") });
            }
            crate::ui::MenuEntry::Item(it)
        })
        .collect();
    m.separator().submenu("expr-clone", if sticker { t("Clone sticker to…") } else { t("Clone emoji to...") }, entries)
}

fn hover_cell<S: 'static>(e: El, key: String, acc: ShellAccess<S>) -> El {
    e.on_hover_change(move |s: &mut S, inside, _| {
        let x = &mut acc(s).expr;
        if inside {
            x.hover = Some(key.clone());
        } else if x.hover.as_deref() == Some(key.as_str()) {
            x.hover = None;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frecency_ranks_by_decayed_log_count() {
        let mut u = BTreeMap::new();
        let day = 86_400_000;
        bump(&mut u, "a", 0);
        for _ in 0..3 {
            bump(&mut u, "b", 0);
        }
        bump(&mut u, "c", 30 * day);
        // b: log2(4)=2 at 0 → after 30 days ≈ 2·0.05; c: log2(2)=1 fresh.
        assert_eq!(ranked(&u, 30 * day), ["c", "b", "a"]);
        assert_eq!(ranked(&u, 0)[0], "b");
        for i in 0..250 {
            bump(&mut u, &format!("k{i}"), i);
        }
        assert_eq!(u.len(), MAX_TRACKED);
    }

    #[test]
    fn rows_fold_sections_and_chunk_items() {
        let e = |n: &str| Item::Unicode(crate::shell::emoji::by_name(n).expect(n));
        let s = |cat: &str, items: Vec<Item>| Section { cat: cat.into(), name: cat.into(), guild: None, items };
        let rows = build_rows(None, vec![s("a", vec![e("grinning"), e("clap"), e("tada")]), s("empty", vec![]), s("b", vec![e("clap")])], 2, &["b".into()], "");
        let kinds: Vec<&str> = rows.iter().map(|r| if let Row::Head { .. } = r { "h" } else { "i" }).collect();
        assert_eq!(kinds, ["h", "i", "i", "h"], "empty sections drop; folded ones keep their header");
        let found = build_rows(Some(vec![e("clap")]), vec![], 9, &[], "");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn grid_keys_move_and_pick() {
        let e = |n: &str| Item::Unicode(crate::shell::emoji::by_name(n).expect(n));
        let rows = vec![Row::Head { cat: "x".into(), name: "x".into(), guild: None }, Row::Items(vec![e("grinning"), e("clap")]), Row::Items(vec![e("tada")])];
        let mut st = ShellState::default();
        let mut tx = Tx::default();
        let k = |key: Key| KeyEvent { key, mods: Default::default() };
        fn pick(st: &mut ShellState, it: &Item, _: bool, _: &mut Tx) {
            st.expr.hover = Some(format!("picked {}", it.key()));
        }
        grid_key(&mut st, &rows, &k(Key::Down), &mut tx, "g", pick);
        assert_eq!(st.expr.sel, Some((0, 0)));
        grid_key(&mut st, &rows, &k(Key::Right), &mut tx, "g", pick);
        grid_key(&mut st, &rows, &k(Key::Down), &mut tx, "g", pick);
        assert_eq!(st.expr.sel, Some((1, 0)), "clamped to the shorter row");
        grid_key(&mut st, &rows, &k(Key::Enter), &mut tx, "g", pick);
        assert_eq!(st.expr.hover.as_deref(), Some("picked unicode:tada"));
    }
}
