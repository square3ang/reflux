//! Community management: the guild header menu, add/join community, invites, channel creation,
//! notification settings, and the community and channel settings overlays.
//!
//! Views only queue [`Intent::Api`] calls; the runtime (`app/guild.rs`) sends them and applies
//! the [`Then`] when they answer. Most effects arrive over the gateway (GUILD_UPDATE,
//! GUILD_ROLE_*, CHANNEL_*), so the store stays the source of truth.

mod channel;
mod forum;
mod menu;
mod modals;
mod perms;
mod settings;
mod stickers;

use std::collections::HashMap;

use reflux_model::{Snowflake, Permissions};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, FileDialogOptions, TextEdit, Tx, text};
use serde_json::Value;

use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Input, mix};

pub use channel::first_tab as first_channel_tab;
pub use settings::tab_ids;
pub use menu::{HEADER_MENU, first_settings_tab, header_menu, invitable_channel, open_invite};
pub use modals::parse_invite;

/// The management dialog that's open (one at a time).
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    /// `AddGuildModal`.
    Add(AddView),
    /// `InviteModal` for a channel; `true` shows the link settings.
    Invite(Snowflake, bool),
    /// `ChannelCreateModal` / `CategoryCreateModal`.
    CreateChannel { guild: Snowflake, parent: Option<Snowflake>, category: bool },
    /// `GuildNotificationSettingsModal`.
    Notifications(Snowflake),
    /// `GuildPrivacySettingsModal`.
    Privacy(Snowflake),
    /// `GuildSettingsModal`; `role` is the role being edited on the Roles tab.
    Guild { guild: Snowflake, tab: &'static str, role: Option<Snowflake> },
    /// `ChannelSettingsModal`; `target` is the role or member whose overwrite is being edited.
    Channel { channel: Snowflake, tab: &'static str, target: Option<Snowflake> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddView {
    Landing,
    Create,
    Join,
}

/// A destructive action waiting on its confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    DeleteGuild(Snowflake),
    DeleteChannel(Snowflake),
    DeleteRole(Snowflake, Snowflake),
    Kick(Snowflake, Snowflake),
    Ban(Snowflake, Snowflake),
}

/// The open dialog's form: text fields and loaded or picked values by name.
#[derive(Default)]
pub struct Form {
    pub text: HashMap<String, TextEdit>,
    pub data: HashMap<String, Value>,
    pub busy: bool,
    pub error: Option<String>,
    pub confirm: Option<Confirm>,
}

impl Form {
    /// A field's text, or `initial` while it hasn't been touched.
    pub fn value(&self, key: &str, initial: &str) -> String {
        self.text.get(key).map_or_else(|| initial.to_string(), |t| t.text.clone())
    }
}

/// An API request a view asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub method: Method,
    pub path: String,
    pub body: Option<Value>,
    pub then: Then,
}

/// What to do once a [`Call`] succeeds (a failure shows on the dialog as `form.error`).
#[derive(Clone, Debug, PartialEq)]
pub enum Then {
    /// Effects arrive over the gateway.
    Nothing,
    /// Close the dialog.
    Close,
    /// Keep the response as `form.data[key]`.
    Keep(String),
    /// GET `path` again into `form.data[key]` (a list the call changed).
    Reload(String, String),
    /// Open the guild or channel the response is (a created guild or channel, an accepted invite)
    /// and close the dialog.
    Enter,
    /// Edit the role or overwrite the response is.
    Select,
    /// Drop the form's fields and picks whose keys start with `clear` (a saved editor), then GET
    /// `reload` (key, path) into `form.data`.
    Saved { clear: String, reload: Option<(String, String)> },
    /// The response carries a new session `token` (a password change ended every session).
    Token,
    /// The account is gone from this device (disabled or deleted): sign out locally.
    SignOut,
}

impl ShellState {
    pub fn open_dialog(&mut self, d: Dialog) {
        self.popups.close();
        self.form = Form::default();
        self.dialog = Some(d);
    }

    pub fn api(&mut self, method: Method, path: impl Into<String>, body: Option<Value>, then: Then) {
        self.intents.push(Intent::Api(Call { method, path: path.into(), body, then }));
    }

    /// GETs `path` into `form.data[key]`.
    pub fn load(&mut self, key: &str, path: impl Into<String>) {
        self.api(Method::GET, path, None, Then::Keep(key.into()));
    }

    /// Opens community settings on `tab`, loading what it lists.
    pub fn open_guild_settings(&mut self, guild: Snowflake, tab: &'static str) {
        self.open_dialog(Dialog::Guild { guild, tab, role: None });
        settings::load_tab(self, guild, tab);
    }
}

/// Every overlay this module draws for the shell.
pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if st.popups.is_open(HEADER_MENU)
        && let Some(g) = st.guild
    {
        header_menu(store, st, g, p, acc, layer);
    }
    let Some(d) = &st.dialog else { return };
    match *d {
        Dialog::Guild { guild, tab, role } => settings::view(store, st, guild, tab, role, p, acc, layer),
        Dialog::Channel { channel, tab, target } => channel::view(store, st, channel, tab, target, p, acc, layer),
        _ => modals::view(store, st, d, p, acc, layer),
    }
}

// ---- Shared bits ---------------------------------------------------------------------------

pub(crate) fn can(store: &Store, guild: Snowflake, p: Permissions) -> bool {
    store.guild_permissions(guild).intersects(p)
}

/// A form text field showing what's typed so far, else `initial`.
fn field<S: 'static>(st: &ShellState, acc: ShellAccess<S>, key: &'static str, initial: &str, label: &str, placeholder: &str, p: &Palette) -> El {
    let edit = st.form.text.get(key).cloned().unwrap_or_else(|| TextEdit::new(initial));
    let init = initial.to_string();
    let el_key = format!("form-{key}");
    let mut i = Input::new(&el_key, &edit, move |s: &mut S| -> &mut TextEdit {
        acc(s).form.text.entry(key.to_string()).or_insert_with(|| TextEdit::new(init.clone()))
    })
    .placeholder(placeholder);
    if !label.is_empty() {
        i = i.label(label);
    }
    i.el(p)
}

/// `form.error` in the danger colour, if any.
fn error(st: &ShellState, p: &Palette) -> Option<El> {
    st.form.error.as_ref().map(|e| text(e.clone()).font(font(14.0, 400.0).lh(20.0)).color(mix(p.status_danger, p.text_primary, 78.0)).wrap())
}

/// A press handler that asks for an image and keeps it in `form.data[key]` as a data URL.
pub(crate) fn pick_image<S: 'static>(acc: ShellAccess<S>, key: &'static str) -> impl Fn(&mut S, &mut Tx) + 'static {
    move |_: &mut S, tx: &mut Tx| {
        let options = FileDialogOptions { title: "Choose an image".into(), ..Default::default() }.filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "avif"]);
        tx.open_file(options, move |s: &mut S, r, _| {
            let form = &mut acc(s).form;
            match r.map_err(|e| e.to_string()).and_then(|p| p.map(|p| std::fs::read(&p).map(|b| data_url(&p, &b)).map_err(|e| e.to_string())).transpose()) {
                Ok(Some(url)) => {
                    form.data.insert(key.into(), Value::String(url));
                }
                Ok(None) => {}
                Err(e) => form.error = Some(format!("Couldn't open the image: {e}")),
            }
        });
    }
}

/// `data:<mime>;base64,…`, the image form the API takes (`guilds.mdx`: raw base64 or a data URL).
pub(crate) fn data_url(path: &std::path::Path, bytes: &[u8]) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        _ => "image/png",
    };
    format!("data:{mime};base64,{}", base64(bytes))
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// "#rrggbb" ↔ `0xRRGGBB`.
pub(crate) fn hex(c: u32) -> String {
    format!("#{c:06x}")
}

pub(crate) fn parse_hex(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('#');
    (s.len() == 6).then(|| u32::from_str_radix(s, 16).ok()).flatten()
}

/// The API error's message for the dialog.
pub fn describe(e: &reflux_net::Error) -> String {
    match e {
        reflux_net::Error::Api { error, .. } if !error.message.is_empty() => error.message.clone(),
        e => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_pads_like_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(data_url(std::path::Path::new("a.JPG"), b"hi"), "data:image/jpeg;base64,aGk=");
    }

    #[test]
    fn hex_colours() {
        assert_eq!(hex(0x2ecc71), "#2ecc71");
        assert_eq!(parse_hex("#2ECC71"), Some(0x2ecc71));
        assert_eq!(parse_hex("2ecc7"), None);
    }
}
