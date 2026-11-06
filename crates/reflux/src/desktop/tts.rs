//! Text-to-speech (`VoiceTtsUtils.tsx`): `/tts` messages in the open channel and the automatic
//! narration mode, spoken by the OS speech tool at the chosen rate.
// shortcut: speech goes through the platform's command-line voice (speech-dispatcher's `spd-say`,
// else `espeak-ng`, `say` on macOS, PowerShell's System.Speech on Windows) instead of a TTS crate;
// messages aren't queued, a new one interrupts the last; markdown isn't simplified for speech
// (`TtsTextFormatter.ts`) beyond `@name` mentions, and replies aren't announced as such.

use std::process::{Child, Command};
use std::sync::Mutex;

use reflux_model::Message;

use crate::State;
use crate::i18n::tf;

static SPEAKING: Mutex<Option<Child>> = Mutex::new(None);

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
    let name = store.display_name(c.guild_id, m.author.id);
    let body = super::notify::body(store, m);
    let line = if !m.content.trim().is_empty() {
        tf("{authorName} said: {formatted}", &[("authorName", &name), ("formatted", &body)])
    } else if !m.stickers.is_empty() {
        tf("{authorName} sent a sticker", &[("authorName", &name)])
    } else if !m.attachments.is_empty() {
        crate::i18n::tp("{count, plural, one {{authorName} sent an attachment} other {{authorName} sent # attachments}}", m.attachments.len() as i64, &[("authorName", &name)])
    } else if !m.embeds.is_empty() {
        tf("{authorName} sent an embed", &[("authorName", &name)])
    } else {
        return;
    };
    speak(&line, look.tts_rate);
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
    if let Some(mut c) = SPEAKING.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// Whether the last [`speak`] is still talking.
pub fn speaking() -> bool {
    SPEAKING.lock().unwrap_or_else(|e| e.into_inner()).as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
}
