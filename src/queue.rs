//! The list of jobs, kept on disk so it survives closing the program. The window runs one job at a time,
//! from the top; this file only knows what is in the list and in which state.

use crate::encode::Job;
use crate::model::{AudioCodec, EncodeSettings, RateMode, SIZE_STEPS};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum State {
    Waiting,
    Running,
    Done {
        out: PathBuf,
        bytes: u64,
        secs: f64,
    },
    Failed(String),
    /// The user stopped it.
    Stopped,
    /// The file was there already and the user asked to skip such files.
    Skipped(PathBuf),
}

impl State {
    /// Waiting and running jobs are on the Queued page, the rest on the Completed page.
    pub fn is_open(&self) -> bool {
        matches!(self, State::Waiting | State::Running)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub job: Job,
    pub state: State,
}

#[derive(Default)]
pub struct Queue {
    pub items: Vec<Item>,
    next_id: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    items: Vec<Item>,
}

impl Queue {
    /// Reads the list. A job that was running when the program ended did not finish: it waits again.
    pub fn load(path: &Path) -> Queue {
        let file: File = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let mut items = file.items;
        for i in &mut items {
            if i.state == State::Running {
                i.state = State::Waiting;
            }
        }
        let next_id = items.iter().map(|i| i.id).max().map_or(1, |m| m + 1);
        Queue { items, next_id }
    }

    pub fn save(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(json) = serde_json::to_string(&File { items: self.items.clone() }) else {
            return;
        };
        let part = path.with_extension("json.part");
        if std::fs::write(&part, json).is_ok() {
            let _ = std::fs::rename(&part, path);
        }
    }

    pub fn add(&mut self, job: Job) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.items.push(Item { id, job, state: State::Waiting });
        id
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// The job that comes next: the first one that is waiting.
    pub fn next_waiting(&self) -> Option<u64> {
        self.items.iter().find(|i| i.state == State::Waiting).map(|i| i.id)
    }

    pub fn waiting(&self) -> usize {
        self.items.iter().filter(|i| i.state == State::Waiting).count()
    }

    /// Jobs on the Queued page.
    pub fn open(&self) -> usize {
        self.items.iter().filter(|i| i.state.is_open()).count()
    }

    /// Jobs on the Completed page.
    pub fn finished(&self) -> usize {
        self.items.len() - self.open()
    }

    /// Takes a job off the list. A running one is stopped first, not removed.
    pub fn remove(&mut self, id: u64) {
        self.items.retain(|i| i.id != id || i.state == State::Running);
    }

    /// Puts a job that did not finish back in line, at the end.
    pub fn retry(&mut self, id: u64) {
        if let Some(pos) = self.items.iter().position(|i| i.id == id && !i.state.is_open()) {
            let mut item = self.items.remove(pos);
            item.state = State::Waiting;
            self.items.push(item);
        }
    }

    pub fn clear_finished(&mut self) {
        self.items.retain(|i| i.state.is_open());
    }
}

/// One line on what a job will do, for the lists.
pub fn summary(s: &EncodeSettings) -> String {
    let mut parts = vec![s.container.ext().to_uppercase()];
    if !s.container.audio_only() {
        parts.push(match s.rate {
            RateMode::Quality => format!("{}, quality {}", s.video_codec.label(), s.crf),
            RateMode::Bitrate => format!("{}, {} kb/s{}", s.video_codec.label(), s.video_kbps, if s.two_pass { ", 2 passes" } else { "" }),
            RateMode::TargetSize => {
                format!("{}, about {} MB{}", s.video_codec.label(), s.target_mb, if s.two_pass { ", 2 passes" } else { "" })
            }
        });
        if s.bit_depth >= 10 {
            parts.push("10-bit".into());
        }
        if let Some((l, sh)) = s.max_size {
            let name = SIZE_STEPS.iter().find(|(_, a, b)| (*a, *b) == (l, sh)).map(|(n, ..)| n.to_string());
            parts.push(format!("up to {}", name.unwrap_or_else(|| format!("{l} x {sh}"))));
        }
        let filters = s.filters.active();
        if !filters.is_empty() {
            parts.push(filters.join(", "));
        }
    }
    match s.audio_codec {
        AudioCodec::None => parts.push("no audio".into()),
        AudioCodec::Copy => parts.push("audio copied".into()),
        c if c.has_bitrate() => parts.push(format!("{} {} kb/s", c.label(), s.audio_kbps)),
        c => parts.push(c.label().to_string()),
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::SourceInfo;

    fn job(name: &str) -> Job {
        let src = SourceInfo { path: PathBuf::from(format!("/in/{name}.mkv")), duration: Some(60.0), ..Default::default() };
        Job {
            src,
            audio_sel: vec![],
            sub_sel: vec![],
            settings: EncodeSettings::default(),
            range: None,
            out: PathBuf::from(format!("/out/{name}.mp4")),
        }
    }

    fn names(q: &Queue) -> Vec<String> {
        q.items.iter().map(|i| i.job.src.path.file_stem().unwrap().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn jobs_run_from_the_top_and_finished_ones_are_not_picked_again() {
        let mut q = Queue::default();
        let (a, b, c) = (q.add(job("a")), q.add(job("b")), q.add(job("c")));
        assert_eq!(q.next_waiting(), Some(a));
        q.get_mut(a).unwrap().state = State::Running;
        assert_eq!(q.next_waiting(), Some(b), "a running job is not picked again");
        q.get_mut(a).unwrap().state = State::Done { out: "/out/a.mp4".into(), bytes: 9, secs: 1.0 };
        q.get_mut(b).unwrap().state = State::Failed("no".into());
        assert_eq!(q.next_waiting(), Some(c));
        assert_eq!((q.waiting(), q.open(), q.finished()), (1, 1, 2));
        q.get_mut(c).unwrap().state = State::Stopped;
        assert_eq!(q.next_waiting(), None);
    }

    #[test]
    fn a_running_job_stays_when_removal_is_asked_and_the_others_go() {
        let mut q = Queue::default();
        let (a, b) = (q.add(job("a")), q.add(job("b")));
        q.get_mut(a).unwrap().state = State::Running;
        q.remove(a);
        q.remove(b);
        assert_eq!(names(&q), ["a"]);
    }

    #[test]
    fn retry_sends_a_finished_job_to_the_end_and_leaves_open_ones_alone() {
        let mut q = Queue::default();
        let (a, b, c) = (q.add(job("a")), q.add(job("b")), q.add(job("c")));
        q.get_mut(a).unwrap().state = State::Failed("x".into());
        q.retry(a);
        assert_eq!(names(&q), ["b", "c", "a"]);
        assert_eq!(q.get(a).unwrap().state, State::Waiting);
        q.retry(b);
        assert_eq!(names(&q), ["b", "c", "a"], "b was waiting: nothing moves");
        let _ = c;
    }

    #[test]
    fn clearing_the_finished_keeps_the_open_ones() {
        let mut q = Queue::default();
        let (a, _b) = (q.add(job("a")), q.add(job("b")));
        q.get_mut(a).unwrap().state = State::Skipped("/out/a.mp4".into());
        q.clear_finished();
        assert_eq!(names(&q), ["b"]);
    }

    #[test]
    fn the_list_survives_a_restart_and_an_interrupted_job_waits_again() {
        let dir = std::env::temp_dir().join(format!("stiva-queue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("queue.json");
        let mut q = Queue::default();
        let (a, b, c) = (q.add(job("a")), q.add(job("b")), q.add(job("c")));
        q.get_mut(a).unwrap().state = State::Done { out: "/out/a.mp4".into(), bytes: 5, secs: 2.5 };
        q.get_mut(b).unwrap().state = State::Running;
        q.save(&file);
        let back = Queue::load(&file);
        assert_eq!(names(&back), ["a", "b", "c"]);
        assert_eq!(back.get(a).unwrap().state, State::Done { out: "/out/a.mp4".into(), bytes: 5, secs: 2.5 });
        assert_eq!(back.get(b).unwrap().state, State::Waiting, "it did not finish, so it waits again");
        assert_eq!(back.get(c).unwrap().state, State::Waiting);
        let mut back = back;
        assert!(back.add(job("d")) > c, "new ids never repeat old ones");
        assert_eq!(Queue::load(&dir.join("missing.json")).items.len(), 0);
        std::fs::write(&file, "garbage").unwrap();
        assert_eq!(Queue::load(&file).items.len(), 0, "a damaged file gives an empty list, not a crash");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_summary_names_what_will_happen() {
        let mut s = EncodeSettings::default();
        assert_eq!(summary(&s), "MP4 · H.264 (x264), quality 22 · AAC 160 kb/s");
        s.max_size = Some((1280, 720));
        s.bit_depth = 10;
        s.audio_codec = AudioCodec::Copy;
        assert_eq!(summary(&s), "MP4 · H.264 (x264), quality 22 · 10-bit · up to 720p · audio copied");
        s.max_size = Some((1000, 500));
        s.audio_codec = AudioCodec::None;
        assert!(summary(&s).ends_with("up to 1000 x 500 · no audio"));
        s.container = crate::model::Container::Flac;
        s.normalize();
        assert_eq!(summary(&s), "FLAC · FLAC (lossless)");
    }
}
