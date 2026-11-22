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

/// Standard base64, which PowerShell's -EncodedCommand asks for (of the script in UTF-16LE).
#[cfg_attr(not(windows), allow(dead_code))]
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// The script behind the notification. The texts come in through the environment (STIVA_TITLE, STIVA_BODY,
/// STIVA_ICON), never pasted into the script, so nothing in a file name can be taken for code.
/// Unpackaged programs have no identity for Windows to attach a notification to, so the script first
/// registers one for the current user (HKCU only): a name and an icon under the id "Stiva".
#[cfg_attr(not(windows), allow(dead_code))]
const TOAST_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$key = 'HKCU:\Software\Classes\AppUserModelId\Stiva'
if (-not (Test-Path $key)) { New-Item -Path $key -Force | Out-Null }
Set-ItemProperty -Path $key -Name DisplayName -Value 'Stiva'
if ($env:STIVA_ICON -and (Test-Path $env:STIVA_ICON)) { Set-ItemProperty -Path $key -Name IconUri -Value $env:STIVA_ICON }
[void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]
[void][Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime]
$title = [System.Security.SecurityElement]::Escape($env:STIVA_TITLE)
$body = [System.Security.SecurityElement]::Escape($env:STIVA_BODY)
$xml = New-Object Windows.Data.Xml.Dom.XmlDocument
$xml.LoadXml("<toast><visual><binding template='ToastGeneric'><text>$title</text><text>$body</text></binding></visual></toast>")
$toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Stiva').Show($toast)
"#;

#[cfg(windows)]
mod imp {
    use super::{TOAST_SCRIPT, base64};
    use crate::after::TaskbarState;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::process::Command;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::UI::Shell::{ITaskbarList3, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TBPF_PAUSED, TaskbarList};

    pub fn notify(title: &str, body: &str) {
        let utf16: Vec<u8> = TOAST_SCRIPT.encode_utf16().flat_map(u16::to_le_bytes).collect();
        // The program's icon as a file, which is what the notification wants to show next to the name.
        let icon = crate::deps::data_dir().join("icon.png");
        if !icon.is_file() {
            let _ = std::fs::create_dir_all(crate::deps::data_dir());
            let _ = std::fs::write(&icon, crate::icon::PNG);
        }
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-EncodedCommand"])
            .arg(base64(&utf16))
            .env("STIVA_TITLE", title)
            .env("STIVA_BODY", body)
            .env("STIVA_ICON", icon);
        crate::deps::quiet(&mut cmd);
        let _ = cmd.spawn();
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
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
        let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
        // Known values, worked out with Python's base64 module.
        assert_eq!(base64(&utf16("Write-Host hi")), "VwByAGkAdABlAC0ASABvAHMAdAAgAGgAaQA=");
        assert_eq!(base64(&utf16("$x=1")), "JAB4AD0AMQA=");
    }

    #[test]
    fn the_toast_script_takes_its_texts_from_the_environment_and_touches_only_the_users_hive() {
        assert!(TOAST_SCRIPT.contains("$env:STIVA_TITLE") && TOAST_SCRIPT.contains("$env:STIVA_BODY"));
        assert!(TOAST_SCRIPT.contains("SecurityElement]::Escape"), "texts are escaped before they go into the XML");
        assert!(TOAST_SCRIPT.contains("HKCU:"), "the identity is registered for the current user only");
        assert!(!TOAST_SCRIPT.contains("HKLM"));
        assert!(!TOAST_SCRIPT.contains("Invoke-Expression") && !TOAST_SCRIPT.contains("iex "));
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
