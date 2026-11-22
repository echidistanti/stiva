//! What happens when the queue has run dry: the "When done" choice, the tally behind the notification,
//! and the state the taskbar button should show. Everything here is plain logic; what touches the
//! operating system is in platform.rs.

use crate::probe::human_change;
use std::path::PathBuf;
use std::time::Duration;

/// What to do once the whole queue is finished by itself (not when the user pressed Stop).
/// It is not saved between runs: a PC that shuts down because of a choice made last week is no favour.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WhenDone {
    #[default]
    Nothing,
    OpenFolder,
    CloseApp,
    Sleep,
    Shutdown,
}

impl WhenDone {
    pub const ALL: [WhenDone; 5] = [WhenDone::Nothing, WhenDone::OpenFolder, WhenDone::CloseApp, WhenDone::Sleep, WhenDone::Shutdown];

    pub fn label(self) -> &'static str {
        match self {
            WhenDone::Nothing => "Do nothing",
            WhenDone::OpenFolder => "Open the folder",
            WhenDone::CloseApp => "Close Stiva",
            WhenDone::Sleep => "Sleep",
            WhenDone::Shutdown => "Shut down",
        }
    }

    /// Choices that switch the PC off or to sleep give a few seconds to change one's mind.
    pub fn grace(self) -> Option<Duration> {
        match self {
            WhenDone::Sleep => Some(Duration::from_secs(15)),
            WhenDone::Shutdown => Some(Duration::from_secs(60)),
            _ => None,
        }
    }
}

/// What one run of the queue did, for the notification and for "Open the folder".
#[derive(Default, Debug, PartialEq)]
pub struct Batch {
    pub done: u32,
    pub failed: u32,
    /// Bytes of the sources and of the results that were made, for the saving shown in the notification.
    pub from: u64,
    pub to: u64,
    pub last_out: Option<PathBuf>,
}

impl Batch {
    pub fn finished_one(&mut self, out: PathBuf, from: u64, to: u64) {
        self.done += 1;
        self.from += from;
        self.to += to;
        self.last_out = Some(out);
    }

    pub fn failed_one(&mut self) {
        self.failed += 1;
    }

    /// Jobs that ended, one way or the other.
    pub fn ended(&self) -> u32 {
        self.done + self.failed
    }

    /// Title and text of the notification.
    pub fn summary(&self) -> (String, String) {
        let title = if self.failed == 0 { "Encoding finished" } else { "Encoding finished with errors" };
        let noun = |n: u32| if n == 1 { "file" } else { "files" };
        let mut parts = vec![];
        if self.done > 0 {
            parts.push(format!("{} {} done", self.done, noun(self.done)));
        }
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        if self.done > 0 {
            parts.push(human_change(self.from, self.to));
        }
        (title.to_string(), parts.join(" · "))
    }
}

/// What the taskbar button shows. `Normal` and `Paused` carry the progress in thousandths.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskbarState {
    None,
    Indeterminate,
    Normal(u16),
    Paused(u16),
}

/// The state for a running job. `fraction` is the share of the current job that is done, if known;
/// `before` is how many jobs of this run ended already and `after` how many still wait, so the bar
/// shows the whole run and not just the file in hand.
pub fn taskbar_state(fraction: Option<f32>, paused: bool, before: u32, after: usize) -> TaskbarState {
    let Some(f) = fraction else {
        return if paused { TaskbarState::Paused(0) } else { TaskbarState::Indeterminate };
    };
    let total = before as f32 + 1.0 + after as f32;
    let overall = ((before as f32 + f.clamp(0.0, 1.0)) / total).clamp(0.0, 1.0);
    let thousandths = (overall * 1000.0).round() as u16;
    if paused { TaskbarState::Paused(thousandths) } else { TaskbarState::Normal(thousandths) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notification_tells_what_was_done_and_what_it_saved() {
        let mut b = Batch::default();
        b.finished_one("/o/a.mp4".into(), 10 * 1_048_576, 4 * 1_048_576);
        b.finished_one("/o/b.mp4".into(), 10 * 1_048_576, 4 * 1_048_576);
        let (title, text) = b.summary();
        assert_eq!(title, "Encoding finished");
        assert_eq!(text, "2 files done · 20.0 MB \u{2192} 8.0 MB (-60%)");
        assert_eq!(b.last_out.as_deref(), Some(std::path::Path::new("/o/b.mp4")));
        b.failed_one();
        let (title, text) = b.summary();
        assert_eq!(title, "Encoding finished with errors");
        assert!(text.contains("2 files done") && text.contains("1 failed"));
    }

    #[test]
    fn a_run_where_everything_failed_has_no_saving_to_report() {
        let mut b = Batch::default();
        b.failed_one();
        assert_eq!(b.summary(), ("Encoding finished with errors".to_string(), "1 failed".to_string()));
        assert_eq!(b.ended(), 1);
        assert_eq!(Batch::default().ended(), 0);
        let mut one = Batch::default();
        one.finished_one("/o/a.mp4".into(), 100, 50);
        assert!(one.summary().1.starts_with("1 file done"));
    }

    #[test]
    fn only_sleep_and_shutdown_wait_before_acting() {
        for w in WhenDone::ALL {
            assert_eq!(w.grace().is_some(), matches!(w, WhenDone::Sleep | WhenDone::Shutdown), "{w:?}");
        }
        assert_eq!(WhenDone::default(), WhenDone::Nothing);
    }

    #[test]
    fn the_taskbar_shows_the_whole_run_not_just_the_file_in_hand() {
        // Second of four jobs, half done: (1 + 0.5) / 4.
        assert_eq!(taskbar_state(Some(0.5), false, 1, 2), TaskbarState::Normal(375));
        assert_eq!(taskbar_state(Some(0.5), true, 1, 2), TaskbarState::Paused(375));
        assert_eq!(taskbar_state(Some(1.0), false, 0, 0), TaskbarState::Normal(1000));
        assert_eq!(taskbar_state(None, false, 0, 0), TaskbarState::Indeterminate);
        assert_eq!(taskbar_state(None, true, 3, 3), TaskbarState::Paused(0));
        assert_eq!(taskbar_state(Some(7.0), false, 0, 0), TaskbarState::Normal(1000), "a fraction past the end is held at the end");
    }
}
