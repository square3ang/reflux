//! Picks several files at once. Rune's `Tx::open_file` returns one path, so on Linux this asks
//! the XDG desktop portal's `FileChooser.OpenFile` itself with `multiple`, and falls back to
//! Rune's one-file dialog where the portal can't be reached.
// shortcut: macOS and Windows (and desktops without the portal) pick one file per dialog; the
// portal dialog isn't parented to the window (Rune keeps the X11 parent id private).

use std::path::PathBuf;

use crate::shell::{ShellAccess, ShellState};

/// What `then` gets: the picked files (empty when cancelled), or why the dialog failed.
pub type Picked = Result<Vec<PathBuf>, String>;

/// Opens the picker; `then` runs on the UI thread with the answer.
pub fn open_files<S: 'static>(tx: &mut rune::Tx, title: &str, filter: Option<(&str, &[&str])>, acc: ShellAccess<S>, then: impl FnOnce(&mut ShellState, Picked) + Send + 'static) {
    #[cfg(target_os = "linux")]
    if !rune::is_headless() {
        let (title, filter) = (title.to_string(), filter.map(|(n, e)| (n.to_string(), e.iter().map(|e| e.to_string()).collect::<Vec<_>>())));
        std::thread::spawn(move || match portal(&title, filter.clone()) {
            Ok(r) => crate::app::update(move |s| then(&mut s.app.shell, r)),
            // No portal: Rune's dialog, one file.
            Err(()) => crate::app::update_tx(move |_, tx| fallback(tx, &title, filter, |s: &mut crate::State| &mut s.app.shell, then)),
        });
        return;
    }
    fallback(tx, title, filter.map(|(n, e)| (n.to_string(), e.iter().map(|e| e.to_string()).collect())), acc, then);
}

fn fallback<S: 'static>(tx: &mut rune::Tx, title: &str, filter: Option<(String, Vec<String>)>, acc: ShellAccess<S>, then: impl FnOnce(&mut ShellState, Picked) + 'static) {
    let mut options = rune::FileDialogOptions { title: title.into(), ..Default::default() };
    if let Some((name, ext)) = &filter {
        options = options.filter(name, &ext.iter().map(String::as_str).collect::<Vec<_>>());
    }
    tx.open_file(options, move |s: &mut S, r, _| then(acc(s), r.map(|p| p.into_iter().collect()).map_err(|e| e.to_string())));
}

/// `OpenFile` with `multiple`; `Err(())` when the portal can't be reached (before it shows).
#[cfg(target_os = "linux")]
fn portal(title: &str, filter: Option<(String, Vec<String>)>) -> Result<Picked, ()> {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
    const DEST: &str = "org.freedesktop.portal.Desktop";
    static TOKEN: AtomicU64 = AtomicU64::new(0);

    let conn = Connection::session().map_err(drop)?;
    let token = format!("reflux_files_{}", TOKEN.fetch_add(1, Ordering::Relaxed));
    let sender = conn.unique_name().ok_or(())?.trim_start_matches(':').replace('.', "_");
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    // Subscribe before calling: the portal can answer before the call returns.
    let request = Proxy::new(&conn, DEST, path.as_str(), "org.freedesktop.portal.Request").map_err(drop)?;
    let mut responses = request.receive_signal("Response").map_err(drop)?;
    let mut opts: HashMap<&str, Value> = HashMap::from([("handle_token", Value::from(token.as_str())), ("multiple", Value::from(true)), ("modal", Value::from(true))]);
    if let Some((name, ext)) = filter {
        let globs: Vec<(u32, String)> = ext.iter().map(|e| (0, format!("*.{e}"))).collect();
        opts.insert("filters", Value::from(vec![(name, globs)]));
    }
    let chooser = Proxy::new(&conn, DEST, "/org/freedesktop/portal/desktop", "org.freedesktop.portal.FileChooser").map_err(drop)?;
    let handle: OwnedObjectPath = chooser.call("OpenFile", &("", title, opts)).map_err(drop)?;
    let other;
    if handle.as_str() != path {
        // A portal without handle_token support picks its own request path.
        other = Proxy::new(&conn, DEST, handle.as_str().to_owned(), "org.freedesktop.portal.Request").map_err(drop)?;
        responses = other.receive_signal("Response").map_err(drop)?;
    }
    let Some(msg) = responses.next() else { return Ok(Err("The file dialog closed unexpectedly.".into())) };
    let (status, results) = match msg.body().deserialize::<(u32, HashMap<String, OwnedValue>)>() {
        Ok(v) => v,
        Err(e) => return Ok(Err(e.to_string())),
    };
    let uris = results.get("uris").and_then(|v| v.try_clone().ok()).and_then(|v| Vec::<String>::try_from(v).ok()).unwrap_or_default();
    Ok(match status {
        0 => Ok(uris.iter().filter_map(|u| url::Url::parse(u).ok()?.to_file_path().ok()).collect()),
        1 => Ok(Vec::new()),
        _ => Err("The file dialog failed.".into()),
    })
}
