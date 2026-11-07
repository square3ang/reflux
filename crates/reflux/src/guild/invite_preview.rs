//! The Overview tab's invite previews (`GuildInviteSplashSettingsField`): "Invite page"
//! (`InvitePagePreviewModal`: the logged-out invite page over the splash, with the card
//! alignment controls and an "Exit preview" pill) and "Invite modal" (`InviteAcceptModalPreview`),
//! both from the unsaved name, icon, splash and alignment. `form.data["invite-preview"]` holds
//! which one is open (`page` / `modal`).

use reflux_model::{Guild, Snowflake};
use reflux_store::Store;
use rune::{Color, El, Pixels, col, el, row, text};
use serde_json::{Value, json};

use crate::i18n::{t, tf};
use crate::icons;
use crate::media::{self, Load};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, mix, over, rgb, rgba};

const KEY: &str = "invite-preview";

/// A press handler opening preview `which`.
fn open<S: 'static>(acc: ShellAccess<S>, which: &'static str) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| {
        acc(s).form.data.insert(KEY.into(), json!(which));
    }
}

fn close<S: 'static>(acc: ShellAccess<S>) -> impl Fn(&mut S) + Clone + 'static {
    move |s: &mut S| {
        acc(s).form.data.remove(KEY);
    }
}

/// The "Preview" column: the two buttons and their helper.
pub fn buttons<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
    col()
        .gap(8.0)
        .items_start()
        .child(super::settings::title(t("Preview"), p))
        .child(
            row()
                .gap(8.0)
                .child(Button::new(t("Invite page")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).icon(&icons::EYE_BOLD).key("preview-invite-page").on_press(open(acc, "page")).el(p))
                .child(Button::new(t("Invite modal")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).icon(&icons::EYE_BOLD).key("preview-invite-modal").on_press(open(acc, "modal")).el(p)),
        )
        .child(super::settings::small(t("See how your invite looks to visitors."), p))
}

/// The draft image under `key` (a picked data URL, `None` when removed), else the saved one.
fn image(st: &ShellState, key: &str, saved: impl FnOnce() -> Load) -> Option<Pixels> {
    match st.form.data.get(key) {
        Some(Value::String(url)) => crate::settings::profile::picked_px(st, url),
        Some(_) => None,
        None => match saved() {
            Load::Ready(px) => Some(px),
            _ => None,
        },
    }
}

/// `PreviewGuildInviteHeader`: icon, "You've been invited to join", name and counts.
fn header<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, info: &Guild, p: &Palette, acc: ShellAccess<S>) -> El {
    let name = st.form.text.get("name").map_or(info.name.clone(), |e| e.text.clone());
    let icon = image(st, "icon", || info.icon.as_deref().map_or(Load::Failed, |h| st.media.asset(|b, s| media::guild_icon_url(b, g, h, 80.0, s))));
    let icon = match icon {
        Some(px) => el().size(80.0, 80.0).radius(40.0).pixels_fit(&px, rune::SurfaceFit::Fill),
        None => row().size(80.0, 80.0).center().radius(40.0).fill(p.background_primary).child(text(crate::shell::acronym(&name)).font(font(24.0, 600.0)).color(p.text_primary)),
    };
    let (online, members) = store.guilds.get(&g).map_or((0, 0), |gs| (gs.online_count, gs.member_count));
    let stat = |dot: Color, s: String| row().gap(4.0).center_y().child(el().size(10.0, 10.0).radius(5.0).fill(dot)).child(text(s).font(font(14.0, 400.0).lh(20.0)).color(p.text_tertiary));
    let n = members.to_string();
    let members_text = tf("{formattedMemberCount} {memberCount, plural, one {member} other {members}}", &[("formattedMemberCount", &n), ("memberCount", &n)]);
    col()
        .w_full()
        .gap(12.0)
        .items_center()
        .child(icon)
        .child(
            col()
                .gap(4.0)
                .items_center()
                .child(text(t("You've been invited to join")).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary))
                .child(row().gap(8.0).center_y().child(text(name).font(font(20.0, 700.0).lh(26.0)).color(p.text_primary)).children(crate::shell::invite_embed::guild_badge(&info.features, &info.badge_ids, st, p, acc)))
                .child(
                    row()
                        .gap(16.0)
                        .center()
                        .child(stat(p.status_online, tf("{formattedPresenceCount} online", &[("formattedPresenceCount", &online.to_string())])))
                        .child(stat(p.text_tertiary_secondary, members_text)),
                ),
        )
}

/// A read-only form field (`MockMinimalRegisterForm`).
fn mock_input(label: &str, placeholder: &str, p: &Palette) -> El {
    let bg = p.form_surface_background;
    col()
        .w_full()
        .gap(8.0)
        .child(text(label.to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary))
        .child(row().w_full().h(44.0).px(16.0).center_y().radius(8.0).fill(bg).border(1.0, over(p.background_modifier_accent, bg)).child(text(placeholder.to_string()).font(font(16.0, 400.0).lh(20.0)).color(p.text_tertiary)))
}

/// The overlay for the open preview, if any.
pub fn view<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, info: &Guild, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let which = st.form.data.get(KEY)?.as_str()?;
    let splash = image(st, "splash", || info.splash.as_deref().map_or(Load::Failed, |h| st.media.asset(|b, s| media::guild_wide_url(b, "splashes", g, h, s))));
    let head = header(store, st, g, info, p, acc);
    Some(if which == "page" { page(st, info, head, splash, p, acc) } else { modal(head, splash, p, acc) })
}

/// `InvitePagePreviewModal`.
fn page<S: 'static>(st: &ShellState, info: &Guild, head: El, splash: Option<Pixels>, p: &Palette, acc: ShellAccess<S>) -> El {
    let align = st.form.data.get("splash_align").and_then(Value::as_u64).unwrap_or(u64::from(info.splash_card_alignment));
    let bg = p.background_secondary;
    let card = col()
        .w(512.0)
        .padding(41.0)
        .gap(24.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(rgba(255, 255, 255, 0.08), bg))
        .box_shadow(rune::Shadow { x: 0.0, y: 20.0, blur: 64.0, spread: -24.0, color: rgba(0, 0, 0, 0.7), inset: false })
        .a11y_hidden()
        .child(head)
        .child(
            col()
                .w_full()
                .gap(20.0)
                .child(mock_input(t("Display name (optional)"), t("What should people call you?"), p))
                .child(text(t("By creating an account, you agree to the terms of service. The privacy policy explains how we handle your data.")).w_full().font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).wrap())
                .child(row().w_full().h(44.0).center().radius(8.0).fill(p.brand_primary).child(text(t("Create account")).font(font(16.0, 600.0).lh(20.0)).color(p.brand_primary_fill))),
        )
        .child(
            row()
                .w_full()
                .gap(4.0)
                .child(text(t("Already have an account?")).font(font(14.0, 400.0).lh(20.0)).color(p.text_tertiary))
                .child(text(t("Sign in")).font(font(14.0, 400.0).lh(20.0)).color(p.text_link)),
        );
    // `.leftSplit` / `.alignLeft` / `.alignRight`.
    let mut place = row().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).padding(48.0).center_y();
    place = match align {
        1 => place.justify_start(),
        2 => place.justify_end(),
        _ => place.justify_center(),
    };
    let mut back = el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(rgb(8, 6, 22));
    back = match splash {
        Some(px) => back.child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).pixels_fit(&px, rune::SurfaceFit::Fill)),
        None => back.linear_gradient(145.0 * std::f32::consts::PI / 180.0, [(0.0, rgb(5, 4, 13)), (0.46, rgb(8, 6, 22)), (1.0, rgb(13, 10, 28))]),
    };
    let x = close(acc);
    let pill_bg = mix(p.background_secondary, Color::TRANSPARENT, 85.0);
    let pill = row()
        .gap(12.0)
        .py(10.0)
        .px(18.0)
        .center_y()
        .radius(12.0)
        .fill(pill_bg)
        .border(1.0, mix(p.background_modifier_accent, Color::TRANSPARENT, 70.0))
        .box_shadow(rune::Shadow { x: 0.0, y: 12.0, blur: 36.0, spread: 0.0, color: rgba(0, 0, 0, 0.35), inset: false })
        .child(text(t("You're in preview mode")).font(font(13.0, 600.0).lh(18.0)).color(p.text_primary))
        .child(Button::new(t("Exit preview")).size(ButtonSize::Small).key("exit-invite-preview").on_press(x.clone()).el(p));
    let controls = super::overview_extra::alignment_controls(align, p, acc);
    el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .role("dialog")
        .label(t("Invite page preview"))
        .child(back)
        .child(place.child(card))
        .child(row().pin(Some(0.0), Some(16.0), Some(0.0), None).justify_center().child(pill))
        .child(row().pin(Some(0.0), None, Some(0.0), Some(24.0)).justify_center().child(controls))
        .modal(true, x)
}

/// `InviteAcceptModalPreview`.
fn modal<S: 'static>(head: El, splash: Option<Pixels>, p: &Palette, acc: ShellAccess<S>) -> El {
    let x = close(acc);
    let mut root = el().w_full().max_w(1040.0).h_full().max_h(780.0).radius(16.0).clip_rounded(16.0).fill(p.brand_primary).border(1.0, p.background_header_secondary);
    if let Some(px) = splash {
        root = root.child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).pixels_fit(&px, rune::SurfaceFit::Fill));
    }
    let card_bg = mix(p.background_secondary, Color::TRANSPARENT, 86.0);
    let card = col()
        .w(560.0)
        .padding(30.0)
        .gap(16.0)
        .radius(18.0)
        .fill(card_bg)
        .border(1.0, mix(p.background_modifier_accent, Color::TRANSPARENT, 70.0))
        .box_shadow(rune::Shadow { x: 0.0, y: 24.0, blur: 60.0, spread: 0.0, color: rgba(0, 0, 0, 0.28), inset: false })
        .child(head)
        .child(row().w_full().justify_center().child(Button::<S>::new(t("Join community")).disabled(true).el(p)));
    let inset_close = row()
        .pin(Some(16.0), Some(16.0), None, None)
        .size(32.0, 32.0)
        .center()
        .radius(16.0)
        .fill(rgba(0, 0, 0, 0.4))
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("Close"))
        .focusable(true)
        .on_press(x.clone())
        .child(rune::icon(&icons::X_BOLD, 18.0).color(Color::WHITE));
    root = root.child(row().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).padding(24.0).center().child(card)).child(inset_close).role("dialog").label(t("Invite modal preview"));
    el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(Color::BLACK).opacity(0.85))
        .child(col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).padding(24.0).center().child(root.modal(true, x)))
}
