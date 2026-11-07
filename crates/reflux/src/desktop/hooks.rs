//! Listen-only keyboard hooks for global shortcuts (Fluxer's `HookBackend`s: x11, windows,
//! macos): every key press and release goes to [`super::shortcuts::key`]; nothing is consumed,
//! so other apps still get the keys. [`start`] says whether one is running.
// shortcut: keyboard only; Fluxer's hooks also see mouse buttons, which our push-to-talk
// recorder can't record anyway.

#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
use super::shortcuts::{GKey, Mods, key};

/// Starts this platform's hook on its own thread; true once it's listening.
pub fn start() -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("reflux-keyhook".into()).spawn(move || platform::run(&tx));
    spawned.is_ok() && rx.recv().unwrap_or(false)
}

/// X11: the RECORD extension's device events (as uiohook does), on a second connection.
#[cfg(target_os = "linux")]
mod platform {
    use x11rb::connection::{Connection, RequestConnection};
    use x11rb::protocol::record::{self, ConnectionExt as _};
    use x11rb::protocol::xproto::ConnectionExt as _;

    use super::{GKey, Mods, key};

    const KEY_PRESS: u8 = 2;
    const KEY_RELEASE: u8 = 3;

    pub fn run(ready: &std::sync::mpsc::Sender<bool>) {
        if let Err(e) = listen(ready) {
            eprintln!("reflux: X11 key hook: {e}");
        }
        let _ = ready.send(false);
    }

    fn listen(ready: &std::sync::mpsc::Sender<bool>) -> Result<(), Box<dyn std::error::Error>> {
        let (ctl, _) = x11rb::connect(None)?;
        if ctl.extension_information(record::X11_EXTENSION_NAME)?.is_none() {
            return Err("no RECORD extension".into());
        }
        let min = ctl.setup().min_keycode;
        let mut keysyms = keymap(&ctl)?;
        let ctx = ctl.generate_id()?;
        let range = record::Range { device_events: record::Range8 { first: KEY_PRESS, last: KEY_RELEASE }, ..Default::default() };
        ctl.record_create_context(ctx, 0, &[record::CS::ALL_CLIENTS.into()], &[range])?.check()?;
        let (data, _) = x11rb::connect(None)?;
        let replies = data.record_enable_context(ctx)?;
        let _ = ready.send(true);
        let mut mods = [0u8; 3]; // Ctrl, Alt, Shift keys down
        for reply in replies {
            let reply = reply?;
            // A new keyboard layout (`MappingNotify`, sent to every client): read the map again.
            while let Some(ev) = ctl.poll_for_event()? {
                if let x11rb::protocol::Event::MappingNotify(m) = ev
                    && m.request == x11rb::protocol::xproto::Mapping::KEYBOARD
                {
                    keysyms = keymap(&ctl)?;
                }
            }
            // Category 0 = FromServer: intercepted events, 32 bytes each.
            if reply.category != 0 {
                continue;
            }
            for ev in reply.data.as_chunks::<32>().0 {
                let down = match ev[0] & 0x7f {
                    KEY_PRESS => true,
                    KEY_RELEASE => false,
                    _ => continue,
                };
                let Some(k) = ev[1].checked_sub(min).and_then(|i| keysyms.get(i as usize)).and_then(|&s| from_keysym(s)) else { continue };
                if let Some(i) = [GKey::Ctrl, GKey::Alt, GKey::Shift].iter().position(|m| *m == k) {
                    mods[i] = if down { mods[i].saturating_add(1) } else { mods[i].saturating_sub(1) };
                }
                let m = Mods { ctrl: mods[0] > 0, alt: mods[1] > 0, shift: mods[2] > 0 };
                key(k, m, down);
            }
        }
        Ok(())
    }

    /// Each keycode's first keysym (`XkbKeycodeToKeysym(…, group 0, level 0)` as uiohook reads it).
    fn keymap(ctl: &x11rb::rust_connection::RustConnection) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
        let (min, max) = (ctl.setup().min_keycode, ctl.setup().max_keycode);
        let map = ctl.get_keyboard_mapping(min, max - min + 1)?.reply()?;
        let per = (map.keysyms_per_keycode as usize).max(1);
        Ok(map.keysyms.chunks(per).map(|c| c[0]).collect())
    }

    /// X keysyms (`X11/keysymdef.h`).
    fn from_keysym(s: u32) -> Option<GKey> {
        Some(match s {
            0x20..=0x7e => GKey::char(char::from(s as u8)),
            0xff08 => GKey::Backspace,
            0xff09 => GKey::Tab,
            0xff0d => GKey::Enter,
            0xff1b => GKey::Escape,
            0xffff => GKey::Delete,
            0xff63 => GKey::Insert,
            0xff50 => GKey::Home,
            0xff57 => GKey::End,
            0xff55 => GKey::PageUp,
            0xff56 => GKey::PageDown,
            0xff51 => GKey::Left,
            0xff52 => GKey::Up,
            0xff53 => GKey::Right,
            0xff54 => GKey::Down,
            0xffbe..=0xffd5 => GKey::F((s - 0xffbe + 1) as u8),
            0xffe1 | 0xffe2 => GKey::Shift,
            0xffe3 | 0xffe4 => GKey::Ctrl,
            0xffe7..=0xffea => GKey::Alt,
            0xffeb | 0xffec => GKey::Super,
            _ => return None,
        })
    }

    #[test]
    fn keysyms_map() {
        assert_eq!(from_keysym(0x4d), Some(GKey::Char('m')));
        assert_eq!(from_keysym(0xffc9), Some(GKey::F(12)));
        assert_eq!(from_keysym(0xffe4), Some(GKey::Ctrl));
        assert_eq!(from_keysym(0x1008ff11), None);
    }
}

/// Windows: a `WH_KEYBOARD_LL` hook, which needs a message loop on its thread.
#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, MAPVK_VK_TO_CHAR, MapVirtualKeyW, VK_CONTROL, VK_MENU, VK_SHIFT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, SetWindowsHookExW, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN};

    use super::{GKey, Mods, key};

    pub fn run(ready: &std::sync::mpsc::Sender<bool>) {
        // SAFETY: Win32 calls on this thread; the hook lives as long as its message loop.
        unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), GetModuleHandleW(std::ptr::null()), 0);
            if hook.is_null() {
                eprintln!("reflux: keyboard hook: {}", std::io::Error::last_os_error());
                let _ = ready.send(false);
                return;
            }
            let _ = ready.send(true);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        }
    }

    unsafe extern "system" fn proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            // SAFETY: for HC_ACTION, `l` points at the event's KBDLLHOOKSTRUCT.
            let vk = unsafe { (*(l as *const KBDLLHOOKSTRUCT)).vkCode };
            if let Some(k) = from_vk(vk) {
                // SAFETY: plain Win32 calls.
                let held = |v: u16| unsafe { GetAsyncKeyState(v as i32) } < 0;
                let down = matches!(w as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                key(k, Mods { ctrl: held(VK_CONTROL), alt: held(VK_MENU), shift: held(VK_SHIFT) }, down);
            }
        }
        // SAFETY: passes the event on unchanged (listen-only).
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, w, l) }
    }

    /// Virtual-key codes; punctuation through the layout's unshifted character.
    fn from_vk(vk: u32) -> Option<GKey> {
        Some(match vk {
            0x41..=0x5a | 0x30..=0x39 => GKey::char(char::from(vk as u8)),
            0x70..=0x87 => GKey::F((vk - 0x70 + 1) as u8),
            0x20 => GKey::Char(' '),
            0x26 => GKey::Up,
            0x28 => GKey::Down,
            0x25 => GKey::Left,
            0x27 => GKey::Right,
            0x1b => GKey::Escape,
            0x09 => GKey::Tab,
            0x0d => GKey::Enter,
            0x08 => GKey::Backspace,
            0x2e => GKey::Delete,
            0x2d => GKey::Insert,
            0x24 => GKey::Home,
            0x23 => GKey::End,
            0x21 => GKey::PageUp,
            0x22 => GKey::PageDown,
            0x10 | 0xa0 | 0xa1 => GKey::Shift,
            0x11 | 0xa2 | 0xa3 => GKey::Ctrl,
            0x12 | 0xa4 | 0xa5 => GKey::Alt,
            0x5b | 0x5c => GKey::Super,
            _ => {
                // SAFETY: a plain Win32 call; the high bit marks a dead key.
                let c = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) } & 0x7fff_ffff;
                return char::from_u32(c).filter(|c| c.is_ascii_graphic()).map(GKey::char);
            }
        })
    }
}

/// macOS: a listen-only CGEventTap on its own run loop. It needs the Input Monitoring
/// permission (`CGRequestListenEventAccess` asks for it once); without it no tap is made.
#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicPtr, Ordering};

    use super::{GKey, Mods, key};

    type Ref = *mut c_void;
    type Callback = unsafe extern "C" fn(proxy: Ref, ty: u32, event: Ref, info: Ref) -> Ref;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: Callback, info: Ref) -> Ref;
        fn CGEventTapEnable(tap: Ref, enable: bool);
        fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
        fn CGEventGetFlags(event: Ref) -> u64;
        fn CGRequestListenEventAccess() -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFMachPortCreateRunLoopSource(allocator: *const c_void, port: Ref, order: isize) -> Ref;
        fn CFRunLoopGetCurrent() -> Ref;
        fn CFRunLoopAddSource(rl: Ref, source: Ref, mode: *const c_void);
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: *const c_void;
    }

    const SESSION_TAP: u32 = 1;
    const HEAD_INSERT: u32 = 0;
    const LISTEN_ONLY: u32 = 1;
    const KEY_DOWN: u32 = 10;
    const KEY_UP: u32 = 11;
    const FLAGS_CHANGED: u32 = 12;
    const DISABLED_BY_TIMEOUT: u32 = 0xffff_fffe;
    const DISABLED_BY_USER_INPUT: u32 = 0xffff_ffff;
    const KEYCODE_FIELD: u32 = 9;
    const SHIFT: u64 = 0x2_0000;
    const CONTROL: u64 = 0x4_0000;
    const OPTION: u64 = 0x8_0000;
    const COMMAND: u64 = 0x10_0000;

    static TAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

    pub fn run(ready: &std::sync::mpsc::Sender<bool>) {
        let mask = (1u64 << KEY_DOWN) | (1 << KEY_UP) | (1 << FLAGS_CHANGED);
        // SAFETY: CoreGraphics / CoreFoundation calls on this thread, whose run loop owns the tap.
        unsafe {
            let tap = CGEventTapCreate(SESSION_TAP, HEAD_INSERT, LISTEN_ONLY, mask, callback, std::ptr::null_mut());
            if tap.is_null() {
                CGRequestListenEventAccess();
                eprintln!("reflux: global shortcuts need the Input Monitoring permission");
                let _ = ready.send(false);
                return;
            }
            TAP.store(tap, Ordering::Relaxed);
            let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            let _ = ready.send(true);
            CFRunLoopRun();
        }
    }

    unsafe extern "C" fn callback(_proxy: Ref, ty: u32, event: Ref, _info: Ref) -> Ref {
        if ty == DISABLED_BY_TIMEOUT || ty == DISABLED_BY_USER_INPUT {
            // SAFETY: the tap made in `run`.
            unsafe { CGEventTapEnable(TAP.load(Ordering::Relaxed), true) };
            return event;
        }
        // SAFETY: `event` is the event being delivered.
        let (code, flags) = unsafe { (CGEventGetIntegerValueField(event, KEYCODE_FIELD) as u16, CGEventGetFlags(event)) };
        if let Some(k) = from_keycode(code) {
            let down = match ty {
                KEY_DOWN => true,
                KEY_UP => false,
                // A modifier: down while its flag is set.
                FLAGS_CHANGED => {
                    flags
                        & match k {
                            GKey::Shift => SHIFT,
                            GKey::Ctrl => CONTROL,
                            GKey::Alt => OPTION,
                            _ => COMMAND,
                        }
                        != 0
                }
                _ => return event,
            };
            // `ctrlOrMeta` is Cmd here.
            key(k, Mods { ctrl: flags & COMMAND != 0, alt: flags & OPTION != 0, shift: flags & SHIFT != 0 }, down);
        }
        event
    }

    /// `kVK_*` virtual key codes (ANSI positions).
    fn from_keycode(code: u16) -> Option<GKey> {
        const ANSI: &[(u16, char)] = &[
            (0, 'a'), (1, 's'), (2, 'd'), (3, 'f'), (4, 'h'), (5, 'g'), (6, 'z'), (7, 'x'), (8, 'c'), (9, 'v'), (11, 'b'), (12, 'q'),
            (13, 'w'), (14, 'e'), (15, 'r'), (16, 'y'), (17, 't'), (18, '1'), (19, '2'), (20, '3'), (21, '4'), (22, '6'), (23, '5'),
            (24, '='), (25, '9'), (26, '7'), (27, '-'), (28, '8'), (29, '0'), (30, ']'), (31, 'o'), (32, 'u'), (33, '['), (34, 'i'),
            (35, 'p'), (37, 'l'), (38, 'j'), (39, '\''), (40, 'k'), (41, ';'), (42, '\\'), (43, ','), (44, '/'), (45, 'n'), (46, 'm'),
            (47, '.'), (49, ' '), (50, '`'),
        ];
        const F: &[u16] = &[122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64, 79, 80, 90];
        if let Some(&(_, c)) = ANSI.iter().find(|(k, _)| *k == code) {
            return Some(GKey::Char(c));
        }
        if let Some(i) = F.iter().position(|&k| k == code) {
            return Some(GKey::F(i as u8 + 1));
        }
        Some(match code {
            36 => GKey::Enter,
            48 => GKey::Tab,
            51 => GKey::Backspace,
            53 => GKey::Escape,
            117 => GKey::Delete,
            114 => GKey::Insert,
            115 => GKey::Home,
            119 => GKey::End,
            116 => GKey::PageUp,
            121 => GKey::PageDown,
            123 => GKey::Left,
            124 => GKey::Right,
            125 => GKey::Down,
            126 => GKey::Up,
            56 | 60 => GKey::Shift,
            59 | 62 => GKey::Ctrl,
            58 | 61 => GKey::Alt,
            54 | 55 => GKey::Super,
            _ => return None,
        })
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod platform {
    pub fn run(ready: &std::sync::mpsc::Sender<bool>) {
        let _ = ready.send(false);
    }
}
