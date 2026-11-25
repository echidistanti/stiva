//! What only the operating system can do: a notification, the progress on the taskbar button, sleep and
//! shutdown, showing a file in the file manager. On Windows these go through Windows Runtime and the shell;
//! on Linux through D-Bus (the notification server, systemd-logind, the file manager). The taskbar progress is
//! Windows only. In the tests nothing here touches the machine: a test run that put the PC to sleep would
//! be no favour.

use crate::after::TaskbarState;
use eframe::egui;

/// Shows a notification under Stiva's own name. Fire and forget: if the system will not show it, nothing happens.
pub fn notify(title: &str, body: &str) {
    if !cfg!(test) {
        imp::notify(title, body);
    }
}

/// Puts the PC to sleep. Returns false when the system did not accept it.
pub fn sleep() -> bool {
    !cfg!(test) && imp::sleep()
}

/// Shuts the PC down at once, without forcing programs that have something unsaved to close.
pub fn shutdown() -> bool {
    !cfg!(test) && imp::shutdown()
}

/// Opens the folder that holds `path` with the file selected, where the file manager can do that.
#[cfg(not(windows))]
pub fn reveal(path: &std::path::Path) {
    if !cfg!(test) {
        imp::reveal(path);
    }
}

/// Whether to open the window through X11 on a Wayland desktop, which only `STIVA_X11=1` asks for. Dropping a
/// file onto the window works only there: the window library hands over dropped files through X11 alone, so
/// under plain Wayland the drop never arrives. XWayland carries it, but on COSMIC the window flashes now and
/// then, so the default is native Wayland and the drop is given up.
#[cfg(not(windows))]
pub fn use_x11() -> bool {
    x11_wanted(wayland_session(), env_set("DISPLAY"), env_set("STIVA_X11"))
}

#[cfg(not(windows))]
fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

#[cfg(not(windows))]
fn wayland_session() -> bool {
    env_set("WAYLAND_DISPLAY") || env_set("WAYLAND_SOCKET")
}

/// Whether a file can be dropped onto the window: always on Windows and on X11, never on a native Wayland
/// window. The texts that invite a drop are shown only where it works.
pub fn drop_works() -> bool {
    #[cfg(windows)]
    {
        true
    }
    #[cfg(not(windows))]
    {
        static WORKS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *WORKS.get_or_init(|| drop_possible(wayland_session(), use_x11()))
    }
}

#[cfg(not(windows))]
fn drop_possible(wayland_session: bool, x11_chosen: bool) -> bool {
    !wayland_session || x11_chosen
}

#[cfg(not(windows))]
fn x11_wanted(wayland_session: bool, x11_available: bool, asked_for: bool) -> bool {
    wayland_session && x11_available && asked_for
}

/// The progress shown on Stiva's button in the taskbar.
#[derive(Default)]
pub struct Taskbar {
    #[cfg(windows)]
    list: Option<windows::Win32::UI::Shell::ITaskbarList3>,
    shown: Option<TaskbarState>,
}

impl Taskbar {
    /// Shows `state`, if it is not what is shown already.
    #[cfg_attr(not(windows), allow(unused_variables))]
    pub fn set(&mut self, frame: &eframe::Frame, state: TaskbarState) {
        let state = Some(state).filter(|s| *s != TaskbarState::None);
        if self.shown == state || (state.is_none() && self.shown.is_none()) {
            return;
        }
        #[cfg(windows)]
        {
            let Some(hwnd) = imp::hwnd(frame) else { return };
            let state = state.unwrap_or(TaskbarState::None);
            if imp::set_progress(&mut self.list, hwnd, state) {
                self.shown = Some(state).filter(|s| *s != TaskbarState::None);
            }
        }
        #[cfg(not(windows))]
        {
            self.shown = state;
        }
    }

    /// What was last shown, for the tests and the screenshots.
    #[cfg(test)]
    pub fn shown(&self) -> Option<TaskbarState> {
        self.shown
    }
}

/// Whether the window has the keyboard focus, to leave out a notification for a window the user is looking at.
pub fn focused(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().focused.unwrap_or(true))
}

/// The id Windows files Stiva's notifications under. The installer gives the shortcuts the same one.
#[cfg_attr(not(windows), allow(dead_code))]
const APP_ID: &str = "Stiva";

/// The notification as Windows wants it: XML with two lines of text. The texts are escaped, so a file name
/// with a `<` or an `&` in it stays text.
#[cfg_attr(not(windows), allow(dead_code))]
fn toast_xml(title: &str, body: &str) -> String {
    fn escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                // Characters XML cannot hold at all.
                c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
                c => out.push(c),
            }
        }
        out
    }
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escape(title),
        escape(body)
    )
}

/// The text of a notification for a server that reads a little markup: `&`, `<` and `>` stay what they are.
#[cfg_attr(windows, allow(dead_code))]
fn markup_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `file:///home/me/My%20Videos/a.mkv`: the address a file manager is asked to show.
#[cfg_attr(windows, allow(dead_code))]
fn file_uri(path: &std::path::Path) -> String {
    use std::fmt::Write;
    let mut out = String::from("file://");
    #[cfg(unix)]
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()).to_vec();
    #[cfg(not(unix))]
    let bytes = path.to_string_lossy().into_owned().into_bytes();
    for b in bytes {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => out.push(b as char),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[cfg(not(windows))]
mod imp {
    use std::collections::HashMap;
    use std::path::Path;
    use zbus::blocking::Connection;
    use zbus::zvariant::Value;

    const NOTIFICATIONS: &str = "org.freedesktop.Notifications";

    /// Sends the notification from a thread of its own, so the window never waits for the bus.
    pub fn notify(title: &str, body: &str) {
        let (title, body) = (super::markup_escape(title), super::markup_escape(body));
        let _ = std::thread::Builder::new().name("notify".into()).spawn(move || {
            let _ = show(&title, &body);
        });
    }

    pub(super) fn show(title: &str, body: &str) -> zbus::Result<()> {
        // The program's icon as a file: an AppImage puts none in the icon theme.
        let icon = crate::deps::data_dir().join("icon.png");
        if !icon.is_file() {
            let _ = std::fs::create_dir_all(crate::deps::data_dir());
            let _ = std::fs::write(&icon, crate::icon::PNG);
        }
        let icon = icon.to_str().filter(|_| icon.is_file()).unwrap_or("stiva").to_owned();
        // "desktop-entry" lets the shell tie the notification to Stiva's launcher when there is one.
        let hints: HashMap<&str, Value<'_>> = HashMap::from([("desktop-entry", Value::from("stiva"))]);
        Connection::session()?.call_method(
            Some(NOTIFICATIONS),
            "/org/freedesktop/Notifications",
            Some(NOTIFICATIONS),
            "Notify",
            &("Stiva", 0u32, icon.as_str(), title, body, Vec::<&str>::new(), hints, -1i32),
        )?;
        Ok(())
    }

    /// Asks systemd-logind (or elogind), as the desktop's own power menu does. Not interactive: when the
    /// system wants a password for it (somebody else is logged in), the answer is no and Stiva says so.
    fn logind(method: &str) -> bool {
        Connection::system()
            .and_then(|c| {
                c.call_method(
                    Some("org.freedesktop.login1"),
                    "/org/freedesktop/login1",
                    Some("org.freedesktop.login1.Manager"),
                    method,
                    &(false,),
                )
            })
            .is_ok()
    }

    pub fn sleep() -> bool {
        logind("Suspend")
    }

    pub fn shutdown() -> bool {
        logind("PowerOff")
    }

    /// Asks the file manager to show the file, selected; failing that, opens its folder. From a thread of its
    /// own: a file manager that has to start first would hold the window up.
    pub fn reveal(path: &Path) {
        let path = path.to_path_buf();
        let _ = std::thread::Builder::new().name("reveal".into()).spawn(move || {
            if show_item(&path).is_err() {
                crate::app::open_path(path.parent().unwrap_or(&path));
            }
        });
    }

    pub(super) fn show_item(path: &Path) -> zbus::Result<()> {
        Connection::session()?.call_method(
            Some("org.freedesktop.FileManager1"),
            "/org/freedesktop/FileManager1",
            Some("org.freedesktop.FileManager1"),
            "ShowItems",
            &(vec![super::file_uri(path)], ""),
        )?;
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use super::{APP_ID, toast_xml};
    use crate::after::TaskbarState;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::process::Command;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::Win32::UI::Shell::{ITaskbarList3, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TBPF_PAUSED, TaskbarList};
    use windows::core::HSTRING;

    /// Shows the notification from a thread of its own, so the window never waits for Windows.
    pub fn notify(title: &str, body: &str) {
        let (title, body) = (title.to_owned(), body.to_owned());
        let _ = std::thread::Builder::new().name("notify".into()).spawn(move || {
            let _ = show(&title, &body);
        });
    }

    fn show(title: &str, body: &str) -> windows::core::Result<()> {
        // The program's icon as a file, which is what the notification wants to show next to the name.
        let icon = crate::deps::data_dir().join("icon.png");
        if !icon.is_file() {
            let _ = std::fs::create_dir_all(crate::deps::data_dir());
            let _ = std::fs::write(&icon, crate::icon::PNG);
        }
        register_identity(&icon);
        unsafe {
            // This thread has not used Windows Runtime yet. "Already initialised" is fine.
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let xml = XmlDocument::new()?;
        xml.LoadXml(&HSTRING::from(toast_xml(title, body)))?;
        let toast = ToastNotification::CreateToastNotification(&xml)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
    }

    /// Unpackaged programs have no identity for Windows to attach a notification to, so Stiva registers one
    /// for the current user (HKCU only): a name and an icon under the id "Stiva". The installer does the same.
    fn register_identity(icon: &std::path::Path) {
        let Ok(key) = windows_registry::CURRENT_USER.create(format!(r"Software\Classes\AppUserModelId\{APP_ID}")) else { return };
        let _ = key.set_string("DisplayName", "Stiva");
        if let Some(icon) = icon.to_str().filter(|_| icon.is_file()) {
            let _ = key.set_string("IconUri", icon);
        }
    }

    pub fn sleep() -> bool {
        #[link(name = "powrprof")]
        unsafe extern "system" {
            fn SetSuspendState(hibernate: u8, force: u8, disable_wake_events: u8) -> u8;
        }
        // Sleep, not hibernate, and without forcing: programs may still say no.
        unsafe { SetSuspendState(0, 0, 0) != 0 }
    }

    pub fn shutdown() -> bool {
        let mut cmd = Command::new("shutdown.exe");
        cmd.args(["/s", "/t", "0"]);
        crate::deps::quiet(&mut cmd);
        cmd.spawn().is_ok()
    }

    pub fn hwnd(frame: &eframe::Frame) -> Option<isize> {
        match frame.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
            _ => None,
        }
    }

    /// Returns whether the taskbar took it. The COM object is made on first use, on this (the UI) thread.
    pub fn set_progress(list: &mut Option<ITaskbarList3>, hwnd: isize, state: TaskbarState) -> bool {
        if list.is_none() {
            unsafe {
                // winit has set COM up already; this only matters if it has not. "Already initialised" is fine.
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                let made: windows::core::Result<ITaskbarList3> = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER);
                match made {
                    Ok(l) if l.HrInit().is_ok() => *list = Some(l),
                    _ => return false,
                }
            }
        }
        let Some(l) = list.as_ref() else { return false };
        let hwnd = HWND(hwnd as *mut std::ffi::c_void);
        let done = unsafe {
            match state {
                TaskbarState::None => l.SetProgressState(hwnd, TBPF_NOPROGRESS),
                TaskbarState::Indeterminate => l.SetProgressState(hwnd, TBPF_INDETERMINATE),
                TaskbarState::Normal(t) => {
                    l.SetProgressState(hwnd, TBPF_NORMAL).and_then(|()| l.SetProgressValue(hwnd, u64::from(t), 1000))
                }
                TaskbarState::Paused(t) => {
                    l.SetProgressState(hwnd, TBPF_PAUSED).and_then(|()| l.SetProgressValue(hwnd, u64::from(t), 1000))
                }
            }
        };
        if done.is_err() {
            // The button may not exist yet, or Explorer restarted: start from scratch next time.
            *list = None;
        }
        done.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_has_both_texts_and_they_cannot_break_out_of_the_xml() {
        let xml = toast_xml("Encoding finished", "2 files done");
        assert!(xml.contains("<text>Encoding finished</text><text>2 files done</text>"));
        let xml = toast_xml("a<b>&c", "\"x\" 'y'\u{1}");
        assert!(xml.contains("<text>a&lt;b&gt;&amp;c</text>"));
        assert!(xml.contains("&quot;x&quot; &apos;y&apos;</text>"), "{xml}");
        assert_eq!(xml.matches("<text>").count(), 2, "nothing a file name holds can open a new element");
    }

    #[cfg(not(windows))]
    #[test]
    fn a_drop_works_everywhere_except_in_a_native_wayland_window() {
        assert!(drop_possible(false, false), "an X11 session");
        assert!(drop_possible(true, true), "Wayland, opened through X11");
        assert!(!drop_possible(true, false), "a native Wayland window never gets the drop");
    }

    #[test]
    fn x11_is_chosen_only_on_wayland_with_xwayland_and_when_asked_for() {
        assert!(x11_wanted(true, true, true), "STIVA_X11 on Wayland with XWayland: X11, so that dropped files arrive");
        assert!(!x11_wanted(true, true, false), "by default native Wayland stays");
        assert!(!x11_wanted(true, false, true), "no X server to go through: stay on Wayland");
        assert!(!x11_wanted(false, true, true), "a plain X11 session needs no detour");
    }

    #[test]
    fn in_the_tests_the_power_actions_refuse_and_the_taskbar_just_remembers() {
        assert!(!sleep() && !shutdown());
        let t = Taskbar::default();
        assert_eq!(t.shown(), None);
    }

    #[test]
    fn notification_text_cannot_open_a_tag() {
        assert_eq!(markup_escape("3 files done · -41%"), "3 files done · -41%");
        assert_eq!(markup_escape("a<b>&c"), "a&lt;b&gt;&amp;c");
    }

    /// Needs a session bus with a notification server and a file manager on it; the sandbox runs it against a
    /// stand-in for both (`dbus-run-session`), the CI does not.
    #[cfg(unix)]
    #[test]
    #[ignore = "needs a session bus with a notification server and a file manager"]
    fn a_notification_and_a_reveal_reach_the_bus() {
        imp::show("Encoding finished", "2 files done · -41% & <more>").unwrap();
        imp::show_item(std::path::Path::new("/home/me/My Videos/a b.mkv")).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_path_becomes_an_address_a_file_manager_understands() {
        use std::path::Path;
        assert_eq!(file_uri(Path::new("/home/me/Videos/Stiva/a.mkv")), "file:///home/me/Videos/Stiva/a.mkv");
        assert_eq!(file_uri(Path::new("/home/me/My Videos/100% #1 é.mp4")), "file:///home/me/My%20Videos/100%25%20%231%20%C3%A9.mp4");
    }
}
