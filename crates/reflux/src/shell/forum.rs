//! Forum and media channels (`features/forum`): the post list with its search card, sort & view
//! popout and tag filter (`ForumChannelView`, `ForumToolbar`, `ForumPostCard`, `ForumTagPill`),
//! and the new post composer (`ForumPostComposer`, `ForumTagPicker`).
// Search and the older posts go through `/threads/search` (tags filtered by the server); with
// search off on the instance (`FEATURE_TEMPORARILY_DISABLED`) older posts page `archived/public`
// and the search filters loaded posts by name, as before.

use std::collections::{HashMap, HashSet};

use reflux_model::{Channel, ChannelFlags, ForumTag, Message, ReactionEmoji, Snowflake};
use reflux_store::Store;
use reflux_store::threads as rules;
use rune::text::Span;
use rune::{El, ScrollSpec, TextEdit, Tx, col, el, icon, rich, row, text, text_input};

use super::threads::{self, Op, Tab};
use super::{Intent, ShellAccess, ShellState, emoji, format_relative};
use crate::icons;
use crate::media::Load;
use crate::theme::{Palette, font};
use crate::ui::overlay::{anchor, popout_shadow};
use crate::ui::{Button, ButtonSize, ButtonVariant, Placement, RadioGroup, over, scroller, spinner};
use crate::i18n::t;

const SORT: &str = "forum-sort";
const TAG_PICKER: &str = "forum-tag-picker";
/// `MAX_APPLIED_TAGS_PER_THREAD`.
const MAX_TAGS: usize = 5;
const MAX_CARD_TAGS: usize = 3;

/// Forum UI state.
#[derive(Default)]
pub struct Forum {
    pub query: TextEdit,
    /// The user's sort order and layout per forum (else the channel's defaults).
    pub sort: HashMap<Snowflake, u8>,
    pub layout: HashMap<Snowflake, u8>,
    pub tag_filter: HashMap<Snowflake, Vec<Snowflake>>,
    pub guidelines: bool,
    pub draft: Option<Draft>,
    /// First messages of posts (`post-data`); `None` = deleted.
    pub first: HashMap<Snowflake, Option<Message>>,
    pub requested: HashSet<Snowflake>,
    /// The post list's laid-out width, for the gallery's column count.
    pub width: f32,
    /// `ForumReadState`: unread counts of unjoined posts from FORUM_UNREADS (count, the ack
    /// they were counted from), the acks already asked about, the send times of the last 5 s,
    /// and the forum's ack when it was opened (newer posts get "New").
    pub post_unreads: HashMap<Snowflake, (u32, Snowflake)>,
    pub sent_acks: HashMap<Snowflake, Snowflake>,
    pub sends: Vec<u64>,
    pub snapshot: Option<(Snowflake, Snowflake)>,
}

/// `getPostUnreadCount`: an unjoined post's count while its ack hasn't moved.
pub fn post_unread_count(store: &Store, st: &ShellState, post: &Channel) -> u32 {
    let ack = store.read_states.get(&post.id).and_then(|r| r.last_message_id);
    match st.threads.forum.post_unreads.get(&post.id) {
        Some((n, at)) if post.member.is_none() && ack == Some(*at) => *n,
        _ => 0,
    }
}

/// `isNewPost`: created since the forum was opened, by someone else.
fn is_new_post(store: &Store, st: &ShellState, post: &Channel) -> bool {
    let me = store.me.as_ref().map(|m| m.id);
    matches!(st.threads.forum.snapshot, Some((f, at)) if Some(f) == post.parent_id && post.id > at && post.owner_id.is_none_or(|o| Some(o) != me))
}

/// The new post being written.
pub struct Draft {
    pub forum: Snowflake,
    pub title: TextEdit,
    pub body: TextEdit,
    pub tags: Vec<Snowflake>,
    pub error: Option<String>,
    pub preview: bool,
}

fn sort_of(st: &ShellState, c: &Channel) -> u8 {
    st.threads.forum.sort.get(&c.id).copied().unwrap_or(c.default_sort_order.unwrap_or(0))
}

/// Media channels are always a gallery.
fn grid(st: &ShellState, c: &Channel) -> bool {
    c.kind == reflux_model::ChannelType::GUILD_MEDIA || st.threads.forum.layout.get(&c.id).copied().unwrap_or(c.default_forum_layout) == 2
}

fn require_tag(c: &Channel) -> bool {
    c.flags.contains(ChannelFlags::REQUIRE_TAG)
}

/// Can post here (`canCreatePost`).
fn can_post(store: &Store, c: &Channel) -> bool {
    store.can_create_thread(c, rules::CreateKind::ForumPost)
}

/// The `/threads/search` queries the forum wants: (name search, older posts). `None` while the
/// instance has search off; older posts wait for "Load older posts" when 25 or more are active.
pub fn search_keys(store: &Store, st: &ShellState, c: &Channel) -> (Option<String>, Option<String>) {
    if st.threads.search_disabled {
        return (None, None);
    }
    let tags = st.threads.forum.tag_filter.get(&c.id).cloned().unwrap_or_default();
    let setting = c.default_tag_setting.as_deref();
    let q = st.threads.forum.query.text.trim();
    let search = (!q.is_empty()).then(|| threads::search_query(Some(q), &tags, setting, None, "relevance"));
    let sort = if sort_of(st, c) == 1 { "creation_time" } else { "last_message_time" };
    let started = st.threads.archived.contains_key(&(c.id, Tab::Archived));
    let few = store.threads(c.id).into_iter().filter(|t| !rules::is_archived(t)).count() < threads::SEARCH_LIMIT;
    (search, (started || few).then(|| threads::search_query(None, &tags, setting, Some(true), sort)))
}

/// Whether the posts come from a server search (`ForumPosts.isSearching`).
fn server_search(st: &ShellState) -> bool {
    !st.threads.search_disabled && !st.threads.forum.query.text.trim().is_empty()
}

/// The posts to list: (current, older), each filtered and sorted.
pub fn posts<'a>(store: &'a Store, st: &ShellState, c: &Channel) -> (Vec<&'a Channel>, Vec<&'a Channel>) {
    if server_search(st) {
        let ids = st.threads.archived.get(&(c.id, Tab::Search)).map(|a| a.ids.clone()).unwrap_or_default();
        return (ids.iter().filter_map(|id| store.thread(*id)).filter(|t| t.parent_id == Some(c.id)).collect(), vec![]);
    }
    let tags = st.threads.forum.tag_filter.get(&c.id).cloned().unwrap_or_default();
    let all = c.default_tag_setting.as_deref() == Some("match_all");
    let keep = |t: &&Channel| {
        tags.is_empty() || if all { tags.iter().all(|x| t.applied_tags.contains(x)) } else { tags.iter().any(|x| t.applied_tags.contains(x)) }
    };
    let by_creation = sort_of(st, c) == 1;
    let key = |t: &&Channel| std::cmp::Reverse(if by_creation { t.id } else { rules::last_activity(t) });
    let (mut cur, mut old): (Vec<&Channel>, Vec<&Channel>) = store.threads(c.id).into_iter().filter(keep).partition(|t| !rules::is_archived(t));
    cur.sort_by_key(key);
    // The pinned post leads.
    cur.sort_by_key(|t| !t.flags.contains(ChannelFlags::PINNED));
    let loaded: HashSet<Snowflake> = st.threads.archived.get(&(c.id, Tab::Archived)).map_or_else(HashSet::new, |a| a.ids.iter().copied().collect());
    old.retain(|t| loaded.contains(&t.id));
    old.sort_by_key(key);
    (cur, old)
}

/// `ForumChannelView`: guidelines, the search card (or the composer) and the posts.
pub fn view<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let f = &st.threads.forum;
    let mut content = col().w_full().max_w(960.0).gap(12.0).padding(16.0).on_layout(move |s: &mut S, r| {
        let f = &mut acc(s).threads.forum;
        if (f.width - (r[2] - 32.0)).abs() > 0.5 {
            f.width = r[2] - 32.0;
        }
    });
    if let Some(topic) = c.topic.as_ref().filter(|t| !t.is_empty()) {
        content = content.child(guidelines(store, st, c, topic, p, acc));
    }
    match &f.draft {
        Some(d) if d.forum == c.id => content = content.child(composer(store, st, c, d, p, acc, layer)),
        _ => content = content.child(search_card(store, st, c, p, acc)).child(filter_row(st, c, p, acc, layer)),
    }
    let (cur, old) = posts(store, st, c);
    let is_grid = grid(st, c);
    let searching = !f.query.text.trim().is_empty() || f.tag_filter.get(&c.id).is_some_and(|t| !t.is_empty());
    if !cur.is_empty() {
        content = content.child(cards(store, st, c, &cur, is_grid, p, acc));
    }
    let kind = if server_search(st) { Tab::Search } else { Tab::Archived };
    let entry = st.threads.archived.get(&(c.id, kind));
    let a = entry.cloned().unwrap_or_default();
    if !old.is_empty() {
        content = content
            .child(text(t("Older posts")).font(font(12.0, 600.0)).color(p.text_tertiary).pt(8.0))
            .child(cards(store, st, c, &old, is_grid, p, acc));
    }
    let notice = || col().w_full().padding(16.0).gap(8.0).items_center();
    let settled = !a.loading && !a.indexing;
    if a.indexing {
        content = content.child(
            notice()
                .role("status")
                .child(spinner(p.text_primary_muted))
                .child(text(t("Search is getting ready for this community. Trying again shortly...")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap()),
        );
    } else if a.loading {
        content = content.child(notice().child(spinner(p.text_primary_muted)));
    } else if cur.is_empty() && old.is_empty() && entry.is_some() && !a.failed {
        content = content.child(empty(searching, p));
    }
    let fid = c.id;
    // Load more (or the first older page when 25+ posts are active), or retry a failed list.
    let more = |label: &str, key: &str, retry: bool| {
        Button::new(label)
            .variant(ButtonVariant::Secondary)
            .size(ButtonSize::Small)
            .key(key)
            .on_press(move |s: &mut S| {
                let store_key = (fid, kind);
                let st = acc(s);
                if retry {
                    // A cleared key reloads the list from the start (`tick`); without search the
                    // missing list does.
                    if st.threads.search_disabled {
                        st.threads.archived.remove(&store_key);
                    } else {
                        let e = st.threads.archived.entry(store_key).or_default();
                        (e.key, e.failed) = (String::new(), false);
                    }
                    return;
                }
                let e = st.threads.archived.entry(store_key).or_default();
                e.loading = true;
                let o = if st.threads.search_disabled { Op::LoadArchived { parent: fid, tab: kind, more: true } } else { Op::Search { parent: fid, tab: kind, query: e.key.clone(), more: true } };
                st.intents.push(Intent::Thread(o));
            })
            .el(p)
    };
    if a.failed {
        content = content.child(
            notice()
                .role("alert")
                .child(text(t("Couldn't load posts. Try again later.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted))
                .child(more(t("Retry"), "forum-retry", true)),
        );
    } else if settled && entry.is_some() && a.has_more {
        content = content.child(notice().child(more(t("Load older posts"), "forum-older", false)));
    } else if settled && entry.is_none() && kind == Tab::Archived && !st.threads.search_disabled {
        // The first older page waits for a press while 25+ posts are active (`needsFirstPage`).
        content = content.child(notice().child(more(t("Load older posts"), "forum-older", true)));
    }
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).key("forum-posts").items_center().child(content);
    col().grow(1.0).min_w(0.0).h_full().min_h(0.0).fill(p.background_primary).key(format!("forum-{}", c.id)).child(scroller(viewport, true, p))
}

/// `ForumGuidelines`: the channel topic, collapsed under "Post guidelines".
fn guidelines<S: 'static>(store: &Store, st: &ShellState, c: &Channel, topic: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let open = st.threads.forum.guidelines;
    let mut card = col().w_full().radius(8.0).fill(p.background_secondary).child(
        row()
            .w_full()
            .py(10.0)
            .px(12.0)
            .gap(8.0)
            .center_y()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(if open { t("Hide guidelines") } else { t("Show guidelines") })
            .key("forum-guidelines")
            .on_press(move |s: &mut S| acc(s).threads.forum.guidelines ^= true)
            .child(text(t("Post guidelines")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).grow(1.0))
            .child(icon(if open { &icons::CARET_UP_BOLD } else { &icons::CARET_DOWN_BOLD }, 16.0).color(p.text_secondary)),
    );
    if open {
        let md = super::markdown::Md::new(store, &st.media, c.guild_id, p, format!("forum-topic-{}", c.id));
        card = card.child(col().w_full().px(12.0).pb(12.0).child(super::markdown::render(topic, &md)));
    }
    card
}

/// `ForumSearchCard`: search the posts, or start one with the typed title.
fn search_card<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let q = &st.threads.forum.query;
    let posting = can_post(store, c);
    let fid = c.id;
    let field = text_input(q, font(15.0, 400.0).lh(20.0))
        .single_line()
        .key("forum-search")
        .grow(1.0)
        .shrink()
        .color(p.text_primary)
        .placeholder(if posting { t("Search or create a post...") } else { t("Search posts") }, p.text_tertiary)
        .caret(p.text_primary, p.background_modifier_selected)
        .on_edit(move |s: &mut S| -> &mut TextEdit { &mut acc(s).threads.forum.query })
        .on_input_key(move |s: &mut S, k, _| {
            // Shift+Enter starts a post titled with the query.
            if k.key != rune::Key::Enter || !k.mods.shift || !posting {
                return false;
            }
            let st = acc(s);
            let title = st.threads.forum.query.text.trim().to_string();
            new_post(st, fid, title);
            true
        });
    let mut entry = row().w_full().min_h(48.0).py(6.0).pl(12.0).pr(6.0).gap(8.0).center_y();
    // Without search on the instance only "New post" shows (`.entrySpacer`).
    entry = if st.threads.search_disabled {
        entry.child(el().grow(1.0))
    } else {
        entry.child(icon(&icons::MAGNIFYING_GLASS_BOLD, 18.0).color(p.text_tertiary)).child(row().grow(1.0).min_w(0.0).h(34.0).center_y().child(field))
    };
    let searching = !q.text.trim().is_empty();
    if searching {
        entry = entry.child(
            row()
                .size(28.0, 28.0)
                .center()
                .radius(6.0)
                .hover(p.background_modifier_hover)
                .hover_group()
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Clear search"))
                .on_press(move |s: &mut S| acc(s).threads.forum.query = TextEdit::new(""))
                .child(icon(&icons::X_BOLD, 16.0).color(p.text_tertiary).group_color(p.text_primary)),
        );
    }
    if posting {
        entry = entry.child(
            Button::new(t("New post"))
                .size(ButtonSize::Small)
                .key("forum-new-post")
                .on_press(move |s: &mut S| {
                    let st = acc(s);
                    let title = st.threads.forum.query.text.trim().to_string();
                    new_post(st, fid, title);
                })
                .el(p),
        );
    }
    let border = if q.focused { p.background_modifier_accent_focus } else { p.background_modifier_accent };
    let mut card = col().w_full().radius(8.0).fill(p.form_surface_background).border(1.0, over(border, p.form_surface_background)).child(entry);
    if searching {
        let n = posts(store, st, c).0.len();
        // `SearchStatus`.
        let pending = st.threads.archived.get(&(c.id, Tab::Search)).is_none_or(|a| a.loading || a.indexing);
        let status = match n {
            _ if pending => t("Searching...").to_string(),
            0 => t("No matching posts").to_string(),
            n => crate::i18n::tp("{count, plural, one {# matching post} other {# matching posts}}", n as i64, &[]),
        };
        let small = font(13.0, 400.0).lh(16.0);
        let mut footer = row()
            .w_full()
            .py(8.0)
            .px(12.0)
            .gap(16.0)
            .center_y()
            .child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(text(status).font(small).color(p.text_primary_muted))
                    .child(text(t("Clear")).font(font(13.0, 500.0).lh(16.0)).color(p.text_link).cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| acc(s).threads.forum.query = TextEdit::new(""))),
            )
            .child(el().grow(1.0));
        if posting {
            footer = footer.child(text("Shift + Enter to create a post").font(small).color(p.text_primary_muted));
        }
        card = card.child(footer.child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(1.0).fill(p.background_modifier_accent)));
    }
    card
}

fn new_post(st: &mut ShellState, forum: Snowflake, title: String) {
    let mut t = TextEdit::new(title);
    t.focused = true;
    st.threads.forum.draft = Some(Draft { forum, title: t, body: TextEdit::new(""), tags: vec![], error: None, preview: false });
    st.threads.forum.query = TextEdit::new("");
    st.threads.uploads.clear();
}

/// `ForumFilterRow`: Sort & view, then the tag filter pills.
fn filter_row<S: 'static>(st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let open = st.popups.is_open(SORT);
    let sort_b = row()
        .h(32.0)
        .px(12.0)
        .gap(6.0)
        .center_y()
        .radius(16.0)
        .fill(if open { p.background_modifier_selected } else { p.background_secondary_alt })
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .key(SORT)
        .on_press(move |s: &mut S| acc(s).popups.toggle(SORT))
        .child(icon(&icons::SORT_ASCENDING_BOLD, 16.0).color(p.text_primary))
        .child(text(t("Sort & view")).font(font(14.0, 500.0).lh(18.0)).color(p.text_primary))
        .child(icon(&icons::CARET_DOWN_BOLD, 12.0).color(p.text_primary));
    let mut r = row().w_full().flex_wrap().gaps(8.0, 8.0).center_y().child(sort_b);
    if !c.available_tags.is_empty() {
        r = r.child(el().w(1.0).h(24.0).mx(2.0).fill(p.background_modifier_accent));
        let selected = st.threads.forum.tag_filter.get(&c.id).cloned().unwrap_or_default();
        let fid = c.id;
        for t in &c.available_tags {
            let on = selected.contains(&t.id);
            let tid = t.id;
            r = r.child(tag_pill(st, t, Some(on), false, p).key(format!("forum-filter-{tid}")).on_press(move |s: &mut S| {
                let f = acc(s).threads.forum.tag_filter.entry(fid).or_default();
                if let Some(i) = f.iter().position(|x| *x == tid) {
                    f.remove(i);
                } else {
                    f.push(tid);
                }
            }));
        }
        if !selected.is_empty() {
            r = r.child(
                row()
                    .py(2.0)
                    .px(8.0)
                    .radius(16.0)
                    .hover(p.background_modifier_hover)
                    .hover_group()
                    .cursor(rune::Cursor::Pointer)
                    .child(text(t("Clear")).font(font(12.0, 600.0).lh(20.0)).color(p.text_primary_muted).group_color(p.text_primary))
                    .on_press(move |s: &mut S| {
                        acc(s).threads.forum.tag_filter.remove(&fid);
                    }),
            );
        }
    }
    if open {
        layer.push(sort_panel(st, c, p, acc));
    }
    r
}

/// `ForumSortViewPanel`.
fn sort_panel<S: 'static>(st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let fid = c.id;
    let sort = sort_of(st, c).to_string();
    let sorts: [(&str, &str, Option<&str>); 2] = [("0", "Recently active", None), ("1", "Date posted", None)];
    let layout = if grid(st, c) { "2" } else { "1" };
    let layouts: [(&str, &str, Option<&str>); 2] = [("1", "List", None), ("2", "Gallery", None)];
    let heading = |t: &str| text(t.to_string()).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted);
    let mut panel = col()
        .min_w(224.0)
        .py(12.0)
        .px(16.0)
        .gap(16.0)
        .radius(8.0)
        .fill(p.background_secondary)
        .border(1.0, over(p.background_modifier_accent, p.background_secondary))
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .child(col().gap(4.0).child(heading(t("Sort by"))).child(RadioGroup::new(&sorts, Some(&sort)).on_change(move |s: &mut S, v| {
            acc(s).threads.forum.sort.insert(fid, v.parse().unwrap_or(0));
        }).el(p)));
    if c.kind != reflux_model::ChannelType::GUILD_MEDIA {
        panel = panel.child(col().gap(4.0).child(heading(t("View as"))).child(RadioGroup::new(&layouts, Some(layout)).on_change(move |s: &mut S, v| {
            acc(s).threads.forum.layout.insert(fid, v.parse().unwrap_or(1));
        }).el(p)));
    }
    anchor(panel.modal(true, move |s: &mut S| acc(s).popups.close()), SORT, Placement::BottomStart, 8.0)
}

/// A forum emoji (tag or default reaction): custom by ID, else the unicode sprite.
pub(crate) fn forum_emoji(st: &ShellState, id: Option<Snowflake>, name: Option<&str>, size: f32) -> Option<El> {
    let load = match (id, name) {
        (Some(id), _) => st.media.asset(|b, _| crate::media::emoji_url(b, &id.to_string())),
        (None, Some(n)) => emoji::sprite_index(n).map_or(Load::Failed, |i| st.media.sprite(i)),
        _ => return None,
    };
    Some(match load {
        Load::Ready(px) => el().size(size, size).pixels_fit(&px, rune::SurfaceFit::Fit),
        _ => text(name.unwrap_or_default().to_string()).font(font(size - 2.0, 400.0).lh(size)),
    })
}

/// `ForumTagPill`; `selected: None` = a plain label.
pub(crate) fn tag_pill(st: &ShellState, t: &ForumTag, selected: Option<bool>, disabled: bool, p: &Palette) -> El {
    let on = selected == Some(true);
    let (bg, border, fg) = if on { (p.brand_primary, p.brand_primary, p.text_on_brand_primary) } else { (p.background_secondary_alt, p.background_modifier_accent, p.text_secondary) };
    let mut pill = row()
        .max_w(192.0)
        .py(2.0)
        .px(8.0)
        .gap(4.0)
        .center_y()
        .radius(16.0)
        .fill(bg)
        .border(1.0, over(border, bg))
        .children(forum_emoji(st, t.emoji_id, t.emoji_name.as_deref(), 16.0))
        .child(text(t.name.clone()).font(font(12.0, 600.0).lh(20.0)).color(fg).truncate().shrink());
    if selected.is_some() {
        pill = pill.cursor(rune::Cursor::Pointer).role("button").checked(on);
        if !on {
            pill = pill.hover(p.background_modifier_hover);
        }
    }
    if disabled {
        pill = pill.opacity(0.5);
    }
    pill
}

fn empty(searching: bool, p: &Palette) -> El {
    let (title, hint) = if searching {
        ("No posts match your search", "Try different words or clear the tag filter.")
    } else {
        ("There are no posts yet", "Be the first to start a conversation here.")
    };
    col()
        .w_full()
        .py(48.0)
        .px(16.0)
        .gap(8.0)
        .items_center()
        .child(icon(&icons::CHATS_TEARDROP, 48.0).color(p.text_tertiary))
        .child(text(title).font(font(18.0, 600.0).lh(24.0)).color(p.text_primary))
        .child(text(hint).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted))
}

fn cards<S: 'static>(store: &Store, st: &ShellState, forum: &Channel, posts: &[&Channel], is_grid: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let columns = grid_columns(st.threads.forum.width).0;
    let list = if is_grid { el().w_full().equal_columns(columns).gap(12.0) } else { col().w_full().gap(8.0) };
    list.children(posts.iter().map(|t| card(store, st, forum, t, is_grid, p, acc)))
}

/// `.postGrid`: `repeat(auto-fill, minmax(14rem, 1fr))` with a 12px gap: (columns, cell width).
fn grid_columns(width: f32) -> (usize, f32) {
    // Before the first layout: the content's widest (60rem less padding).
    let width = if width > 0.0 { width } else { 928.0 };
    let n = (((width + 12.0) / (224.0 + 12.0)).floor() as usize).max(1);
    (n, ((width - 12.0 * (n - 1) as f32) / n as f32).max(1.0))
}

/// The first image of a message: (proxy url, video).
fn first_media(m: &Message) -> Option<(String, bool)> {
    m.attachments.iter().find(|a| a.content_type.starts_with("image/") || a.content_type.starts_with("video/")).and_then(|a| Some((a.proxy_url.clone().or(a.url.clone())?, a.content_type.starts_with("video/"))))
}

/// `ForumPostCard`.
fn card<S: 'static>(store: &Store, st: &ShellState, forum: &Channel, post: &Channel, is_grid: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let first = st.threads.forum.first.get(&post.id);
    let loaded_first = store.messages(post.id).first().filter(|m| m.id == post.id);
    let first: Option<Option<&Message>> = match (loaded_first, first) {
        (Some(m), _) => Some(Some(m)),
        (None, Some(x)) => Some(x.as_ref()),
        (None, None) => None,
    };
    let joined = post.member.is_some();
    let post_unreads = if joined { 0 } else { post_unread_count(store, st, post) };
    let unread = (joined && store.has_unread(post.id)) || post_unreads > 0;
    let is_new = !unread && is_new_post(store, st, post);
    let open = st.threads.panel == Some(threads::Panel::Thread(post.id));
    let pinned = post.flags.contains(ChannelFlags::PINNED);
    // header: pin, tags
    let tags: Vec<&ForumTag> = post.applied_tags.iter().filter_map(|id| forum.available_tags.iter().find(|t| t.id == *id)).collect();
    let mut header = row().flex_wrap().gaps(6.0, 6.0).center_y();
    if pinned {
        header = header.child(icon(&icons::PUSH_PIN_SIMPLE_FILL, 16.0).color(p.text_tertiary).label(t("Pinned")));
    }
    if is_new {
        // `.newBadge`.
        header = header.child(row().px(6.0).radius(4.0).fill(p.status_danger).child(text(t("New").to_uppercase()).font(font(11.0, 700.0).lh(18.0)).color(p.text_on_dark)));
    }
    for t in tags.iter().take(MAX_CARD_TAGS) {
        header = header.child(tag_pill(st, t, None, false, p));
    }
    if tags.len() > MAX_CARD_TAGS {
        header = header.child(
            row().py(2.0).px(8.0).radius(16.0).fill(p.background_secondary_alt).border(1.0, over(p.background_modifier_accent, p.background_secondary_alt)).child(text(format!("+{}", tags.len() - MAX_CARD_TAGS)).font(font(12.0, 600.0).lh(20.0)).color(p.text_secondary)),
        );
    }
    let mut body = col().grow(1.0).min_w(0.0).gap(6.0);
    if pinned || is_new || !tags.is_empty() {
        body = body.child(header);
    }
    body = body.child(text(post.name.clone().unwrap_or_default()).font(font(16.0, 600.0).lh(20.0)).color(if unread { p.text_primary } else { p.text_secondary }).truncate());
    let preview = match first {
        Some(Some(m)) => Some((Some(store.display_name(forum.guild_id, m.author.id)), threads::notification_body(store, m))),
        Some(None) => Some((None, "Original message was deleted".to_string())),
        None => None,
    };
    if let Some((author, line)) = preview {
        // `.cardPreview`: one run of text clamped to two lines, the author inline in bold.
        let (line, spans) = match author.filter(|_| !line.is_empty()) {
            Some(a) => {
                let head = format!("{a}: ");
                let n = head.len();
                (head + &line, vec![Span::new(0, n).weight(600.0).color(p.text_primary)])
            }
            None => (line, vec![]),
        };
        body = body.child(col().w_full().max_h(40.0).clip_rounded(0.0).child(rich(line, spans).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap().w_full()));
    }
    let small = font(13.0, 400.0).lh(16.0);
    let mut footer = row().flex_wrap().gaps(8.0, 8.0).center_y();
    if let (Some(r), Some(Some(m))) = (&forum.default_reaction_emoji, first) {
        footer = footer.child(default_reaction(st, post, m, r, p, acc));
    }
    footer = footer.child(row().gap(4.0).center_y().child(icon(&icons::CHAT_CIRCLE_FILL, 14.0).color(p.text_primary_muted)).child(text(threads::message_count(post.message_count)).font(small).color(p.text_primary_muted)));
    if unread {
        let label = if post_unreads > 0 { crate::i18n::tp("{count, plural, one {# new message} other {# new messages}}", post_unreads as i64, &[]) } else { t("New messages").to_string() };
        footer = footer.child(text(label).font(font(13.0, 600.0).lh(16.0)).color(p.brand_primary_light));
    }
    footer = footer.child(text(format_relative(rules::last_activity(post).timestamp_ms(), st.now_ms)).font(small).color(p.text_primary_muted));
    body = body.child(footer);
    let media = first.flatten().and_then(first_media);
    let thumb = |w: f32, h: f32, r: f32| match &media {
        Some((u, _)) => super::picture(st.media.get(&crate::media::fit_url(u, w, st.media.scale())), w, h, r, p),
        None => row().size(w, h).center().radius(r).fill(p.background_tertiary).child(icon(&icons::IMAGE, 32.0).color(p.text_tertiary)),
    };
    let (thread, id, parent) = (post.clone(), post.id, post.parent_id);
    let mut c = if is_grid {
        // `.cardMedia`: full width, 4:3.
        let w = grid_columns(st.threads.forum.width).1;
        col().w_full().min_w(0.0).clip_rounded(8.0).child(thumb(w, w * 0.75, 0.0)).child(col().w_full().padding(12.0).child(body))
    } else {
        let mut r = row().w_full().padding(12.0).gap(12.0).items_start().child(body);
        if media.is_some() {
            r = r.child(thumb(96.0, 96.0, 6.0));
        }
        r
    };
    let (bg, border) = if open { (p.background_modifier_selected, p.background_modifier_accent_focus) } else { (p.background_secondary, rune::Color::TRANSPARENT) };
    c = c.radius(8.0).fill(bg).border(1.0, over(border, bg)).cursor(rune::Cursor::Pointer).role("button").key(format!("post-{id}"));
    if !open {
        c = c.hover(p.background_modifier_hover);
    }
    c.on_press_tx(move |s: &mut S, tx: &mut Tx| {
        let st = acc(s);
        if tx.mods().shift { threads::open_full(st, id) } else { threads::open(st, parent, thread.id) }
    })
    .on_secondary_press(move |s: &mut S, at, _| threads::open_menu(acc(s), id, [at.x, at.y]))
}

/// `DefaultReactionButton`: the forum's reaction on the post's first message, with its count.
fn default_reaction<S: 'static>(st: &ShellState, post: &Channel, m: &Message, r: &reflux_model::DefaultReaction, p: &Palette, acc: ShellAccess<S>) -> El {
    let mine = m.reactions.iter().find(|x| if r.emoji_id.is_some() { x.emoji.id == r.emoji_id } else { Some(&x.emoji.name) == r.emoji_name.as_ref() });
    let (me, count) = mine.map_or((false, 0), |x| (x.me, x.count));
    let (bg, border, fg) = if me { (p.brand_tint, p.brand_tint_border, p.text_on_brand_tint) } else { (p.background_tertiary, rune::Color::TRANSPARENT, p.text_primary_muted) };
    let emoji = ReactionEmoji { id: r.emoji_id, name: r.emoji_name.clone().unwrap_or_else(|| "emoji".into()), ..Default::default() };
    let (ch, id) = (post.id, m.id);
    let mut b = row()
        .py(2.0)
        .px(6.0)
        .gap(6.0)
        .center_y()
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(border, bg))
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .checked(me)
        .children(forum_emoji(st, r.emoji_id, r.emoji_name.as_deref(), 20.0).map(|e| e.my(2.0)));
    if count > 0 {
        b = b.child(text(count.to_string()).font(font(14.0, 600.0).lh(16.0)).color(fg));
    }
    let locked = rules::is_locked(post) || rules::is_archived(post);
    if locked {
        return b;
    }
    b.on_press(move |s: &mut S| acc(s).intents.push(Intent::React { channel: ch, message: id, emoji: emoji.clone(), add: !me }))
}

// ---- Composer ------------------------------------------------------------------------------------

/// `ForumPostComposer`: title, body (or its preview), tags and attachments, then Post.
fn composer<S: 'static>(store: &Store, st: &ShellState, c: &Channel, d: &Draft, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let icon_button = |i: &'static rune::Icon, label: &str, active: bool| {
        let mut b = row().size(32.0, 32.0).center().radius(6.0).cursor(rune::Cursor::Pointer).role("button").label(label.to_string()).hover_group().child(icon(i, 20.0).color(if active { p.text_primary } else { p.text_tertiary }).group_color(p.text_primary));
        b = if active { b.fill(p.background_modifier_selected) } else { b.hover(p.background_modifier_hover) };
        b
    };
    let title = text_input(&d.title, font(20.0, 600.0).lh(28.0))
        .single_line()
        .key("forum-post-title")
        .grow(1.0)
        .shrink()
        .color(p.text_primary)
        .placeholder(t("Give your post a title"), p.text_tertiary)
        .caret(p.text_primary, p.background_modifier_selected)
        .label(t("Post title"))
        .on_edit(move |s: &mut S| -> &mut TextEdit { &mut draft(acc(s)).title })
        .on_input_key(move |s: &mut S, k, _| {
            if k.key != rune::Key::Enter {
                return false;
            }
            let d = draft(acc(s));
            d.title.focused = false;
            d.body.focused = true;
            true
        });
    let head = row()
        .w_full()
        .gap(8.0)
        .items_start()
        .child(row().grow(1.0).min_w(0.0).py(4.0).child(title))
        .child(icon_button(&icons::X_BOLD, t("Discard post"), false).key("forum-post-discard").on_press(move |s: &mut S| acc(s).threads.forum.draft = None));
    let mut card = col()
        .w_full()
        .gap(8.0)
        .pt(12.0)
        .pb(8.0)
        .pl(16.0)
        .pr(12.0)
        .radius(8.0)
        .fill(p.form_surface_background)
        .border(1.0, over(p.background_modifier_accent_focus, p.form_surface_background))
        .key("forum-composer")
        .child(head);
    if d.preview {
        let md = super::markdown::Md::new(store, &st.media, c.guild_id, p, "forum-preview");
        let inner = if d.body.text.trim().is_empty() {
            text(t("Nothing to preview yet.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted)
        } else {
            super::markdown::render(&d.body.text, &md)
        };
        card = card.child(
            col()
                .w_full()
                .min_h(48.0)
                .py(8.0)
                .px(12.0)
                .gap(6.0)
                .radius(6.0)
                .border(1.0, p.background_modifier_accent)
                .child(text(t("Preview")).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted))
                .child(inner),
        );
    } else {
        let body = text_input(&d.body, font(16.0, 400.0).lh(22.0))
            .key("forum-post-body")
            .w_full()
            .color(p.text_chat)
            .placeholder(if c.kind == reflux_model::ChannelType::GUILD_MEDIA { t("Add a description (optional)") } else { t("Enter a message") }, p.text_tertiary_secondary)
            .caret(p.text_primary, p.text_selection)
            .on_edit(move |s: &mut S| -> &mut TextEdit { &mut draft(acc(s)).body });
        card = card.child(col().w_full().min_h(48.0).child(body));
    }
    if !st.threads.uploads.is_empty() {
        let mut files = row().flex_wrap().gaps(6.0, 6.0);
        for u in &st.threads.uploads {
            let uid = u.id;
            files = files.child(
                row()
                    .py(4.0)
                    .pl(8.0)
                    .pr(4.0)
                    .gap(4.0)
                    .center_y()
                    .radius(6.0)
                    .fill(p.background_secondary_alt)
                    .child(icon(&icons::FILE_TEXT, 16.0).color(p.text_tertiary))
                    .child(text(u.name.clone()).font(font(13.0, 500.0).lh(16.0)).color(p.text_primary).truncate().max_w(200.0))
                    .child(
                        row().size(20.0, 20.0).center().radius(4.0).hover(p.background_modifier_hover).cursor(rune::Cursor::Pointer).label(t("Remove")).child(icon(&icons::X_BOLD, 12.0).color(p.text_tertiary)).on_press(move |s: &mut S| acc(s).threads.uploads.retain(|x| x.id != uid)),
                    ),
            );
        }
        card = card.child(files);
    }
    if let Some(e) = &d.error {
        card = card.child(text(e.clone()).font(font(13.0, 400.0).lh(16.0)).color(p.status_danger));
    }
    // `.composerBar`.
    let mut start = row().flex_wrap().gaps(4.0, 4.0).center_y().min_w(0.0).grow(1.0).shrink();
    start = start.child(icon_button(&icons::IMAGE_SQUARE_BOLD, t("Add images or files"), false).key("forum-post-attach").on_press_tx(move |_: &mut S, tx: &mut Tx| {
        let options = rune::FileDialogOptions { title: "Upload file".into(), ..Default::default() };
        tx.open_file(options, move |s: &mut S, r, _| match r {
            Ok(Some(path)) => {
                let st = acc(s);
                std::mem::swap(&mut st.uploads, &mut st.threads.uploads);
                super::upload::add_paths(st, &[path]);
                std::mem::swap(&mut st.uploads, &mut st.threads.uploads);
            }
            Ok(None) => {}
            Err(e) => acc(s).notice = Some(("Upload failed".into(), e.to_string())),
        });
    }));
    start = start.child(icon_button(&icons::SMILEY_BOLD, t("Add emoji"), false));
    if !c.available_tags.is_empty() {
        let mut tags = row().flex_wrap().gaps(6.0, 6.0).center_y().ml(4.0).min_w(0.0);
        tags = tags.child(Button::new(t("Add tags")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).key(TAG_PICKER).on_press(move |s: &mut S| acc(s).popups.toggle(TAG_PICKER)).el(p));
        for t in d.tags.iter().filter_map(|id| c.available_tags.iter().find(|x| x.id == *id)) {
            let tid = t.id;
            tags = tags.child(tag_pill(st, t, Some(true), false, p).on_press(move |s: &mut S| draft(acc(s)).tags.retain(|x| *x != tid)));
        }
        if require_tag(c) && d.tags.is_empty() {
            tags = tags.child(text(t("Required")).font(font(12.0, 400.0)).color(p.text_primary_muted));
        }
        start = start.child(tags);
        if st.popups.is_open(TAG_PICKER) {
            layer.push(tag_picker(store, st, c, d, p, acc));
        }
    }
    let ready = !d.title.text.trim().is_empty() && (!d.body.text.trim().is_empty() || !st.threads.uploads.is_empty()) && (!require_tag(c) || !d.tags.is_empty());
    let fid = c.id;
    let end = row()
        .gap(4.0)
        .center_y()
        .child(icon_button(&icons::EYE_BOLD, t("Preview"), d.preview).key("forum-post-preview").on_press(move |s: &mut S| draft(acc(s)).preview ^= true))
        .child(Button::new(t("Post")).size(ButtonSize::Small).disabled(!ready).key("forum-post-submit").on_press(move |s: &mut S| submit(acc(s), fid)).el(p));
    card.child(
        row()
            .w_full()
            .ml(-8.0)
            .mr(-4.0)
            .pt(8.0)
            .gap(8.0)
            .center_y()
            .child(start)
            .child(end)
            .child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(1.0).fill(p.background_modifier_accent)),
    )
}

/// The draft being edited (a throwaway one if it closed under a stale handler).
fn draft(st: &mut ShellState) -> &mut Draft {
    st.threads.forum.draft.get_or_insert_with(|| Draft { forum: Snowflake(0), title: TextEdit::new(""), body: TextEdit::new(""), tags: vec![], error: None, preview: false })
}

/// Post: checks the title and required tags, then creates the post.
fn submit(st: &mut ShellState, forum: Snowflake) {
    let Some(d) = st.threads.forum.draft.as_mut() else { return };
    let name: String = d.title.text.trim().chars().take(100).collect();
    if name.is_empty() {
        d.error = Some(t("Give the post a title before you send.").into());
        return;
    }
    let (content, tags) = (d.body.text.trim().to_string(), d.tags.clone());
    let attachments = std::mem::take(&mut st.threads.uploads);
    st.threads.forum.draft = None;
    st.intents.push(Intent::Thread(Op::Post { forum, name, content, tags, attachments }));
}

/// `ForumTagPicker` in its popout over "Add tags".
fn tag_picker<S: 'static>(store: &Store, st: &ShellState, c: &Channel, d: &Draft, p: &Palette, acc: ShellAccess<S>) -> El {
    let full = d.tags.len() >= MAX_TAGS;
    let moderator = store.is_thread_moderator(c.id);
    let mut pills = row().flex_wrap().gaps(6.0, 6.0).center_y();
    for t in &c.available_tags {
        let on = d.tags.contains(&t.id);
        let disabled = (t.moderated && !moderator) || (!on && full);
        let tid = t.id;
        let mut pill = tag_pill(st, t, Some(on), disabled, p).key(format!("forum-pick-{tid}"));
        if !disabled {
            pill = pill.on_press(move |s: &mut S| {
                let d = draft(acc(s));
                if let Some(i) = d.tags.iter().position(|x| *x == tid) {
                    d.tags.remove(i);
                } else {
                    d.tags.push(tid);
                    d.error = None;
                }
            });
        }
        pills = pills.child(pill);
    }
    let surface = col()
        .max_w(352.0)
        .padding(12.0)
        .gap(6.0)
        .radius(8.0)
        .fill(p.background_secondary)
        .border(1.0, over(p.background_modifier_accent, p.background_secondary))
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .child(pills)
        .child(text(crate::i18n::tf("You can pick up to {count} tags.", &[("count", &MAX_TAGS.to_string())])).font(font(12.0, 400.0)).color(p.text_primary_muted))
        .modal(true, move |s: &mut S| acc(s).popups.close());
    anchor(surface, TAG_PICKER, Placement::Top, 8.0)
}
