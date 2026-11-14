//! Native message notifications (`ui/state/Notification.ts` `showBackgroundNotification`): title
//! "Name (#channel)", the message text as the body, the author's avatar as the image; a click
//! focuses the window and opens the channel.

use std::path::PathBuf;

use reflux_model::{ChannelType, Message, MessageType, Snowflake};
use reflux_store::Store;

use crate::i18n::t;

/// Linux `.desktop` id (`packaging/reflux.desktop`), for the notification hint and the badge.
pub const DESKTOP_ID: &str = "reflux";
/// Windows `AppUserModelID`, the toasts' identity (`main` registers it).
#[cfg(windows)]
pub const APP_USER_MODEL_ID: &str = "app.fluxer.reflux";

/// "Name (#channel)" / "Name (Group name)" / "Name", as Fluxer titles non-macOS notifications.
pub fn title(store: &Store, m: &Message) -> String {
    let c = store.channels.get(&m.channel_id);
    let name = store.display_name(c.and_then(|c| c.guild_id), m.author.id);
    match c {
        Some(c) if c.kind == ChannelType::GROUP_DM => {
            let group = c.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| t("Group DM").into());
            format!("{name} ({group})")
        }
        // A thread: "New post in #forum" for a post's opening message, else "#thread, #parent,
        // Community".
        Some(c) if c.kind.is_thread() => {
            let context = match store.new_post_forum(c, m.id) {
                Some(f) => crate::i18n::tf("New post in #{forumName}", &[("forumName", f.name.as_deref().unwrap_or_default())]),
                None => {
                    let parent = c.parent_id.and_then(|p| store.channels.get(&p)).map(|p| format!("#{}", p.name.as_deref().unwrap_or_default()));
                    let guild = c.guild_id.and_then(|g| store.guilds.get(&g)?.info.as_ref()).map(|i| i.name.clone());
                    [Some(format!("#{}", c.name.as_deref().unwrap_or_default())), parent, guild].into_iter().flatten().collect::<Vec<_>>().join(", ")
                }
            };
            format!("{name} ({context})")
        }
        Some(c) if c.guild_id.is_some() => {
            let prefix = if c.kind == ChannelType::GUILD_VOICE { "" } else { "#" };
            let channel = format!("{prefix}{}", c.name.as_deref().unwrap_or_default());
            // A system message is titled by the community, not its author.
            match c.guild_id.and_then(|g| store.guilds.get(&g)?.info.as_ref()).filter(|_| m.kind != reflux_model::MessageType::DEFAULT) {
                Some(g) => format!("{} ({channel})", g.name),
                None => format!("{name} ({channel})"),
            }
        }
        _ => name,
    }
}

/// `buildMessageNotificationBody` (`MessageNotificationPreview.ts`): the content rendered to
/// plain text with markdown kept (mentions as `@name` / `@role` / `#channel`, custom emoji as
/// `:name:`), else a sticker, attachment or embed line, else the forwarded message's.
/// System messages use `SystemMessageUtils.stringify`.
pub fn body(store: &Store, m: &Message) -> String {
    let guild = store.channels.get(&m.channel_id).and_then(|c| c.guild_id);
    if !matches!(m.kind, MessageType::DEFAULT | MessageType::REPLY) {
        if let Some(s) = crate::shell::system_msg::stringify(store, m) {
            return s;
        }
    } else {
        let users: Vec<Snowflake> = m.mentions.iter().map(|u| u.id).collect();
        let text = markdown(store, guild, &m.content, &users, &m.mention_roles, &m.mention_channels);
        if !text.is_empty() {
            return text;
        }
    }
    if let Some(s) = fallback(&m.stickers, &m.attachments, &m.embeds) {
        return s;
    }
    let Some(snap) = m.message_snapshots.first() else { return String::new() };
    let text = markdown(store, guild, &snap.content, &snap.mentions, &snap.mention_roles, &snap.mention_channels);
    if !text.is_empty() {
        return text;
    }
    fallback(&snap.stickers, &snap.attachments, &snap.embeds).unwrap_or_else(|| t("Forwarded a message").into())
}

/// `renderMarkdownPreview`: `parseAndRenderToPlaintext` with `preserveMarkdown`.
fn markdown(store: &Store, guild: Option<Snowflake>, content: &str, users: &[Snowflake], roles: &[Snowflake], channels: &[reflux_model::ChannelMention]) -> String {
    if content.is_empty() {
        return String::new();
    }
    let mut o = reflux_markdown::PlaintextOptions { preserve_markdown: true, ..Default::default() };
    for &u in users {
        o.users.insert(u.to_string(), store.display_name(guild, u));
    }
    if let Some(g) = guild.and_then(|g| store.guilds.get(&g)) {
        for r in g.roles.iter().filter(|r| roles.contains(&r.id)) {
            o.roles.insert(r.id.to_string(), r.name.clone());
        }
    }
    for c in channels {
        o.channels.insert(c.id.to_string(), c.name.clone());
    }
    // Channel mentions the message didn't list resolve from the store.
    for c in store.channels.values().filter(|c| content.contains(&format!("<#{}>", c.id))) {
        o.channels.entry(c.id.to_string()).or_insert_with(|| c.name.clone().unwrap_or_default());
    }
    let emoji = crate::shell::emoji::context(content);
    let nodes = reflux_markdown::parse(content, &reflux_markdown::Options { flags: reflux_markdown::presets::STANDARD, emoji });
    reflux_markdown::render_ast_to_plaintext(&nodes, &o).trim().to_string()
}

/// `buildMessageContentFallbackPreview`: stickers, then the first attachment, then the first embed.
fn fallback(stickers: &[reflux_model::StickerItem], attachments: &[reflux_model::Attachment], embeds: &[reflux_model::Embed]) -> Option<String> {
    if !stickers.is_empty() {
        let names: Vec<&str> = stickers.iter().map(|s| s.name.trim()).filter(|n| !n.is_empty()).collect();
        return Some(match names.as_slice() {
            [] if stickers.len() == 1 => t("Sticker").into(),
            [] => t("Stickers").into(),
            [one] if stickers.len() == 1 => crate::i18n::tf("Sticker: {stickerName}", &[("stickerName", one)]),
            _ => crate::i18n::tf("Stickers: {stickerNames}", &[("stickerNames", &crate::i18n::list_and(&names))]),
        });
    }
    if let Some(a) = attachments.first() {
        return Some(crate::i18n::tf("Attachment: {filename}", &[("filename", &a.filename)]));
    }
    let e = embeds.first()?;
    let s = match (&e.title, &e.description) {
        (Some(t), Some(d)) => format!("{t}: {d}"),
        (None, Some(d)) => d.clone(),
        (Some(t), None) => t.clone(),
        (None, None) => e.fields.first().map(|f| [f.name.as_str(), f.value.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(": ")).unwrap_or_default(),
    };
    (!s.is_empty()).then_some(s)
}

/// Shows one notification; `on_click` runs on the notification's thread when it's clicked.
pub fn show(title: String, body: String, image: Option<PathBuf>, on_click: impl FnOnce() + Send + 'static) {
    #[cfg(windows)]
    std::thread::spawn(move || {
        use tauri_winrt_notification::{IconCrop, Toast};
        let (tx, rx) = std::sync::mpsc::channel();
        let dismissed = tx.clone();
        // Electron's toast: the icon as the circular app logo beside the text.
        let mut toast = Toast::new(APP_USER_MODEL_ID).title(&title).text1(&body);
        if let Some(p) = &image {
            toast = toast.icon(p, IconCrop::Circular, "");
        }
        let toast = toast
            .on_activated(move |_| {
                let _ = tx.send(true);
                Ok(())
            })
            .on_dismissed(move |_| {
                let _ = dismissed.send(false);
                Ok(())
            });
        match toast.show() {
            Ok(()) => {
                if rx.recv() == Ok(true) {
                    on_click();
                }
            }
            Err(e) => eprintln!("reflux: notification: {e:?}"),
        }
    });
    #[cfg(not(windows))]
    std::thread::spawn(move || {
        let mut n = notify_rust::Notification::new();
        n.appname("Fluxer").summary(&title).body(&body);
        if let Some(p) = &image {
            n.image_path(&p.to_string_lossy());
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        n.action("default", t("Open"))
            .hint(notify_rust::Hint::DesktopEntry(DESKTOP_ID.into()))
            .hint(notify_rust::Hint::Category("im.received".into()));
        match n.show() {
            #[cfg(all(unix, not(target_os = "macos")))]
            Ok(h) => h.wait_for_action(|a| {
                if a == "default" {
                    on_click();
                }
            }),
            // A body click: the toast's in-process `Activated` event (Windows) or the
            // NSUserNotificationCenter delegate (macOS; this thread waits for it).
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            Ok(h) => {
                let _ = h.wait_for_response(|r: &notify_rust::NotificationResponse| {
                    if matches!(r, notify_rust::NotificationResponse::Default) {
                        on_click();
                    }
                });
            }
            Err(e) => eprintln!("reflux: notification: {e}"),
        }
    });
}

/// The avatar at `url` as a PNG file in the cache dir (notification servers can't all read
/// WebP), fetched once per user and hash.
pub async fn avatar_file(http: reqwest::Client, url: String, user: Snowflake, hash: String) -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("reflux/avatars");
    let path = dir.join(format!("{user}-{hash}.png"));
    if path.exists() {
        return Some(path);
    }
    let bytes = http.get(&url).send().await.ok()?.error_for_status().ok()?.bytes().await.ok()?;
    tokio::task::spawn_blocking(move || {
        let img = image::load_from_memory(&bytes).ok()?;
        std::fs::create_dir_all(&dir).ok()?;
        img.save_with_format(&path, image::ImageFormat::Png).ok()?;
        Some(path)
    })
    .await
    .ok()
    .flatten()
}
