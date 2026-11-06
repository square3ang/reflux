//! Invite previews for the invite cards in messages (`InviteEmbed`).

use reflux_model::Invite;

use super::{spawn, update};
use crate::State;
use crate::shell::invite_embed::Preview;

/// `GET /invites/{code}` into `media_ui.invites`.
// shortcut: any failure reads as "Unknown invite"; Fluxer retries transient errors.
pub(super) fn fetch(s: &mut State, code: String) {
    let Some(inst) = s.app.instance.clone() else { return };
    spawn(async move {
        let r = inst.rest.get::<Invite>(&format!("/invites/{code}")).await;
        update(move |s| {
            let preview = match r {
                Ok(inv) => Preview::Ready(Box::new(inv)),
                Err(e) => {
                    eprintln!("reflux: invite {code}: {e}");
                    Preview::Unknown
                }
            };
            s.app.shell.media_ui.invites.insert(code, preview);
        });
    });
}
