//! Runs the expression surfaces' requests ([`crate::shell::expr`]).

use reflux_model::{CreateMessage, Snowflake};
use serde_json::Value;

use super::{now_ms, spawn, update};
use crate::State;
use crate::shell::expr::Fetched;

/// `Intent::Fetch`: the answer (or error) goes to `expr.fetched[key]`.
pub(super) fn fetch(s: &mut State, key: String, method: reqwest::Method, path: String, body: Option<Value>) {
    let Some(inst) = s.app.instance.clone() else { return };
    spawn(async move {
        let r = inst.rest.request::<Value, Value>(method.clone(), &path, body.as_ref()).await;
        if let Err(e) = &r {
            eprintln!("reflux: {method} {path}: {e}");
        }
        if key.is_empty() {
            return;
        }
        update(move |s| {
            let v = r.map_or_else(|e| Fetched::Failed(e.to_string()), Fetched::Done);
            s.app.shell.expr.fetched.insert(key, v);
        });
    });
}

/// `Intent::SendExtra`: a sticker or saved meme, no optimistic message (the gateway echoes it).
pub(super) fn send(s: &mut State, channel: Snowflake, sticker: Option<Snowflake>, meme: Option<Snowflake>) {
    let Some(inst) = s.app.instance.clone() else { return };
    let nonce = Snowflake(Snowflake::from_timestamp_ms(now_ms()).0 | fastrand::u64(..1 << 22));
    let body = CreateMessage { nonce: Some(nonce.to_string()), sticker_ids: sticker.into_iter().collect(), favorite_meme_id: meme, ..Default::default() };
    spawn(async move {
        if let Err(e) = inst.rest.send_message(channel, &body).await {
            eprintln!("reflux: send to {channel}: {e}");
            update(move |s| s.app.shell.notice = Some(("Message failed".into(), e.to_string())));
        }
    });
}
