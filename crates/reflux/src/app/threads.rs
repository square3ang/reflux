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
                                error: Some(format!("Couldn't create the post: {}", crate::guild::describe(&e))),
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
            Err(e) => notice(s, "Something went wrong", e),
        }),
        Op::Delete(t) => call::<Value>(inst, generation, Method::DELETE, format!("/channels/{t}"), None, move |s, r| {
            if let Err(e) = r {
                notice(s, "Something went wrong", e);
            }
        }),
        Op::Join(t) => call::<Value>(inst, generation, Method::PUT, format!("/channels/{t}/thread-members/@me"), None, move |s, r| {
            if let Err(e) = r {
                notice(s, "Something went wrong", e);
            }
        }),
        Op::Leave(t) => call::<Value>(inst, generation, Method::DELETE, format!("/channels/{t}/thread-members/@me"), None, move |s, r| {
            if let Err(e) = r {
                notice(s, "Something went wrong", e);
            }
        }),
        Op::MemberSettings { thread, body } => {
            call::<Value>(inst, generation, Method::PATCH, format!("/channels/{thread}/thread-members/@me/settings"), Some(body), move |s, r| {
                if let Err(e) = r {
                    notice(s, "Something went wrong", e);
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
    if let Some(forum) = sh.channel.and_then(|c| store.channels.get(&c)).filter(|c| reflux_store::threads::is_forum(c.kind)) {
        if !sh.threads.archived.contains_key(&(forum.id, Tab::Archived)) {
            ops.push(Op::LoadArchived { parent: forum.id, tab: Tab::Archived, more: false });
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
    let panel = match sh.threads.panel {
        Some(threads::Panel::Thread(t)) if sh.threads.at_bottom && store.has_unread(t) => Some(t),
        _ => None,
    };
    let mut changed = false;
    for o in ops {
        match &o {
            Op::LoadArchived { parent, tab, .. } => s.app.shell.threads.archived.entry((*parent, *tab)).or_default().loading = true,
            Op::PostData { ids, .. } => s.app.shell.threads.forum.requested.extend(ids.iter().copied()),
            _ => {}
        }
        run(s, o);
        changed = true;
    }
    // The panel's thread is read while its newest message shows (like the open channel).
    if let Some(t) = panel {
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
