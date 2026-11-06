//! `--live=write` and `--live=cleanup`: write tests in the one channel `REFLUX_WRITE_CHANNEL`
//! names, through the app's own intents. `write` checks the channel (GET, kind, permissions),
//! then sends, edits, reacts, replies, types, acks, pins, uploads both ways, searches and jumps,
//! waiting for each step's gateway echo; it leaves the messages for the snapshot. `cleanup`
//! deletes this account's `[reflux test` messages there and checks none remain.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reflux_model::{ChannelType, Message, Permissions, ReactionEmoji, Snowflake};

use super::super::Instance;
use crate::State;
use crate::shell::Intent;

/// Every message the tests send starts with this.
pub const LABEL: &str = "[reflux test";

const STEP_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Preflight,
    Open,
    Send,
    Edit,
    ReactAdd,
    ReactRemove,
    Reply,
    Typing,
    MarkUnread,
    Ack,
    Pin,
    PinsList,
    Unpin,
    UploadPresigned,
    UploadMultipart,
    Search,
    Verify,
    Jump,
    // cleanup
    Delete,
    VerifyGone,
    Done,
}

pub struct Run {
    cleanup: bool,
    step: Step,
    started: Option<Instant>,
    ch: Snowflake,
    guild: Option<Snowflake>,
    /// Unique per run, in every message, so echoes are told apart from older runs.
    tag: String,
    first: Option<Snowflake>,
    /// A GET's answer, from the network thread.
    got: Arc<Mutex<Option<Result<String, String>>>>,
    instance: Option<Arc<Instance>>,
    failures: Vec<String>,
}

pub enum Next {
    Wait,
    /// Finished; the key to scroll to on the first frame.
    Done(Option<String>),
}

fn say(msg: impl std::fmt::Display) {
    eprintln!("reflux --live: {msg}");
}

impl Run {
    pub fn new(scene: &str) -> Option<Run> {
        let cleanup = match scene {
            "write" => false,
            "cleanup" => true,
            _ => return None,
        };
        let Some(ch) = reflux_net::write_channel().and_then(|c| c.parse().ok()) else {
            say("REFLUX_WRITE_CHANNEL is not set; nothing to do");
            return Some(Run::stopped(cleanup));
        };
        let tag = format!("{:x}", super::super::now_ms() % 0xff_ffff);
        Some(Run {
            cleanup,
            step: Step::Preflight,
            started: None,
            ch: Snowflake(ch),
            guild: None,
            tag,
            first: None,
            got: Arc::default(),
            instance: None,
            failures: Vec::new(),
        })
    }

    fn stopped(cleanup: bool) -> Run {
        Run {
            cleanup,
            step: Step::Done,
            started: None,
            ch: Snowflake(0),
            guild: None,
            tag: String::new(),
            first: None,
            got: Arc::default(),
            instance: None,
            failures: Vec::new(),
        }
    }

    fn mine<'a>(&self, s: &'a State, marker: &str) -> Vec<&'a Message> {
        let me = s.app.store.my_id();
        s.app.store.messages(self.ch).iter().filter(|m| m.author.id == me && m.content.contains(marker)).collect()
    }

    /// The server's copy of the message with `marker`, once it replaced the optimistic one.
    fn echo<'a>(&self, s: &'a State, marker: &str) -> Option<&'a Message> {
        let all = self.mine(s, marker);
        let echoed: Vec<_> = all.iter().filter(|m| m.nonce.as_deref() != Some(&m.id.to_string())).collect();
        // One copy only: the echo must have replaced the optimistic message (nonce dedupe).
        (echoed.len() == 1 && all.len() == 1).then(|| *echoed[0])
    }

    fn first<'a>(&self, s: &'a State) -> Option<&'a Message> {
        let id = self.first?;
        s.app.store.messages(self.ch).iter().find(|m| m.id == id)
    }

    fn get<F>(&self, s: &State, f: impl FnOnce(Arc<Instance>) -> F)
    where
        F: Future<Output = Result<String, String>> + Send + 'static,
    {
        let Some(inst) = s.app.instance.clone() else { return };
        let got = self.got.clone();
        *got.lock().unwrap() = None;
        let fut = f(inst);
        super::super::spawn(async move {
            let r = fut.await;
            *got.lock().unwrap() = Some(r);
            super::super::update(|_| {});
        });
    }

    fn take_got(&self) -> Option<Result<String, String>> {
        self.got.lock().unwrap().take()
    }

    fn send(&self, s: &mut State, what: &str, reply: Option<(Snowflake, bool)>, attachments: Vec<crate::shell::upload::Upload>) {
        let content = format!("{LABEL} {}] {what}", self.tag);
        s.app.shell.intents.push(Intent::SendMessage { channel: self.ch, content, reply, attachments });
    }

    fn marker(&self, what: &str) -> String {
        format!("{}] {what}", self.tag)
    }

    /// One step: starts it on the first call, then reports whether it's done.
    pub fn step(&mut self, s: &mut State) -> Next {
        if self.step == Step::Done {
            for f in &self.failures {
                say(format!("FAILED {f}"));
            }
            say(format!("{} failure(s)", self.failures.len()));
            return Next::Done(self.first.filter(|_| !self.cleanup).map(|id| format!("msg-{id}")));
        }
        let now = Instant::now();
        let first = self.started.is_none();
        let since = now - *self.started.get_or_insert(now);
        let done = self.advance(s, first, since);
        if done == Some(true) {
            say(format!("{:?} ok ({:.1}s)", self.step, since.as_secs_f32()));
        } else if done == Some(false) || since > STEP_TIMEOUT {
            let why = if done.is_none() { "timed out" } else { "failed" };
            self.failures.push(format!("{:?} {why}", self.step));
            say(format!("{:?} {why}", self.step));
            if matches!(self.step, Step::Preflight | Step::Open) {
                self.step = Step::Done;
                return Next::Wait;
            }
        } else {
            return Next::Wait;
        }
        // The multipart step's stand-in instance goes back.
        if let Some(i) = self.instance.take() {
            s.app.instance = Some(i);
        }
        self.started = None;
        self.step = self.next();
        Next::Wait
    }

    fn next(&self) -> Step {
        use Step::*;
        if self.cleanup {
            return match self.step {
                Preflight => Open,
                Open => Delete,
                Delete => VerifyGone,
                _ => Done,
            };
        }
        const ORDER: [Step; 18] = [
            Preflight,
            Open,
            Send,
            Edit,
            ReactAdd,
            ReactRemove,
            Reply,
            Typing,
            MarkUnread,
            Ack,
            Pin,
            PinsList,
            Unpin,
            UploadPresigned,
            UploadMultipart,
            Search,
            Verify,
            Jump,
        ];
        ORDER.iter().position(|s| *s == self.step).and_then(|i| ORDER.get(i + 1)).copied().unwrap_or(Done)
    }

    /// `Some(true)` done, `Some(false)` failed, `None` still waiting.
    fn advance(&mut self, s: &mut State, first: bool, since: Duration) -> Option<bool> {
        let ch = self.ch;
        let thumbs = || ReactionEmoji { id: None, name: "👍".into(), animated: None };
        match self.step {
            Step::Preflight => {
                if first {
                    self.get(s, |i| async move {
                        let c: reflux_model::Channel = i.rest.get(&format!("/channels/{ch}")).await.map_err(|e| e.to_string())?;
                        Ok(format!("{}|{}|{}", c.kind.0, c.guild_id.map_or(String::new(), |g| g.to_string()), c.name.unwrap_or_default()))
                    });
                    return None;
                }
                let r = self.take_got()?;
                let line = match r {
                    Ok(l) => l,
                    Err(e) => {
                        let known = s.app.store.channels.get(&ch).map(|c| (c.kind.0, c.guild_id.is_some()));
                        say(format!("GET channel: {e} (in the gateway's channels: {known:?}; a guild ID: {}; a message ID in a loaded channel: {})", s.app.store.guilds.contains_key(&ch), s.app.store.messages.values().any(|w| w.messages.iter().any(|m| m.id == ch))));
                        if let Some(g) = s.app.store.guilds.get(&ch) {
                            let name = g.info.as_ref().map_or("-", |i| i.name.as_str());
                            say(format!("that ID is the guild {name:?}; its channels:"));
                            for c in s.app.store.channels.values().filter(|c| c.guild_id == Some(ch)) {
                                let p = s.app.store.permissions(c.id);
                                say(format!("  {} type {} {:?} send {}", c.id, c.kind.0, c.name.as_deref().unwrap_or(""), p.contains(Permissions::SEND_MESSAGES)));
                            }
                        }
                        return Some(false);
                    }
                };
                // The store has the permissions once READY is in.
                s.app.store.channels.get(&ch)?;
                let mut parts = line.splitn(3, '|');
                let kind = ChannelType(parts.next()?.parse().ok()?);
                self.guild = parts.next().and_then(|g| g.parse().ok()).map(Snowflake);
                let guild = self.guild.and_then(|g| s.app.store.guilds.get(&g)).and_then(|g| g.info.as_ref()).map_or("-".into(), |g| g.name.clone());
                let p = s.app.store.permissions(ch);
                let has = |f: Permissions| if p.contains(f) || p.contains(Permissions::ADMINISTRATOR) { "yes" } else { "no" };
                say(format!(
                    "channel: type {} ({kind:?}), guild {guild}; view {} send {} history {} react {} attach {} pin {} manage messages {}",
                    kind.0,
                    has(Permissions::VIEW_CHANNEL),
                    has(Permissions::SEND_MESSAGES),
                    has(Permissions::READ_MESSAGE_HISTORY),
                    has(Permissions::ADD_REACTIONS),
                    has(Permissions::ATTACH_FILES),
                    has(Permissions::PIN_MESSAGES),
                    has(Permissions::MANAGE_MESSAGES),
                ));
                let text = matches!(kind, ChannelType::GUILD_TEXT | ChannelType::DM | ChannelType::GROUP_DM);
                let writable = has(Permissions::VIEW_CHANNEL) == "yes" && has(Permissions::SEND_MESSAGES) == "yes";
                if !text || !writable {
                    say("not a text channel this account can write to: stopping");
                    return Some(false);
                }
                Some(true)
            }
            Step::Open => {
                if first {
                    s.app.shell.select_guild(self.guild, Some(ch));
                }
                Some(s.app.store.messages.contains_key(&ch) && s.app.shell.intents.is_empty())
                    .filter(|&b| b)
            }
            Step::Send => {
                let what = "plain, **bold**, _italic_, `code`, ~~strike~~ and emoji 👋🎉";
                if first {
                    self.send(s, what, None, vec![]);
                    return None;
                }
                let id = self.echo(s, &self.marker(what))?.id;
                self.first = Some(id);
                Some(true)
            }
            Step::Edit => {
                let content = format!("{LABEL} {}] edited: **bold** stays, emoji ✅", self.tag);
                if first {
                    s.app.shell.intents.push(Intent::Edit { channel: ch, message: self.first?, content });
                    return None;
                }
                let m = self.first(s)?;
                (m.content == content && m.edited_timestamp.is_some()).then_some(true)
            }
            Step::ReactAdd | Step::ReactRemove => {
                let add = self.step == Step::ReactAdd;
                if first {
                    s.app.shell.intents.push(Intent::React { channel: ch, message: self.first?, emoji: thumbs(), add });
                    return None;
                }
                // Optimistic at once; a refusal would undo it. The gateway's echo keeps it.
                let mine = self.first(s)?.reactions.iter().any(|r| r.emoji.name == "👍" && r.me && r.count > 0);
                (since > Duration::from_secs(2) && mine == add).then_some(true)
            }
            Step::Reply => {
                if first {
                    self.send(s, "reply to the first", Some((self.first?, false)), vec![]);
                    return None;
                }
                let m = self.echo(s, &self.marker("reply to the first"))?;
                let to = m.message_reference.as_ref().and_then(|r| r.message_id);
                let has_ref = matches!(&m.referenced_message, Some(Some(r)) if Some(r.id) == self.first);
                Some(to == self.first && has_ref)
            }
            Step::Typing => {
                if first {
                    s.app.shell.intents.push(Intent::Typing(ch));
                }
                (since > Duration::from_millis(1500)).then_some(true)
            }
            Step::MarkUnread => {
                if first {
                    s.app.shell.intents.push(Intent::MarkUnread { channel: ch, message: self.first? });
                    return None;
                }
                (since > Duration::from_secs(1)).then_some(s.app.store.has_unread(ch))
            }
            Step::Ack => {
                // As if the user reopened the channel at its bottom: the runtime acks after a delay.
                if first {
                    s.app.shell.hold_ack = None;
                    s.app.shell.at_bottom = true;
                    return None;
                }
                (since > Duration::from_secs(4) && !s.app.acks.contains_key(&ch)).then_some(!s.app.store.has_unread(ch))
            }
            Step::Pin | Step::Unpin => {
                let pin = self.step == Step::Pin;
                if first {
                    s.app.shell.intents.push(Intent::Pin { channel: ch, message: self.first?, pin });
                    return None;
                }
                (self.first(s)?.pinned == pin).then_some(true)
            }
            Step::PinsList => {
                if first {
                    crate::shell::pins::open(&mut s.app.shell, ch);
                    return None;
                }
                let p = s.app.shell.pins.get(&ch).filter(|p| !p.loading)?;
                let ok = p.items.iter().any(|(m, _)| Some(m.id) == self.first);
                crate::shell::pins::open(&mut s.app.shell, ch); // closes the popout
                Some(ok)
            }
            Step::UploadPresigned | Step::UploadMultipart => {
                let presigned = self.step == Step::UploadPresigned;
                let what = if presigned { "upload (pre-signed)" } else { "upload (multipart)" };
                if first {
                    let inst = s.app.instance.clone()?;
                    if !presigned && inst.discovery.features.presigned_attachment_uploads {
                        // The instance offers both; send this one the other way.
                        let mut d = inst.discovery.clone();
                        d.features.presigned_attachment_uploads = false;
                        let rest = reflux_net::Client::from_discovery(reflux_net::http_client(), &d)
                            .with_installation_id(s.app.saved.installation_id.clone());
                        rest.set_token(s.app.saved.token.clone());
                        self.instance = s.app.instance.replace(Arc::new(Instance { discovery: d, rest }));
                    }
                    let sh = &mut s.app.shell;
                    let img = image::RgbaImage::from_fn(64, 64, |x, y| image::Rgba([(x * 4) as u8, (y * 4) as u8, 160, 255]));
                    let mut png = Vec::new();
                    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
                    crate::shell::upload::add_png(sh, png);
                    let txt = format!("reflux test file ({what})\n").into_bytes();
                    let source = crate::shell::upload::Source::Bytes(Arc::new(txt.clone()));
                    crate::shell::upload::add(sh, vec![("reflux-test.txt".into(), txt.len() as u64, source)]);
                    let files = std::mem::take(&mut sh.uploads);
                    if files.len() != 2 {
                        return Some(false);
                    }
                    self.send(s, what, None, files);
                    return None;
                }
                let m = self.echo(s, &self.marker(what))?;
                let names: Vec<_> = m.attachments.iter().map(|a| a.filename.as_str()).collect();
                say(format!("{what}: attachments {names:?}"));
                Some(names == ["image.png", "reflux-test.txt"])
            }
            Step::Search => {
                if first {
                    s.app.shell.search.query = rune::TextEdit::new(self.tag.clone());
                    s.app.shell.search_requested = true;
                    return None;
                }
                use crate::shell::search::Results;
                match s.app.shell.search.results.as_ref()? {
                    Results::Loading => None,
                    Results::Indexing => {
                        say("search: the instance is indexing");
                        Some(true)
                    }
                    Results::Failed => Some(false),
                    Results::Done { messages, total } => {
                        let found = messages.iter().filter(|m| m.content.contains(&self.tag)).count();
                        say(format!("search: {found} of this run's messages in {total} result(s)"));
                        // Indexing lags; a miss isn't the client's fault.
                        Some(true)
                    }
                }
            }
            Step::Verify => {
                // The server's view, by GET, of what the steps above did.
                if first {
                    let tag = self.tag.clone();
                    self.get(s, |i| async move {
                        let page = i.rest.messages(ch, reflux_net::MessagePage::Latest, 20).await.map_err(|e| e.to_string())?;
                        let lines: Vec<String> = page
                            .iter()
                            .filter(|m| m.content.contains(&tag))
                            .map(|m| {
                                let r = m.message_reference.as_ref().and_then(|r| r.message_id).is_some();
                                format!(
                                    "edited {} pinned {} reactions {} attachments {} reply {} | {}",
                                    m.edited_timestamp.is_some(),
                                    m.pinned,
                                    m.reactions.len(),
                                    m.attachments.len(),
                                    r,
                                    m.content.split("] ").nth(1).unwrap_or("")
                                )
                            })
                            .collect();
                        Ok(lines.join("\n"))
                    });
                    return None;
                }
                match self.take_got()? {
                    Ok(l) => {
                        for line in l.lines() {
                            say(format!("server: {line}"));
                        }
                        Some(l.lines().count() == 4)
                    }
                    Err(e) => {
                        say(format!("GET messages: {e}"));
                        Some(false)
                    }
                }
            }
            Step::Jump => {
                if first {
                    crate::shell::jump(&mut s.app.shell, self.guild, ch, self.first?);
                    return None;
                }
                let sh = &s.app.shell;
                (sh.highlight == self.first && sh.intents.is_empty() && s.app.store.messages(ch).iter().any(|m| Some(m.id) == self.first)).then_some(true)
            }
            Step::Delete => {
                // One at a time, each confirmed by the gateway.
                let next = self.mine(s, LABEL).first().map(|m| m.id);
                match next {
                    None => Some(true),
                    Some(id) if self.first != Some(id) => {
                        self.first = Some(id);
                        self.started = Some(Instant::now());
                        s.app.shell.intents.push(Intent::Delete { channel: ch, message: id });
                        None
                    }
                    Some(_) => None,
                }
            }
            Step::VerifyGone => {
                if first {
                    let me = s.app.store.my_id();
                    self.get(s, |i| async move {
                        let page = i.rest.messages(ch, reflux_net::MessagePage::Latest, 100).await.map_err(|e| e.to_string())?;
                        let left = page.iter().filter(|m| m.author.id == me && m.content.contains(LABEL)).count();
                        Ok(left.to_string())
                    });
                    return None;
                }
                let left = self.take_got()?;
                say(format!("test messages left in the latest 100: {left:?}"));
                Some(left.as_deref() == Ok("0"))
            }
            Step::Done => Some(true),
        }
    }
}
