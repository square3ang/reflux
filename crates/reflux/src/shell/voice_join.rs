//! "Confirm before joining voice channels" (`Accessibility.confirmBeforeJoiningVoiceChannels`):
//! what a click on a voice channel does before `join_voice`.

use reflux_model::Snowflake;
use rune::{el, row, text};

use super::{ShellState, ShellAccess};
use crate::i18n::t;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Modal};

/// `usePendingVoiceConnection.startConnection`: asks first when the setting is on (and the
/// double-click requirement is off).
pub fn request<S: 'static>(sh: &mut ShellState, guild: Option<Snowflake>, channel: Snowflake) {
    if guild.is_some() && sh.prefs.look.confirm_join_voice && sh.prefs.look.join_double_click == false {
        sh.join_confirm = Some((guild, channel));
    } else {
        sh.join_voice(guild, channel);
    }
}

/// The confirmation dialog.
pub fn modal<S: 'static>(_store: &reflux_store::Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<rune::El> {
    let (guild, channel) = st.join_confirm?;
    let close = move |s: &mut S| acc(s).join_confirm = None;
    let body = text(t("Are you sure you want to join this voice channel?"))
        .font(font(14.0, 400.0).lh(20.0))
        .color(p.text_primary)
        .wrap();
    Some(
        Modal::new(t("Join voice channel"), close)
            .size(crate::ui::ModalSize::Small)
            .body(el().w_full().child(body))
            .footer(
                row()
                    .w_full()
                    .gap(8.0)
                    .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("join-voice-cancel").on_press(close).el(p))
                    .child(
                        Button::new(t("Join voice channel"))
                            .key("join-voice-confirm")
                            .on_press(move |s: &mut S| {
                                let sh = acc(s);
                                sh.join_confirm = None;
                                sh.join_voice(guild, channel);
                            })
                            .el(p),
                    ),
            )
            .el(p),
    )
}
