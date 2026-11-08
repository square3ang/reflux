//! The Voice & video tab (`VoiceVideoTab.tsx` = `UserVoiceTab` + `UserVideoTab`): devices,
//! volume, voice processing (profiles, push-to-talk, activity threshold), mic test, voice
//! channel display, camera and screen share quality.

use reflux_model::Snowflake;
use rune::{El, col, el, row};
use crate::ui::mark::marked as text;

use super::tabs::section;
use crate::shell::voice::{Modal, Op, Settings};
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, dim, font};
use crate::ui::{Button, ButtonVariant, RadioGroup, Select, Slider, Switch};
use crate::i18n::t;

/// Changes a setting, then saves and applies it.
fn set<S: 'static>(acc: ShellAccess<S>, f: impl Fn(&mut Settings) + 'static) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| {
        let sh = acc(s);
        f(&mut sh.voice.settings);
        sh.intents.push(Intent::Voice(Op::Settings));
    }
}

fn desc(s: &str, p: &Palette) -> El {
    text(s.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap()
}

fn label(s: &str, p: &Palette) -> El {
    crate::ui::mark::marked(s).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary)
}

/// The Voice & video tab body.
pub fn tab<S: 'static>(store: &reflux_store::Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let v = &st.voice;
    let set_ = &v.settings;
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);

    // ---- Devices
    let devices = match &v.devices {
        None => desc("Looking for devices…", p),
        Some((inputs, outputs)) => {
            let options = |names: &[String]| -> Vec<(String, String)> { std::iter::once((String::new(), "Default".to_string())).chain(names.iter().map(|n| (n.clone(), n.clone()))).collect() };
            let (ins, outs) = (options(inputs), options(outputs));
            let mut pick = |id: &str, name: &str, opts: &[(String, String)], value: &Option<String>, output: bool| {
                let refs: Vec<(&str, &str)> = opts.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
                // A device that disappeared (or a pre-Pulse cpal name) reads as Default, which
                // is what the engine falls back to.
                let selected = value.as_deref().filter(|v| opts.iter().any(|(id, _)| id == v)).unwrap_or("");
                Select::new(id, &refs, Some(selected))
                    .label(name)
                    .width(340.0)
                    .on_choose(move |s: &mut S, v| {
                        let sh = acc(s);
                        let v = (!v.is_empty()).then(|| v.to_string());
                        if output { sh.voice.output = v } else { sh.voice.input = v }
                        sh.intents.push(Intent::VoiceDevices);
                    })
                    .el(&st.popups, pops, p, layer)
            };
            let a = pick("voice-input", t("Input device"), &ins, &v.input, false);
            let b = pick("voice-output", t("Output device"), &outs, &v.output, true);
            col().w_full().gap(16.0).child(a).child(b)
        }
    };
    let mut out = col().w_full().child(section(t("Devices"), devices, true, p));

    // ---- Volume (applied by our own mic path and mixer, `reflux_voice::audio`)
    let vol = |key: &'static str, name: &str, value: f32, input: bool| {
        col().w_full().gap(8.0).child(label(name, p)).child(
            Slider::new(key, value, 0.0, 200.0)
                .step(1.0)
                .on_change(move |s: &mut S, x| {
                    set(acc, move |st: &mut Settings| if input { st.input_volume = x } else { st.output_volume = x })(s);
                })
                .el(p),
        )
    };
    out = out.child(section(t("Volume"), col().w_full().gap(20.0).child(vol("voice-in-vol", t("Input volume"), set_.input_volume, true)).child(vol("voice-out-vol", t("Output volume"), set_.output_volume, false)), false, p));

    // ---- Voice processing
    let profiles = [
        ("voice", "Focused voice", Some(t("Recommended. Cleans up your mic for clear speech."))),
        ("studio", "Direct input", Some(t("Sends your audio untouched. Best if you're using external audio software."))),
        ("custom", "Custom", Some(t("Adjust noise suppression, echo cancellation, and gain yourself."))),
    ];
    let mut processing = col().w_full().gap(20.0).child(
        RadioGroup::new(&profiles, Some(set_.processing.as_str()))
            .on_change(move |s: &mut S, val| {
                let val = val.to_string();
                set(acc, move |st: &mut Settings| st.processing = val.clone())(s)
            })
            .el(p),
    );
    if set_.processing != "studio" {
        processing = processing.child(Switch::new(set_.push_to_talk).label(t("Push-to-talk")).on_toggle(set(acc, |st: &mut Settings| st.push_to_talk ^= true)).el(p));
        if set_.push_to_talk {
            let key = if v.recording_key { "Press a key…".to_string() } else { set_.ptt_key.clone().unwrap_or_else(|| t("No keybind set").into()) };
            let record = Button::new(if v.recording_key { t("Cancel") } else { t("Edit keybind") })
                .variant(ButtonVariant::Secondary)
                .key("ptt-record")
                .on_press(move |s: &mut S| acc(s).voice.recording_key ^= true)
                .el(p);
            processing = processing
                .child(
                    col().w_full().gap(8.0).child(label(t("Keybind"), p)).child(
                        row()
                            .w_full()
                            .gap(12.0)
                            .center_y()
                            .child(row().grow(1.0).h(40.0).px(12.0).center_y().radius(dim::RADIUS_LG).fill(p.background_tertiary).child(text(key).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary)))
                            .child(record),
                    ),
                )
                .child(desc("This key works while Reflux has focus. The global Push-to-talk shortcut (set in your desktop's settings) works anywhere.", p))
                .child(
                    col()
                        .w_full()
                        .gap(8.0)
                        .child(label(t("Release delay"), p))
                        .child(desc(t("Keep transmitting briefly after release."), p))
                        .child(
                            Slider::new("ptt-release", set_.ptt_release_ms as f32, 0.0, 2000.0)
                                .step(20.0)
                                .on_change(move |s: &mut S, x| set(acc, move |st: &mut Settings| st.ptt_release_ms = x as u64)(s))
                                .el(p),
                        )
                        .child(text(format!("{} ms", set_.ptt_release_ms)).font(font(12.0, 500.0).lh(16.0)).color(p.text_tertiary)),
                );
        }
        processing = processing
            .child(
                Switch::new(set_.auto_gain_control)
                    .label(t("Automatic gain control"))
                    .description(t("Evens out your mic volume so you are not too quiet."))
                    .on_toggle(set(acc, |st: &mut Settings| st.auto_gain_control ^= true))
                    .el(p),
            )
            .child(Switch::new(set_.stereo).label(t("Stereo microphone")).on_toggle(set(acc, |st: &mut Settings| st.stereo ^= true)).el(p));
    }
    if !set_.ptt() && set_.processing != "studio" {
        processing = processing.child(Switch::new(set_.vad_auto).label(t("Auto-set activity threshold")).on_toggle(set(acc, |st: &mut Settings| st.vad_auto ^= true)).el(p));
        if !set_.vad_auto {
            processing = processing.child(
                col()
                    .w_full()
                    .gap(8.0)
                    .child(label(t("Activity threshold"), p))
                    .child(meter(v.mic_test, v.level, Some(set_.vad_threshold), p))
                    .child(Slider::new("vad-threshold", set_.vad_threshold, -100.0, 0.0).step(1.0).on_change(move |s: &mut S, x| set(acc, move |st: &mut Settings| st.vad_threshold = x)(s)).el(p)),
            );
        }
    }
    if set_.processing == "custom" {
        processing = processing
            .child(crate::shell::call::noise_select(st, p, acc, layer))
            .child(Switch::new(set_.echo_cancellation).label(t("Echo cancellation")).on_toggle(set(acc, |st: &mut Settings| st.echo_cancellation ^= true)).el(p));
    }
    out = out.child(section(t("Voice processing"), processing, false, p));

    // ---- Mic test
    // `MicTestSection`: a square play / stop button (`Stop mic test` / `Start mic test`) that
    // plays the processed mic back through the speakers 0.9 s late (`useMicTest`).
    let test = Button::new(String::new())
        .icon(if v.mic_test { &crate::icons::STOP_FILL } else { &crate::icons::PLAY_FILL })
        .square()
        .variant(if v.mic_test { ButtonVariant::Secondary } else { ButtonVariant::Primary })
        .key("mic-test")
        .on_press({
            let on = !v.mic_test;
            move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::MicTest(on)))
        })
        .el(p)
        .size(40.0, 40.0) // `.actionButton`: 2.5rem
        .label(if v.mic_test { t("Stop mic test") } else { t("Start mic test") });
    let threshold = (!set_.vad_auto && !set_.ptt()).then_some(set_.vad_threshold);
    // `.bar`: meter then button, gap 10, padding 12, 1px accent border, radius 8.
    let bar = row()
        .w_full()
        .gap(10.0)
        .padding(12.0)
        .center_y()
        .radius(8.0)
        .border(1.0, p.background_modifier_accent)
        .fill(p.background_secondary)
        .child(row().grow(1.0).min_w(0.0).center_y().child(meter(v.mic_test, v.level, threshold, p)))
        .child(test);
    out = out.child(section(t("Mic test"), bar, false, p));
    out = out.child(section(t("Entrance sound"), entrance(store, st, p, acc, layer), false, p));

    // ---- Voice channels
    out = out.child(section(
        "Voice channels",
        Switch::new(set_.show_connection_ids).label(t("Show connection IDs")).on_toggle(set(acc, |st: &mut Settings| st.show_connection_ids ^= true)).el(p),
        false,
        p,
    ));

    // ---- Video
    let cams = v.cameras.clone().unwrap_or_default();
    let cam_opts: Vec<(&str, &str)> = std::iter::once(("", "Default")).chain(cams.iter().map(|n| (n.as_str(), n.as_str()))).collect();
    let cam_ok = crate::camera::available();
    let camera = Select::new("settings-camera", &cam_opts, Some(set_.camera.as_deref().unwrap_or("")))
        .label(t("Camera"))
        .width(340.0)
        .disabled(!cam_ok)
        .on_choose(move |s: &mut S, val| {
            let val = (!val.is_empty()).then(|| val.to_string());
            set(acc, move |st: &mut Settings| st.camera = val.clone())(s)
        })
        .el(&st.popups, pops, p, layer);
    let preview = Button::new(t("Start camera test and configure effects"))
        .variant(ButtonVariant::Secondary)
        .key("camera-test")
        .disabled(!cam_ok)
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.modal = Some(Modal::CameraPreview);
            sh.intents.push(Intent::Voice(Op::LoadCameras));
            sh.intents.push(Intent::Voice(Op::Preview(true)));
        })
        .el(p);
    let quality: Vec<(&str, &str)> =
        if st.limits.higher_video_quality { vec![("low", "480p"), ("medium", "720p"), ("high", "1080p")] } else { vec![("low", "480p"), ("medium", "720p")] };
    let cam_q = if quality.iter().any(|(v, _)| *v == set_.camera_quality) { set_.camera_quality.as_str() } else { "medium" };
    let cam_quality = Select::new("camera-quality", &quality, Some(cam_q))
        .label(t("Camera quality"))
        .width(340.0)
        .on_choose(move |s: &mut S, val| {
            let val = val.to_string();
            set(acc, move |st: &mut Settings| st.camera_quality = val.clone())(s)
        })
        .el(&st.popups, pops, p, layer);
    let mut video = col().w_full().gap(20.0).child(camera);
    if !cam_ok {
        video = video.child(desc("Camera needs Reflux built with GStreamer.", p));
    }
    video = video
        .child(Switch::new(set_.mirror_camera).label(t("Mirror camera")).on_toggle(set(acc, |st: &mut Settings| st.mirror_camera ^= true)).el(p))
        .child(row().child(preview))
        .child(cam_quality);
    out = out.child(section(t("Video"), video, false, p));

    // ---- Screen share
    let higher = st.limits.higher_video_quality;
    let quality = set_.screen_quality(&st.limits, higher);
    let res = crate::shell::voice::screen_resolutions(higher);
    let res: Vec<(&str, &str)> = res.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
    let fps = crate::shell::voice::screen_frame_rates(higher);
    let fps: Vec<(&str, &str)> = fps.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
    let res_v = quality.max_height.map_or("source".to_string(), |h| h.to_string());
    let fps_v = quality.fps.to_string();
    let r = Select::new("settings-share-res", &res, Some(&res_v))
        .label(t("Screen share quality"))
        .width(220.0)
        .on_choose(move |s: &mut S, val| {
            let h = val.parse().ok();
            set(acc, move |st: &mut Settings| {
                (st.screen_height, st.streaming_mode) = (h, "custom".into());
            })(s)
        })
        .el(&st.popups, pops, p, layer);
    let f = Select::new("settings-share-fps", &fps, Some(&fps_v))
        .label(t("Frame rate"))
        .width(220.0)
        .on_choose(move |s: &mut S, val| {
            let n = val.parse().unwrap_or(30);
            set(acc, move |st: &mut Settings| {
                (st.screen_fps, st.streaming_mode) = (n, "custom".into());
            })(s)
        })
        .el(&st.popups, pops, p, layer);
    let mut share = col().w_full().gap(20.0).child(row().w_full().gap(16.0).child(r).child(f));
    if let Some(preset) = set_.screen_preset() {
        let note = crate::i18n::tf("Set by the {preset} preset. Picking a value here switches to Custom.", &[("preset", t(preset.0))]);
        share = share.child(desc(&note, p));
    }
    if !higher && st.premium {
        let note = crate::i18n::tf("Frame rates above 30 FPS require {premiumProductName}", &[("premiumProductName", "Plutonium")]);
        share = share.child(desc(&note, p));
    }
    // `UserVideoTab`: the bitrate select is part of the higher-quality tier.
    if higher {
        let rates = crate::shell::voice::screen_bitrates(st.limits.screen_share_bitrate, set_.screen_bitrate);
        let rates: Vec<(&str, &str)> = rates.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
        let rate_v = set_.screen_bitrate.map_or("auto".to_string(), |b| b.to_string());
        let b = Select::new("settings-share-bitrate", &rates, Some(&rate_v))
            .label(t("Bitrate"))
            .width(220.0)
            .on_choose(move |s: &mut S, val| {
                let b = val.parse().ok();
                set(acc, move |st: &mut Settings| st.screen_bitrate = b)(s)
            })
            .el(&st.popups, pops, p, layer);
        share = share.child(
            col()
                .w_full()
                .gap(8.0)
                .child(b)
                .child(desc(t("With Automatic, the bitrate follows your resolution and frame rate. Higher values keep fast motion sharper and use more upload bandwidth."), p)),
        );
    }
    out = out.child(section(t("Screen share"), share, false, p));

    // The encoder path lives in Advanced (`ScreenShareEncoderControls`), like Fluxer.
    out
}

/// `EntranceSoundSection`: which scope a sound applies to (everywhere, all communities, DMs, the
/// open community, or another community's override), the sound for it (or the one it
/// inherits), the library with preview / delete, and upload through the trimmer.
fn entrance<S: 'static>(store: &reflux_store::Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(lib) = &st.voice.entrance else {
        return col().w_full().gap(8.0).child(desc(t("Loading…"), p)).child(
            Button::new(t("Load")).variant(ButtonVariant::Secondary).key("entrance-load").on_press(move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::LoadEntrance))).el(p),
        );
    };
    let here = st.guild;
    let scope = st.voice.entrance_scope.clone().unwrap_or_else(|| here.map_or("global".into(), |g| format!("guild:{g}")));
    let guild_name = |g: Snowflake| store.guilds.get(&g).and_then(|x| x.info.as_ref()).map_or_else(|| t("Unknown community").to_string(), |i| i.name.clone());
    let label = |s: &str| match s {
        "global" => t("All communities and DMs").to_string(),
        "guilds" => t("All communities").to_string(),
        "dms" => t("Direct messages").to_string(),
        _ => s.strip_prefix("guild:").and_then(|g| g.parse().ok()).map_or_else(|| t("Unknown community").to_string(), |g| guild_name(Snowflake(g))),
    };
    let hint = match scope.as_str() {
        "global" => t("Your fallback entrance sound for all communities and direct messages. More specific settings override it.").to_string(),
        "guilds" => t("Used in communities that do not have their own sound set.").to_string(),
        "dms" => t("Used in direct-message calls. If you do not set one, your global sound is used instead.").to_string(),
        _ => crate::i18n::tf("Used in this community. If you do not set one, {productName} falls back to your community default, then your global default.", &[("productName", "Fluxer")]),
    };
    let tile = |value: String, glyph: Option<&'static rune::Icon>, guild: Option<Snowflake>, title: String, about: Option<String>| {
        let on = value == scope;
        let art = match (glyph, guild) {
            (_, Some(g)) => crate::shell::guild_face(store, st, g, if about.is_some() { 32.0 } else { 24.0 }, false, p),
            (Some(i), _) => row().size(32.0, 32.0).center().radius(8.0).fill(if on { p.brand_primary } else { p.background_modifier_hover }).child(rune::icon(i, 18.0).color(if on { rune::Color::WHITE } else { p.text_primary_muted })),
            _ => row(),
        };
        let v = value.clone();
        row()
            .min_w(0.0)
            .padding(10.0)
            .gap(10.0)
            .center_y()
            .radius(8.0)
            .fill(if on { p.background_modifier_selected } else { p.background_secondary })
            .border(1.0, if on { p.brand_primary } else { p.background_modifier_accent })
            .hover(p.background_modifier_hover)
            .cursor(rune::Cursor::Pointer)
            .role("radio")
            .label(title.clone())
            .key(format!("entrance-scope-{value}"))
            .child(art)
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text(title).font(font(14.0, 600.0).lh(18.0)).color(p.text_primary).truncate())
                    .children(about.map(|a| text(a).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).wrap())),
            )
            .on_press(move |s: &mut S| acc(s).voice.entrance_scope = Some(v.clone()))
    };
    let mut tiles = vec![
        tile("global".into(), Some(&crate::icons::GLOBE_HEMISPHERE_WEST_FILL), None, label("global"), Some(t("Used by default unless a community or DM sound overrides it.").into())),
        tile("guilds".into(), Some(&crate::icons::USERS_THREE_FILL), None, label("guilds"), Some(t("Used in communities that do not have their own sound.").into())),
        tile("dms".into(), Some(&crate::icons::CHAT_CIRCLE_TEXT_FILL), None, label("dms"), Some(t("Used for all direct-message calls.").into())),
    ];
    if let Some(g) = here {
        tiles.push(tile(format!("guild:{g}"), None, Some(g), guild_name(g), Some(t("Used only in this community.").into())));
    }
    let grid = el().w_full().equal_columns(2).gap(8.0).children(tiles);
    // `orderedGuilds` minus the open one, by name.
    let mut others: Vec<(Snowflake, String)> = store.guilds.keys().filter(|g| Some(**g) != here && store.guilds[*g].info.is_some()).map(|g| (*g, guild_name(*g))).collect();
    others.sort_by(|a, b| a.1.cmp(&b.1));
    let mut c = col()
        .w_full()
        .gap(12.0)
        .child(desc(t("Plays for everyone else in the voice channel when you join."), p))
        .child(col().w_full().gap(4.0).child(text(t("Applies to").to_uppercase()).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted)).child(desc(&hint, p)))
        .child(grid);
    if !others.is_empty() {
        let list = el().w_full().equal_columns(2).gap(6.0).children(others.into_iter().map(|(g, n)| tile(format!("guild:{g}"), None, Some(g), n, None)));
        c = c.child(text(t("Community overrides").to_uppercase()).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted)).child(list);
    }
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    let ids: Vec<String> = lib.sounds.iter().map(|x| x.id.to_string()).collect();
    let opts: Vec<(&str, &str)> = std::iter::once(("", t("None"))).chain(lib.sounds.iter().zip(&ids).map(|(x, id)| (id.as_str(), x.name.as_str()))).collect();
    let cur = lib.selections.get(&scope).map(|x| x.to_string()).unwrap_or_default();
    let sc = scope.clone();
    let pick = Select::new("entrance-scope-sound", &opts, Some(&cur))
        .label(t("Sound for this scope"))
        .width(340.0)
        .on_choose(move |s: &mut S, v| acc(s).intents.push(Intent::Voice(Op::SelectEntrance(sc.clone(), v.parse().ok().map(reflux_model::Snowflake)))))
        .el(&st.popups, pops, p, layer);
    c = c.child(pick);
    // The sound this scope plays: set here, or inherited.
    if let Some((id, from)) = lib.resolve_scope(&scope)
        && let Some(x) = lib.sounds.iter().find(|x| x.id == id)
    {
        let badge = if from == scope { t("Set here").to_string() } else { crate::i18n::tf("Inherited from {inheritedFromLabel}", &[("inheritedFromLabel", &label(&from))]) };
        c = c.child(
            row()
                .w_full()
                .padding(10.0)
                .gap(10.0)
                .center_y()
                .radius(8.0)
                .fill(p.background_secondary)
                .child(
                    row()
                        .size(36.0, 36.0)
                        .center()
                        .radius(18.0)
                        .fill(p.background_modifier_hover)
                        .hover(p.background_modifier_selected)
                        .cursor(rune::Cursor::Pointer)
                        .role("button")
                        .label(t("Preview sound"))
                        .key("entrance-preview-resolved")
                        .child(rune::icon(&crate::icons::SPEAKER_HIGH_FILL, 20.0).color(p.text_primary))
                        .on_press(move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::PreviewEntrance(id)))),
                )
                .child(col().grow(1.0).min_w(0.0).child(text(x.name.clone()).font(font(14.0, 600.0).lh(18.0)).color(p.text_primary).truncate()).child(text(format!("{:.1} s", x.duration_ms as f32 / 1000.0)).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted)))
                .child(text(badge).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted)),
        );
    }
    for x in &lib.sounds {
        let id = x.id;
        c = c.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(text(format!("{} · {:.1} s", x.name, x.duration_ms as f32 / 1000.0)).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0).truncate())
                .child(Button::new(t("Play")).variant(ButtonVariant::Secondary).key(format!("entrance-play-{id}")).on_press(move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::PreviewEntrance(id)))).el(p))
                .child(Button::new(t("Delete")).variant(ButtonVariant::Danger).key(format!("entrance-del-{id}")).on_press(move |s: &mut S| acc(s).intents.push(Intent::Voice(Op::DeleteEntrance(id)))).el(p)),
        );
    }
    // `ENTRANCE_SOUND_MAX_PER_USER`.
    let full = lib.sounds.len() >= 8;
    let upload = Button::new(t("Upload")).key("entrance-upload").disabled(full).on_press_tx(move |_: &mut S, tx: &mut rune::Tx| {
        let options = rune::FileDialogOptions { title: t("Upload").into(), ..Default::default() }.filter(t("Audio"), &["mp3", "ogg", "wav", "m4a"]);
        tx.open_file(options, move |s: &mut S, r, _| {
            let Ok(Some(path)) = r else { return };
            let sh = acc(s);
            let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("Entrance sound").chars().take(32).collect();
            match std::fs::read(&path).ok().and_then(crate::voice_input::decode) {
                Some(pcm) => {
                    let mut trim = crate::shell::voice::Trim { name, samples: std::sync::Arc::new(pcm), ..Default::default() };
                    trim.set(false, crate::shell::voice::Trim::MAX_SECS);
                    sh.voice.trim = Some(trim);
                    sh.voice.modal = Some(Modal::Trim);
                }
                None => sh.notice = Some((t("Entrance sound").into(), t("Could not decode this audio file.").into())),
            }
        });
    });
    c = c.child(row().child(upload.el(p)));
    if full {
        c = c.child(desc(t("Library full (8 sounds max). Delete one before adding another."), p));
    }
    c
}

/// The mic level bar (-100..0 dB), with the activity threshold marked.
fn meter(on: bool, level: f32, threshold: Option<f32>, p: &Palette) -> El {
    let frac = |db: f32| ((db + 100.0) / 100.0).clamp(0.0, 1.0);
    let lvl = if on { frac(level) } else { 0.0 };
    let over = threshold.is_none_or(|t| level > t);
    let mut bar = el()
        .grow(1.0)
        .min_w(120.0)
        .h(8.0)
        .radius(4.0)
        .fill(p.background_tertiary)
        .role("meter")
        .label("Mic level")
        .child(el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w_frac(lvl).radius(4.0).fill(if over { p.status_online } else { p.status_idle }));
    if let Some(t) = threshold {
        bar = bar.child(row().pin(Some(-2.0), Some(0.0), Some(0.0), Some(-2.0)).inert().child(el().w_frac(frac(t))).child(el().w(2.0).h_full().fill(p.text_primary)));
    }
    bar
}
