//! A dev-only screen showing every component in its states, for snapshot checks:
//! `cargo run -p reflux -- --gallery --snap /tmp/opencode/snaps/gallery.png --size 1280x900 --scale 1`
//! (`--scene modal|select|light|coal` for the other states).

use rune::{Cx, El, Image, ImageCache, ScrollSpec, Waker, col, el, row, text};

use super::menu::MenuEntry;
use super::overlay::{Placement, Popups, TooltipKind, tooltip_surface};
use super::{
    Avatar, Button, ButtonVariant, Checkbox, Menu, MenuItem, Modal, ModalSize, RadioGroup, Select, Slider, SpinnerSize,
    Status, Switch, Tooltip, mention_badge, misc, notification_dot, scroller,
};
use crate::State;
use crate::icons;
use crate::theme::{self, Kind, Palette, font};

pub struct Gallery {
    pub theme: Kind,
    pub popups: Popups,
    pub switch_on: bool,
    pub checked: bool,
    pub radio: String,
    pub slider: f32,
    pub select: Option<String>,
    pub modal: bool,
    pub notify: bool,
    pub avatar: Option<Image>,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            theme: Kind::Dark,
            popups: Popups::default(),
            switch_on: true,
            checked: true,
            radio: "cozy".into(),
            slider: 65.0,
            select: Some("dark".into()),
            modal: false,
            notify: true,
            avatar: None,
        }
    }
}

impl Gallery {
    /// The `--gallery` screen: the menu shows open, with its submenu, so a snapshot has it.
    pub fn screen() -> Self {
        Self { popups: Popups { open: Some("g-menu".into()), sub: Some("status".into()), ..Popups::default() }, ..Self::default() }
    }
}

pub const SCENES: &[(&str, fn(&mut State))] = &[
    ("modal", |s| g(s).modal = true),
    ("select", |s| g(s).popups = Popups { open: Some("g-select".into()), ..Popups::default() }),
    ("light", |s| g(s).theme = Kind::Light),
    ("coal", |s| g(s).theme = Kind::Coal),
    // Logged-out screens (no gallery).
    ("register", crate::login::scenes::register),
    ("register-filled", crate::login::scenes::register_filled),
    ("register-dob", crate::login::scenes::register_dob),
    ("register-pending", crate::login::scenes::register_pending),
    ("forgot", crate::login::scenes::forgot),
    ("forgot-sent", crate::login::scenes::forgot_sent),
    ("mfa-webauthn", crate::login::scenes::mfa_webauthn),
    ("passkey", crate::login::scenes::passkey),
    ("handoff", crate::login::scenes::handoff),
    ("passkey-failed", crate::login::scenes::passkey_failed),
    ("passkey-pin", crate::login::scenes::passkey_pin),
    ("restricted-notice", crate::login::scenes::restricted_notice),
    ("restricted-network", crate::login::scenes::restricted_network),
    ("age-blocked", crate::login::scenes::age_blocked),
    // `--shell-demo` with an unverified account: the email verification nagbar.
    ("unverified", |s| {
        if let Some(me) = s.shell_demo.as_mut().and_then(|d| d.store.me.as_mut()) {
            me.verified = false;
            me.email = Some("me@example.com".into());
        }
    }),
];

/// The `--gallery` screen's state, else the Design system settings tab's.
fn g(s: &mut State) -> &mut Gallery {
    if s.gallery.is_some() {
        return s.gallery.get_or_insert_with(Gallery::screen);
    }
    match s.shell_demo.as_mut() {
        Some(d) => &mut d.shell.nav.gallery,
        None => &mut s.app.shell.nav.gallery,
    }
}

fn pops(s: &mut State) -> &mut Popups {
    &mut g(s).popups
}

/// Decodes a default avatar for the avatar rows (waits: it's a dev screen).
pub fn setup(s: &mut State, waker: Waker) {
    if let Some(gal) = s.gallery.as_mut() {
        let img = ImageCache::new(waker).load(&include_bytes!("../../../../assets/media/images/avatars/2.png")[..]);
        let _ = img.wait();
        gal.avatar = Some(img);
    }
}

fn heading(t: &str, p: &Palette) -> El {
    text(t).font(font(12.0, 600.0).lh(16.0).tracking(0.24)).color(p.text_tertiary)
}

fn section(t: &str, p: &Palette, body: El) -> El {
    col().w_full().gap(10.0).child(heading(t, p)).child(body)
}

pub fn view(gal: &Gallery, _cx: &mut Cx<State>) -> El {
    let (body, layer) = parts(gal, false);
    rune_kit::root(body, layer)
}

/// The gallery and its overlays: three columns for the screen, `stacked` for a settings tab.
pub fn parts(gal: &Gallery, stacked: bool) -> (El, Vec<El>) {
    let p = theme::palette(gal.theme);
    let mut layer = Vec::new();
    let px = gal.avatar.as_ref().map(|i| i.pixels());

    // Avatars: every size with a status, then each status at 40, mobile and typing.
    let sizes = row().gap(12.0).items_end().children(
        [16.0, 20.0, 24.0, 32.0, 36.0, 40.0, 44.0, 48.0, 56.0, 80.0].map(|n| Avatar::new(n).pixels(px).status(Some(Status::Online)).el(p)),
    );
    let states = row().gap(16.0).center_y().children(
        [
            Avatar::new(40.0).pixels(px).status(Some(Status::Online)),
            Avatar::new(40.0).pixels(px).status(Some(Status::Idle)),
            Avatar::new(40.0).pixels(px).status(Some(Status::Dnd)),
            Avatar::new(40.0).pixels(px).status(Some(Status::Offline)),
            Avatar::new(40.0).pixels(px).status(Some(Status::Online)).mobile(true),
            Avatar::new(40.0).pixels(px).status(Some(Status::Online)).typing(true),
            Avatar::new(40.0).clickable(true),
            Avatar::new(120.0).pixels(px).status(Some(Status::Dnd)),
        ]
        .into_iter()
        .map(|a| a.el(p)),
    );
    let avatars = col().gap(16.0).child(sizes).child(states);

    let badges = row()
        .gap(12.0)
        .center_y()
        .children([1, 12, 150, 12_600].into_iter().filter_map(|n| mention_badge(n, false, p)))
        .children([3, 120].into_iter().filter_map(|n| mention_badge(n, true, p)))
        .child(notification_dot(p))
        .child(misc::spinner(SpinnerSize::Small, p))
        .child(misc::spinner(SpinnerSize::Medium, p))
        .child(misc::spinner(SpinnerSize::Large, p));

    // Tooltips, drawn as they show on hover: four placements, danger and large.
    let tip_trigger = |label: &str| {
        Button::<State>::new(label).variant(ButtonVariant::Secondary).key(format!("g-tip-{label}")).el(p)
    };
    let tips = row()
        .gap(16.0)
        .py(48.0)
        .px(8.0)
        .child(Tooltip::new("g-t1", "Top tooltip").force(true).el(tip_trigger("Top"), &gal.popups, pops, p, &mut layer))
        .child(Tooltip::new("g-t2", "Bottom").placement(Placement::Bottom).force(true).el(tip_trigger("Bottom"), &gal.popups, pops, p, &mut layer))
        .child(
            Tooltip::new("g-t3", "Can't do that")
                .danger()
                .force(true)
                .el(tip_trigger("Danger"), &gal.popups, pops, p, &mut layer),
        )
        .child(Tooltip::new("g-t4", "Hover me").el(tip_trigger("Hover"), &gal.popups, pops, p, &mut layer));
    let side_tips = row()
        .gap(140.0)
        .pl(120.0)
        .child(Tooltip::new("g-t5", "Left").placement(Placement::Left).force(true).el(tip_trigger("L"), &gal.popups, pops, p, &mut layer))
        .child(
            Tooltip::new("g-t6", "A large tooltip with a longer label that wraps")
                .placement(Placement::Right)
                .large()
                .force(true)
                .el(tip_trigger("R"), &gal.popups, pops, p, &mut layer),
        );
    // A free-standing one for an exact look at the surface.
    let lone = tooltip_surface("Standalone", Placement::Bottom, TooltipKind::Normal, false, p);

    let left = col()
        .w(560.0)
        .gap(28.0)
        .child(section("AVATARS", p, avatars))
        .child(section("BADGES & SPINNERS", p, badges))
        .child(section("TOOLTIPS", p, col().gap(8.0).child(tips).child(side_tips).child(row().pt(12.0).child(lone))));

    // Form controls.
    let switches = col()
        .w_full()
        .gap(8.0)
        .child(
            row()
                .gap(16.0)
                .child(Switch::new(gal.switch_on).on_toggle(|s: &mut State| g(s).switch_on ^= true).el(p))
                .child(Switch::<State>::new(!gal.switch_on).el(p))
                .child(Switch::<State>::new(true).disabled(true).el(p))
                .child(Switch::<State>::new(false).disabled(true).el(p)),
        )
        .child(
            Switch::new(gal.notify)
                .label("Enable notifications")
                .description("Show a desktop notification for mentions.")
                .on_toggle(|s: &mut State| g(s).notify ^= true)
                .el(p),
        );
    let checks = row()
        .gap(16.0)
        .center_y()
        .child(Checkbox::new(gal.checked).on_toggle(|s: &mut State| g(s).checked ^= true).el(p))
        .child(Checkbox::<State>::new(false).el(p))
        .child(Checkbox::<State>::new(true).size(18.0).el(p))
        .child(Checkbox::<State>::new(true).round().el(p))
        .child(Checkbox::<State>::new(true).disabled(true).el(p))
        .child(Checkbox::<State>::new(false).label("Remember me").el(p));
    const DENSITY: &[(&str, &str, Option<&str>)] =
        &[("cozy", "Cozy", Some("Modern, beautiful, and easy on your eyes.")), ("compact", "Compact", None), ("off", "Disabled option", None)];
    let radios = RadioGroup::new(DENSITY, Some(gal.radio.as_str())).on_change(|s: &mut State, v| g(s).radio = v.into()).el(p);
    let sliders = col()
        .w_full()
        .gap(6.0)
        .child(Slider::new("g-slider", gal.slider, 0.0, 100.0).on_change(|s: &mut State, v| g(s).slider = v).el(p))
        .child(Slider::<State>::new("g-slider-mini", 25.0, 0.0, 100.0).mini().el(p))
        .child(Slider::<State>::new("g-slider-off", 50.0, 0.0, 100.0).disabled(true).el(p));
    const THEMES: &[(&str, &str)] = &[("system", "Sync with system"), ("dark", "Dark"), ("light", "Light"), ("coal", "Coal")];
    let selects = col()
        .gap(12.0)
        .child(
            Select::new("g-select", THEMES, gal.select.as_deref())
                .label("Theme")
                .width(300.0)
                .on_choose(|s: &mut State, v| g(s).select = Some(v.into()))
                .el(&gal.popups, pops, p, &mut layer),
        )
        .child(
            row()
                .gap(12.0)
                .child(Select::<State>::new("g-select-2", THEMES, None).placeholder("Pick one").width(180.0).el(&gal.popups, pops, p, &mut layer))
                .child(Select::<State>::new("g-select-3", THEMES, Some("coal")).compact().width(140.0).el(&gal.popups, pops, p, &mut layer))
                .child(Select::<State>::new("g-select-4", THEMES, Some("dark")).disabled(true).width(120.0).el(&gal.popups, pops, p, &mut layer)),
        );
    let middle = col()
        .w(470.0)
        .gap(24.0)
        .child(section("SWITCH", p, switches))
        .child(section("CHECKBOX", p, checks))
        .child(section("RADIO", p, radios))
        .child(section("SLIDER", p, sliders))
        .child(section("SELECT", p, selects))
        .child(section(
            "MODAL",
            p,
            row().child(Button::new("Open modal").key("g-open-modal").on_press(|s: &mut State| g(s).modal = true).el(p)),
        ));

    // Menu trigger and a scroller.
    let menu_btn = Button::new("Menu")
        .variant(ButtonVariant::Secondary)
        .icon(&icons::DOTS_THREE_BOLD)
        .key("g-menu")
        .on_press(|s: &mut State| pops(s).toggle("g-menu"))
        .el(p)
        .on_secondary_press(|s: &mut State, at, _| pops(s).open_at("g-menu", [at.x, at.y]));
    Menu::new("g-menu")
        .group("Message")
        .item(MenuItem::new("reply", "Reply").icon(&icons::ARROW_BEND_UP_LEFT))
        .item(MenuItem::new("edit", "Edit message").icon(&icons::PENCIL_SIMPLE).shortcut("Ctrl+E"))
        .item(MenuItem::new("copy", "Copy text").icon(&icons::COPY).hint("Copies the raw markdown"))
        .separator()
        .submenu(
            "status",
            "Set status",
            vec![
                MenuEntry::Item(MenuItem::new("on", "Online").radio(true)),
                MenuEntry::Item(MenuItem::new("idle", "Idle").radio(false)),
                MenuEntry::Item(MenuItem::new("dnd", "Do Not Disturb").radio(false)),
            ],
        )
        .item(MenuItem::new("mute", "Mute channel").checkbox(true))
        .item(MenuItem::new("pin", "Pin message").checkbox(false))
        .item(MenuItem::new("nope", "Disabled item").disabled(true))
        .separator()
        .separator()
        .item(MenuItem::new("delete", "Delete message").icon(&icons::TRASH).danger())
        .el(&gal.popups, pops, p, &mut layer);
    let list = col().w_full().children((1..=30).map(|i| {
        row().w_full().h(32.0).px(10.0).center_y().child(text(format!("Row {i}")).font(font(14.0, 400.0)).color(p.text_primary))
    }));
    let scroll = scroller(col().key("g-scroll").w_full().h(220.0).scroll(ScrollSpec { stick_bottom: false }).child(list), false, p);
    let right = col()
        .w(170.0)
        .gap(24.0)
        .child(section("MENU", p, row().child(menu_btn)))
        .child(el().h(300.0))
        .child(section("SCROLLER", p, col().w_full().radius(8.0).fill(p.background_secondary).child(scroll)));

    let body = if stacked { col().w_full().radius(8.0) } else { row().w_full().h_full() };
    let body = body.padding(24.0).gap(24.0).fill(p.background_primary).child(left).child(middle).child(right);
    if gal.modal {
        layer.push(
            Modal::new("Delete Message", |s: &mut State| g(s).modal = false)
                .size(ModalSize::Small)
                .body(
                    text("Are you sure you want to delete this message? This cannot be undone.")
                        .font(font(14.0, 400.0).lh(19.6))
                        .color(p.text_primary_muted)
                        .wrap(),
                )
                .footer(
                    row()
                        .gap(8.0)
                        .child(Button::new("Cancel").variant(ButtonVariant::Secondary).on_press(|s: &mut State| g(s).modal = false).el(p))
                        .child(Button::new("Delete").variant(ButtonVariant::Danger).on_press(|s: &mut State| g(s).modal = false).el(p)),
                )
                .el(p),
        );
    }
    (body, layer)
}
