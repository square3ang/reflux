//! Text-to-speech (`VoiceTtsUtils.tsx`): `/tts` messages in the open channel and the automatic
//! narration mode, spoken by the OS speech tool at the chosen rate. Like Fluxer, a new message
//! interrupts the one being read (`speakText` stops first), the last 10 aren't read twice, and
//! deleting the message being read or switching away from its channel stops it. The text is
//! simplified for speech like `TtsTextFormatter.ts`.
// shortcut: speech goes through the platform's command-line voice (speech-dispatcher's `spd-say`,
// else `espeak-ng`, `say` on macOS, PowerShell's System.Speech on Windows) instead of a TTS crate.

use std::process::{Child, Command};
use std::sync::Mutex;

use reflux_model::{Message, MessageType, Snowflake};
use reflux_store::Store;
use regex::{Captures, Regex};

use crate::State;
use crate::i18n::{t, tf};

static SPEAKING: Mutex<Option<Child>> = Mutex::new(None);
/// `recentMessageIds` (`MAX_RECENT_MESSAGES` = 10) and `currentMessage` (channel, message).
type Recent = (Vec<Snowflake>, Option<(Snowflake, Snowflake)>);
static RECENT: Mutex<Recent> = Mutex::new((Vec::new(), None));

/// `shouldSpeakMessage` + `handleIncomingTtsMessage`.
pub fn on_message(s: &State, m: &Message) {
    let look = &s.app.saved.desktop.look;
    let store = &s.app.store;
    let viewing = s.app.shell.channel == Some(m.channel_id) && s.desktop.focused;
    let wanted = (look.tts_command && m.tts && viewing)
        || (m.author.id != store.my_id() && (look.tts_mode == 0 || (look.tts_mode == 1 && viewing)));
    if !wanted || s.app.shell.voice.self_deaf || store.is_blocked(m.author.id) || rune::is_headless() {
        return;
    }
    let Some(c) = store.channels.get(&m.channel_id) else { return };
    if store.is_muted(c) {
        return;
    }
    if RECENT.lock().unwrap_or_else(|e| e.into_inner()).0.contains(&m.id) {
        return;
    }
    let name = store.display_name(c.guild_id, m.author.id);
    let line = if !matches!(m.kind, MessageType::DEFAULT | MessageType::REPLY) {
        // shortcut: system messages read their raw content (`SystemMessageUtils.stringify` isn't ported).
        if m.content.trim().is_empty() {
            return;
        }
        m.content.clone()
    } else if !m.content.trim().is_empty() {
        let reply = m.referenced_message.as_ref().and_then(|r| r.as_deref()).map(|r| store.display_name(c.guild_id, r.author.id)).or_else(|| {
            let id = m.message_reference.as_ref()?.message_id?;
            store.messages(m.channel_id).iter().find(|x| x.id == id).map(|r| store.display_name(c.guild_id, r.author.id))
        });
        format_message(store, &m.content, &name, c.guild_id, reply.as_deref())
    } else if !m.stickers.is_empty() {
        tf("{authorName} sent a sticker", &[("authorName", &name)])
    } else if !m.attachments.is_empty() {
        crate::i18n::tp("{count, plural, one {{authorName} sent an attachment} other {{authorName} sent # attachments}}", m.attachments.len() as i64, &[("authorName", &name)])
    } else if !m.embeds.is_empty() {
        tf("{authorName} sent an embed", &[("authorName", &name)])
    } else {
        return;
    };
    {
        let mut r = RECENT.lock().unwrap_or_else(|e| e.into_inner());
        r.0.push(m.id);
        let extra = r.0.len().saturating_sub(10);
        r.0.drain(..extra);
    }
    speak(&line, look.tts_rate);
    RECENT.lock().unwrap_or_else(|e| e.into_inner()).1 = Some((m.channel_id, m.id));
}

/// `handleMessageDelete`: the message being read was deleted.
pub fn on_delete(channel: Snowflake, id: Snowflake) {
    if RECENT.lock().unwrap_or_else(|e| e.into_inner()).1 == Some((channel, id)) {
        stop();
    }
}

/// `handleChannelSelect`: switching away from the channel being read stops it.
pub fn on_channel(channel: Option<Snowflake>) {
    if RECENT.lock().unwrap_or_else(|e| e.into_inner()).1.is_some_and(|(c, _)| Some(c) != channel) {
        stop();
    }
}

/// `formatMessageForTts`: code blocks, spoilers, mentions, custom emoji, commands and timestamps
/// become words, markdown markers go, whitespace collapses; then "{author} said: …" or
/// "Replying to {reply}, {author} said: …".
pub fn format_message(store: &Store, content: &str, author: &str, guild: Option<Snowflake>, reply: Option<&str>) -> String {
    use std::sync::LazyLock;
    macro_rules! re {
        ($name:ident, $pat:expr) => {
            static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect("tts pattern"));
        };
    }
    re!(CODE_BLOCK, r"```[\s\S]*?```");
    re!(SPOILER, r"\|\|[^|]+\|\|");
    re!(USER, r"<@!?(\d+)>");
    re!(ROLE, r"<@&(\d+)>");
    re!(CHANNEL, r"<#(\d+)>");
    re!(EMOJI, r"<a?:([^:]+):\d+>");
    re!(SLASH, r"</([^:]+):\d+>");
    re!(TIMESTAMP, r"<t:(\d+)(?::([tTdDfFR]))?>");
    re!(INLINE_CODE, r"`([^`\n]+)`");
    re!(MASKED_LINK, r"\[([^\]]+)\]\([^)]+\)");
    re!(BOLD_ITALIC, r"\*{3}(.+?)\*{3}");
    re!(BOLD, r"\*{2}(.+?)\*{2}");
    re!(UNDERLINE, r"__(.+?)__");
    re!(STRIKE, r"~~(.+?)~~");
    re!(ITALIC, r"\*(.+?)\*");
    re!(QUOTE, r"(?m)^>\s?");
    re!(HEADER, r"(?m)^(?:-#|#{1,3})\s+");
    re!(SPACE, r"\s+");
    let id = |c: &Captures| c[1].parse::<u64>().map(Snowflake).ok();
    let s = CODE_BLOCK.replace_all(content, format!(" {} ", t("code block")).as_str());
    let s = SPOILER.replace_all(&s, t("spoiler"));
    let s = USER.replace_all(&s, |c: &Captures| match id(c).filter(|u| store.users.contains_key(u)) {
        Some(u) => store.display_name(guild, u),
        None => t("unknown user").to_string(),
    });
    let s = ROLE.replace_all(&s, |c: &Captures| {
        let role = guild.and_then(|g| store.guilds.get(&g)).zip(id(c)).and_then(|(g, r)| g.roles.iter().find(|x| x.id == r));
        role.map_or_else(|| t("unknown role").to_string(), |r| r.name.clone())
    });
    let s = CHANNEL.replace_all(&s, |c: &Captures| id(c).and_then(|ch| store.channels.get(&ch)?.name.clone()).unwrap_or_else(|| t("unknown channel").to_string()));
    let s = EMOJI.replace_all(&s, |c: &Captures| tf("emoji {emojiName}", &[("emojiName", &c[1])]));
    let s = SLASH.replace_all(&s, |c: &Captures| tf("slash {commandName}", &[("commandName", &c[1])]));
    let s = TIMESTAMP.replace_all(&s, |c: &Captures| {
        let Some(d) = c[1].parse::<i64>().ok().and_then(|secs| chrono::DateTime::from_timestamp(secs, 0)) else { return c[0].to_string() };
        let d = d.with_timezone(&chrono::Local);
        let loc = crate::i18n::locale();
        // `TTS_STYLE_MAP`, defaulting to short date + time.
        match c.get(2).map(|m| m.as_str()) {
            Some("t" | "T") => super::time::time(&d, loc),
            Some("d") => super::time::short_date(&d, loc),
            Some("D") => super::time::long_date(&d, loc),
            _ => format!("{} {}", super::time::long_date(&d, loc), super::time::time(&d, loc)),
        }
    });
    let mut s = s.into_owned();
    for r in [&*INLINE_CODE, &*MASKED_LINK, &*BOLD_ITALIC, &*BOLD, &*UNDERLINE, &*STRIKE, &*ITALIC] {
        s = r.replace_all(&s, "$1").into_owned();
    }
    let s = HEADER.replace_all(&QUOTE.replace_all(&s, ""), "").into_owned();
    let formatted = SPACE.replace_all(&s, " ").trim().to_string();
    match reply {
        Some(r) => tf("Replying to {replyAuthorName}, {authorName} said: {formatted}", &[("replyAuthorName", r), ("authorName", author), ("formatted", &formatted)]),
        None => tf("{authorName} said: {formatted}", &[("authorName", author), ("formatted", &formatted)]),
    }
}

/// Speaks `text` at `rate` (1 = normal), stopping what was being said.
pub fn speak(text: &str, rate: f32) {
    stop();
    let rate = rate.clamp(0.1, 2.0);
    let child = if cfg!(target_os = "macos") {
        Command::new("say").arg("-r").arg(format!("{}", (175.0 * rate).round())).arg(text).spawn()
    } else if cfg!(windows) {
        let script = format!(
            "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.Rate = {}; $s.Speak($env:REFLUX_TTS)",
            ((rate - 1.0) * 10.0).round().clamp(-10.0, 10.0)
        );
        Command::new("powershell").args(["-NoProfile", "-Command", &script]).env("REFLUX_TTS", text).spawn()
    } else {
        let lang = crate::i18n::locale().split('-').next().unwrap_or("en").to_string();
        Command::new("spd-say")
            .args(["-w", "-l", &lang, "-r", &format!("{}", ((rate - 1.0) * 100.0).round().clamp(-100.0, 100.0)), "--"])
            .arg(text)
            .spawn()
            .or_else(|_| Command::new("espeak-ng").args(["-v", &lang, "-s", &format!("{}", (175.0 * rate).round())]).arg("--").arg(text).spawn())
    };
    match child {
        Ok(c) => *SPEAKING.lock().unwrap_or_else(|e| e.into_inner()) = Some(c),
        Err(e) => eprintln!("reflux: text-to-speech: {e}"),
    }
}

pub fn stop() {
    RECENT.lock().unwrap_or_else(|e| e.into_inner()).1 = None;
    if let Some(mut c) = SPEAKING.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// Whether the last [`speak`] is still talking.
pub fn speaking() -> bool {
    SPEAKING.lock().unwrap_or_else(|e| e.into_inner()).as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_tts_text_formatter() {
        let store = Store::default();
        let s = format_message(&store, "**hi** <@1> ||secret|| `x` [docs](https://a.b)\n> quoted\n```rs\ncode\n``` <:wave:9>", "Ann", None, None);
        assert_eq!(s, "Ann said: hi unknown user spoiler x docs quoted code block emoji wave");
        let r = format_message(&store, "# Title", "Ann", None, Some("Bob"));
        assert_eq!(r, "Replying to Bob, Ann said: Title");
    }
}
