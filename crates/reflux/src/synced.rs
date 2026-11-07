//! `synced_preferences` (`fluxer.user.preferences.v1.SyncedPreferences`, base64 protobuf in the
//! user settings) at the wire level: read one top-level field, or replace it and send the whole
//! snapshot back with every other field's bytes untouched (`settings-protobuf.md`).
//!
//! Every module hands its changed top-level fields to one [`Queue`] (Fluxer's dirty fields) and
//! reads the snapshot through [`Queue::view`], so its own unsent edits read back as current.
//! [`flush`] sends them with `synced_preferences_base_hash`; a 409 refetches the settings and sends
//! again, the dirty fields winning over the refetched remote (`UserSettings.flushSyncedPreferences`).

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE};

/// One field: its number, wire type, and the whole record (tag included).
#[derive(Clone, Debug, PartialEq)]
pub struct Field<'a> {
    pub num: u32,
    pub wire: u8,
    pub raw: &'a [u8],
    /// The value: varint bytes, fixed bytes, or a length-delimited payload.
    pub value: &'a [u8],
}

fn varint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*at)?;
        *at += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

pub fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// A message's fields in order; `None` if it doesn't parse.
pub fn fields(b: &[u8]) -> Option<Vec<Field<'_>>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < b.len() {
        let start = at;
        let key = varint(b, &mut at)?;
        let (num, wire) = ((key >> 3) as u32, (key & 7) as u8);
        let vstart;
        match wire {
            0 => {
                vstart = at;
                varint(b, &mut at)?;
            }
            1 | 5 => {
                vstart = at;
                at += if wire == 1 { 8 } else { 4 };
            }
            2 => {
                let len = varint(b, &mut at)? as usize;
                vstart = at;
                at = at.checked_add(len)?;
            }
            _ => return None,
        }
        if at > b.len() {
            return None;
        }
        out.push(Field { num, wire, raw: &b[start..at], value: &b[vstart..at] });
    }
    Some(out)
}

pub fn decode(b64: &str) -> Vec<u8> {
    STANDARD.decode(b64).or_else(|_| URL_SAFE.decode(b64)).unwrap_or_default()
}

/// The payload of top-level message field `num` (the last one, as protobuf merges), if set.
pub fn get(b64: &str, num: u32) -> Option<Vec<u8>> {
    let b = decode(b64);
    fields(&b)?.into_iter().rev().find(|f| f.num == num && f.wire == 2).map(|f| f.value.to_vec())
}

/// `b64` with message field `num` replaced by `payload` (`None` removes it).
pub fn set(b64: &str, num: u32, payload: Option<&[u8]>) -> String {
    let b = decode(b64);
    let mut out: Vec<u8> = fields(&b).unwrap_or_default().iter().filter(|f| f.num != num).flat_map(|f| f.raw.iter().copied()).collect();
    if let Some(p) = payload {
        bytes(&mut out, num, p);
    }
    if out.is_empty() { String::new() } else { STANDARD.encode(out) }
}

/// Length-delimited field `num`.
pub fn bytes(out: &mut Vec<u8>, num: u32, v: &[u8]) {
    put_varint(out, u64::from(num) << 3 | 2);
    put_varint(out, v.len() as u64);
    out.extend_from_slice(v);
}

/// Varint field `num` (proto3 skips zero unless `optional`).
pub fn uint(out: &mut Vec<u8>, num: u32, v: u64) {
    put_varint(out, u64::from(num) << 3);
    put_varint(out, v);
}

pub fn str_of(f: &Field) -> String {
    String::from_utf8_lossy(f.value).into_owned()
}

pub fn uint_of(f: &Field) -> u64 {
    varint(f.value, &mut 0).unwrap_or(0)
}

use std::collections::BTreeMap;

use reflux_model::Snowflake;

/// `MAX_SYNCED_PREFERENCES_CONFLICT_RETRIES`.
const MAX_CONFLICT_RETRIES: u32 = 3;

type Fields = BTreeMap<u32, Option<Vec<u8>>>;

/// Dirty top-level fields (payload, `None` = removed) and the request in flight.
#[derive(Debug, Default)]
pub struct Queue {
    account: Snowflake,
    pending: Fields,
    in_flight: Option<Fields>,
    conflicts: u32,
    /// The last send failed for another reason; waits for the next edit (Fluxer rejects the save).
    stalled: bool,
}

fn splice(b64: &str, fields: &Fields) -> String {
    fields.iter().fold(b64.to_string(), |b, (&n, p)| set(&b, n, p.as_deref()))
}

impl Queue {
    /// The gateway's snapshot with this device's unacknowledged fields over it.
    pub fn view(&self, b64: &str) -> String {
        let mut all = self.in_flight.clone().unwrap_or_default();
        all.extend(self.pending.clone());
        splice(b64, &all)
    }

    /// Marks field `num` dirty with `payload` (`None` removes it).
    pub fn put(&mut self, account: Snowflake, num: u32, payload: Option<&[u8]>) {
        if self.account != account {
            *self = Self { account, ..Self::default() };
        }
        self.pending.insert(num, payload.map(<[u8]>::to_vec));
        self.stalled = false;
    }

    /// The fields to send and the request body, or `None` when there's nothing to send.
    fn take(&mut self, account: Snowflake, base: &str) -> Option<serde_json::Value> {
        if self.account != account {
            *self = Self { account, ..Self::default() };
        }
        if self.in_flight.is_some() || self.stalled || self.pending.is_empty() {
            return None;
        }
        let sent = std::mem::take(&mut self.pending);
        let next = splice(base, &sent);
        self.in_flight = Some(sent);
        use sha2::Digest;
        let hash: String = sha2::Sha256::digest(decode(base)).iter().map(|b| format!("{b:02x}")).collect();
        let value = if next.is_empty() { serde_json::Value::Null } else { next.into() };
        Some(serde_json::json!({ "synced_preferences": value, "synced_preferences_base_hash": hash }))
    }

    /// The request ended: acknowledged fields drop out unless edited again since; a failed one's
    /// fields go back to pending.
    fn done(&mut self, account: Snowflake, ok: bool) {
        if self.account != account {
            return;
        }
        let sent = self.in_flight.take().unwrap_or_default();
        if !ok {
            for (n, p) in sent {
                self.pending.entry(n).or_insert(p);
            }
        }
    }
}

/// Once a frame: sends the queued fields over the latest snapshot.
pub fn flush(s: &mut crate::State) {
    let Some(me) = s.app.store.me.as_ref().map(|u| u.id) else { return };
    let base = s.app.store.settings.synced_preferences.clone();
    let Some(body) = s.app.shell.synced.take(me, &base) else { return };
    let Some(inst) = s.app.instance.clone() else { return };
    crate::app::spawn(async move {
        let r = inst.rest.update_settings(&body).await;
        let conflict = matches!(&r, Err(e @ reflux_net::Error::Api { status, .. }) if status.as_u16() == 409 && e.code() == Some("SYNCED_PREFERENCES_CONFLICT"));
        let refetched = if conflict { Some(inst.rest.settings().await) } else { None };
        crate::app::update(move |s| {
            let q = &mut s.app.shell.synced;
            match (r, refetched) {
                (Ok(new), _) => {
                    q.done(me, true);
                    q.conflicts = 0;
                    s.app.store.settings = new;
                }
                (Err(_), Some(Ok(fresh))) if q.conflicts < MAX_CONFLICT_RETRIES => {
                    q.conflicts += 1;
                    q.done(me, false);
                    // Only the snapshot: the other settings may have moved on through the gateway.
                    s.app.store.settings.synced_preferences = fresh.synced_preferences;
                }
                (Err(e), _) => {
                    eprintln!("reflux: synced preferences: {e}");
                    q.done(me, false);
                    q.conflicts = 0;
                    q.stalled = true;
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splices_one_field_and_keeps_the_rest() {
        let mut m = Vec::new();
        uint(&mut m, 25, 1); // sanitize_urls
        bytes(&mut m, 40, b"\x20\x01"); // favorites { hide_muted_channels }
        bytes(&mut m, 111, b"\x12\x03ptt");
        let b64 = STANDARD.encode(&m);
        assert_eq!(get(&b64, 40).as_deref(), Some(&b"\x20\x01"[..]));
        let out = decode(&set(&b64, 40, Some(b"\x28\x01")));
        let f = fields(&out).unwrap();
        assert_eq!(f.iter().map(|f| f.num).collect::<Vec<_>>(), [25, 111, 40]);
        assert_eq!(f[2].value, b"\x28\x01");
        assert_eq!(set("", 40, None), "");
        assert!(fields(b"\x0a\x05ab").is_none(), "truncated");
    }

    #[test]
    fn queue_keeps_local_fields_over_the_refetched_remote() {
        let me = Snowflake(1);
        let remote = |v: &[u8]| {
            let mut m = Vec::new();
            bytes(&mut m, 40, v);
            bytes(&mut m, 41, b"remote");
            STANDARD.encode(m)
        };
        let mut q = Queue::default();
        q.put(me, 40, Some(b"local"));
        assert_eq!(get(&q.view(&remote(b"old")), 40).as_deref(), Some(&b"local"[..]));
        let body = q.take(me, &remote(b"old")).unwrap();
        assert!(q.take(me, &remote(b"old")).is_none(), "one request at a time");
        assert_eq!(body["synced_preferences_base_hash"].as_str().unwrap().len(), 64);
        // 409: the refetched snapshot changed both fields; ours still wins, theirs is kept.
        q.done(me, false);
        let fresh = remote(b"theirs");
        let body = q.take(me, &fresh).unwrap();
        let sent = body["synced_preferences"].as_str().unwrap();
        assert_eq!(get(sent, 40).as_deref(), Some(&b"local"[..]));
        assert_eq!(get(sent, 41).as_deref(), Some(&b"remote"[..]));
        // An edit while in flight survives the acknowledgement.
        q.put(me, 40, Some(b"newer"));
        q.done(me, true);
        assert_eq!(get(&q.view(&fresh), 40).as_deref(), Some(&b"newer"[..]));
    }
}
