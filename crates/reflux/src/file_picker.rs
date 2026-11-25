//! Multi-file picker backed by Rune's `Tx::open_files`.

use std::path::PathBuf;

use crate::shell::{ShellAccess, ShellState};

/// What `then` gets: the picked files (empty when cancelled), or why the dialog failed.
pub type Picked = Result<Vec<PathBuf>, String>;

/// Opens the picker; `then` runs on the UI thread with the answer.
pub fn open_files<S: 'static>(tx: &mut rune::Tx, title: &str, filter: Option<(&str, &[&str])>, acc: ShellAccess<S>, then: impl FnOnce(&mut ShellState, Picked) + 'static) {
    let mut options = rune::FileDialogOptions { title: title.into(), ..Default::default() };
    if let Some((name, ext)) = filter {
        options = options.filter(name, ext);
    }
    tx.open_files(options, move |s: &mut S, r, _| then(acc(s), r.map(Option::unwrap_or_default).map_err(|e| e.to_string())));
}
