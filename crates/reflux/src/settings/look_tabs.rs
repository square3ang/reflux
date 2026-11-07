//! Appearance (`AppearanceTab.tsx`), Chat (`ChatSettingsTab.tsx`), Accessibility
//! (`AccessibilityTab.tsx`), Notifications (`NotificationsTab.tsx`) and Themes (`ThemesTab.tsx`).
//! Device preferences go to [`crate::look::Prefs`]; account ones are `PatchSettings`.

use reflux_store::Store;
use rune::{El, TextEdit, Tx, col, el, row, text, text_input};
use serde_json::json;

use super::desktop_tabs;
use super::tabs::section;
use crate::desktop::sound::{ALL as SOUNDS, CUSTOM_MAX_BYTES, Sound};
use crate::i18n::{t, tf, tp};
use crate::look::Prefs;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, custom, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, RadioGroup, Select, Slider, Switch};

// ---- Helpers -------------------------------------------------------------------------------

/// Saves a changed copy of the look preferences.
pub(super) fn update<S>(s: &mut S, acc: ShellAccess<S>, f: impl FnOnce(&mut Prefs)) {
    let sh = acc(s);
    let mut prefs = sh.prefs.clone();
    f(&mut prefs.look);
    sh.intents.push(Intent::Prefs(Box::new(prefs)));
}

fn switch<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, label: &str, desc: Option<&str>, get: fn(&Prefs) -> bool, put: fn(&mut Prefs, bool)) -> El {
    switch_if(st, p, acc, label, desc, false, get, put)
}

#[allow(clippy::too_many_arguments)]
fn switch_if<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, label: &str, desc: Option<&str>, disabled: bool, get: fn(&Prefs) -> bool, put: fn(&mut Prefs, bool)) -> El {
    let on = get(&st.prefs.look);
    let mut sw = Switch::new(on).label(label.to_string()).disabled(disabled);
    if let Some(d) = desc {
        sw = sw.description(d.to_string());
    }
    sw.on_toggle(move |s: &mut S| update(s, acc, |l| put(l, !on))).el(p)
}

/// A switch for a boolean account setting.
fn account_switch<S: 'static>(p: &Palette, acc: ShellAccess<S>, label: &str, key: &'static str, on: bool) -> El {
    Switch::new(on).label(label.to_string()).on_toggle(move |s: &mut S| acc(s).intents.push(Intent::PatchSettings(json!({ key: !on })))).el(p)
}

/// `SettingsTabSection`: a 15/600 heading over its content, 12 apart.
fn sub(title: &str, content: El, p: &Palette) -> El {
    col().w_full().gap(12.0).child(text(title.to_string()).font(font(15.0, 600.0).lh(19.5)).color(p.text_primary)).child(content)
}

/// `CompactComboboxRow`: the label, then a small select on the right.
fn combo_row(label: &str, select: El, p: &Palette) -> El {
    row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(text(label.to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0).min_w(0.0).wrap())
        .child(select)
}

fn radio<S: 'static>(p: &Palette, options: &[(&str, &str, Option<&str>)], value: &str, f: impl Fn(&mut S, &str) + 'static) -> El {
    RadioGroup::new(options, Some(value)).on_change(f).el(p)
}

fn muted(s: &str, p: &Palette) -> El {
    crate::ui::mark::marked(s).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap()
}

/// A numeric select whose options are `values` labelled by `label`.
#[allow(clippy::too_many_arguments)]
fn number_select<S: 'static>(
    st: &ShellState,
    p: &Palette,
    acc: ShellAccess<S>,
    layer: &mut Vec<El>,
    id: &'static str,
    values: &[f32],
    current: f32,
    label: impl Fn(f32) -> String,
    put: fn(&mut Prefs, f32),
) -> El {
    let nearest = values.iter().copied().min_by(|a, b| (a - current).abs().total_cmp(&(b - current).abs())).unwrap_or(current);
    let opts: Vec<(String, String)> = values.iter().map(|v| (v.to_string(), label(*v))).collect();
    let refs: Vec<(&str, &str)> = opts.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    Select::new(id, &refs, Some(&nearest.to_string()))
        .width(128.0)
        .compact()
        .on_choose(move |s: &mut S, v| {
            if let Ok(v) = v.parse::<f32>() {
                update(s, acc, |l| put(l, v));
            }
        })
        .el(&st.popups, pops, p, layer)
}

// ---- Appearance ----------------------------------------------------------------------------

pub fn appearance<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let link = text(format!("\"{}\"", t("Themes")))
        .font(font(14.0, 400.0).lh(20.0))
        .color(p.text_link)
        .cursor(rune::Cursor::Pointer)
        .role("link")
        .on_press(super::select(acc, "themes"));
    let theme = row()
        .w_full()
        .child(text(t("You can pick a dark or light theme and change colors and quick CSS in ")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted))
        .child(link)
        .child(text(".").font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted));

    let look = &st.prefs.look;
    let compact = store.settings.message_display_compact;
    let font_size = number_select(st, p, acc, layer, "chat-font-size", &[12.0, 14.0, 15.0, 16.0, 18.0, 20.0, 24.0], look.font_size, |v| format!("{v}px"), |l, v| l.font_size = v);
    let spacing_now = if compact { look.compact_group_spacing } else { look.group_spacing };
    let spacing = number_select(st, p, acc, layer, "group-spacing", &[0.0, 4.0, 8.0, 16.0, 24.0], spacing_now, |v| tf("{messageGroupSpacing}px", &[("messageGroupSpacing", &v.to_string())]), if compact {
        |l: &mut Prefs, v| l.compact_group_spacing = v
    } else {
        |l: &mut Prefs, v| l.group_spacing = v
    });
    let modes = [
        ("cozy", t("Comfy"), Some(t("Spacious layout with clear visual separation between messages."))),
        ("compact", t("Dense"), Some(t("Maximizes visible messages with minimal spacing."))),
    ];
    let mut messages = col()
        .w_full()
        .gap(16.0)
        .child(combo_row(t("Chat font size"), font_size, p))
        .child(combo_row(t("Space between message groups"), spacing, p))
        .child(radio(p, &modes, if compact { "compact" } else { "cozy" }, move |s: &mut S, v| {
            acc(s).intents.push(Intent::PatchSettings(json!({ "message_display_compact": v == "compact" })))
        }));
    if compact {
        messages = messages.child(switch(st, p, acc, t("Hide user avatars"), None, |l| !l.compact_avatars, |l, v| l.compact_avatars = !v));
    }

    let typing = [
        ("0", t("Typing indicator + avatars"), Some(t("Show typing indicator with user avatars in the channel list"))),
        ("1", t("Typing indicator only"), Some(t("Show just the typing indicator without avatars"))),
        ("2", t("Hidden"), Some(t("Don't show typing indicators in the channel list"))),
    ];
    let mode = look.typing_indicator.to_string();
    let interface = sub(
        t("Channel list typing indicators"),
        radio(p, &typing, &mode, move |s: &mut S, v| update(s, acc, |l| l.typing_indicator = v.parse().unwrap_or(0))),
        p,
    );
    let channel_list = switch(st, p, acc, t("Show unread indicator on muted channels"), None, |l| l.faded_unread_muted, |l, v| l.faded_unread_muted = v);
    let active_now = switch(
        st,
        p,
        acc,
        t("Show active now on the home screen"),
        Some(t("Show active now on the home screen to surface friends active in voice. You'll see a preview, the channel context, who's already there, and a quick way to join in.")),
        |l| l.active_now,
        |l, v| l.active_now = v,
    );
    col()
        .w_full()
        .child(section(t("Theme"), theme, true, p))
        .child(desktop_tabs::scaling(st, p, acc))
        .child(section(t("Messages"), messages, false, p))
        .child(section(t("Interface"), interface, false, p))
        .child(section(t("Channel list"), channel_list, false, p))
        .child(section(t("Active now"), active_now, false, p))
        .child(desktop_tabs::streaming(st, p, acc))
}

// ---- Chat ----------------------------------------------------------------------------------

pub fn chat<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let s = &store.settings;
    let product = [("productName", "Fluxer")];
    let media = col()
        .w_full()
        .gap(12.0)
        .child(account_switch(p, acc, t("When posted as links in chat"), "inline_embed_media", s.inline_embed_media))
        .child(account_switch(p, acc, &tf("When uploaded directly to {productName}", &product), "inline_attachment_media", s.inline_attachment_media));
    let perm = [("manageMessagesPermissionLabel", t("Manage Messages"))];
    let moderate = tf("Always show spoiler content in channels where you have the \"{manageMessagesPermissionLabel}\" permission", &perm);
    let spoilers = [
        ("1", t("On click"), Some(t("Show spoiler content when clicked"))),
        ("2", t("In channels I moderate"), Some(moderate.as_str())),
        ("0", t("Always"), Some(t("Always show spoiler content"))),
    ];
    let spoiler_value = s.render_spoilers.to_string();
    let display = col()
        .w_full()
        .gap(24.0)
        .child(sub(t("Media display"), media, p))
        .child(sub(t("Link previews"), account_switch(p, acc, t("Show embeds and preview website links"), "render_embeds", s.render_embeds), p))
        .child(sub(t("Reactions"), account_switch(p, acc, t("Show emoji reactions on messages"), "render_reactions", s.render_reactions), p))
        .child(sub(
            t("Spoiler content"),
            radio(p, &spoilers, &spoiler_value, move |s: &mut S, v| acc(s).intents.push(Intent::PatchSettings(json!({ "render_spoilers": v.parse::<u8>().unwrap_or(1) })))),
            p,
        ));
    let input = switch(st, p, acc, t("Turn text smileys into emoji"), None, |l| l.convert_emoticons, |l, v| l.convert_emoticons = v);
    col().w_full().child(section(t("Display"), display, true, p)).child(section(t("Input"), input, false, p))
}

// ---- Accessibility -------------------------------------------------------------------------

pub fn accessibility<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let look = &st.prefs.look;
    let pct = (look.saturation * 100.0).round();
    let saturation = col()
        .w_full()
        .gap(8.0)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .child(text(t("Saturation")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0))
                .child(text(format!("{pct}%")).font(font(14.0, 500.0).lh(20.0)).color(p.text_secondary)),
        )
        .child(Slider::new("saturation", pct, 0.0, 100.0).step(1.0).on_change(move |s: &mut S, v| update(s, acc, |l| l.saturation = (v / 100.0).clamp(0.0, 1.0))).el(p));
    let previews = [
        ("0", t("All messages"), Some(t("Show message previews for all DM conversations"))),
        ("1", t("Unread DMs only"), Some(t("Only show message previews for DMs with unread messages"))),
        ("2", t("None"), Some(t("Don't show message previews in the DM list"))),
    ];
    let preview_value = look.dm_previews.to_string();
    let visual = col()
        .w_full()
        .gap(16.0)
        .child(saturation)
        .child(switch(st, p, acc, t("Always underline links"), None, |l| l.underline_links, |l, v| l.underline_links = v))
        .child(switch(st, p, acc, t("Dim strikethrough text"), None, |l| l.dim_strikethrough, |l, v| l.dim_strikethrough = v))
        .child(sub(t("DM message previews"), radio(p, &previews, &preview_value, move |s: &mut S, v| update(s, acc, |l| l.dm_previews = v.parse().unwrap_or(2))), p));

    let reader = switch(
        st,
        p,
        acc,
        t("Announce new messages"),
        Some(t("Let screen readers announce new messages as they arrive in the open channel. Notification sounds are unaffected.")),
        |l| l.announce_messages,
        |l, v| l.announce_messages = v,
    );

    let rate = number_select(st, p, acc, layer, "tts-rate", &[0.1, 0.5, 1.0, 1.5, 2.0], look.tts_rate, |v| format!("x{v:.1}"), |l, v| l.tts_rate = v);
    let speaking = crate::desktop::tts::speaking();
    let rate_now = look.tts_rate;
    let sample = Button::new(t(if speaking { "Silence sample" } else { "Play sample" }))
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::Small)
        .key("tts-sample")
        .on_press(move |_: &mut S| {
            if crate::desktop::tts::speaking() {
                crate::desktop::tts::stop();
            } else {
                crate::desktop::tts::speak(t("Doc, I'm from the future. I came here in a time machine that you invented. Now, I need your help to get back to the year 1985."), rate_now);
            }
        })
        .el(p);
    let tts = col()
        .w_full()
        .gap(12.0)
        .child(combo_row(t("Speech playback speed"), rate, p))
        .child(muted(t("Choose a speed for spoken text."), p))
        .child(row().w_full().gap(12.0).center_y().child(muted(t("Hear the sample line spoken at your chosen speed."), p).grow(1.0).min_w(0.0)).child(sample));

    let keyboard = col()
        .w_full()
        .gap(16.0)
        .child(switch(st, p, acc, t("Show focus ring on chat text area"), None, |l| l.focus_ring, |l, v| l.focus_ring = v))
        .child(switch(st, p, acc, t("Escape key exits keyboard mode"), None, |l| l.escape_exits_keyboard, |l, v| l.escape_exits_keyboard = v))
        .child(switch(st, p, acc, t("Show context menu keybinds"), None, |l| l.menu_shortcuts, |l, v| l.menu_shortcuts = v))
        .child(switch(st, p, acc, t("Confirm before starting calls"), None, |l| l.confirm_calls, |l, v| l.confirm_calls = v))
        .child(switch(st, p, acc, t("Join P2P calls without asking"), Some(t("Other participants in a P2P call can see your IP address.")), |l| l.skip_p2p_join, |l, v| l.skip_p2p_join = v));

    let s = &store.settings;
    let stickers = [
        ("0", t("Always animate"), Some(t("Stickers will always animate"))),
        ("1", t("Animate on interaction"), Some(t("Stickers will animate when you hover or interact with them"))),
        ("2", t("Never animate"), Some(t("Stickers will never animate"))),
    ];
    let sticker_value = s.animate_stickers.to_string();
    let animation = col()
        .w_full()
        .gap(16.0)
        .child(account_switch(p, acc, t("Play animated emojis"), "animate_emoji", s.animate_emoji))
        .child(account_switch(p, acc, &tf("Automatically play GIFs when {productName} is focused", &[("productName", "Fluxer")]), "gif_auto_play", s.gif_auto_play))
        .child(sub(
            t("Stickers"),
            radio(p, &stickers, &sticker_value, move |s: &mut S, v| acc(s).intents.push(Intent::PatchSettings(json!({ "animate_stickers": v.parse::<u8>().unwrap_or(1) })))),
            p,
        ));

    let synced = look.sync_motion;
    let motion = col()
        .w_full()
        .gap(16.0)
        .child(switch(st, p, acc, t("Sync reduced motion setting with system"), Some(t("Use this device's system reduced motion preference, or customize it below.")), |l| l.sync_motion, |l, v| l.sync_motion = v))
        .child(switch_if(
            st,
            p,
            acc,
            t("Reduce motion"),
            Some(t(if synced { "Disable animations and transitions. Currently controlled by your system setting." } else { "Disable animations and transitions throughout the app." })),
            synced,
            |l| if l.sync_motion { crate::look::reduced_motion() } else { l.reduce_motion },
            |l, v| l.reduce_motion = v,
        ))
        .child(muted(t("Animated emojis, GIFs and stickers stay under your control in the Animation tab."), p));

    col()
        .w_full()
        .child(section(t("Visual"), visual, true, p))
        .child(section(t("Screen reader"), reader, false, p))
        .child(section(t("Text-to-speech"), tts, false, p))
        .child(section(t("Keyboard"), keyboard, false, p))
        .child(section(t("Animation"), animation, false, p))
        .child(section(t("Motion"), motion, false, p))
}

// ---- Notifications -------------------------------------------------------------------------

pub fn notifications<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let badge = switch(st, p, acc, t("Enable unread message badge"), None, |l| l.unread_badge, |l, v| l.unread_badge = v);
    let notes = col().w_full().gap(16.0).child(desktop_tabs::notifications_switch(st, p, acc)).child(badge);

    let current = store.me.as_ref().and_then(|u| u.base.mention_flags).map_or(0, |m| m.0);
    let mention = [
        ("0", t("No preference"), Some(t("Respect the sender's intent, with no warning when they toggle the @mention"))),
        ("1", t("Prefer @mention"), Some(t("Default replies to @mention you, and warn the sender if they disable it"))),
        ("2", t("Prefer no @mention"), Some(t("Default replies to omit the @mention, and warn the sender if they enable it"))),
    ];
    let mention = radio(p, &mention, &current.to_string(), move |s: &mut S, v| {
        let flags: u8 = v.parse().unwrap_or(0);
        acc(s).api(reqwest::Method::PATCH, "/users/@me", Some(json!({ "mention_flags": flags })), crate::guild::Then::Nothing);
    });

    let look = &st.prefs.look;
    let all_off = !st.prefs.sounds;
    let volume = col()
        .w_full()
        .gap(8.0)
        .child(
            row()
                .w_full()
                .child(text(t("Master volume")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0))
                .child(text(format!("{}%", look.volume.round())).font(font(14.0, 500.0).lh(20.0)).color(p.text_secondary)),
        )
        .child(muted(t("Sets the level for every sound effect. Per-sound overrides ignore this."), p))
        .child(Slider::new("master-volume", look.volume, 0.0, 200.0).step(1.0).on_change(move |s: &mut S, v| update(s, acc, |l| l.volume = v.round().clamp(0.0, 200.0))).el(p));
    let all = Switch::new(all_off)
        .label(t("Disable all notification sounds"))
        .description(t("Your existing notification sound settings will be preserved."))
        .on_toggle(move |s: &mut S| {
            let sh = acc(s);
            let mut prefs = sh.prefs.clone();
            prefs.sounds = all_off;
            sh.intents.push(Intent::Prefs(Box::new(prefs)));
        })
        .el(p);
    let max = format!("{} MB", CUSTOM_MAX_BYTES / 1024 / 1024);
    let rows = col().w_full().gap(4.0).children(SOUNDS.iter().map(|&snd| sound_row(snd, look, all_off, p, acc)));
    let sounds = col()
        .w_full()
        .gap(16.0)
        .child(volume)
        .child(all)
        .child(muted(&tf("Click the upload icon next to any sound to customize it with your own audio file. Max {customSoundMaxSizeLabel}.", &[("customSoundMaxSizeLabel", &max)]), p))
        .child(rows)
        .child(overrides(st, look, all_off, p, acc));

    let narration = [
        ("0", t("Every channel"), Some(t("Let every incoming message be spoken, regardless of which channel is open."))),
        ("1", t("Active channel only"), Some(t("Narrates only the channel you're viewing. Narration follows you between channels."))),
        ("2", t("Never automatically"), Some(t("Remain silent unless someone runs /tts in the channel you're viewing."))),
    ];
    let mode = look.tts_mode.to_string();
    let tts = col()
        .w_full()
        .gap(16.0)
        .child(switch(st, p, acc, t("Enable /tts speech playback"), Some(t("Let /tts read your message aloud. Disabling the setting keeps those commands as regular text.")), |l| l.tts_command, |l, v| l.tts_command = v))
        .child(sub(
            t("Automatic message narration"),
            col()
                .w_full()
                .gap(12.0)
                .child(muted(t("Converts incoming content to speech, regardless of whether it came from /tts."), p))
                .child(radio(p, &narration, &mode, move |s: &mut S, v| update(s, acc, |l| l.tts_mode = v.parse().unwrap_or(2)))),
            p,
        ));

    col()
        .w_full()
        .child(section(t("Notifications"), notes, true, p))
        .child(section(t("Mention preference"), mention, false, p))
        .child(section(t("Sounds"), sounds, false, p))
        .child(section(t("Text-to-speech notifications"), tts, false, p))
}

/// The "Per-sound volume" accordion: a slider per sound that follows the master volume until moved.
fn overrides<S: 'static>(st: &ShellState, look: &Prefs, all_off: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let open = st.form.data.contains_key("sound-overrides");
    let n = look.sound_volumes.len();
    let desc = if n > 0 {
        tf("Active custom sound volume overrides: {overrideCount}.", &[("overrideCount", &n.to_string())])
    } else {
        t("Set custom volumes for individual sounds. Sounds without an override follow the master volume.").to_string()
    };
    let head = row()
        .w_full()
        .gap(12.0)
        .py(8.0)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .expanded(open)
        .on_press(move |s: &mut S| {
            let d = &mut acc(s).form.data;
            if d.remove("sound-overrides").is_none() {
                d.insert("sound-overrides".into(), json!(true));
            }
        })
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text(t("Per-sound volume")).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary))
                .child(text(desc).font(font(13.0, 400.0).lh(18.0)).color(p.text_secondary).wrap()),
        )
        .child(rune::icon(if open { &crate::icons::CARET_UP_BOLD } else { &crate::icons::CARET_DOWN_BOLD }, 16.0).color(p.text_tertiary));
    let mut c = col().w_full().px(12.0).radius(8.0).fill(p.form_surface_background).border(1.0, p.background_modifier_accent).child(head);
    if !open {
        return c;
    }
    let pct = |v: f32| format!("{}%", v.round());
    for snd in SOUNDS {
        let id = snd.id();
        let label = t(snd.label());
        let over = look.sound_volumes.get(id).copied();
        let value = over.unwrap_or(look.volume);
        let enabled = look.sound_on(id);
        let disabled = all_off || !enabled;
        let shown = match over {
            Some(_) => pct(value),
            None => tf("Following master • {effectiveValue}", &[("effectiveValue", &pct(value))]),
        };
        let mute = rune::icon(if enabled { &crate::icons::SPEAKER_HIGH_FILL } else { &crate::icons::SPEAKER_X_FILL }, 14.0)
            .color(if enabled { p.text_secondary } else { p.status_danger })
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(tf(if enabled { "Mute {label}" } else { "Unmute {label}" }, &[("label", label)]))
            .on_press(move |s: &mut S| update(s, acc, |l| { l.sounds_off.insert(id.to_string(), enabled); }));
        let mut header = row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(text(label.to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).truncate().shrink())
            .child(mute)
            .child(el().grow(1.0))
            .child(text(shown).font(font(12.0, 500.0).lh(16.0)).color(p.text_secondary));
        if over.is_some() && !disabled {
            header = header.child(
                rune::icon(&crate::icons::ARROW_COUNTER_CLOCKWISE_BOLD, 14.0)
                    .color(p.text_tertiary)
                    .cursor(rune::Cursor::Pointer)
                    .role("button")
                    .label(tf("Reset {label} to master volume", &[("label", label)]))
                    .on_press(move |s: &mut S| update(s, acc, |l| { l.sound_volumes.remove(id); })),
            );
        }
        let slider = Slider::new(format!("sound-volume-{id}"), value, 0.0, 200.0)
            .step(1.0)
            .disabled(disabled)
            .on_change(move |s: &mut S, v| update(s, acc, |l| { l.sound_volumes.insert(id.to_string(), v.round().clamp(0.0, 200.0)); }))
            .el(p);
        c = c.child(col().w_full().gap(6.0).px(4.0).py(8.0).opacity(if over.is_some() { 1.0 } else { 0.78 }).child(header).child(slider));
    }
    if n > 0 && !all_off {
        c = c.child(
            row().w_full().py(8.0).child(
                Button::new(t("Reset all overrides"))
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Small)
                    .key("sound-overrides-reset")
                    .on_press(move |s: &mut S| update(s, acc, |l| l.sound_volumes.clear()))
                    .el(p),
            ),
        );
    }
    c
}

/// One sound: its switch, preview, and custom file upload / removal.
fn sound_row<S: 'static>(snd: Sound, look: &Prefs, all_off: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let id = snd.id();
    let on = !all_off && look.sound_on(id);
    let label = t(snd.label());
    let custom = look.custom_sounds.get(id).cloned();
    let icon_btn = |icon: &'static rune::Icon, label: String| {
        rune::icon(icon, 16.0).color(p.text_tertiary).cursor(rune::Cursor::Pointer).role("button").label(label)
    };
    let (volume, custom2) = (look.sound_gain(id), custom.clone());
    let mut r = row()
        .w_full()
        .h(40.0)
        .gap(12.0)
        .center_y()
        .child(text(label.to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0).min_w(0.0).truncate())
        .child(icon_btn(&crate::icons::PLAY_BOLD, t("Preview sound").into()).on_press(move |_: &mut S| crate::desktop::sound::play(snd, volume, custom2.as_deref())));
    if custom.is_some() {
        r = r.child(icon_btn(&crate::icons::TRASH_BOLD, tf("Remove custom sound for {label}", &[("label", label)])).on_press(move |s: &mut S| {
            if let Some(path) = crate::desktop::sound::custom_path(snd) {
                let _ = std::fs::remove_file(path);
            }
            update(s, acc, |l| {
                l.custom_sounds.remove(id);
            })
        }));
    }
    r = r.child(icon_btn(&crate::icons::UPLOAD_SIMPLE_BOLD, tf("Upload custom sound for {label}", &[("label", label)])).on_press_tx(move |_: &mut S, tx: &mut Tx| {
        let options = rune::FileDialogOptions { title: t("Custom sounds").into(), ..Default::default() }.filter(t("Audio"), &["mp3", "ogg", "wav", "flac"]);
        tx.open_file(options, move |s: &mut S, r, _| {
            let Ok(Some(src)) = r else { return };
            match store_custom(snd, &src) {
                Ok(path) => update(s, acc, |l| {
                    l.custom_sounds.insert(id.to_string(), path);
                }),
                Err(e) => crate::desktop::toast(&mut acc(s).desktop, e, false),
            }
        });
    }));
    r.child(Switch::new(on).disabled(all_off).on_toggle(move |s: &mut S| update(s, acc, |l| {
        l.sounds_off.insert(id.to_string(), on);
    })).el(p))
}

/// Copies a picked audio file under the config dir (`CustomSoundDB`), checking its size.
fn store_custom(snd: Sound, src: &std::path::Path) -> Result<String, String> {
    let len = std::fs::metadata(src).map_err(|e| e.to_string())?.len();
    if len > CUSTOM_MAX_BYTES {
        return Err(tf("Sound files can be at most {maxSize}.", &[("maxSize", &format!("{} MB", CUSTOM_MAX_BYTES / 1024 / 1024))]));
    }
    let dst = crate::desktop::sound::custom_path(snd).ok_or("no config directory")?;
    std::fs::create_dir_all(dst.parent().ok_or("no config directory")?).map_err(|e| e.to_string())?;
    std::fs::copy(src, &dst).map_err(|e| e.to_string())?;
    Ok(dst.to_string_lossy().into_owned())
}

// ---- Themes --------------------------------------------------------------------------------

/// `ThemeSelector` options: (setting value, label, swatch).
const THEMES: [(&str, &str, [u8; 3]); 5] = [
    ("dark", "Dark theme", [29, 28, 34]),
    ("dark_legacy", "Dark (legacy) theme", [25, 27, 32]),
    ("coal", "Coal theme", [5, 5, 6]),
    ("light", "Light theme", [251, 251, 252]),
    ("system", "System", [12, 11, 14]),
];

/// Token groups of the colour editor (`TokenGroups.ts` order), by CSS name prefix.
const GROUPS: [(&str, &[&str]); 7] = [
    ("Surfaces", &["--background-", "--guild-list", "--panel-", "--control-"]),
    ("Text", &["--text-"]),
    ("Brand & accents", &["--brand-", "--accent-"]),
    ("Status", &["--status-"]),
    ("Borders", &["--border-"]),
    ("Alerts", &["--alert-"]),
    ("Other", &[]),
];

fn group_of(name: &str) -> &'static str {
    GROUPS.iter().find(|(_, pre)| pre.iter().any(|p| name.starts_with(p))).map_or(t("Other"), |(g, _)| g)
}

pub fn themes<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let look = &st.prefs.look;
    let current = look.theme(&store.settings.theme).to_string();
    let current = current.as_str();
    let sync = look.theme_sync;
    let buttons = row().gap(12.0).role("radiogroup").label(t("Theme")).children(THEMES.iter().map(|&(value, label, [r, g, b])| {
        let selected = current == value || (value == "dark" && !THEMES.iter().any(|(v, ..)| *v == current));
        let light = value == "light";
        let border = if selected { p.brand_primary_foreground } else if light { p.border_color } else { rune::Color::WHITE };
        let swatch = crate::ui::rgb(r, g, b);
        let mut b = el().size(56.0, 56.0).radius(28.0).fill(swatch).border(2.0, crate::ui::over(border, swatch)).role("radio").checked(selected).label(t(label));
        if value == "system" {
            b = b.child(row().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().child(rune::icon(&crate::icons::ARROWS_COUNTER_CLOCKWISE_BOLD, 24.0).color(rune::Color::WHITE)));
        }
        if selected {
            b.child(
                row()
                    .pin(None, Some(-4.0), Some(-4.0), None)
                    .size(20.0, 20.0)
                    .radius(10.0)
                    .center()
                    .fill(p.brand_primary)
                    .child(rune::icon(&crate::icons::CHECK_BOLD, 12.0).color(p.brand_primary_fill)),
            )
        } else {
            b.cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| {
                if sync {
                    acc(s).intents.push(Intent::PatchSettings(json!({ "theme": value })));
                } else {
                    update(s, acc, |l| l.local_theme = value.to_string());
                }
            })
        }
    }));
    // `setSyncAcrossDevices`: turning it off starts this device on the theme it shows.
    let shown = current.to_string();
    let sync_switch = Switch::new(sync)
        .label(t("Sync theme across devices"))
        .description(t("Theme changes sync to all your devices. Off keeps this device on its own theme."))
        .on_toggle(move |s: &mut S| {
            let shown = shown.clone();
            update(s, acc, move |l| {
                if l.theme_sync {
                    l.local_theme = shown;
                }
                l.theme_sync = !l.theme_sync;
            })
        })
        .el(p);
    col()
        .w_full()
        .child(section(t("Base theme"), col().w_full().gap(16.0).child(buttons).child(sync_switch), true, p))
        .child(section(t("Your themes"), super::theme_library::your_themes(st, p, acc, layer), false, p))
        .child(colors(store, st, p, acc))
        .child(quick_css(st, p, acc))
        .child(share(st, p, acc))
}

/// A text field in `form.text[key]`, created with `initial` the first time it's edited.
fn field<S: 'static>(st: &ShellState, key: &str, initial: &str, p: &Palette, acc: ShellAccess<S>) -> rune::El {
    let fallback = TextEdit::new(initial);
    let edit = st.form.text.get(key).unwrap_or(&fallback);
    let (k, init) = (key.to_string(), initial.to_string());
    text_input(edit, font(14.0, 400.0).lh(20.0))
        .key(key)
        .color(p.text_primary)
        .caret(p.text_primary, p.background_modifier_selected)
        .on_edit(move |s: &mut S| -> &mut TextEdit { acc(s).form.text.entry(k.clone()).or_insert_with(|| TextEdit::new(&init)) })
}

fn boxed(f: El, p: &Palette) -> El {
    row().w_full().px(12.0).py(8.0).radius(8.0).fill(p.form_surface_background).border(1.0, p.background_modifier_accent).child(f.grow(1.0))
}

/// "Colors and fonts": every palette colour, its current value, and an override field.
fn colors<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let css = &st.prefs.look.css;
    let (overrides, _) = custom::overrides(css);
    let search = st.form.text.get("token-search").map(|e| e.text.trim().to_lowercase()).unwrap_or_default();
    let base = custom::effective(st.prefs.look.theme(&store.settings.theme), !crate::ui::is_light(p), 1.0, "");
    let mut out = col()
        .w_full()
        .gap(12.0)
        .child(muted(t("Change single colors and fonts of the base theme. Your changes are saved in the quick CSS of the theme you're editing."), p))
        .child(boxed(field(st, "token-search", "", p, acc).single_line().placeholder(t("Search colors and fonts…"), p.text_tertiary), p));
    if !overrides.is_empty() {
        let n = overrides.len() as i64;
        out = out.child(
            row()
                .gap(12.0)
                .center_y()
                .child(text(tp("{count, plural, one {# changed} other {# changed}}", n, &[])).font(font(13.0, 500.0).lh(18.0)).color(p.text_secondary))
                .child(
                    Button::new(t("Clear all changes"))
                        .variant(ButtonVariant::Secondary)
                        .size(ButtonSize::Small)
                        .key("clear-tokens")
                        .on_press(move |s: &mut S| {
                            let names: Vec<String> = custom::overrides(&acc(s).prefs.look.css).0.into_iter().map(|(n, _)| n).collect();
                            acc(s).form.text.retain(|k, _| !k.starts_with("tok-") && k != "quick-css");
                            update(s, acc, |l| l.css = names.iter().fold(l.css.clone(), |css, n| custom::set_var(&css, n, None)));
                        })
                        .el(p),
                ),
        );
    }
    let open = st.form.data.get("token-group").and_then(|v| v.as_str()).unwrap_or("");
    // Typography: the font families (Rune swaps them with `Tx::set_font_families`).
    let fonts: Vec<&'static str> = custom::FONTS.into_iter().filter(|n| search.is_empty() || n.contains(&search)).collect();
    if !fonts.is_empty() {
        let expanded = !search.is_empty() || open == "Typography";
        out = out.child(group_head("Typography", fonts.len(), expanded, p, acc));
        if expanded {
            let set = custom::fonts(css);
            out = out.children(fonts.into_iter().map(|name| {
                let i = usize::from(name == "--font-mono");
                font_row(st, name, set[i].clone(), [crate::theme::FAMILIES.0, crate::theme::FAMILIES.1][i], p, acc)
            }));
        }
    }
    let mut base = base;
    for (group, _) in GROUPS {
        let names: Vec<&str> = crate::theme::NAMES.iter().copied().filter(|n| group_of(n) == group && (search.is_empty() || n.contains(&search))).collect();
        if names.is_empty() {
            continue;
        }
        let expanded = !search.is_empty() || open == group;
        out = out.child(group_head(group, names.len(), expanded, p, acc));
        if !expanded {
            continue;
        }
        out = out.children(names.into_iter().map(|name| token_row(st, name, &overrides, base.get_mut(name).copied(), p, acc)));
    }
    // shortcut: sizes, spacing and other non-colour, non-font variables can't be overridden
    // (Rune lays out with fixed metrics).
    section(t("Colors and fonts"), out, false, p)
}

/// A collapsible token group heading with its count.
fn group_head<S: 'static>(group: &'static str, n: usize, expanded: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    row()
        .w_full()
        .h(36.0)
        .gap(8.0)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .child(rune::icon(if expanded { &crate::icons::CARET_DOWN_BOLD } else { &crate::icons::CARET_RIGHT_BOLD }, 14.0).color(p.text_tertiary))
        .child(text(t(group)).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary).grow(1.0))
        .child(text(n.to_string()).font(font(12.0, 500.0).lh(16.0)).color(p.text_tertiary))
        .on_press(move |s: &mut S| {
            let form = &mut acc(s).form;
            let same = form.data.get("token-group").and_then(|v| v.as_str()) == Some(group);
            form.data.insert("token-group".into(), json!(if same { "" } else { group }));
        })
}

/// A font family token: the override (a CSS family list) or the bundled family it falls back to.
fn font_row<S: 'static>(st: &ShellState, name: &'static str, value: Option<String>, base: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let key = format!("tok-{name}");
    let shown = value.clone().unwrap_or_else(|| base.to_string());
    let input = field(st, &key, &shown, p, acc).single_line().font(font(13.0, 400.0).mono().lh(18.0)).on_submit(move |s: &mut S, _| {
        let sh = acc(s);
        let v = sh.form.text.get(&format!("tok-{name}")).map(|e| e.text.trim().to_string()).unwrap_or_default();
        sh.form.text.remove("quick-css");
        let base = if name == "--font-mono" { crate::theme::FAMILIES.1 } else { crate::theme::FAMILIES.0 };
        update(s, acc, |l| l.css = custom::set_var(&l.css, name, (!v.is_empty() && v != base).then_some(v.as_str())));
    });
    let mut r = row()
        .w_full()
        .h(36.0)
        .gap(12.0)
        .pl(22.0)
        .center_y()
        .child(text("Aa").font(font(14.0, 500.0).lh(20.0)).color(p.text_secondary).w(20.0))
        .child(text(name).font(font(13.0, 400.0).mono().lh(18.0)).color(if value.is_some() { p.text_primary } else { p.text_secondary }).grow(1.0).min_w(0.0).truncate())
        .child(row().w(200.0).px(8.0).py(4.0).radius(6.0).fill(p.form_surface_background).border(1.0, p.background_modifier_accent).child(input.grow(1.0)));
    if value.is_some() {
        r = r.child(rune::icon(&crate::icons::ARROW_COUNTER_CLOCKWISE_BOLD, 14.0).color(p.text_tertiary).cursor(rune::Cursor::Pointer).role("button").label(t("Reset")).on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.form.text.remove(&format!("tok-{name}"));
            sh.form.text.remove("quick-css");
            update(s, acc, |l| l.css = custom::set_var(&l.css, name, None));
        }));
    }
    r
}

fn token_row<S: 'static>(st: &ShellState, name: &'static str, overrides: &[(String, String)], base: Option<rune::Color>, p: &Palette, acc: ShellAccess<S>) -> El {
    let value = overrides.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
    let shown = value.clone().unwrap_or_else(|| base.map(custom::to_hex).unwrap_or_default());
    let swatch = value.as_deref().and_then(custom::parse_color).or(base).unwrap_or(rune::Color::TRANSPARENT);
    let key = format!("tok-{name}");
    let input = field(st, &key, &shown, p, acc).single_line().font(font(13.0, 400.0).mono().lh(18.0)).on_submit(move |s: &mut S, _| {
        let sh = acc(s);
        let v = sh.form.text.get(&format!("tok-{name}")).map(|e| e.text.trim().to_string()).unwrap_or_default();
        if v.is_empty() || custom::parse_color(&v).is_some() {
            sh.form.text.remove("quick-css");
            update(s, acc, |l| l.css = custom::set_var(&l.css, name, (!v.is_empty()).then_some(v.as_str())));
        }
    });
    let mut r = row()
        .w_full()
        .h(36.0)
        .gap(12.0)
        .pl(22.0)
        .center_y()
        .child(el().size(20.0, 20.0).radius(4.0).fill(swatch).border(1.0, p.background_modifier_accent))
        .child(text(name).font(font(13.0, 400.0).mono().lh(18.0)).color(if value.is_some() { p.text_primary } else { p.text_secondary }).grow(1.0).min_w(0.0).truncate())
        .child(row().w(140.0).px(8.0).py(4.0).radius(6.0).fill(p.form_surface_background).border(1.0, p.background_modifier_accent).child(input.grow(1.0)));
    if value.is_some() {
        r = r.child(rune::icon(&crate::icons::ARROW_COUNTER_CLOCKWISE_BOLD, 14.0).color(p.text_tertiary).cursor(rune::Cursor::Pointer).role("button").label(t("Reset")).on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.form.text.remove(&format!("tok-{name}"));
            sh.form.text.remove("quick-css");
            update(s, acc, |l| l.css = custom::set_var(&l.css, name, None));
        }));
    }
    r
}

/// Quick CSS: the text, saved on demand; only theme variable overrides apply.
fn quick_css<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let css = &st.prefs.look.css;
    let (applied, ignored) = custom::overrides(css);
    let shown = st.form.text.get("quick-css").map_or(css.as_str(), |e| &e.text);
    let editor = field(st, "quick-css", css, p, acc)
        .font(font(13.0, 400.0).mono().lh(18.0))
        .min_h(160.0)
        .placeholder(":root { --text-link: #00a8fc; }", p.text_tertiary)
        .decorations(crate::quick_css_syntax::spans(shown, p))
        .on_input_key(move |s: &mut S, k, _| acc(s).form.text.get_mut("quick-css").is_some_and(|e| crate::quick_css_syntax::key(e, 0, k)));
    let lines = css.lines().count() as i64;
    let summary = if css.trim().is_empty() { t("You haven't written any quick CSS yet.").to_string() } else { tp("{lines, plural, one {# line} other {# lines}} of quick CSS.", lines, &[("lines", &lines.to_string())]) };
    let mut c = col()
        .w_full()
        .gap(12.0)
        .child(muted(&summary, p))
        .child(boxed(editor, p))
        .child(
            row()
                .gap(8.0)
                .child(
                    Button::new(t("Save"))
                        .size(ButtonSize::Small)
                        .key("quick-css-save")
                        .on_press(move |s: &mut S| {
                            let sh = acc(s);
                            let Some(v) = sh.form.text.get("quick-css").map(|e| e.text.to_string()) else { return };
                            sh.form.text.retain(|k, _| !k.starts_with("tok-"));
                            update(s, acc, |l| l.css = v);
                        })
                        .el(p),
                )
                .child({
                    // The editor's own window, on the text as typed here.
                    let pal = *p;
                    Button::new(t("Open in a new window"))
                        .variant(ButtonVariant::Secondary)
                        .size(ButtonSize::Small)
                        .icon(&crate::icons::ARROW_SQUARE_OUT)
                        .key("quick-css-window")
                        .on_press(move |s: &mut S| {
                            let sh = acc(s);
                            let css = sh.form.value("quick-css", &sh.prefs.look.css);
                            crate::quick_css::open(&css, &pal);
                        })
                        .el(p)
                }),
        );
    // Rune draws no CSS: say what of it is used.
    let note = if ignored > 0 {
        format!("Reflux applies only theme variable overrides inside :root {{ … }} ({} applied). {} other rule(s) or declaration(s) are ignored.", applied.len(), ignored)
    } else {
        format!("Reflux applies only theme variable overrides inside :root {{ … }} ({} applied).", applied.len())
    };
    c = c.child(text(note).font(font(13.0, 400.0).lh(18.0)).color(if ignored > 0 { p.status_warning } else { p.text_tertiary }).wrap());
    section(t("Quick CSS"), c, false, p)
}

/// "Share your theme": a theme link (`POST /users/@me/themes`), copying the CSS, and importing a
/// theme link or CSS text.
fn share<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let css = st.prefs.look.css.clone();
    let link = st.form.data.get("theme-share").and_then(|v| v.get("id")).and_then(|v| v.as_str()).map(|id| format!("{}/theme/{id}", st.web_base.trim_end_matches('/')));
    let (css2, empty) = (css.clone(), css.trim().is_empty());
    let mut c = col()
        .w_full()
        .gap(12.0)
        .child(muted(t("Create a link to your theme or copy its CSS."), p))
        .child(
            row()
                .gap(8.0)
                .child(
                    Button::new(t("Share your theme"))
                        .size(ButtonSize::Small)
                        .disabled(empty)
                        .key("theme-share")
                        .on_press(move |s: &mut S| acc(s).api(reqwest::Method::POST, "/users/@me/themes", Some(json!({ "css": css2 })), crate::guild::Then::Keep("theme-share".into())))
                        .el(p),
                )
                .child(Button::new(t("Copy CSS")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).disabled(empty).key("theme-copy").on_press_tx(move |_: &mut S, tx: &mut Tx| tx.copy(css.clone())).el(p)),
        );
    if empty {
        c = c.child(muted(t("Turn on a saved theme or write some quick CSS before sharing."), p));
    }
    if let Some(url) = link {
        let u = url.clone();
        c = c.child(row().gap(8.0).center_y().child(text(url).font(font(14.0, 500.0).lh(20.0)).color(p.text_link).selectable(true)).child(
            Button::new(t("Copy link")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).key("theme-link-copy").on_press_tx(move |_: &mut S, tx: &mut Tx| tx.copy(u.clone())).el(p),
        ));
    }
    // Import: a theme link is fetched from the media proxy (`/themes/<id>.css`); other text is CSS.
    let input = st.form.text.get("theme-import").map(|e| e.text.trim().to_string()).unwrap_or_default();
    let pending = theme_id(&input).map(|id| format!("{}/themes/{id}.css", st.media.base().trim_end_matches('/')));
    let fetched = pending.as_deref().and_then(|u| st.media.text(u));
    let ready = match (&pending, &fetched) {
        (Some(_), Some(Ok(css))) => Some(css.clone()),
        (None, _) if !input.is_empty() => Some(input.clone()),
        _ => None,
    };
    let status = match (&pending, &fetched) {
        (Some(_), None) => Some(t("Loading…").to_string()),
        (Some(_), Some(Err(e))) => Some(e.clone()),
        _ => None,
    };
    let import = Button::new(t("Import"))
        .size(ButtonSize::Small)
        .disabled(ready.is_none())
        .key("theme-import")
        .on_press(move |s: &mut S| {
            let Some(new) = ready.clone() else { return };
            let sh = acc(s);
            sh.form.text.remove("theme-import");
            sh.form.text.retain(|k, _| !k.starts_with("tok-") && k != "quick-css");
            update(s, acc, |l| l.css = new);
        })
        .el(p);
    c = c
        .child(text(t("Import a theme")).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary))
        .child(row().w_full().gap(8.0).center_y().child(boxed(field(st, "theme-import", "", p, acc).single_line().placeholder("https://fluxer.app/theme/…", p.text_tertiary), p).grow(1.0)).child(import));
    if let Some(s) = status {
        c = c.child(muted(&s, p));
    }
    c = c.children(super::theme_library::backup_rows(st, p, acc));
    section(t("Share and back up"), c, false, p)
}

/// The ID in a theme link (`…/theme/<16 hex>`).
pub fn theme_id(s: &str) -> Option<&str> {
    let id = s.trim().trim_end_matches('/').rsplit_once("/theme/")?.1;
    (id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn theme_links() {
        assert_eq!(super::theme_id("https://fluxer.app/theme/0123456789abcdef"), Some("0123456789abcdef"));
        assert_eq!(super::theme_id(":root { --x: red }"), None);
        assert_eq!(super::group_of("--background-primary"), "Surfaces");
        assert_eq!(super::group_of("--ansi-fg-red"), "Other");
    }
}
