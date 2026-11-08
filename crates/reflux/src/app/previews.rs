//! Fetches the Active Now sidebar's stream previews (`useStreamPreview`,
//! `GET /streams/{stream_key}/preview`, `http-api/streams.mdx`).

use std::sync::Arc;

use super::{Route, now_ms, spawn, update};
use crate::State;

pub(super) fn tick(s: &mut State, tx: &mut rune::Tx) {
    if s.app.route != Route::Main {
        return;
    }
    let Some(inst) = s.app.instance.clone() else { return };
    let wanted = crate::shell::active_now::wanted_previews(&s.app.store, &s.app.shell);
    let previews = &mut s.app.shell.voice.previews;
    previews.retain(|k, _| wanted.contains(k));
    if wanted.is_empty() {
        return;
    }
    let (now, generation) = (now_ms(), s.app.generation);
    for key in wanted {
        let p = previews.entry(key.clone()).or_default();
        if !p.due(now) {
            continue;
        }
        p.loading = true;
        let inst = inst.clone();
        spawn(async move {
            let r = inst.rest.get_bytes(&format!("/streams/{key}/preview")).await;
            if let Err(e) = &r {
                eprintln!("reflux: stream preview {key}: {e}");
            }
            update(move |s| {
                if s.app.generation != generation {
                    return;
                }
                let Some(p) = s.app.shell.voice.previews.get_mut(&key) else { return };
                (p.loading, p.fetched_ms) = (false, now_ms());
                match r {
                    Ok(Some(b)) => {
                        if crate::ui::misc::debug_scroll() {
                            eprintln!("preview: fetched {key} ({} bytes)", b.len());
                        }
                        (p.bytes, p.missing) = (Some(Arc::new(b)), 0)
                    }
                    _ => p.missing = p.missing.saturating_add(1),
                }
            });
        });
    }
    // Check again when the next one could be due.
    let soon = s.app.shell.voice.previews.values().any(|p| p.loading || p.due(now + 1_000));
    tx.wake_after(std::time::Duration::from_secs(if soon { 1 } else { 60 }));
}
