//! Native message notifications (`ui/state/Notification.ts` `showBackgroundNotification`): title
//! "Name (#channel)", the message text as the body, the author's avatar as the image; a click
//! focuses the window and opens the channel.

use std::path::PathBuf;

use reflux_model::{ChannelType, Message, Snowflake};
use reflux_store::Store;

use crate::i18n::t;

/// Linux `.desktop` id (`packaging/reflux.desktop`), for the notification hint and the badge.
pub const DESKTOP_ID: &str = "reflux";

/// "Name (#channel)" / "Name (Group name)" / "Name", as Fluxer titles non-macOS notifications.
pub fn title(store: &Store, m: &Message) -> String {
    let c = store.channels.get(&m.channel_id);
    let name = store.display_name(c.and_then(|c| c.guild_id), m.author.id);
    match c {
        Some(c) if c.kind == ChannelType::GROUP_DM => {
            let group = c.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| t("Group DM").into());
            format!("{name} ({group})")
        }
        Some(c) if c.guild_id.is_some() => {
            let prefix = if c.kind == ChannelType::GUILD_VOICE { "" } else { "#" };
            format!("{name} ({prefix}{})", c.name.as_deref().unwrap_or_default())
        }
        _ => name,
    }
}

/// The message text with user mentions as `@name`; attachments-only messages say so.
// shortcut: no markdown stripping, role/channel mentions or embeds (Fluxer's
// `buildMessageNotificationBody` renders those to plain text).
pub fn body(store: &Store, m: &Message) -> String {
    let guild = store.channels.get(&m.channel_id).and_then(|c| c.guild_id);
    let mut s = m.mentions.iter().fold(m.content.clone(), |s, u| {
        let name = format!("@{}", store.display_name(guild, u.id));
        s.replace(&format!("<@{}>", u.id), &name).replace(&format!("<@!{}>", u.id), &name)
    });
    if s.trim().is_empty() && !m.attachments.is_empty() {
        s = t("Sent an attachment").into();
    }
    s
}

/// Shows one notification; `on_click` runs on the notification's thread when it's clicked.
pub fn show(title: String, body: String, image: Option<PathBuf>, on_click: impl FnOnce() + Send + 'static) {
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
            // shortcut: clicks aren't routed back on macOS and Windows yet.
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            Ok(_) => drop(on_click),
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
