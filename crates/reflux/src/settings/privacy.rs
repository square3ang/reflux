//! Privacy (`tabs/privacy_safety_tab/*`), Devices (`DevicesTab`), Authorized apps
//! (`AuthorizedAppsTab`) and Connections (`LinkedAccountsTab`), and their dialogs.

use reflux_model::{FriendSourceFlags, GroupDmAddPermissionFlags, IncomingCallFlags};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, col, el, row};
use crate::ui::mark::marked as text;
use serde_json::{Value, json};

use super::account::{AccountModal, card, card_row, day, muted, open, small_button, txt};
use super::tabs::section;
use super::input;
use crate::guild::Then;
use crate::i18n::t;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Checkbox, Modal, RadioGroup, Select, Switch, mix};

fn patch<S: 'static>(acc: ShellAccess<S>, v: Value) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| acc(s).intents.push(Intent::PatchSettings(v.clone()))
}

/// Opens a URL in the default browser.
pub(crate) fn open_url(url: &str) {
    let r = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = r {
        eprintln!("reflux: opening {url}: {e}");
    }
}

// ---- Communication flags -----------------------------------------------------------------------

/// `resolveBaseValue`: which of nobody / friends only / everyone / custom the flags are.
pub fn base_value(flags: u64, nobody: u64, friends: u64, everyone: u64) -> &'static str {
    if flags & nobody == nobody {
        "nobody"
    } else if flags & everyone == everyone {
        "everyone"
    } else if flags & friends == friends {
        "friends_only"
    } else {
        "custom"
    }
}

/// `nextFlagsForBaseChange` (`fof | members` are the custom bits).
pub fn base_change(value: &str, current: u64, [nobody, friends, everyone, fof, members]: [u64; 5]) -> u64 {
    match value {
        "nobody" => nobody,
        "friends_only" => friends,
        "everyone" => everyone,
        _ => match current & (fof | members) {
            0 => fof,
            c => c,
        },
    }
}

/// `nextFlagsForAdditiveToggle`.
pub fn toggle_custom(current: u64, flag: u64, on: bool, [nobody, friends, everyone, _, _]: [u64; 5]) -> u64 {
    let mut f = current & !(nobody | friends | everyone);
    if on { f |= flag } else { f &= !flag }
    if f == 0 { friends } else { f }
}

/// The "Incoming calls" / "Group chats" picker with its custom checkboxes.
#[allow(clippy::too_many_arguments)]
fn permission<S: 'static>(st: &ShellState, id: &'static str, field: &'static str, current: u64, bits: [u64; 5], labels: [&'static str; 4], p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let value = base_value(current, bits[0], bits[1], bits[2]);
    let opts = [("nobody", labels[0]), ("friends_only", labels[1]), ("custom", labels[2]), ("everyone", labels[3])];
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    let select = Select::new(id, &opts, Some(value))
        .width(420.0)
        .on_choose(move |s: &mut S, v| acc(s).intents.push(Intent::PatchSettings(json!({ field: base_change(v, current, bits) }))))
        .el(&st.popups, pops, p, layer);
    let mut c = col().w_full().gap(12.0).child(select);
    if value == "custom" {
        for (flag, label) in [(bits[3], t("Friends of friends")), (bits[4], t("Community members"))] {
            let on = current & flag != 0;
            c = c.child(
                Checkbox::new(on)
                    .label(label)
                    .on_toggle(move |s: &mut S| acc(s).intents.push(Intent::PatchSettings(json!({ field: toggle_custom(current, flag, !on, bits) }))))
                    .el(p),
            );
        }
    }
    c
}

fn sub(title: &str, desc: &str, content: El, p: &Palette) -> El {
    col().w_full().gap(8.0).child(text(title.to_string()).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary)).child(muted(desc, p)).child(content)
}

// ---- Privacy -----------------------------------------------------------------------------------

pub(super) fn tab<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let s = &store.settings;
    // Friend requests (`ConnectionsTab`).
    let fs = s.friend_source_flags.bits();
    let everyone = fs & FriendSourceFlags::NO_RELATION.bits() != 0;
    let all = (FriendSourceFlags::NO_RELATION | FriendSourceFlags::MUTUAL_FRIENDS | FriendSourceFlags::MUTUAL_GUILDS).bits();
    let mut friends = col().w_full().gap(12.0).child(
        Switch::new(everyone)
            .label(t("Everyone"))
            .description(t("Anyone who finds you can send a request. Turn this off to pick groups below."))
            .on_toggle(patch(acc, json!({ "friend_source_flags": if everyone { all & !FriendSourceFlags::NO_RELATION.bits() } else { all } })))
            .el(p),
    );
    if !everyone {
        for (flag, label, desc) in [
            (FriendSourceFlags::MUTUAL_FRIENDS.bits(), t("Friends of friends"), t("People who share at least one friend with you.")),
            (FriendSourceFlags::MUTUAL_GUILDS.bits(), t("Community members"), t("People who share a community with you.")),
        ] {
            let on = fs & flag != 0;
            friends = friends.child(Switch::new(on).label(label).description(desc).on_toggle(patch(acc, json!({ "friend_source_flags": fs ^ flag }))).el(p));
        }
    }
    let dms = col()
        .w_full()
        .gap(12.0)
        .child(
            Switch::new(!s.default_guilds_restricted)
                .label(t("Allow direct messages from community members"))
                .description(t("Applies to communities you join from now on. You can also change it for each community."))
                .on_toggle(patch(acc, json!({ "default_guilds_restricted": !s.default_guilds_restricted })))
                .el(p),
        )
        .child(
            Switch::new(!s.bot_default_guilds_restricted)
                .label(t("Allow direct messages from community bots"))
                .on_toggle(patch(acc, json!({ "bot_default_guilds_restricted": !s.bot_default_guilds_restricted })))
                .el(p),
        );
    let ic = IncomingCallFlags::all();
    let call_bits = [IncomingCallFlags::NOBODY.bits(), IncomingCallFlags::FRIENDS_ONLY.bits(), IncomingCallFlags::EVERYONE.bits(), IncomingCallFlags::FRIENDS_OF_FRIENDS.bits(), IncomingCallFlags::GUILD_MEMBERS.bits()];
    let calls_now = s.incoming_call_flags.bits() & ic.bits();
    let silent = calls_now & IncomingCallFlags::SILENT_EVERYONE.bits() != 0;
    let calls = permission(
        st,
        "privacy-calls",
        "incoming_call_flags",
        calls_now,
        call_bits,
        [t("Block all incoming calls"), t("Only friends can call you (recommended)"), t("Friends and selected groups can call you"), t("Anyone can call you, including strangers")],
        p,
        acc,
        layer,
    )
    .child(
        Switch::new(silent)
            .label(t("Receive calls silently"))
            .description(t("Calls you allow still come through, but they don't ring."))
            .on_toggle(patch(acc, json!({ "incoming_call_flags": calls_now ^ IncomingCallFlags::SILENT_EVERYONE.bits() })))
            .el(p),
    );
    let gd_bits = [
        GroupDmAddPermissionFlags::NOBODY.bits(),
        GroupDmAddPermissionFlags::FRIENDS_ONLY.bits(),
        GroupDmAddPermissionFlags::EVERYONE.bits(),
        GroupDmAddPermissionFlags::FRIENDS_OF_FRIENDS.bits(),
        GroupDmAddPermissionFlags::GUILD_MEMBERS.bits(),
    ];
    let groups = permission(
        st,
        "privacy-groups",
        "group_dm_add_permission_flags",
        s.group_dm_add_permission_flags.bits(),
        gd_bits,
        [t("No one can add you to group chats without asking"), t("Only friends can add you without asking (recommended)"), t("Friends and selected groups can add you without asking"), t("Anyone can add you to group chats")],
        p,
        acc,
        layer,
    );
    let communication = col()
        .w_full()
        .gap(24.0)
        .child(sub(t("Friend requests"), t("Choose who can send you a friend request."), friends, p))
        .child(sub(t("Direct messages"), t("Review who can message you"), dms, p))
        .child(sub(t("Incoming calls"), t("Choose who can start a call with you."), calls, p))
        .child(sub(t("Group chats"), t("Choose who can add you to a group chat without asking first."), groups, p));

    let level = s.profile_privacy.to_string();
    let profile_opts = [
        ("0", t("Friends and all communities"), Some(t("Your full profile is visible to friends and to anyone in your communities"))),
        ("1", t("Friends and small communities only"), Some(t("Your full profile is visible to friends and members of your communities with 200 or fewer members"))),
        ("2", t("Friends only"), Some(t("Your full profile is only visible to your friends"))),
    ];
    let profile = RadioGroup::new(&profile_opts, Some(&level))
        .on_change(move |s: &mut S, v| acc(s).intents.push(Intent::PatchSettings(json!({ "profile_privacy": v.parse::<u8>().unwrap_or(0) }))))
        .el(p);

    let filter = |id: &'static str, field: &'static str, label: &str, desc: &str, v: u8, layer: &mut Vec<El>| {
        let opts = [("0", t("Show")), ("1", t("Blur")), ("2", t("Block"))];
        let cur = v.to_string();
        let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
        row()
            .w_full()
            .gap(16.0)
            .center_y()
            .child(col().grow(1.0).min_w(0.0).gap(2.0).child(txt(label, 14.0, 500.0, p.text_primary)).child(muted(desc, p)))
            .child(
                Select::new(id, &opts, Some(&cur))
                    .width(140.0)
                    .on_choose(move |s: &mut S, v| acc(s).intents.push(Intent::PatchSettings(json!({ field: v.parse::<u8>().unwrap_or(0) }))))
                    .el(&st.popups, pops, p, layer),
            )
    };
    let sensitive = col()
        .w_full()
        .gap(16.0)
        .child(filter("sens-friend", "sensitive_content_friend_dm_filter", t("Direct messages from friends"), t("Media your friends send you in direct messages."), s.sensitive_content_friend_dm_filter, layer))
        .child(filter("sens-other", "sensitive_content_non_friend_dm_filter", t("Direct messages from others"), t("Media in group chats and in direct messages from people who aren't your friends."), s.sensitive_content_non_friend_dm_filter, layer))
        .child(filter("sens-guild", "sensitive_content_guild_filter", t("Messages in community channels"), t("Media in channels that are not marked 18+. Channels marked 18+ always show it."), s.sensitive_content_guild_filter, layer));

    // Data export (`DataExportTab`).
    let latest = st.form.data.get("harvest").filter(|v| v.is_object());
    let mut export = col().w_full().gap(12.0);
    if let Some(h) = latest {
        let at = day(h["created_at"].as_str().unwrap_or_default());
        let status = h["status"].as_str().unwrap_or_default();
        let msg = match status {
            "completed" => crate::i18n::tf("Your export from {requestedAt} is ready.", &[("requestedAt", &at.to_string())]),
            "failed" => crate::i18n::tf("Your export from {requestedAt} failed. You can request a new one.", &[("requestedAt", &at.to_string())]),
            _ => crate::i18n::tf("Your export from {requestedAt} is being prepared ({progress}%).", &[("requestedAt", &at.to_string()), ("progress", &(h["progress_percent"].as_f64().unwrap_or(0.0) as u32).to_string())]),
        };
        export = export.child(muted(msg, p));
        if status == "completed"
            && let Some(id) = h["harvest_id"].as_str()
        {
            let path = format!("/users/@me/harvest/{id}/download");
            export = export.child(small_button(t("Download export"), true, "harvest-download", move |s: &mut S| acc(s).api(Method::GET, path.clone(), None, Then::Keep("harvest_url".into())), p));
        }
    }
    if let Some(url) = st.form.data.get("harvest_url").and_then(|v| v["url"].as_str()) {
        open_url_once(url);
    }
    let busy = latest.is_some_and(|h| matches!(h["status"].as_str(), Some("pending" | "processing")));
    export = export.child(
        Button::new(t("Export my data"))
            .size(ButtonSize::Small)
            .disabled(busy)
            .key("harvest-request")
            .on_press(move |s: &mut S| acc(s).api(Method::POST, "/users/@me/harvest", Some(json!({})), Then::Reload("harvest".into(), "/users/@me/harvest/latest".into())))
            .el(p),
    );
    let deletion = small_button(t("Delete my messages"), true, "delete-my-messages", open(acc, AccountModal::DeleteMessages), p);

    col()
        .w_full()
        .child(section(t("Communication"), communication, true, p))
        .child(section(t("Who can see your full profile"), profile, false, p))
        .child(section(t("Sensitive content"), sensitive, false, p))
        .child(section(t("Data export"), export, false, p))
        .child(section(t("Data deletion"), deletion, false, p))
}

/// Opens each harvest download URL once (the view runs every frame).
fn open_url_once(url: &str) {
    static OPENED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let mut last = OPENED.lock().unwrap_or_else(|e| e.into_inner());
    if last.as_deref() != Some(url) {
        *last = Some(url.to_string());
        open_url(url);
    }
}

// ---- Devices -----------------------------------------------------------------------------------

/// `DevicesTab`: the current session, then the others, each with Sign out.
pub(super) fn devices<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let sessions = match super::loaded::<Vec<Value>>(st, "sessions", p) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let describe = |s: &Value| {
        let c = &s["client_info"];
        let name = [c["os"].as_str(), c["browser"].as_str().or(c["platform"].as_str())].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        let loc = &c["location"];
        let place = [loc["city"].as_str(), loc["region"].as_str(), loc["country"].as_str()].into_iter().flatten().collect::<Vec<_>>().join(", ");
        let mut bits = vec![];
        if !place.is_empty() {
            bits.push(place);
        }
        if let Some(ip) = s["masked_ip"].as_str() {
            bits.push(ip.to_string());
        }
        if let Some(at) = s["approx_last_used_at"].as_str() {
            bits.push(format!("{} {}", t("Last used"), day(at)));
        }
        (if name.is_empty() { t("Unknown device").to_string() } else { name }, bits.join(" · "), c["device"].as_str() == Some("mobile"))
    };
    let device_row = |s: &Value, current: bool| {
        let (name, detail, mobile) = describe(s);
        let id = s["id_hash"].as_str().unwrap_or_default().to_string();
        let glyph = rune::icon(if mobile { &crate::icons::DEVICE_MOBILE } else { &crate::icons::DESKTOP }, 24.0).color(p.text_secondary);
        let mut r = row()
            .w_full()
            .px(16.0)
            .py(12.0)
            .gap(12.0)
            .center_y()
            .child(glyph)
            .child(col().grow(1.0).min_w(0.0).gap(2.0).child(txt(name, 14.0, 500.0, p.text_primary)).child(muted(detail, p)));
        if !current {
            r = r.child(small_button(t("Sign out"), true, &format!("device-out-{id}"), open(acc, AccountModal::SignOutDevices(vec![id])), p));
        }
        r
    };
    let (current, others): (Vec<&Value>, Vec<&Value>) = sessions.iter().partition(|s| s["current"].as_bool() == Some(true));
    let mut c = col().w_full().gap(24.0);
    if let Some(cur) = current.first() {
        c = c.child(col().w_full().gap(8.0).child(txt(t("Current device"), 12.0, 600.0, p.text_tertiary)).child(card(None, vec![device_row(cur, true)], p)));
    }
    if !others.is_empty() {
        let ids: Vec<String> = others.iter().filter_map(|s| s["id_hash"].as_str().map(str::to_string)).collect();
        c = c
            .child(col().w_full().gap(8.0).child(txt(t("Other devices"), 12.0, 600.0, p.text_tertiary)).child(card(None, others.iter().map(|s| device_row(s, false)).collect(), p)))
            .child(
                Button::new(t("Sign out all other devices"))
                    .variant(ButtonVariant::Danger)
                    .size(ButtonSize::Small)
                    .key("devices-out-all")
                    .on_press(open(acc, AccountModal::SignOutDevices(ids)))
                    .el(p),
            );
    }
    section(t("Devices"), c, true, p)
}

// ---- Authorized apps ---------------------------------------------------------------------------

pub(super) fn apps<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let list = match super::loaded::<Vec<Value>>(st, "apps", p) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if list.is_empty() {
        return section(t("Authorized apps"), muted(t("You haven't authorized any apps."), p), true, p);
    }
    let rows = list
        .iter()
        .map(|a| {
            let app = &a["application"];
            let id = app["id"].as_str().unwrap_or_default().to_string();
            let scopes = a["scopes"].as_array().map(|s| s.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            let desc = format!("{} · {} {}", scopes, t("Authorized"), day(a["authorized_at"].as_str().unwrap_or_default()));
            card_row(app["name"].as_str().unwrap_or_default(), Some(muted(desc, p)), vec![small_button(t("Deauthorize"), true, &format!("deauth-{id}"), open(acc, AccountModal::Deauthorize(id.clone())), p)], p)
        })
        .collect();
    section(t("Authorized apps"), card(None, rows, p), true, p)
}

// ---- Connections -------------------------------------------------------------------------------

/// `ConnectionVisibilityFlags`.
const VIS_EVERYONE: u64 = 1;
const VIS_FRIENDS: u64 = 2;
const VIS_GUILDS: u64 = 4;

pub(super) fn connections<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let list = match super::loaded::<Vec<Value>>(st, "connections", p) {
        Ok(v) => v,
        Err(e) => return e,
    };
    // `handleAddConnection(type)`: the dialog opens on that connection type.
    let add_kind = move |kind: &'static str| {
        move |s: &mut S| {
            let sh = acc(s);
            sh.open_account_modal(AccountModal::AddConnection);
            sh.form.data.insert("m.ctype".into(), json!(kind));
        }
    };
    let mut add = row().gap(8.0);
    if st.bluesky {
        add = add.child(small_button(&crate::i18n::tf("Add {blueskyProviderName} connection", &[("blueskyProviderName", "Bluesky")]), false, "conn-add-bsky", add_kind("bsky"), p));
    }
    let add = add.child(small_button(t("Add domain connection"), !st.bluesky, "conn-add", add_kind("domain"), p));
    let mut rows = Vec::new();
    if list.is_empty() {
        let hint = if st.bluesky { crate::i18n::tf("Link your {blueskyProviderName} account or verify domain ownership to display them on your profile.", &[("blueskyProviderName", "Bluesky")]) } else { t("Verify domain ownership to display it on your profile.").to_string() };
        rows.push(row().w_full().px(16.0).py(12.0).child(muted(hint, p)));
    }
    for c in &list {
        let (kind, id) = (c["type"].as_str().unwrap_or_default().to_string(), c["id"].as_str().unwrap_or_default().to_string());
        let vis = c["visibility_flags"].as_u64().unwrap_or(VIS_EVERYONE);
        let path = format!("/users/@me/connections/{kind}/{id}");
        let label = format!("{} ({})", c["name"].as_str().unwrap_or_default(), if kind == "bsky" { t("Bluesky") } else { t("Domain") });
        let toggles = row().gap(12.0).children([(VIS_EVERYONE, t("Everyone")), (VIS_FRIENDS, t("Friends")), (VIS_GUILDS, t("Community members"))].map(|(bit, l)| {
            let on = vis & bit != 0;
            let path = path.clone();
            Checkbox::new(on)
                .label(l)
                .on_toggle(move |s: &mut S| acc(s).api(Method::PATCH, path.clone(), Some(json!({ "visibility_flags": vis ^ bit })), Then::Reload("connections".into(), "/users/@me/connections".into())))
                .el(p)
        }));
        let verified = if c["verified"].as_bool() == Some(true) { t("Verified") } else { t("Not verified") };
        rows.push(card_row(
            &label,
            Some(col().gap(8.0).child(muted(verified, p)).child(toggles)),
            vec![Button::new(t("Remove")).variant(ButtonVariant::Ghost).size(ButtonSize::Small).on_press(open(acc, AccountModal::RemoveConnection { kind, id })).el(p)],
            p,
        ));
    }
    section(t("Connections"), card(Some((t("Connections"), Some(t("Show accounts and domains you own on your profile.")), Some(add))), rows, p), true, p)
}

// ---- Dialogs -----------------------------------------------------------------------------------

pub(super) fn modal<S: 'static>(_store: &Store, st: &ShellState, m: &AccountModal, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let close = move |s: &mut S| acc(s).close_account_modals();
    let desc = |s: &str| txt(s, 14.0, 400.0, p.text_primary_muted);
    let build = |title: &str, body: Vec<El>, primary: super::account::Primary<'_, S>| {
        let mut b = col().w_full().gap(16.0).children(body);
        if let Some(e) = &st.form.error {
            b = b.child(txt(e.clone(), 14.0, 400.0, mix(p.status_danger, p.text_primary, 78.0)));
        }
        let mut f = row().gap(8.0).child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p));
        if let Some((l, danger, go)) = primary {
            f = f.child(Button::new(l.to_string()).variant(if danger { ButtonVariant::Danger } else { ButtonVariant::Primary }).submitting(st.form.busy).key("modal-primary").on_press(move |s: &mut S| go(s)).el(p));
        }
        Modal::new(title.to_string(), close).body(b).footer(f).el(p)
    };
    Some(match m {
        AccountModal::SignOutDevices(ids) => {
            let ids = ids.clone();
            let n = ids.len();
            let go: Box<dyn Fn(&mut S)> = Box::new(move |s: &mut S| {
                acc(s).api(Method::POST, "/auth/sessions/logout", Some(json!({ "session_id_hashes": ids })), Then::Saved { clear: "m.".into(), reload: Some(("sessions".into(), "/auth/sessions".into())) })
            });
            let label = crate::i18n::tf("Sign out {deviceCount, plural, one {# device} other {# devices}}", &[("deviceCount", &n.to_string())]);
            build(t("Sign out devices"), vec![desc(t("Those devices will need to sign in again."))], Some((&label, true, go)))
        }
        AccountModal::DeleteMessages => {
            let go: Box<dyn Fn(&mut S)> = Box::new(move |s: &mut S| acc(s).api(Method::POST, "/users/@me/messages/delete", Some(json!({})), Then::Close));
            build(t("Delete my messages"), vec![desc(t("Every message you've sent will be deleted one day from now. You can cancel before then. Deleted messages can't be restored."))], Some((t("Delete my messages"), true, go)))
        }
        AccountModal::Deauthorize(id) => {
            let path = format!("/oauth2/@me/authorizations/{id}");
            let go: Box<dyn Fn(&mut S)> = Box::new(move |s: &mut S| acc(s).api(Method::DELETE, path.clone(), None, Then::Saved { clear: "m.".into(), reload: Some(("apps".into(), "/oauth2/@me/authorizations".into())) }));
            build(t("Deauthorize app"), vec![desc(t("The app loses access to your account. You can authorize it again later."))], Some((t("Deauthorize"), true, go)))
        }
        AccountModal::RemoveConnection { kind, id } => {
            let path = format!("/users/@me/connections/{kind}/{id}");
            let go: Box<dyn Fn(&mut S)> = Box::new(move |s: &mut S| acc(s).api(Method::DELETE, path.clone(), None, Then::Saved { clear: "m.".into(), reload: Some(("connections".into(), "/users/@me/connections".into())) }));
            build(t("Remove connection"), vec![desc(t("It will no longer show on your profile."))], Some((t("Remove"), true, go)))
        }
        AccountModal::AddConnection => match st.form.data.get("m.verify") {
            None => {
                let bsky = st.bluesky && st.form.data.get("m.ctype").and_then(Value::as_str) != Some("domain");
                let go: Box<dyn Fn(&mut S)> = Box::new(move |s: &mut S| {
                    let sh = acc(s);
                    let id = sh.form.text.get("m.domain").map(|t| t.text.trim().to_string()).unwrap_or_default();
                    if bsky {
                        // `authorizeBlueskyConnection`: the browser does the OAuth; the dialog
                        // closes when USER_CONNECTIONS_UPDATE arrives.
                        let handle = id.trim_start_matches("https://bsky.app/profile/").trim_start_matches("http://bsky.app/profile/").trim_start_matches('@').to_string();
                        sh.form.data.insert("m.bsky".into(), json!(handle.to_lowercase()));
                        sh.api(Method::POST, "/users/@me/connections/bluesky/authorize", Some(json!({ "handle": handle })), Then::Browse("authorize_url"));
                    } else {
                        sh.api(Method::POST, "/users/@me/connections", Some(json!({ "type": "domain", "identifier": id })), Then::Keep("m.verify".into()));
                    }
                });
                let mut body = Vec::new();
                let mut layer = Vec::new();
                if st.bluesky {
                    let opts = [("bsky", "Bluesky"), ("domain", t("Domain"))];
                    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
                    body.push(
                        Select::new("conn-type", &opts, Some(if bsky { "bsky" } else { "domain" }))
                            .label(t("Connection type"))
                            .on_choose(move |s: &mut S, v| {
                                acc(s).form.data.insert("m.ctype".into(), json!(v));
                            })
                            .el(&st.popups, pops, p, &mut layer),
                    );
                }
                let (label, ph) = if bsky { (t("Handle"), "username.bsky.social") } else { (t("Domain"), "example.com") };
                body.push(input(st, acc, "m.domain", "", label, ph, false, p));
                if bsky && st.form.data.contains_key("m.bsky") && st.form.error.is_none() {
                    body.push(desc(t("Finish in your browser. This closes once the connection is added.")));
                }
                let primary = if bsky { crate::i18n::tf("Connect with {providerName}", &[("providerName", "Bluesky")]) } else { t("Continue").to_string() };
                let dialog = build(t("Add connection"), body, Some((&primary, false, go)));
                if layer.is_empty() { dialog } else { el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).child(dialog).children(layer) }
            }
            Some(v) => {
                let token = v["initiation_token"].as_str().unwrap_or_default().to_string();
                let go: Box<dyn Fn(&mut S)> = Box::new(move |s: &mut S| {
                    acc(s).api(Method::POST, "/users/@me/connections/verify", Some(json!({ "initiation_token": token, "visibility_flags": VIS_EVERYONE })), Then::Saved { clear: "m.".into(), reload: Some(("connections".into(), "/users/@me/connections".into())) })
                });
                let proof = text(v["token"].as_str().unwrap_or_default().to_string()).font(font(13.0, 500.0).lh(18.0).mono()).color(p.text_primary).wrap().selectable(true);
                build(t("Add domain"), vec![desc(v["instructions"].as_str().unwrap_or_default()), row().w_full().padding(12.0).radius(6.0).fill(p.background_tertiary).child(proof)], Some((t("Verify"), false, go)))
            }
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn communication_flags_follow_fluxer() {
        let bits = [16, 8, 4, 1, 2];
        assert_eq!(base_value(16, 16, 8, 4), "nobody");
        assert_eq!(base_value(8, 16, 8, 4), "friends_only");
        assert_eq!(base_value(3, 16, 8, 4), "custom");
        assert_eq!(base_change("custom", 8, bits), 1, "custom starts with friends of friends");
        assert_eq!(base_change("custom", 2 | 8, bits), 2);
        assert_eq!(base_change("everyone", 3, bits), 4);
        assert_eq!(toggle_custom(1, 2, true, bits), 3);
        assert_eq!(toggle_custom(1, 1, false, bits), 8, "nothing left falls back to friends only");
        assert_eq!(toggle_custom(8 | 1 | 32, 2, true, bits), 1 | 2 | 32, "base bits drop, others stay");
    }
}
