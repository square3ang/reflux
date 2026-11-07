//! Runs thread and forum requests ([`crate::shell::threads::Op`], `http-api/threads.mdx`,
//! `thread-members.mdx`, `forums.mdx`), keeps the thread subscription of the open guild, loads
//! what an open forum shows and acks the thread open in the side panel.

use std::sync::Arc;

use reflux_model::{Channel, ChannelType, DispatchEvent, GatewayPayload, GuildSubscription, LazyRequest, Message, MessageAck, OpCode, Snowflake, ThreadMember};
use reflux_net::{Command, ConnectionState};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Instance, Route, now_ms, spawn, update};
use crate::State;
use crate::shell::threads::{self, Op, Tab};
use crate::shell::upload::Upload;
use crate::i18n::t;

/// `ATTACHMENT_FLAG_SPOILER`.
const SPOILER: u32 = 1 << 3;

#[derive(Deserialize, Default)]
#[serde(default)]
struct ThreadList {
    threads: Vec<Channel>,
    members: Vec<ThreadMember>,
    has_more: bool,
}

pub(super) fn run(s: &mut State, o: Op) {
    let Some(inst) = s.app.instance.clone() else { return };
    let generation = s.app.generation;
    // Runs `f` on the answer unless the session changed meanwhile; failures are logged and
    // handed to `f` too.
    fn call<T: for<'de> Deserialize<'de> + Send + 'static>(
        inst: Arc<Instance>,
        generation: u64,
        method: Method,
        path: String,
        body: Option<Value>,
        f: impl FnOnce(&mut State, reflux_net::Result<T>) + Send + 'static,
    ) {
        spawn(async move {
            let r = inst.rest.request::<T, Value>(method.clone(), &path, body.as_ref()).await;
            if let Err(e) = &r {
                eprintln!("reflux: {method} {path}: {e}");
            }
            update(move |s| {
                if s.app.generation == generation {
                    f(s, r)
                }
            });
        });
    }
    let notice = |s: &mut State, what: &str, e: reflux_net::Error| s.app.shell.notice = Some((what.to_string(), crate::guild::describe(&e)));
    match o {
        Op::Open(t) => {
            if !s.app.store.messages.contains_key(&t) {
                super::fetch_messages(s, t);
            }
        }
        Op::Create { parent, message, name, private, content } => {
            let announcement = s.app.store.channels.get(&parent).is_some_and(|c| c.kind == ChannelType::GUILD_ANNOUNCEMENT);
            let (path, body) = match message {
                Some(m) => (format!("/channels/{parent}/messages/{m}/threads"), json!({ "name": name })),
                None => {
                    let kind = if private { 12 } else if announcement { 10 } else { 11 };
                    (format!("/channels/{parent}/threads"), json!({ "name": name, "type": kind }))
                }
            };
            call::<Channel>(inst, generation, Method::POST, path, Some(body), move |s, r| match r {
                Ok(t) => {
                    s.app.store.add_threads(std::slice::from_ref(&t), &[]);
                    if !content.is_empty() {
                        super::chat::send(s, t.id, content, None, vec![]);
                    }
                    threads::open(&mut s.app.shell, t.parent_id, t.id);
                }
                Err(e) => {
                    // The text goes back into the pane.
                    let sh = &mut s.app.shell;
                    if sh.threads.composer.text.is_empty() {
                        sh.threads.composer = rune::TextEdit::new(content);
                    }
                    sh.notice = Some(("Could not start the thread".into(), crate::guild::describe(&e)));
                }
            });
        }
        Op::Post { forum, name, content, tags, attachments } => {
            let presigned = inst.discovery.features.presigned_attachment_uploads;
            let back = (name.clone(), content.clone(), tags.clone(), attachments.clone());
            spawn(async move {
                let r = create_post(&inst, forum, name, content, tags, attachments, presigned).await;
                update(move |s| {
                    if s.app.generation != generation {
                        return;
                    }
                    match r {
                        Ok(v) => {
                            let first: Option<Message> = v.get("message").cloned().and_then(|m| serde_json::from_value(m).ok());
                            if let Ok(t) = serde_json::from_value::<Channel>(v) {
                                s.app.store.add_threads(std::slice::from_ref(&t), &[]);
                                s.app.shell.threads.forum.first.insert(t.id, first);
                                threads::open(&mut s.app.shell, t.parent_id, t.id);
                            }
                        }
                        Err(e) => {
                            eprintln!("reflux: post in {forum}: {e}");
                            // The draft comes back as it was.
                            let (title, body, tags, files) = back;
                            let sh = &mut s.app.shell;
                            sh.threads.forum.draft = Some(crate::shell::forum::Draft {
                                forum,
                                title: rune::TextEdit::new(title),
                                body: rune::TextEdit::new(body),
                                tags,
                                error: Some(crate::i18n::tf("Couldn't create the post: {detail}", &[("detail", &(crate::guild::describe(&e)).to_string())])),
                                preview: false,
                            });
                            sh.threads.uploads = files;
                        }
                    }
                });
            });
        }
        Op::Patch { channel, body } => call::<Channel>(inst, generation, Method::PATCH, format!("/channels/{channel}"), Some(body), move |s, r| match r {
            Ok(c) if c.kind.is_thread() => s.app.store.add_threads(&[c], &[]),
            Ok(_) => {}
            Err(e) => notice(s, t("Something went wrong"), e),
        }),
        Op::Delete(t) => call::<Value>(inst, generation, Method::DELETE, format!("/channels/{t}"), None, move |s, r| {
            if let Err(e) = r {
                notice(s, crate::i18n::t("Something went wrong"), e);
            }
        }),
        Op::Join(t) => call::<Value>(inst, generation, Method::PUT, format!("/channels/{t}/thread-members/@me"), None, move |s, r| {
            if let Err(e) = r {
                notice(s, crate::i18n::t("Something went wrong"), e);
            }
        }),
        Op::Leave(t) => call::<Value>(inst, generation, Method::DELETE, format!("/channels/{t}/thread-members/@me"), None, move |s, r| {
            if let Err(e) = r {
                notice(s, crate::i18n::t("Something went wrong"), e);
            }
        }),
        Op::MemberSettings { thread, body } => {
            call::<Value>(inst, generation, Method::PATCH, format!("/channels/{thread}/thread-members/@me/settings"), Some(body), move |s, r| {
                if let Err(e) = r {
                    notice(s, t("Something went wrong"), e);
                }
            })
        }
        Op::LoadArchived { parent, tab, more } => {
            let store = &s.app.store;
            let loaded = s.app.shell.threads.archived.get(&(parent, tab)).map(|a| a.ids.clone()).unwrap_or_default();
            let last = more.then(|| loaded.last().and_then(|id| store.channels.get(id))).flatten();
            let moderator = store.is_thread_moderator(parent);
            let path = match tab {
                Tab::ArchivedPrivate if !moderator => {
                    let before = last.map_or(String::new(), |t| format!("&before={}", t.id));
                    format!("/channels/{parent}/users/@me/threads/archived/private?limit=50{before}")
                }
                _ => {
                    let kind = if tab == Tab::ArchivedPrivate { "private" } else { "public" };
                    let before = last.and_then(|t| t.thread_metadata.as_ref()).map_or(String::new(), |m| format!("&before={}", urlencode(&m.archive_timestamp)));
                    format!("/channels/{parent}/threads/archived/{kind}?limit=50{before}")
                }
            };
            call::<ThreadList>(inst, generation, Method::GET, path, None, move |s, r| {
                let a = s.app.shell.threads.archived.entry((parent, tab)).or_default();
                a.loading = false;
                match r {
                    Ok(l) => {
                        if !more {
                            a.ids.clear();
                        }
                        a.ids.extend(l.threads.iter().map(|t| t.id));
                        (a.has_more, a.failed) = (l.has_more, false);
                        s.app.store.add_threads(&l.threads, &l.members);
                    }
                    Err(_) => a.failed = true,
                }
            });
        }
        Op::Search { parent, tab, query, more } => {
            let offset = if more { s.app.shell.threads.archived.get(&(parent, tab)).map_or(0, |a| a.ids.len()) } else { 0 };
            spawn(search(inst, generation, parent, tab, query, more, offset));
        }
        Op::PostData { forum, ids } => {
            call::<Value>(inst, generation, Method::POST, format!("/channels/{forum}/post-data"), Some(json!({ "thread_ids": ids })), move |s, r| {
                let Ok(v) = r else { return };
                let Some(map) = v.get("threads").and_then(|t| t.as_object()) else { return };
                for (id, d) in map {
                    let Ok(id) = id.parse::<u64>() else { continue };
                    let first = d.get("first_message").cloned().and_then(|m| serde_json::from_value::<Message>(m).ok());
                    s.app.shell.threads.forum.first.insert(Snowflake(id), first);
                }
            });
        }
    }
}

/// `ForumPosts.fetchList` / `useThreadSearch`: waits out the debounce for a fresh name search,
/// then asks until the index is ready (202 → `retry_after`, doubled per attempt, at most 6
/// retries) and fills the `(parent, tab)` list unless its query changed meanwhile.
async fn search(inst: Arc<Instance>, generation: u64, parent: Snowflake, tab: Tab, query: String, more: bool, offset: usize) {
    // Still the list's query, in this session?
    let current = |query: String| async move {
        let (tx, rx) = tokio::sync::oneshot::channel();
        update(move |s| {
            let _ = tx.send(s.app.generation == generation && s.app.shell.threads.archived.get(&(parent, tab)).is_some_and(|a| a.key == query));
        });
        rx.await.unwrap_or(false)
    };
    if tab == Tab::Search && !more {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    let path = format!("/channels/{parent}/threads/search?{query}&limit={}&offset={offset}", threads::SEARCH_LIMIT);
    for attempt in 0..=6u32 {
        if !current(query.clone()).await {
            return;
        }
        let r = inst.rest.get::<Value>(&path).await;
        let ready = !matches!(&r, Ok(v) if v.get("threads").is_none());
        let q = query.clone();
        let retry_after = r.as_ref().ok().and_then(|v| v["retry_after"].as_f64());
        update(move |s| {
            if s.app.generation != generation || s.app.shell.threads.archived.get(&(parent, tab)).is_none_or(|a| a.key != q) {
                return;
            }
            match r {
                Ok(v) if ready => {
                    let l: ThreadList = serde_json::from_value(v.clone()).unwrap_or_default();
                    s.app.store.add_threads(&l.threads, &l.members);
                    // A forum list carries the posts' first messages.
                    for m in v["first_messages"].as_array().into_iter().flatten() {
                        if let Ok(m) = serde_json::from_value::<Message>(m.clone()) {
                            s.app.shell.threads.forum.first.insert(m.channel_id, Some(m));
                        }
                    }
                    let a = s.app.shell.threads.archived.entry((parent, tab)).or_default();
                    for t in l.threads.iter().filter(|t| t.parent_id == Some(parent)) {
                        if !a.ids.contains(&t.id) {
                            a.ids.push(t.id);
                        }
                    }
                    (a.has_more, a.loading, a.indexing, a.failed) = (l.has_more, false, false, false);
                }
                Ok(_) if attempt < 6 => s.app.shell.threads.archived.entry((parent, tab)).or_default().indexing = true,
                Err(e) if e.code() == Some("FEATURE_TEMPORARILY_DISABLED") && tab == Tab::Archived => {
                    // `fetchArchivedFallback`: page `archived/public` instead.
                    s.app.shell.threads.search_disabled = true;
                    run(s, Op::LoadArchived { parent, tab, more });
                }
                _ => {
                    let a = s.app.shell.threads.archived.entry((parent, tab)).or_default();
                    (a.loading, a.indexing, a.failed) = (false, false, true);
                }
            }
        });
        if ready {
            return;
        }
        // `searchRetryDelayMs`.
        let ms = (retry_after.unwrap_or(1.0).max(1.0) * 1000.0 * 2f64.powi(attempt as i32)).min(60_000.0);
        tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
    }
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// `POST /channels/{forum}/threads` with the first message and its files (pre-signed uploads
/// when the instance offers them, else one multipart request). Returns the forum post.
async fn create_post(inst: &Instance, forum: Snowflake, name: String, content: String, tags: Vec<Snowflake>, files: Vec<Upload>, presigned: bool) -> reflux_net::Result<Value> {
    let path = format!("/channels/{forum}/threads");
    let mut body = json!({ "name": name, "message": { "content": content }, "applied_tags": tags });
    if files.is_empty() {
        return inst.rest.request(Method::POST, &path, Some(&body)).await;
    }
    let data = tokio::task::spawn_blocking(move || files.into_iter().map(|u| u.source.read().map(|b| (u, Arc::new(b)))).collect::<Result<Vec<_>, String>>())
        .await
        .map_err(|e| reflux_net::Error::Upload(e.to_string()))?
        .map_err(reflux_net::Error::Upload)?;
    let meta = |u: &Upload| (Some(u.description.clone()).filter(|d| !d.is_empty()), if u.spoiler { SPOILER } else { 0 });
    if !presigned {
        body["message"]["attachments"] = data
            .iter()
            .enumerate()
            .map(|(i, (u, _))| {
                let (description, flags) = meta(u);
                json!({ "id": i, "filename": u.name, "description": description, "flags": flags })
            })
            .collect();
        let files = data.into_iter().map(|(u, b)| (u.name.clone(), b, u.content_type().to_string())).collect();
        return inst.rest.post_multipart(&path, &body, files, |_| {}).await;
    }
    let req: Vec<_> = data.iter().enumerate().map(|(i, (u, b))| (i as u32, u.name.clone(), b.len() as u64, u.content_type().to_string())).collect();
    let plans = inst.rest.upload_plan(forum, &req).await?;
    let sent = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let progress: Arc<dyn Fn(u64) + Send + Sync> = Arc::new(|_| {});
    let mut complete = Vec::new();
    for (plan, (_, b)) in plans.iter().zip(&data) {
        if let Some(url) = &plan.upload_url {
            inst.rest.put_upload(url, b.clone(), 0..b.len(), Some(&plan.content_type), sent.clone(), progress.clone()).await?;
            continue;
        }
        for part in &plan.parts {
            let start = ((part.part_number.max(1) - 1) as u64 * plan.part_size) as usize;
            let end = (start + plan.part_size as usize).min(b.len());
            inst.rest.put_upload(&part.upload_url, b.clone(), start..end, None, sent.clone(), progress.clone()).await?;
        }
        if let Some(id) = &plan.upload_id {
            complete.push((plan.upload_filename.clone(), id.clone()));
        }
    }
    if !complete.is_empty() {
        inst.rest.complete_uploads(forum, &complete).await?;
    }
    body["message"]["attachments"] = plans
        .iter()
        .zip(&data)
        .map(|(plan, (u, b))| {
            let (description, flags) = meta(u);
            json!({
                "id": plan.id, "filename": u.name, "upload_filename": plan.upload_filename, "file_size": b.len(),
                "content_type": plan.content_type, "description": description, "flags": flags,
            })
        })
        .collect();
    inst.rest.request(Method::POST, &path, Some(&body)).await
}

/// Every frame: the open guild's thread subscription (with the open thread's member list), an
/// open forum's older posts and first messages, and acks for the thread in the side panel.
pub(super) fn tick(s: &mut State) -> bool {
    if s.app.route != Route::Main {
        return false;
    }
    subscribe(s);
    let sh = &s.app.shell;
    let store = &s.app.store;
    let mut ops = Vec::new();
    // `/threads/search` lists whose query changed (re)load from the start.
    let mut lists: Vec<(Snowflake, Tab, String)> = threads::browser_search(store, sh).map(|(p, q)| (p, Tab::Search, q)).into_iter().collect();
    if let Some(forum) = sh.channel.and_then(|c| store.channels.get(&c)).filter(|c| reflux_store::threads::is_forum(c.kind)) {
        let (search, archived) = crate::shell::forum::search_keys(store, sh, forum);
        lists.extend(search.map(|q| (forum.id, Tab::Search, q)));
        match archived {
            Some(q) => lists.push((forum.id, Tab::Archived, q)),
            None if !sh.threads.archived.contains_key(&(forum.id, Tab::Archived)) => ops.push(Op::LoadArchived { parent: forum.id, tab: Tab::Archived, more: false }),
            None => {}
        }
        let missing: Vec<Snowflake> = store
            .threads(forum.id)
            .into_iter()
            .map(|t| t.id)
            .filter(|id| !sh.threads.forum.first.contains_key(id) && !sh.threads.forum.requested.contains(id))
            .take(100)
            .collect();
        if !missing.is_empty() {
            ops.push(Op::PostData { forum: forum.id, ids: missing });
        }
    }
    for (parent, tab, query) in lists {
        if sh.threads.archived.get(&(parent, tab)).is_none_or(|a| a.key != query) {
            ops.push(Op::Search { parent, tab, query, more: false });
        }
    }
    let forum_unreads = forum_unreads(s);
    let sh = &s.app.shell;
    let store = &s.app.store;
    let panel = match sh.threads.panel {
        Some(threads::Panel::Thread(t)) if sh.threads.at_bottom && store.has_unread(t) => Some(t),
        _ => None,
    };
    let mut changed = forum_unreads;
    for o in ops {
        match &o {
            Op::LoadArchived { parent, tab, .. } => s.app.shell.threads.archived.entry((*parent, *tab)).or_default().loading = true,
            Op::Search { parent, tab, query, .. } => {
                s.app.shell.threads.archived.insert((*parent, *tab), threads::Archived { key: query.clone(), loading: true, ..Default::default() });
            }
            Op::PostData { ids, .. } => s.app.shell.threads.forum.requested.extend(ids.iter().copied()),
            _ => {}
        }
        run(s, o);
        changed = true;
    }
    // The panel's thread is read while its newest message shows (like the open channel).
    if let Some(t) = panel.filter(|_| s.desktop.focused || rune::is_headless()) {
        let store = &s.app.store;
        let last = store.channels.get(&t).and_then(|c| c.last_message_id);
        let loaded = store.messages(t).last().map(|m| m.id);
        if let (Some(last), Some(loaded)) = (last, loaded)
            && loaded >= last
        {
            s.app.store.apply(&DispatchEvent::MessageAck(MessageAck { channel_id: t, message_id: last, ..Default::default() }), now_ms());
            let pending = s.app.acks.get(&t).copied();
            s.app.acks.insert(t, (last, pending.map_or(now_ms() + 3000, |(_, d)| d)));
            changed = true;
        }
    }
    changed
}

/// `ForumReadState` for the open forum: the snapshot behind "New", and Request Forum Unreads
/// (op 28) for listed posts the user hasn't joined whose ack moved since it was last asked,
/// up to 40 a request and 4 requests every 5 s.
fn forum_unreads(s: &mut State) -> bool {
    let store = &s.app.store;
    let sh = &s.app.shell;
    let forum = sh.channel.and_then(|c| store.channels.get(&c)).filter(|c| reflux_store::threads::is_forum(c.kind));
    let snapshot = forum.and_then(|f| match sh.threads.forum.snapshot {
        Some((id, at)) if id == f.id => Some((id, at)),
        _ => store.read_states.get(&f.id).and_then(|r| r.last_message_id).map(|at| (f.id, at)),
    });
    let mut changed = snapshot != sh.threads.forum.snapshot;
    let (Some(forum), Some(guild)) = (forum, forum.and_then(|f| f.guild_id)) else {
        s.app.shell.threads.forum.snapshot = snapshot;
        return changed;
    };
    let now = now_ms();
    let f = &sh.threads.forum;
    let recent = f.sends.iter().filter(|t| now - **t < 5000).count();
    let (cur, old) = crate::shell::forum::posts(store, sh, forum);
    let threads: Vec<_> = cur
        .into_iter()
        .chain(old)
        .filter(|t| t.member.is_none())
        .filter_map(|t| Some((t.id, store.read_states.get(&t.id)?.last_message_id?)))
        .filter(|(id, ack)| f.sent_acks.get(id) != Some(ack))
        .take(40)
        .collect();
    let fid = forum.id;
    let connected = s.app.connection == Some(ConnectionState::Connected);
    s.app.shell.threads.forum.snapshot = snapshot;
    if threads.is_empty() || recent >= 4 || !connected {
        return changed;
    }
    let Some(gw) = &s.app.gateway else { return changed };
    let req = reflux_model::RequestForumUnreads {
        guild_id: guild,
        channel_id: fid,
        threads: threads.iter().map(|(id, ack)| reflux_model::ForumUnreadRequest { thread_id: *id, ack_message_id: Some(*ack) }).collect(),
    };
    if gw.send(Command::Send(GatewayPayload::command(OpCode::REQUEST_FORUM_UNREADS, req))).is_ok() {
        let f = &mut s.app.shell.threads.forum;
        f.sends.retain(|t| now - *t < 5000);
        f.sends.push(now);
        f.sent_acks.extend(threads);
        changed = true;
    }
    changed
}

/// `handleForumUnreads`: counts for the acks we asked about; anything else clears.
pub(super) fn on_forum_unreads(s: &mut State, d: &reflux_model::ForumUnreads) {
    let f = &mut s.app.shell.threads.forum;
    for e in &d.threads {
        match (e.count.filter(|n| *n > 0), f.sent_acks.get(&e.thread_id)) {
            (Some(n), Some(ack)) => f.post_unreads.insert(e.thread_id, (n, *ack)),
            _ => f.post_unreads.remove(&e.thread_id),
        };
    }
}

/// `threads: true` for the open guild, plus the member list of the open thread
/// (`ThreadSubscriptions`).
fn subscribe(s: &mut State) {
    let sh = &s.app.shell;
    let Some(guild) = sh.guild else { return };
    let open = match sh.threads.panel {
        Some(threads::Panel::Thread(t)) => Some(t),
        _ => sh.channel.filter(|c| s.app.store.channels.get(c).is_some_and(|c| c.kind.is_thread())),
    };
    let want = Some((guild, open));
    if want == s.app.thread_sub || s.app.connection != Some(ConnectionState::Connected) {
        return;
    }
    let Some(gw) = &s.app.gateway else { return };
    let sub = GuildSubscription { threads: Some(true), thread_member_lists: Some(open.into_iter().collect()), ..Default::default() };
    let req = LazyRequest { subscriptions: [(guild, sub)].into() };
    if gw.send(Command::Send(GatewayPayload::command(OpCode::LAZY_REQUEST, req))).is_ok() {
        s.app.thread_sub = want;
    }
}
