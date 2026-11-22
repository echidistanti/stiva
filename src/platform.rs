//! What only the operating system can do: a notification, the progress on the taskbar button, sleep and
//! shutdown. On Windows these are real. Elsewhere they do nothing (and sleep and shutdown refuse), so the
//! rest of the program can be run and tested on Linux without a machine going to sleep under the tester.

use crate::after::TaskbarState;
use eframe::egui;

/// Shows a notification under Stiva's own name. Fire and forget: if Windows will not show it, nothing happens.
pub fn notify(title: &str, body: &str) {
    #[cfg(windows)]
    imp::notify(title, body);
    #[cfg(not(windows))]
    let _ = (title, body);
}

/// Puts the PC to sleep. Returns false when this is not possible here.
pub fn sleep() -> bool {
    #[cfg(windows)]
    {
        imp::sleep()
    }
    #[cfg(not(windows))]
    false
}

/// Shuts the PC down at once, without forcing programs that have something unsaved to close.
pub fn shutdown() -> bool {
    #[cfg(windows)]
    {
        imp::shutdown()
    }
    #[cfg(not(windows))]
    false
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

    #[test]
    fn off_windows_the_power_actions_refuse_and_the_taskbar_just_remembers() {
        if cfg!(windows) {
            return;
        }
        assert!(!sleep() && !shutdown());
        let t = Taskbar::default();
        assert_eq!(t.shown(), None);
    }
}
