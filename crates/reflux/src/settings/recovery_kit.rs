//! The recovery kit (`account_security_tab/RecoveryKitSettings.tsx`, `RecoveryKitModal.tsx`) on
//! instances where people sign in with a username: the reminder, the "Recovery kit" row, the
//! replace confirmation and the kit itself (key, Download PDF via [`super::recovery_pdf`], Print,
//! Copy, include backup codes, "stored it" check). Status is `form.data["recovery_kit"]`
//! (`GET /users/@me/recovery-kit`); a new kit is `form.data["m.kit"]`.
// shortcut: Print opens the PDF in the system viewer (Rune can't print).

use reflux_store::Store;
use reqwest::Method;
use rune::{El, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use super::account::{AccountModal, card_row, muted, small_button};
use crate::guild::Then;
use crate::i18n::{t, tf};
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Checkbox, Modal, mix};

const STATUS: &str = "recovery_kit";
const PATH: &str = "/users/@me/recovery-kit";

/// `showsRecoveryKit`: username sign-in, claimed account.
pub(super) fn shown(store: &Store, st: &ShellState) -> bool {
    st.username_sign_in && crate::shell::devtools::me(store, st).is_some_and(|m| m.account_tier.as_deref() != Some("unclaimed"))
}

pub(super) fn load(sh: &mut ShellState) {
    if sh.username_sign_in {
        sh.load(STATUS, PATH);
    }
}

/// Creates a kit and shows it (`createAndShowRecoveryKit`).
fn create<S: 'static>(acc: ShellAccess<S>) -> impl Fn(&mut S) + Clone + 'static {
    move |s: &mut S| {
        let sh = acc(s);
        sh.open_account_modal(AccountModal::RecoveryKit);
        sh.api(Method::POST, PATH, Some(json!({})), Then::Keep("m.kit".into()));
    }
}

fn has_kit(st: &ShellState) -> Option<bool> {
    st.form.data.get(STATUS)?["has_recovery_kit"].as_bool()
}

/// `RecoveryKitReminderAlert` (a `WarningAlert`).
pub(super) fn reminder<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if has_kit(st) != Some(false) || st.prefs.recovery_reminder_dismissed.contains(&st.account.me) {
        return None;
    }
    let c = p.alert_warning_color;
    let actions = row()
        .gap(8.0)
        .mt(6.0)
        .child(Button::new(t("Create recovery kit")).size(ButtonSize::Small).key("rk-remind-create").on_press(create(acc)).el(p))
        .child(Button::new(t("Not now")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| {
            let sh = acc(s);
            let me = sh.account.me;
            crate::shell::expr::image_search::save_prefs(sh, |p| {
                p.recovery_reminder_dismissed.insert(me);
            });
        }).el(p));
    Some(
        row()
            .w_full()
            .gap(8.0)
            .items_start()
            .py(6.0)
            .pl(16.0)
            .pr(12.0)
            .mb(16.0)
            .role("alert")
            .child(el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w(4.0).radius(4.0).fill(c))
            .child(icon(&icons::WARNING_FILL, 16.0).color(c).mt(2.0))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text(t("Save a recovery kit")).font(font(13.0, 600.0).lh(18.2)).color(p.text_primary))
                    .child(text(t("If you forget your password, a recovery kit is the only way back into your account.")).font(font(13.0, 400.0).lh(18.2)).color(p.text_primary).wrap())
                    .child(actions),
            ),
    )
}

/// `RecoveryKitRow`.
pub(super) fn row_el<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let status = st.form.data.get(STATUS);
    let missing = has_kit(st) == Some(false);
    let desc = if missing {
        row().gap(4.0).center_y().child(icon(&icons::WARNING_FILL, 14.0).color(p.alert_warning_color)).child(muted(t("You don't have a recovery kit yet"), p))
    } else {
        match status.and_then(|v| v["created_at"].as_str()) {
            Some(at) => muted(tf("Created: {0}", &[("0", &super::account::day(at))]), p),
            None => muted(t("Lets you reset your password if you forget it."), p),
        }
    };
    let button = if missing {
        small_button(t("Create recovery kit"), false, "acct-recovery-kit", create(acc), p)
    } else {
        small_button(t("Create a new kit"), true, "acct-recovery-kit", move |s: &mut S| acc(s).open_account_modal(AccountModal::RecoveryKitReplace), p)
    };
    card_row(t("Recovery kit"), Some(desc), vec![button], p)
}

/// The dialogs this file owns.
pub(super) fn modal<S: 'static>(store: &Store, st: &ShellState, m: &AccountModal, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let close = move |s: &mut S| acc(s).close_account_modals();
    match m {
        AccountModal::RecoveryKitReplace => {
            let body = col().w_full().child(text(t("Your current kit stops working as soon as the new one is created.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap());
            let footer = row().gap(8.0).child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p)).child(Button::new(t("Create a new kit")).key("rk-replace").on_press(create(acc)).el(p));
            Some(Modal::new(t("Create a new recovery kit?"), close).body(body).footer(footer).el(p))
        }
        AccountModal::RecoveryKit => Some(kit(store, st, p, acc)),
        _ => None,
    }
}

/// `RecoveryKitModal`.
fn kit<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).close_account_modals();
    let Some(k) = st.form.data.get("m.kit").cloned() else {
        let msg = match &st.form.error {
            Some(e) => text(format!("{}: {e}", t("Could not create a recovery kit"))).font(font(14.0, 400.0).lh(20.0)).color(mix(p.status_danger, p.text_primary, 78.0)).wrap(),
            None => text(t("Loading…")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted),
        };
        return Modal::new(t("Your recovery kit"), close).body(col().w_full().child(msg)).footer(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p)).el(p);
    };
    let key = k["recovery_key"].as_str().unwrap_or_default().to_string();
    let created = k["created_at"].as_str().unwrap_or_default().to_string();
    let replacing = has_kit(st) == Some(true);
    let me = store.me.as_ref();
    let tag = me.map(|m| if m.base.discriminator.is_empty() || m.base.discriminator == "0000" { m.base.username.clone() } else { format!("{}#{}", m.base.username, m.base.discriminator) }).unwrap_or_default();
    let mfa = me.is_some_and(|m| m.mfa_enabled);
    let ack = st.form.data.get("m.rk_ack").and_then(Value::as_bool).unwrap_or(false);
    let include = st.form.data.get("m.rk_codes").and_then(Value::as_bool).unwrap_or(false);
    let codes: Option<Vec<String>> = st.form.data.get("m.codes").and_then(|v| v["backup_codes"].as_array()).map(|a| a.iter().filter(|c| !c["consumed"].as_bool().unwrap_or(false)).filter_map(|c| c["code"].as_str().map(str::to_string)).collect());

    // `.keyGrid`: four columns.
    let groups: Vec<&str> = key.split('-').collect();
    let grid = col().w_full().gap(8.0).padding(17.0).radius(8.0).fill(p.background_secondary).border(1.0, p.background_modifier_accent).children(groups.chunks(4).map(|line| {
        row().w_full().gap(12.0).children((0..4).map(|i| {
            let g = line.get(i).copied().unwrap_or_default();
            text(g.to_string()).font(font(20.0, 600.0).lh(28.0).mono().tracking(1.6)).color(p.text_primary).align(rune::Align::Center).w(0.0).grow(1.0).selectable(true)
        }))
    }));
    let sheet = sheet(st, &key, &created, &tag, include.then_some(codes.clone()).flatten());
    let pdf_name = format!("fluxer-recovery-kit-{}.pdf", slug(&tag));
    let download = {
        let (sheet, name) = (sheet.clone(), pdf_name.clone());
        move |_: &mut S, tx: &mut Tx| {
            let bytes = super::recovery_pdf::build(&sheet);
            let options = rune::FileDialogOptions { title: t("Download PDF").into(), file_name: Some(name.clone()), ..Default::default() }.filter("PDF", &["pdf"]);
            tx.save_file(options, move |s: &mut S, r, _| {
                if let Ok(Some(path)) = r
                    && std::fs::write(&path, &bytes).is_err()
                {
                    crate::desktop::toast(&mut acc(s).desktop, t("Couldn't create the PDF. Try again or print the kit instead."), false);
                }
            });
        }
    };
    let print = move |s: &mut S, tx: &mut Tx| {
        let path = std::env::temp_dir().join(&pdf_name);
        match std::fs::write(&path, super::recovery_pdf::build(&sheet)) {
            Ok(()) => {
                let _ = tx.open_url(&format!("file://{}", path.display()));
            }
            Err(_) => crate::desktop::toast(&mut acc(s).desktop, t("Couldn't create the PDF. Try again or print the kit instead."), false),
        }
    };
    let copy_key = key.clone();
    let buttons = col()
        .w_full()
        .gap(8.0)
        .child(Button::<S>::new(t("Download PDF")).size(ButtonSize::Small).icon(&icons::DOWNLOAD_SIMPLE).key("rk-download").el(p).on_press_tx(download))
        .child(Button::<S>::new(t("Print")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).key("rk-print").el(p).on_press_tx(print))
        .child(Button::<S>::new(t("Copy")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).key("rk-copy").el(p).on_press_tx(move |_: &mut S, tx: &mut Tx| tx.copy(copy_key.clone())));
    let mut checks = col().w_full().gap(12.0);
    if mfa {
        checks = checks.child(Checkbox::new(include).label(t("Include my two-factor backup codes in the PDF and printout")).on_toggle(move |s: &mut S| {
            let sh = acc(s);
            sh.form.data.insert("m.rk_codes".into(), json!(!include));
            if !include && !sh.form.data.contains_key("m.codes") {
                sh.api(Method::POST, "/users/@me/mfa/backup-codes", Some(json!({ "regenerate": false })), Then::Keep("m.codes".into()));
            }
        }).el(p));
    }
    checks = checks.child(Checkbox::new(ack).label(t("I've stored my recovery kit somewhere safe")).on_toggle(move |s: &mut S| {
        acc(s).form.data.insert("m.rk_ack".into(), json!(!ack));
    }).el(p));
    let intro = if replacing { t("Your old recovery kit no longer works. Store this new one somewhere safe.") } else { t("If you forget your password, this kit is the only way back into your account. Store it somewhere safe, like a password manager or a printed copy.") };
    let body = col()
        .w_full()
        .gap(16.0)
        .child(text(intro).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap())
        .child(grid.role("group").label(t("Recovery key")))
        .child(text(t("Anyone with this key can reset your password. Never share it.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_tertiary).wrap())
        .child(buttons)
        .child(checks);
    let created_at = created.clone();
    let done = move |s: &mut S| {
        let sh = acc(s);
        sh.form.data.insert(STATUS.into(), json!({ "has_recovery_kit": true, "created_at": created_at }));
        sh.close_account_modals();
    };
    let footer = Button::new(t("Done")).disabled(!ack).key("rk-done").on_press(done).el(p);
    // `hideCloseButton`: the kit stays up until it's stored.
    Modal::new(t("Your recovery kit"), |_: &mut S| {}).hide_close().body(body).footer(footer).el(p)
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// The PDF's text; a translation the standard fonts can't draw falls back to English.
fn sheet(st: &ShellState, key: &str, created: &str, tag: &str, codes: Option<Vec<String>>) -> super::recovery_pdf::Sheet {
    let pick = |en: &str| {
        let tr = t(en);
        if super::recovery_pdf::can_encode(tr) { tr.to_string() } else { en.to_string() }
    };
    let pickf = |en: &str, args: &[(&str, &str)]| {
        let tr = tf(en, args);
        if super::recovery_pdf::can_encode(&tr) { tr } else { args.iter().fold(en.to_string(), |s, (k, v)| s.replace(&format!("{{{k}}}"), v)) }
    };
    let product = "Fluxer";
    let web = st.web_base.trim_end_matches('/');
    let recover_page = format!("{web}/recover");
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    let created_label = chrono::DateTime::parse_from_rfc3339(created).map_or_else(|_| created.to_string(), |d| {
        let d = d.with_timezone(&chrono::Local);
        let loc = crate::i18n::locale();
        format!("{}, {}", crate::desktop::time::short_date(&d, loc), crate::desktop::time::time(&d, loc))
    });
    super::recovery_pdf::Sheet {
        product: product.into(),
        title: pickf("{productName} recovery kit", &[("productName", product)]),
        intro: pick("Keep this page somewhere safe. If you forget your password, you can use it to get back into your account. Anyone with this recovery key can reset your password, so never share it."),
        details: vec![(pick("Instance"), web.to_string()), (pick("Username"), tag.to_string()), (pick("Created"), created_label)],
        key_label: pick("Recovery key"),
        key: key.to_string(),
        recover_url: format!("{recover_page}#username={}&key={}", enc(tag), enc(key)),
        qr_caption: pick("Scan to open the recovery page with your details filled in."),
        steps_title: pick("How to recover your account"),
        steps: vec![
            pickf("Go to {recoverUrl} or scan the QR code.", &[("recoverUrl", &recover_page)]),
            pick("Enter your username and this recovery key."),
            pick("Choose a new password. You get a new recovery kit and this one stops working."),
        ],
        backup: codes.map(|c| (pick("Two-factor backup codes"), pick("Each code works once in place of your authenticator app."), c)),
    }
}
