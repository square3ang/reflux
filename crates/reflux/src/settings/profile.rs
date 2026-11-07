//! The Profile tab (`tabs/MyProfileTab.tsx`, `my_profile_tab/*`): the global or a community
//! profile (`ProfileTypeSelector`), username, display name or nickname, pronouns, avatar and
//! banner (through the crop modal, [`super::crop`]), accent colour and bio, with
//! the live card preview and the unsaved-changes bar. Edits live in `form.*["p.*"]`; saving
//! sends `PATCH /users/@me` or `PATCH /guilds/{id}/members/@me` (`guild-members.mdx`).

use std::hash::{Hash, Hasher};

use reflux_model::{Permissions, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, TextEdit, col, el, row, text_input};
use crate::ui::mark::marked as text;
use serde_json::{Map, Value};

use super::account::{AccountModal, muted, open, small_button, txt};
use super::tabs::{not_available, section};
use super::{SettingsUi, input};
use crate::guild::{Then, hex, parse_hex};
use crate::i18n::t;
use crate::media::Load;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Select, mix, over};

/// `DEFAULT_PROFILE_ACCENT_COLOR`.
const DEFAULT_ACCENT: u32 = 0x4641D9;
const BIO_MAX: usize = 320;

/// Loads the community profile being edited.
pub(super) fn load(sh: &mut ShellState) {
    match sh.settings.as_ref().and_then(|u| u.profile_guild) {
        Some(g) => {
            let me = sh.account.me;
            sh.load("guild_profile", format!("/users/{me}/profile?guild_id={g}"));
        }
        None => super::badges::load(sh),
    }
}

/// The saved values the form starts from.
struct Initial {
    name: String,
    pronouns: String,
    bio: String,
    accent: Option<u32>,
    avatar: Option<String>,
    banner: Option<String>,
    /// The global profile's time zone and who sees it.
    timezone: Option<String>,
    tz_flags: u64,
    /// The global profile's badge settings.
    badges: Option<super::badges::Draft>,
}

fn initial(store: &Store, st: &ShellState, guild: Option<Snowflake>) -> Option<Initial> {
    let me = store.me.as_ref()?;
    Some(match guild {
        None => Initial {
            name: me.base.global_name.clone().unwrap_or_default(),
            pronouns: me.pronouns.clone().unwrap_or_default(),
            bio: me.bio.clone().unwrap_or_default(),
            accent: me.accent_color,
            avatar: me.base.avatar.clone(),
            banner: me.banner.clone(),
            timezone: me.timezone.clone(),
            tz_flags: me.timezone_privacy_flags.bits(),
            badges: Some(super::badges::Draft::saved(me)),
        },
        Some(_) => {
            let v = st.form.data.get("guild_profile")?;
            let (m, gp) = (&v["guild_member"], &v["guild_member_profile"]);
            let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
            Initial {
                name: s(&m["nick"]),
                pronouns: s(&gp["pronouns"]),
                bio: s(&gp["bio"]),
                accent: gp["accent_color"].as_u64().map(|c| c as u32),
                avatar: m["avatar"].as_str().map(str::to_string),
                banner: m["banner"].as_str().or(gp["banner"].as_str()).map(str::to_string),
                timezone: None,
                tz_flags: 0,
                badges: None,
            }
        }
    })
}

fn text_of(st: &ShellState, key: &str, init: &str) -> String {
    st.form.text.get(key).map_or_else(|| init.to_string(), |t| t.text.clone())
}

/// The changed fields as a PATCH body (`null` clears).
fn changes(st: &ShellState, init: &Initial, guild: bool) -> Map<String, Value> {
    let mut m = Map::new();
    let opt = |s: String| if s.trim().is_empty() { Value::Null } else { Value::String(s.trim().to_string()) };
    let name_key = if guild { "nick" } else { "global_name" };
    for (key, field, init) in [("p.name", name_key, &init.name), ("p.pronouns", "pronouns", &init.pronouns), ("p.bio", "bio", &init.bio)] {
        let v = text_of(st, key, init);
        if v.trim() != init.trim() {
            m.insert(field.into(), opt(v));
        }
    }
    let accent = st.form.text.get("p.accent").map(|t| parse_hex(&t.text));
    if let Some(a) = accent
        && a != init.accent
    {
        m.insert("accent_color".into(), a.map_or(Value::Null, Value::from));
    }
    if let Some(v) = st.form.data.get("p.timezone")
        && v.as_str() != init.timezone.as_deref()
    {
        m.insert("timezone".into(), v.clone());
    }
    if let Some(v) = st.form.data.get("p.timezone_privacy_flags")
        && v.as_u64() != Some(init.tz_flags)
    {
        m.insert("timezone_privacy_flags".into(), v.clone());
    }
    for k in ["avatar", "banner"] {
        if let Some(v) = st.form.data.get(&format!("p.{k}")) {
            m.insert(k.into(), v.clone());
        }
    }
    if let Some(saved) = &init.badges {
        super::badges::Draft::current(st, saved).changes(saved, &mut m);
    }
    m
}

// ---- Picking and cropping ---------------------------------------------------------------------

/// A picked data URL decoded for the preview.
pub(crate) fn picked_px(st: &ShellState, url: &str) -> Option<rune::Pixels> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    let b64 = url.split_once(',').map_or(url, |(_, b)| b).to_string();
    let read = move || {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| e.to_string())
    };
    match st.media.local(&format!("pick:{:x}", h.finish()), read) {
        Load::Ready(px) => Some(px),
        _ => None,
    }
}

// ---- View --------------------------------------------------------------------------------------

fn label(s: &str, p: &Palette) -> El {
    crate::ui::mark::marked(s).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary)
}

pub(super) fn tab<S: 'static>(store: &Store, st: &ShellState, ui: &SettingsUi, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(me) = store.me.as_ref() else { return not_available(p) };
    let guild = ui.profile_guild;
    let busy = st.form.busy;

    // `ProfileTypeSelector`.
    let mut opts: Vec<(String, String)> = vec![("global".into(), t("Global profile").into())];
    let ids = store.guild_list().into_iter().flat_map(|i| match i {
        reflux_store::GuildListItem::Guild(g) => vec![g],
        reflux_store::GuildListItem::Folder { guilds, .. } => guilds,
    });
    for g in ids {
        if let Some(i) = store.guilds.get(&g).and_then(|g| g.info.as_ref()) {
            opts.push((g.to_string(), i.name.clone()));
        }
    }
    let refs: Vec<(&str, &str)> = opts.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let cur = guild.map_or("global".to_string(), |g| g.to_string());
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    let dirty_before = initial(store, st, guild).is_some_and(|i| !changes(st, &i, guild.is_some()).is_empty());
    let selector = col()
        .w_full()
        .gap(8.0)
        .pb(24.0)
        .child(
            Select::new("profile-type", &refs, Some(&cur))
                .label(t("Profile"))
                .width(360.0)
                .disabled(dirty_before)
                .on_choose(move |s: &mut S, v| {
                    let sh = acc(s);
                    if let Some(u) = sh.settings.as_mut() {
                        u.profile_guild = v.parse().ok();
                    }
                    super::open_tab(sh, "my_profile");
                })
                .el(&st.popups, pops, p, layer),
        )
        .child(muted(t("Pick a community to set a nickname, avatar, banner and bio only it shows."), p));

    let Some(init) = initial(store, st, guild) else {
        let body = if st.form.error.is_some() { muted(st.form.error.clone().unwrap_or_default(), p) } else { muted(t("Loading…"), p) };
        return col().w_full().child(selector).child(body);
    };
    let name = text_of(st, "p.name", &init.name);
    let pronouns = text_of(st, "p.pronouns", &init.pronouns);
    let bio = text_of(st, "p.bio", &init.bio);
    let accent_text = st.form.text.get("p.accent").map(|t| t.text.clone()).unwrap_or_else(|| init.accent.map(hex).unwrap_or_default());
    let accent = parse_hex(&accent_text);

    let mut form = col().w(0.0).grow(1.0).min_w(0.0).gap(24.0);
    if let Some(g) = guild {
        let can = crate::guild::can(store, g, Permissions::CHANGE_NICKNAME);
        let fallback = me.base.global_name.clone().unwrap_or_else(|| me.base.username.clone());
        form = form.child(col().w_full().gap(6.0).child(input(st, acc, "p.name", &init.name, t("Community nickname"), &fallback, false, p)).child(muted(
            if can { t("This nickname will only be visible in this community") } else { t("You can't change your nickname here") },
            p,
        )));
    } else {
        form = form
            .child(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(col().grow(1.0).min_w(0.0).gap(4.0).child(label(t("Username"), p)).child(txt(crate::shell::friends::tag(store, me.base.id), 14.0, 400.0, p.text_primary_muted)))
                    .child(small_button(t("Change"), true, "profile-username", open(acc, AccountModal::Username), p)),
            )
            .child(input(st, acc, "p.name", &init.name, t("Display name"), &me.base.username, false, p));
    }
    form = form.child(input(st, acc, "p.pronouns", &init.pronouns, t("Pronouns"), "", false, p));
    if guild.is_none() {
        form = form.child(super::timezone::section(p, acc));
    }

    let asset_row = |title: &str, key: &'static str, asset: super::crop::Asset, has: bool| {
        let picked = st.form.data.get(key);
        let shown = match picked {
            Some(Value::Null) => false,
            Some(_) => true,
            None => has,
        };
        let mut buttons = row().gap(8.0).child(
            Button::<S>::new(if title == "avatar" { t("Change avatar") } else { t("Change banner") })
                .size(ButtonSize::Small)
                .el(p)
                .on_press_tx(super::crop::pick(acc, key, asset)),
        );
        if shown {
            buttons = buttons.child(
                Button::new(if title == "avatar" { t("Remove avatar") } else { t("Remove banner") })
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Small)
                    .on_press(move |s: &mut S| {
                        acc(s).form.data.insert(key.into(), Value::Null);
                    })
                    .el(p),
            );
        }
        col().w_full().gap(8.0).child(label(if title == "avatar" { t("Avatar") } else { t("Banner") }, p)).child(buttons)
    };
    form = form.child(asset_row("avatar", "p.avatar", super::crop::Asset::Avatar, init.avatar.is_some())).child(asset_row("banner", "p.banner", super::crop::Asset::ProfileBanner, init.banner.is_some()));

    // `AccentColorPicker`.
    let swatch = accent.unwrap_or(DEFAULT_ACCENT);
    let accent_edit = st.form.text.get("p.accent").cloned().unwrap_or_else(|| TextEdit::new(accent_text.clone()));
    let init_hex = init.accent.map(hex).unwrap_or_default();
    let accent_field = crate::ui::Input::new("form-p.accent", &accent_edit, move |s: &mut S| -> &mut TextEdit {
        acc(s).form.text.entry("p.accent".into()).or_insert_with(|| TextEdit::new(init_hex.clone()))
    })
    .placeholder(&hex(DEFAULT_ACCENT))
    .el(p);
    form = form.child(
        col()
            .w_full()
            .gap(8.0)
            .child(label(t("Accent color"), p))
            .child(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(el().size(44.0, 44.0).radius(8.0).fill(crate::shell::role_color(swatch)).border(1.0, p.background_modifier_accent))
                    .child(col().w(160.0).child(accent_field))
                    .child(
                        Button::new(t("Reset"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Small)
                            .disabled(accent.is_none())
                            .on_press(move |s: &mut S| {
                                acc(s).form.text.insert("p.accent".into(), TextEdit::new(""));
                            })
                            .el(p),
                    ),
            )
            .child(muted(t("Customizes the border and banner color on your profile"), p)),
    );

    // `BioEditor`: multi-line, with the length against the limit.
    let bio_edit = st.form.text.get("p.bio").cloned().unwrap_or_else(|| TextEdit::new(init.bio.clone()));
    let init_bio = init.bio.clone();
    let over_limit = bio.chars().count() > BIO_MAX;
    let field = text_input(&bio_edit, font(14.0, 400.0).lh(20.0))
        .key("form-p.bio")
        .label(t("About me"))
        .w_full()
        .min_h(96.0)
        .color(p.text_primary)
        .placeholder(t("Doc, I'm from the future. I came here in a time machine that you invented."), p.text_tertiary)
        .caret(p.text_primary, p.background_modifier_selected)
        .on_edit(move |s: &mut S| -> &mut TextEdit { acc(s).form.text.entry("p.bio".into()).or_insert_with(|| TextEdit::new(init_bio.clone())) });
    let border = if over_limit { p.status_danger } else if bio_edit.focused { p.background_modifier_accent_focus } else { p.background_modifier_accent };
    let count = txt(format!("{}/{BIO_MAX}", bio.chars().count()), 12.0, 400.0, if over_limit { mix(p.status_danger, p.text_primary, 78.0) } else { p.text_tertiary });
    form = form.child(
        col()
            .w_full()
            .gap(8.0)
            .child(row().w_full().justify_between().child(label(t("About me"), p)).child(count))
            .child(col().w_full().py(10.0).px(16.0).radius(8.0).fill(p.form_surface_background).border(1.0, over(border, p.form_surface_background)).cursor(rune::Cursor::Text).child(field)),
    );

    // The live preview.
    let me_id = me.base.id;
    let avatar = match st.form.data.get("p.avatar") {
        Some(Value::String(u)) => match picked_px(st, u) {
            Some(px) => el().size(80.0, 80.0).radius(40.0).pixels_fit(&px, rune::SurfaceFit::Fill),
            None => el().size(80.0, 80.0).radius(40.0).fill(p.background_secondary_alt),
        },
        Some(Value::Null) => {
            let px = st.default_avatars.get((me_id.0 % 6) as usize).map(|i| i.pixels().clone());
            crate::ui::Avatar::new(80.0).pixels(px.as_ref()).el(p)
        }
        _ => crate::shell::avatar(store, st, me_id, 80.0, None, p.background_primary, p),
    };
    let banner = match st.form.data.get("p.banner") {
        Some(Value::String(u)) => picked_px(st, u),
        Some(_) => None,
        None => init.banner.as_deref().and_then(|h| crate::shell::profile::banner_px(st, me_id, h)),
    };
    let display = if !name.trim().is_empty() { name.trim().to_string() } else { store.display_name(None, me_id) };
    let card = crate::shell::profile::Card {
        user: me_id,
        name: display,
        tag: crate::shell::friends::tag(store, me_id),
        pronouns: Some(pronouns.trim().to_string()).filter(|s| !s.is_empty()),
        bio: Some(bio.trim().to_string()).filter(|s| !s.is_empty()),
        accent: crate::shell::role_color(swatch),
        banner,
        avatar,
        avatar_size: 80.0,
        custom_status: None,
        status_emoji: None,
        badges: if guild.is_none() { super::badges::shown(st) } else { vec![] },
        guild,
        local_time: if guild.is_none() { super::timezone::preview_offset(store, st) } else { None },
    };
    let preview = col().child(crate::shell::profile::preview(store, st, card, p));

    let title = match guild.and_then(|g| store.guilds.get(&g)?.info.as_ref()) {
        Some(i) => crate::i18n::tf("Profile customization for {selectedGuildName}", &[("selectedGuildName", &i.name.to_string())]),
        None => t("Profile customization").to_string(),
    };
    let content = col()
        .w_full()
        .gap(16.0)
        .child(muted(t("Edit your profile appearance and see a live preview"), p))
        .child(row().w_full().gap(40.0).items_start().child(form).child(preview));
    let mut page = col().w_full().child(selector).child(section(&title, content, true, p));
    if let Some(saved) = &init.badges {
        page = page.child(section("Badges", super::badges::section(st, saved, busy, p, acc), false, p));
    }

    let body = changes(st, &init, guild.is_some());
    if !body.is_empty() || st.form.error.is_some() {
        let path = match guild {
            Some(g) => format!("/guilds/{g}/members/@me"),
            None => "/users/@me".to_string(),
        };
        let reload = guild.map(|g| ("guild_profile".to_string(), format!("/users/{me_id}/profile?guild_id={g}")));
        let save = move |s: &mut S| acc(s).api(Method::PATCH, path.clone(), Some(Value::Object(body.clone())), Then::Saved { clear: "p.".into(), reload: reload.clone() });
        page = page.child(save_bar(st, busy, over_limit, p, acc, save));
    }
    page
}

/// The sticky "unsaved changes" bar with Reset and Save.
fn save_bar<S: 'static>(st: &ShellState, busy: bool, invalid: bool, p: &Palette, acc: ShellAccess<S>, save: impl Fn(&mut S) + 'static) -> El {
    let bg = p.form_surface_background;
    let mut bar = row()
        .w_full()
        .mt(24.0)
        .py(10.0)
        .pl(17.0)
        .pr(11.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .child(text(t("You have unsaved changes. Save or reset before leaving.")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0))
        .child(
            Button::new(t("Reset"))
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Small)
                .on_press(move |s: &mut S| {
                    let f = &mut acc(s).form;
                    f.text.retain(|k, _| !k.starts_with("p."));
                    f.data.retain(|k, _| !k.starts_with("p."));
                    f.error = None;
                })
                .el(p),
        )
        .child(Button::new(t("Save changes")).size(ButtonSize::Small).disabled(invalid).submitting(busy).key("save-changes").on_press(save).el(p));
    if let Some(e) = &st.form.error {
        bar = col().w_full().gap(8.0).child(bar).child(txt(e.clone(), 14.0, 400.0, mix(p.status_danger, p.text_primary, 78.0)));
    }
    bar
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_only_lists_edited_fields() {
        let mut st = ShellState::default();
        let init = Initial { name: "Ada".into(), pronouns: String::new(), bio: "hi".into(), accent: Some(0x112233), avatar: None, banner: None, timezone: None, tz_flags: 1, badges: None };
        assert!(changes(&st, &init, false).is_empty());
        st.form.text.insert("p.name".into(), TextEdit::new("Ada "));
        st.form.text.insert("p.bio".into(), TextEdit::new(""));
        st.form.text.insert("p.accent".into(), TextEdit::new(""));
        st.form.data.insert("p.avatar".into(), Value::Null);
        let c = changes(&st, &init, true);
        assert_eq!(Value::Object(c), serde_json::json!({ "bio": null, "accent_color": null, "avatar": null }), "trailing space isn't a change; empty clears");
    }
}
