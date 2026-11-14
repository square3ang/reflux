//! File drops on Wayland, which winit 0.30 doesn't report (Rune's `on_drop_files` covers X11,
//! Windows and macOS): a `wl_data_device` of our own, on winit's connection, takes
//! `text/uri-list` drags into the open chat's upload tray (`shell::chat::drop_files`).
// shortcut: a drop anywhere on the window goes to the open channel (Fluxer's target is the chat
// column), and it needs libwayland 1.23+ (`wl_proxy_get_display`); drop this once winit reports
// Wayland drops.

use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;

use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_data_device, wl_data_device_manager as manager, wl_data_offer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};

use crate::State;

const URIS: &str = "text/uri-list";

static APP: Mutex<Option<rune::AppHandle<State>>> = Mutex::new(None);

/// `desktop::start`: the handle drops are delivered through.
pub fn set_app(ui: rune::AppHandle<State>) {
    *APP.lock().unwrap_or_else(|e| e.into_inner()) = Some(ui);
}

/// winit's `wl_display`, from the window's surface.
static DISPLAY: Mutex<Option<usize>> = Mutex::new(None);

/// Our data device's objects. Kept forever: the `Connection` wraps winit's display, and dropping
/// it tears that down under winit's feet (it crashed in `wl_proxy_destroy`).
static WL: Mutex<Option<Wl>> = Mutex::new(None);

struct Wl {
    conn: Connection,
    queue: wayland_client::EventQueue<Drops>,
    drops: Drops,
}

/// `App::on_window`: notes the display; [`pump`] makes the objects.
pub fn on_window(w: &rune::WindowHandle) {
    use rune::raw_window_handle::RawWindowHandle;
    let Ok(Some(surface)) = w.with_raw_handle(|h| match h.as_raw() {
        RawWindowHandle::Wayland(h) => Some(h.surface.as_ptr() as usize),
        _ => None,
    }) else {
        return;
    };
    match display_of(surface as *mut libc::c_void) {
        Some(d) => *DISPLAY.lock().unwrap_or_else(|e| e.into_inner()) = Some(d as usize),
        None => eprintln!("reflux: file drops: libwayland has no wl_proxy_get_display (needs 1.23)"),
    }
}

/// Delivers what the compositor sent for our data device, from the frame's poll on winit's own
/// thread: a second thread reading the same socket raced winit's reads and crashed.
pub fn pump() {
    let Some(display) = *DISPLAY.lock().unwrap_or_else(|e| e.into_inner()) else { return };
    let mut wl = WL.lock().unwrap_or_else(|e| e.into_inner());
    if wl.is_none() {
        let Some(ui) = APP.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
        match start(display as *mut libc::c_void, ui) {
            Ok(w) => *wl = Some(w),
            Err(e) => {
                eprintln!("reflux: file drops: {e}");
                return;
            }
        }
    }
    let Some(w) = wl.as_mut() else { return };
    if let Err(e) = w.queue.dispatch_pending(&mut w.drops) {
        eprintln!("reflux: file drops: {e}");
    }
    let _ = w.conn.flush();
}

/// The `wl_display` a proxy of winit's belongs to, through the already loaded libwayland.
fn display_of(proxy: *mut libc::c_void) -> Option<*mut libc::c_void> {
    // SAFETY: dlopen/dlsym with NUL-terminated names; the symbol, when there, has this C signature,
    // and `proxy` is winit's live `wl_surface`.
    unsafe {
        let lib = libc::dlopen(c"libwayland-client.so.0".as_ptr(), libc::RTLD_LAZY | libc::RTLD_NOLOAD);
        if lib.is_null() {
            return None;
        }
        let f = libc::dlsym(lib, c"wl_proxy_get_display".as_ptr());
        if f.is_null() {
            return None;
        }
        let f: unsafe extern "C" fn(*mut libc::c_void) -> *mut libc::c_void = std::mem::transmute(f);
        let d = f(proxy);
        (!d.is_null()).then_some(d)
    }
}

fn start(display: *mut libc::c_void, ui: rune::AppHandle<State>) -> Result<Wl, String> {
    // SAFETY: winit owns the display for the life of the app; our objects go on a queue of our
    // own, and libwayland queues other queues' events rather than dropping them.
    let conn = Connection::from_backend(unsafe { Backend::from_foreign_display(display.cast()) });
    let (globals, queue) = registry_queue_init::<Drops>(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).map_err(|e| e.to_string())?;
    let m: manager::WlDataDeviceManager = globals.bind(&qh, 1..=3, ()).map_err(|e| e.to_string())?;
    m.get_data_device(&seat, &qh, ());
    let drops = Drops { ui, conn: conn.clone(), uris: HashSet::new(), current: None };
    Ok(Wl { conn, queue, drops })
}

struct Drops {
    ui: rune::AppHandle<State>,
    conn: Connection,
    /// Offers that carry `text/uri-list`.
    uris: HashSet<ObjectId>,
    /// The offer being dragged over us, when it has files.
    current: Option<wl_data_offer::WlDataOffer>,
}

impl Drops {
    fn hover(&self, over: bool) {
        self.ui.update(move |s, _| {
            if s.app.route == crate::app::Route::Main {
                s.app.shell.drop_hover = over;
            }
        });
    }

    fn forget(&mut self, offer: &wl_data_offer::WlDataOffer) {
        self.uris.remove(&offer.id());
        offer.destroy();
    }
}

/// The files of a `text/uri-list` (RFC 2483): `file:` URIs, skipping comments.
fn paths(list: &str) -> Vec<PathBuf> {
    list.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).filter_map(|l| url::Url::parse(l).ok()?.to_file_path().ok()).collect()
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for Drops {
    fn event(d: &mut Drops, _: &wl_data_device::WlDataDevice, e: wl_data_device::Event, _: &(), _: &Connection, _: &QueueHandle<Drops>) {
        use wl_data_device::Event;
        match e {
            Event::Enter { serial, id: Some(offer), .. } => {
                if d.uris.contains(&offer.id()) {
                    offer.accept(serial, Some(URIS.into()));
                    if offer.version() >= 3 {
                        offer.set_actions(manager::DndAction::Copy, manager::DndAction::Copy);
                    }
                    d.current = Some(offer);
                    d.hover(true);
                } else {
                    offer.accept(serial, None);
                    d.forget(&offer);
                }
            }
            Event::Leave => {
                if let Some(o) = d.current.take() {
                    d.forget(&o);
                    d.hover(false);
                }
            }
            Event::Drop => {
                let Some(o) = d.current.take() else { return };
                let mut list = String::new();
                if let Ok((mut read, write)) = std::io::pipe() {
                    o.receive(URIS.into(), std::os::fd::AsFd::as_fd(&write));
                    let _ = d.conn.flush();
                    drop(write);
                    let _ = read.read_to_string(&mut list);
                }
                if o.version() >= 3 {
                    o.finish();
                }
                d.forget(&o);
                let files = paths(&list);
                d.ui.update(move |s, _| {
                    let sh = &mut s.app.shell;
                    sh.drop_hover = false;
                    let Some(c) = sh.channel.and_then(|id| s.app.store.channels.get(&id)) else { return };
                    if s.app.route != crate::app::Route::Main || files.is_empty() {
                        return;
                    }
                    let (send, attach) = (crate::shell::gating::can_send(&s.app.store, sh, c), crate::shell::gating::can_attach(&s.app.store, sh, c));
                    crate::shell::chat::drop_files(sh, send, attach, &files);
                });
            }
            // The clipboard's offers come here too; Rune reads the clipboard itself.
            Event::Selection { id: Some(o) } => d.forget(&o),
            _ => {}
        }
    }

    event_created_child!(Drops, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, ()),
    ]);
}

impl Dispatch<wl_data_offer::WlDataOffer, ()> for Drops {
    fn event(d: &mut Drops, o: &wl_data_offer::WlDataOffer, e: wl_data_offer::Event, _: &(), _: &Connection, _: &QueueHandle<Drops>) {
        if let wl_data_offer::Event::Offer { mime_type } = e
            && mime_type == URIS
        {
            d.uris.insert(o.id());
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Drops {
    fn event(_: &mut Drops, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Drops>) {}
}
impl Dispatch<wl_seat::WlSeat, ()> for Drops {
    fn event(_: &mut Drops, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Drops>) {}
}
impl Dispatch<manager::WlDataDeviceManager, ()> for Drops {
    fn event(_: &mut Drops, _: &manager::WlDataDeviceManager, _: manager::Event, _: &(), _: &Connection, _: &QueueHandle<Drops>) {}
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_file_uris() {
        let list = "# from nautilus\r\nfile:///home/a/My%20Pic.png\r\nhttps://x.y/z\r\n\r\nfile:///tmp/b.txt\r\n";
        assert_eq!(super::paths(list), [std::path::PathBuf::from("/home/a/My Pic.png"), "/tmp/b.txt".into()]);
    }
}
