//! Chat flows past the basics: sending with attachments (`topics/uploads.md`), pins, search and
//! jumping through history.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use reflux_model::{Attachment, CreateMessage, Message, Snowflake};
use reflux_net::{Error, MessagePage, SearchResponse};
use serde_json::json;

use super::{Instance, PAGE, State, iso8601, now_ms, spawn, update};
use crate::shell::Intent;
use crate::shell::search::Results;
use crate::shell::upload::Upload;

/// `ATTACHMENT_FLAG_SPOILER`.
const SPOILER: u32 = 1 << 3;

pub(super) fn run(s: &mut State, i: Intent) {
    let Some(inst) = s.app.instance.clone() else { return };
    let generation = s.app.generation;
    // Runs `f` with a fetched page unless the session changed meanwhile.
    let fetch = inst.clone();
    let (slow, fail) = crate::shell::devtools::load_gate(&s.app.saved.desktop.dev);
    let page = move |channel: Snowflake, at: MessagePage, f: fn(&mut State, Snowflake, Vec<Message>)| {
        let inst = fetch.clone();
        spawn(async move {
            let r = match crate::shell::devtools::gate(slow, fail).await {
                Ok(()) => inst.rest.messages(channel, at, PAGE).await,
                Err(e) => Err(e),
            };
            update(move |s| match r {
                Ok(p) if s.app.generation == generation => f(s, channel, p),
                Ok(_) => {}
                Err(e) => eprintln!("reflux: messages of {channel}: {e}"),
            });
        })
    };
    match i {
        Intent::LoadNewer { channel } => {
            if let Some(newest) = s.app.store.messages(channel).last().map(|m| m.id) {
                page(channel, MessagePage::After(newest), |s, c, p| s.app.store.append_messages(c, p, PAGE as usize));
            }
        }
        Intent::JumpTo { channel, message } => {
            if s.app.store.messages(channel).iter().any(|m| m.id == message) {
                s.app.shell.scroll_to = Some(format!("msg-{message}"));
            } else {
                // `fn` pointers can't capture the target; it's the highlighted message.
                page(channel, MessagePage::Around(message), |s, c, p| {
                    s.app.store.set_around_messages(c, p, PAGE as usize);
                    s.app.shell.scroll_to = s.app.shell.highlight.map(|m| format!("msg-{m}"));
                });
            }
        }
        Intent::JumpToPresent(channel) => {
            if s.app.store.messages.get(&channel).is_some_and(|w| w.has_more_after) {
                page(channel, MessagePage::Latest, |s, c, p| {
                    s.app.store.set_latest_messages(c, p, PAGE as usize);
                    s.app.shell.scroll_to = Some("msgs-end".into());
                });
            } else {
                s.app.shell.scroll_to = Some("msgs-end".into());
            }
        }
        Intent::LoadPins { channel, more } => {
            let before = more.then(|| s.app.shell.pins.get(&channel).and_then(|p| p.items.last()).map(|x| x.1.clone())).flatten();
            spawn(async move {
                let r = inst.rest.pins(channel, before.as_deref()).await;
                update(move |s| {
                    let p = s.app.shell.pins.entry(channel).or_default();
                    p.loading = false;
                    match r {
                        Ok(page) => {
                            if !more {
                                p.items.clear();
                            }
                            p.items.extend(page.items.into_iter().map(|x| (x.message, x.pinned_at)));
                            (p.has_more, p.failed) = (page.has_more, false);
                        }
                        Err(e) => {
                            eprintln!("reflux: pins of {channel}: {e}");
                            p.failed = true;
                        }
                    }
                });
            });
        }
        Intent::Search(body) => {
            let ran = (s.app.shell.search.ran.clone(), s.app.shell.search.page, s.app.shell.search.oldest);
            spawn(async move {
                let r = inst.rest.search_messages(&body).await;
                update(move |s| {
                    let sr = &mut s.app.shell.search;
                    // A newer search, or the panel closed, wins.
                    if (sr.ran.clone(), sr.page, sr.oldest) != ran || sr.results.is_none() {
                        return;
                    }
                    sr.results = Some(match r {
                        Ok(SearchResponse::Indexing { .. }) => Results::Indexing,
                        Ok(SearchResponse::Results { messages, total }) => Results::Done { messages, total },
                        Err(e) => {
                            eprintln!("reflux: search: {e}");
                            Results::Failed
                        }
                    });
                });
            });
        }
        _ => {}
    }
}

/// Sends a message, optimistically shown at once; with files it uploads them first, showing
/// progress on the optimistic message. A failed send puts the text and files back.
pub(super) fn send(s: &mut State, channel: Snowflake, content: String, reply: Option<(Snowflake, bool)>, files: Vec<Upload>) {
    let (Some(inst), Some(me)) = (s.app.instance.clone(), s.app.store.me.as_ref()) else { return };
    // The staged sticker rides along (`useTextareaSubmit`'s `stickerItems`).
    let sticker = s.app.shell.pending_sticker.take_if(|(c, _)| *c == channel).map(|(_, id)| id);
    if content.trim().is_empty() && files.is_empty() && sticker.is_none() {
        return;
    }
    let now = now_ms();
    // A snowflake for now, so the optimistic message sorts last; the server echoes it back.
    let nonce = Snowflake(Snowflake::from_timestamp_ms(now).0 | fastrand::u64(..1 << 22));
    let target = reply.and_then(|(id, _)| s.app.store.messages(channel).iter().find(|m| m.id == id).cloned());
    let reference = reply.map(|(id, _)| reflux_model::MessageReferenceInput { message_id: id, channel_id: Some(channel), ..Default::default() });
    let attachments = files
        .iter()
        .enumerate()
        .map(|(i, u)| Attachment {
            id: Snowflake(i as u64),
            filename: u.name.clone(),
            content_type: u.content_type().into(),
            size: u.size,
            duration: u.voice.as_ref().map(|v| v.0 as f64),
            waveform: u.voice.as_ref().map(|v| v.1.clone()),
            ..Default::default()
        })
        .collect();
    let voice = files.iter().any(|u| u.voice.is_some());
    s.app.store.add_local_message(Message {
        id: nonce,
        channel_id: channel,
        author: me.base.clone(),
        content: content.clone(),
        timestamp: iso8601(now),
        nonce: Some(nonce.to_string()),
        kind: if reply.is_some() { reflux_model::MessageType::REPLY } else { Default::default() },
        message_reference: reply.map(|(id, _)| reflux_model::MessageReference { channel_id: channel, message_id: Some(id), ..Default::default() }),
        referenced_message: reply.map(|_| target.map(Box::new)),
        attachments,
        flags: if voice { reflux_model::MessageFlags::VOICE_MESSAGE } else { Default::default() },
        ..Default::default()
    });
    if !files.is_empty() {
        s.app.shell.progress.insert(nonce, 0.0);
    }
    // A reply pings its author only when the bar's toggle is on.
    let allowed_mentions = reply.map(|(_, ping)| reflux_model::AllowedMentions {
        parse: Some(vec!["users".into(), "roles".into(), "everyone".into()]),
        replied_user: Some(ping),
        ..Default::default()
    });
    let body = CreateMessage {
        nonce: Some(nonce.to_string()),
        content: Some(content.clone()).filter(|c| !c.is_empty()),
        message_reference: reference,
        allowed_mentions,
        flags: if voice { reflux_model::MessageFlags::VOICE_MESSAGE } else { Default::default() },
        sticker_ids: sticker.into_iter().collect(),
        ..Default::default()
    };
    let presigned = inst.discovery.features.presigned_attachment_uploads;
    let back = files.clone();
    let dev = &s.app.saved.desktop.dev;
    let (slow, fail) = (dev.slow_message_send, dev.force_fail_message_sends.then_some("Forced message send failure"));
    spawn(async move {
        // `MessageQueue`: text messages go out one at a time, in order; files at once.
        static QUEUE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let r = if let Err(e) = crate::shell::devtools::gate(slow, fail).await {
            Err(e)
        } else if files.is_empty() {
            let _turn = QUEUE.lock().await;
            inst.rest.send_message(channel, &body).await.map(drop)
        } else {
            upload_and_send(&inst, channel, body, files, nonce, presigned).await
        };
        let Err(e) = r else {
            return update(move |s| {
                s.app.shell.progress.remove(&nonce);
            });
        };
        eprintln!("reflux: send to {channel}: {e}");
        update(move |s| failed(s, channel, nonce, content, reply, back, e));
    });
}

/// `MessageQueue.handleSendError`: a text message stays, marked failed (Retry / Delete); one with
/// files is dropped and its text and files go back to an empty composer. Then Fluxer's modal.
fn failed(s: &mut State, channel: Snowflake, nonce: Snowflake, content: String, reply: Option<(Snowflake, bool)>, files: Vec<Upload>, e: Error) {
    let kind = s.app.store.channels.get(&channel).map_or(0, |c| c.kind.0);
    let sh = &mut s.app.shell;
    sh.progress.remove(&nonce);
    if files.is_empty() {
        sh.failed.insert(nonce, crate::shell::queue::Failed { channel, reply });
    } else {
        drop_local(s, channel, nonce);
        let sh = &mut s.app.shell;
        if sh.channel == Some(channel) && sh.composer.text.is_empty() && sh.uploads.is_empty() {
            sh.composer = rune::TextEdit::new(content);
            sh.uploads = files.clone();
        }
    }
    let sh = &mut s.app.shell;
    if let Error::Api { error, .. } = &e
        && error.code == "SLOWMODE_RATE_LIMITED"
    {
        let wait = (error.retry_after.unwrap_or(0.0) * 1000.0) as u64;
        sh.cooldowns.insert(channel, now_ms() + wait);
    }
    let (title, body) = crate::shell::queue::send_error(&e, !files.is_empty(), kind);
    if crate::shell::queue::posts_note(&e) {
        super::commands::post(s, channel, body);
    } else {
        sh.notice = Some((title, body));
    }
}

/// `deleteLocal`: forget an optimistic message.
pub(super) fn drop_local(s: &mut State, channel: Snowflake, nonce: Snowflake) {
    if let Some(w) = s.app.store.messages.get_mut(&channel) {
        w.messages.retain(|m| m.id != nonce);
    }
    s.app.shell.failed.remove(&nonce);
}

/// `retryFailedMessage`: send a failed message's text again.
pub(super) fn retry(s: &mut State, channel: Snowflake, nonce: Snowflake) {
    let Some(f) = s.app.shell.failed.get(&nonce).cloned() else { return };
    let Some(m) = s.app.store.messages(channel).iter().find(|m| m.id == nonce) else { return };
    let content = m.content.clone();
    drop_local(s, channel, nonce);
    send(s, channel, content, f.reply, Vec::new());
}

/// Uploads the files and sends the message referencing them: pre-signed URLs when the instance
/// offers them, else one `multipart/form-data` request.
async fn upload_and_send(inst: &Instance, channel: Snowflake, mut body: CreateMessage, files: Vec<Upload>, nonce: Snowflake, presigned: bool) -> reflux_net::Result<()> {
    let data = tokio::task::spawn_blocking(move || files.into_iter().map(|u| u.source.read().map(|b| (u, Arc::new(b)))).collect::<Result<Vec<_>, String>>())
        .await
        .map_err(|e| Error::Upload(e.to_string()))?
        .map_err(Error::Upload)?;
    let total = data.iter().map(|d| d.1.len() as u64).sum::<u64>().max(1);
    // Whole percents only, so a big file doesn't wake the UI per chunk.
    let last = AtomicU64::new(u64::MAX);
    let progress: Arc<dyn Fn(u64) + Send + Sync> = Arc::new(move |n| {
        let pct = (n * 100 / total).min(100);
        if last.swap(pct, Ordering::Relaxed) != pct {
            update(move |s| {
                if let Some(p) = s.app.shell.progress.get_mut(&nonce) {
                    *p = pct as f32 / 100.0;
                }
            });
        }
    });
    let meta = |u: &Upload| (Some(u.description.clone()).filter(|d| !d.is_empty()), if u.spoiler { SPOILER } else { 0 });
    if !presigned {
        body.attachments = data
            .iter()
            .enumerate()
            .map(|(i, (u, _))| {
                let (description, flags) = meta(u);
                voice_meta(json!({ "id": i, "filename": u.name, "description": description, "flags": flags }), u)
            })
            .collect();
        let files = data.into_iter().map(|(u, b)| (u.name.clone(), b, u.content_type().to_string())).collect();
        return inst.rest.send_message_multipart(channel, &body, files, move |n| progress(n)).await.map(drop);
    }
    let req: Vec<_> = data.iter().enumerate().map(|(i, (u, b))| (i as u32, u.name.clone(), b.len() as u64, u.content_type().to_string())).collect();
    let plans = inst.rest.upload_plan(channel, &req).await?;
    let sent = Arc::new(AtomicU64::new(0));
    let mut complete = Vec::new();
    for (plan, (_, b)) in plans.iter().zip(&data) {
        if let Some(url) = &plan.upload_url {
            inst.rest.put_upload(url, b.clone(), 0..b.len(), Some(&plan.content_type), sent.clone(), progress.clone()).await?;
            continue;
        }
        // Multipart: part N covers `[(N-1)·part_size, N·part_size)`.
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
        inst.rest.complete_uploads(channel, &complete).await?;
    }
    body.attachments = plans
        .iter()
        .zip(&data)
        .map(|(plan, (u, b))| {
            let (description, flags) = meta(u);
            voice_meta(
                json!({
                    "id": plan.id, "filename": u.name, "upload_filename": plan.upload_filename, "file_size": b.len(),
                    "content_type": plan.content_type, "description": description, "flags": flags,
                }),
                u,
            )
        })
        .collect();
    inst.rest.send_message(channel, &body).await.map(drop)
}

/// A voice message attachment also carries `duration` and `waveform` (`MessageRequestUtils.ts`).
fn voice_meta(mut a: serde_json::Value, u: &Upload) -> serde_json::Value {
    if let Some((secs, waveform)) = &u.voice {
        a["duration"] = json!(secs);
        a["waveform"] = json!(waveform);
    }
    a
}
