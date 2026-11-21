//! The state of the window and what happens when the user does something.
//! Work that takes time (finding ffmpeg, reading a file, encoding) runs on threads that report
//! through a channel; the window drains it at the start of every frame.

use crate::caps::Capabilities;
use crate::deps::{self, Tools};
use crate::encode::{self, Event, Handle, Job, Outcome, Progress};
use crate::model::{Container, EncodeSettings, VideoCodec};
use crate::probe::{self, SourceInfo};
use crate::settings::{self, DestMode, Resolution, Settings, WindowGeom};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

pub const MIN_SIZE: [f32; 2] = [900.0, 600.0];
pub const START_SIZE: [f32; 2] = [1100.0, 760.0];
const LOG_LINES: usize = 4000;

pub enum Msg {
    Tools(Result<Tools, String>),
    Setup(String, Option<f32>),
    Caps(Result<Capabilities, String>),
    Picked(PathBuf),
    PickedFolder(PathBuf),
    PickedToolsDir(PathBuf),
    Probed(PathBuf, Result<SourceInfo, String>),
    Enc(u64, Event),
}

pub enum Setup {
    Checking,
    Downloading { status: String, frac: Option<f32> },
    Ready(Tools),
    Failed(String),
}

pub struct Source {
    pub info: SourceInfo,
    pub audio_on: Vec<bool>,
    pub start: String,
    pub end: String,
}

impl Source {
    fn new(info: SourceInfo) -> Source {
        let n = info.audio.len();
        Source { info, audio_on: vec![true; n], start: String::new(), end: String::new() }
    }

    /// The part of the file to encode, from the two text boxes.
    pub fn range(&self) -> Result<Option<(f64, f64)>, &'static str> {
        use crate::strings as s;
        let (st, en) = (self.start.trim(), self.end.trim());
        if st.is_empty() && en.is_empty() {
            return Ok(None);
        }
        let a = if st.is_empty() { 0.0 } else { probe::parse_time(st).ok_or(s::RANGE_BAD_START)? };
        let b = if en.is_empty() { self.info.duration.unwrap_or(a + 1e9) } else { probe::parse_time(en).ok_or(s::RANGE_BAD_END)? };
        let b = self.info.duration.map_or(b, |d| b.min(d));
        Ok(Some((a, b)))
    }

    pub fn audio_selection(&self) -> Vec<usize> {
        self.audio_on.iter().enumerate().filter(|(_, on)| **on).map(|(i, _)| i).collect()
    }
}

pub enum Src {
    Empty,
    Scanning(PathBuf),
    Failed(PathBuf, String),
    Ready(Source),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Container,
    Video,
    Audio,
}

pub struct Run {
    pub id: u64,
    pub handle: Handle,
    pub job: Job,
    pub progress: Option<Progress>,
    pub started: Instant,
    pub log: Vec<String>,
}

pub enum Report {
    Done {
        out: PathBuf,
        bytes: u64,
        elapsed: Duration,
    },
    Failed {
        code: Option<i32>,
        tail: Vec<String>,
    },
    Cancelled,
    Skipped(PathBuf),
    /// The job was refused before it started.
    Refused(String),
}

pub struct Last {
    pub report: Report,
    pub name: String,
    pub log: Vec<String>,
}

pub struct App {
    pub ctx: egui::Context,
    pub tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub set: Settings,
    pub setup: Setup,
    pub caps: Option<Capabilities>,
    pub src: Src,
    pub tab: Tab,
    pub run: Option<Run>,
    pub last: Option<Last>,
    next_run: u64,
    pub show_options: bool,
    /// A job waiting for the user to say what to do with a file that exists.
    pub ask: Option<Job>,
    /// The window was asked to close during an encode, and the question is on screen.
    pub confirm_quit: bool,
    pub quit_confirmed: bool,
    /// A line in amber under the settings, about something the program changed by itself.
    pub note: Option<String>,
    geom: Option<WindowGeom>,
    dirty: bool,
    last_save: Instant,
    /// A file named on the command line (Open with, Send to), opened as soon as ffmpeg is ready.
    pending_open: Option<PathBuf>,
    /// Debug builds only: start the encode as soon as the file is read, to look at the progress.
    auto_encode: bool,
}

/// What the file picker offers first. Any other file can still be chosen: ffprobe decides.
const MEDIA_EXTS: [&str; 24] = [
    "mp4", "m4v", "mkv", "webm", "avi", "mov", "wmv", "flv", "ts", "m2ts", "mts", "mpg", "mpeg", "ogv", "3gp", "mp3", "m4a", "aac", "flac",
    "wav", "ogg", "opus", "wma", "mka",
];

impl App {
    pub fn new(ctx: egui::Context, mut set: Settings, open: Option<PathBuf>) -> App {
        let (tx, rx) = channel();
        set.encode.normalize();
        let mut app = App {
            ctx,
            tx,
            rx,
            geom: set.window.clone(),
            set,
            setup: Setup::Checking,
            caps: None,
            src: Src::Empty,
            tab: match (cfg!(debug_assertions), std::env::var("STIVA_TAB").as_deref()) {
                (true, Ok("audio")) => Tab::Audio,
                (true, Ok("container")) => Tab::Container,
                _ => Tab::Video,
            },
            run: None,
            last: None,
            next_run: 1,
            show_options: cfg!(debug_assertions) && std::env::var_os("STIVA_SHOW_OPTIONS").is_some(),
            ask: None,
            confirm_quit: false,
            quit_confirmed: false,
            note: None,
            dirty: false,
            last_save: Instant::now(),
            pending_open: open,
            auto_encode: cfg!(debug_assertions) && std::env::var_os("STIVA_AUTOENCODE").is_some(),
        };
        app.start_setup(false);
        app
    }

    /// Finds ffmpeg, or downloads it. `force` downloads even when a copy is there.
    pub fn start_setup(&mut self, force: bool) {
        self.setup = Setup::Checking;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let custom = self.set.ffmpeg_dir.clone();
        std::thread::spawn(move || {
            let found = if force { None } else { deps::locate(custom.as_deref()) };
            // The copy an earlier Stiva downloaded was the "essentials" build, which has no SVT-AV1: replace it.
            let stale = cfg!(windows) && custom.is_none() && found.as_ref().is_some_and(deps::is_essentials);
            let result = match found {
                Some(t) if !stale => Ok(t),
                old => {
                    let _ = tx.send(Msg::Setup("Starting...".into(), None));
                    let fetched = deps::download(&mut |m, f| {
                        let _ = tx.send(Msg::Setup(m.to_string(), f));
                        ctx.request_repaint();
                    });
                    // Offline with an older copy at hand: use it, and try again at the next start.
                    fetched.or_else(|e| old.ok_or(e))
                }
            };
            let _ = tx.send(Msg::Tools(result));
            ctx.request_repaint();
        });
    }

    pub fn tools(&self) -> Option<&Tools> {
        match &self.setup {
            Setup::Ready(t) => Some(t),
            _ => None,
        }
    }

    /// Takes in everything the threads have reported since the last frame.
    pub fn pump(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Setup(status, frac) => self.setup = Setup::Downloading { status, frac },
                Msg::Tools(Ok(t)) => {
                    let (tx, ctx, ff) = (self.tx.clone(), self.ctx.clone(), t.ffmpeg.clone());
                    std::thread::spawn(move || {
                        let _ = tx.send(Msg::Caps(Capabilities::probe(&ff)));
                        ctx.request_repaint();
                    });
                    self.setup = Setup::Ready(t);
                    if let Some(p) = self.pending_open.take() {
                        self.load_source(p);
                    }
                }
                Msg::Tools(Err(e)) => self.setup = Setup::Failed(e),
                Msg::Caps(Ok(c)) => {
                    // A saved choice this ffmpeg or this PC cannot do (a graphics chip that is gone, say) gives way.
                    let enc = &mut self.set.encode;
                    if !enc.container.audio_only() && !c.has(enc.video_codec.encoder()) {
                        enc.set_video_codec(VideoCodec::X264);
                        enc.normalize();
                        self.dirty = true;
                    }
                    // Which graphics-chip encoders passed their test, and why the others did not.
                    let logs = deps::data_dir().join("logs");
                    let _ = std::fs::create_dir_all(&logs);
                    let _ = std::fs::write(logs.join("hardware.txt"), c.hardware_report());
                    self.caps = Some(c);
                }
                Msg::Caps(Err(_)) => {}
                Msg::Picked(p) => self.load_source(p),
                Msg::PickedFolder(p) => {
                    self.set.dest_dir = Some(p);
                    self.dirty = true;
                }
                Msg::PickedToolsDir(p) => {
                    self.set.ffmpeg_dir = Some(p);
                    self.dirty = true;
                    self.start_setup(false);
                }
                Msg::Probed(path, res) => {
                    if matches!(&self.src, Src::Scanning(p) if *p == path) {
                        self.src = match res {
                            Ok(info) => {
                                if std::mem::take(&mut self.auto_encode) {
                                    self.src = Src::Ready(Source::new(info));
                                    self.encode();
                                    continue;
                                }
                                Src::Ready(Source::new(info))
                            }
                            Err(e) => Src::Failed(path, e),
                        };
                    }
                }
                Msg::Enc(id, ev) => self.on_encode_event(id, ev),
            }
        }
    }

    fn on_encode_event(&mut self, id: u64, ev: Event) {
        let Some(run) = self.run.as_mut().filter(|r| r.id == id) else {
            return;
        };
        match ev {
            Event::Progress(p) => run.progress = Some(p),
            Event::Log(l) => {
                run.log.push(l);
                if run.log.len() > LOG_LINES {
                    run.log.drain(..LOG_LINES / 4);
                }
            }
            Event::Done(outcome) => {
                let run = self.run.take().expect("checked above");
                let name = run.job.out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let report = match outcome {
                    Outcome::Finished { out, bytes, elapsed } => Report::Done { out, bytes, elapsed },
                    Outcome::Failed { code, tail } => Report::Failed { code, tail },
                    Outcome::Cancelled => Report::Cancelled,
                };
                save_log(&name, &run.log);
                self.last = Some(Last { report, name, log: run.log });
            }
        }
    }

    pub fn open_picker(&self) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let pick = rfd::FileDialog::new().add_filter("Video and audio", &MEDIA_EXTS).add_filter("All files", &["*"]).pick_file();
            if let Some(p) = pick {
                let _ = tx.send(Msg::Picked(p));
                ctx.request_repaint();
            }
        });
    }

    pub fn pick_folder(&self, current: Option<PathBuf>, make: fn(PathBuf) -> Msg) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut d = rfd::FileDialog::new();
            if let Some(c) = current.filter(|c| c.is_dir()) {
                d = d.set_directory(c);
            }
            if let Some(p) = d.pick_folder() {
                let _ = tx.send(make(p));
                ctx.request_repaint();
            }
        });
    }

    pub fn load_source(&mut self, path: PathBuf) {
        let Some(t) = self.tools().cloned() else {
            return;
        };
        self.src = Src::Scanning(path.clone());
        self.last = None;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = probe::run(&t.ffprobe, &path);
            let _ = tx.send(Msg::Probed(path, r));
            ctx.request_repaint();
        });
    }

    /// The Encode button: check, decide where the file goes, and start.
    pub fn encode(&mut self) {
        let Some(tools) = self.tools().cloned() else {
            return;
        };
        if self.run.is_some() {
            return;
        }
        let Src::Ready(src) = &self.src else { return };
        let name = src.info.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let refuse = |app: &mut App, why: String| {
            app.last = Some(Last { report: Report::Refused(why), name: name.clone(), log: vec![] });
        };
        let range = match src.range() {
            Ok(r) => r,
            Err(e) => return refuse(self, e.to_string()),
        };
        let mut job =
            Job { src: src.info.clone(), audio_sel: src.audio_selection(), settings: self.set.encode.clone(), range, out: PathBuf::new() };
        if let Err(e) = job.validate() {
            return refuse(self, e);
        }
        let desired = self.set.output_path(&src.info.path);
        match settings::resolve(self.set.on_exists, desired.clone(), &src.info.path) {
            Resolution::Use(p) => {
                job.out = p;
                self.launch(job, tools);
            }
            Resolution::Ask(p) => {
                job.out = p;
                self.ask = Some(job);
            }
            Resolution::Skip => self.last = Some(Last { report: Report::Skipped(desired), name, log: vec![] }),
        }
    }

    pub fn launch(&mut self, job: Job, tools: Tools) {
        let id = self.next_run;
        self.next_run += 1;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let handle = encode::start(job.clone(), tools.ffmpeg, move |e| {
            let _ = tx.send(Msg::Enc(id, e));
            ctx.request_repaint();
        });
        self.last = None;
        self.run = Some(Run { id, handle, job, progress: None, started: Instant::now(), log: vec![] });
    }

    pub fn stop(&mut self) {
        if let Some(r) = &self.run {
            r.handle.stop();
        }
    }

    /// The program is closing: end the run and make sure no half file is left.
    pub fn shutdown(&mut self) {
        if let Some(r) = self.run.take() {
            r.handle.shutdown(&r.job.part_path());
        }
    }

    pub fn can_encode(&self) -> bool {
        self.tools().is_some() && self.run.is_none() && matches!(self.src, Src::Ready(_))
    }

    /// A file dropped on the window replaces the source.
    pub fn accept_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if self.tools().is_none() {
            return;
        }
        if let Some(p) = dropped.into_iter().find(|p| p.is_file()) {
            self.load_source(p);
        }
    }

    /// Remembers where the window is, so the next run opens it there.
    pub fn track_window(&mut self, ctx: &egui::Context) {
        let (inner, outer, maximized, minimized) = ctx.input(|i| {
            let v = i.viewport();
            (v.inner_rect, v.outer_rect, v.maximized.unwrap_or(false), v.minimized.unwrap_or(false))
        });
        if minimized {
            return;
        }
        let mut g = self.geom.clone().unwrap_or(WindowGeom { pos: None, size: START_SIZE, maximized: false });
        g.maximized = maximized;
        if !maximized {
            if let Some(r) = inner {
                g.size = [r.width(), r.height()];
            }
            if let Some(r) = outer {
                if r.min.x > -10000.0 && r.min.y > -10000.0 {
                    g.pos = Some([r.min.x, r.min.y]);
                }
            }
        }
        if self.geom.as_ref() != Some(&g) {
            self.geom = Some(g);
            self.dirty = true;
        }
    }

    /// Writes the settings a moment after the last change, not on every step of a slider.
    pub fn save_if_due(&mut self, force: bool) {
        if self.dirty && (force || self.last_save.elapsed() > Duration::from_millis(800)) {
            self.set.window = self.geom.clone();
            self.set.save();
            self.dirty = false;
            self.last_save = Instant::now();
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// After the settings changed in the window: fix impossible combinations and say so.
    pub fn after_settings_edit(&mut self, before: &EncodeSettings) {
        if self.set.encode == *before {
            return;
        }
        self.dirty = true;
        let container_changed = self.set.encode.container != before.container;
        let replaced = self.set.encode.normalize();
        if container_changed {
            self.note = replaced.then(|| crate::strings::CHANGED_FOR_FORMAT.to_string());
        }
    }

    pub fn out_dir_for_display(&self) -> PathBuf {
        match (&self.src, self.set.dest_mode) {
            (Src::Ready(s), _) => self.set.output_path(&s.info.path),
            (_, _) => self.set.out_dir(),
        }
    }

    pub fn using_custom_tools(&self) -> bool {
        self.set.ffmpeg_dir.as_deref().and_then(|d| deps::locate(Some(d))).is_some_and(|t| self.tools() == Some(&t))
    }

    pub fn reset_note(&mut self) {
        self.note = None;
    }

    pub fn dest_mode_next(&self) -> bool {
        self.set.dest_mode == DestMode::NextToSource
    }
}

/// Keeps the log of each encode next to the program's other data, the newest 30.
fn save_log(name: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let dir = deps::data_dir().join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    let _ = std::fs::write(dir.join(format!("{stamp}-{safe}.log")), lines.join("\n"));
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    files.sort();
    let extra = files.len().saturating_sub(30);
    for old in files.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
}

pub fn open_path(p: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

/// Opens the folder with the file selected.
pub fn reveal(p: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", p.display())).spawn();
    }
    #[cfg(not(windows))]
    open_path(p.parent().unwrap_or(p));
}

pub fn container_note(c: Container) -> &'static str {
    use crate::strings as s;
    match c {
        Container::Mp4 => s::FORMAT_NOTE_MP4,
        Container::Mkv => s::FORMAT_NOTE_MKV,
        Container::WebM => s::FORMAT_NOTE_WEBM,
        _ => s::FORMAT_NOTE_AUDIO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(duration: Option<f64>) -> Source {
        Source::new(SourceInfo { duration, audio: vec![Default::default(), Default::default()], ..Default::default() })
    }

    #[test]
    fn the_range_boxes_make_a_range() {
        let mut s = src(Some(600.0));
        assert_eq!(s.range(), Ok(None));
        s.start = "1:30".into();
        assert_eq!(s.range(), Ok(Some((90.0, 600.0))), "an empty end means the end of the file");
        s.end = "2:00".into();
        assert_eq!(s.range(), Ok(Some((90.0, 120.0))));
        s.end = "99:00".into();
        assert_eq!(s.range(), Ok(Some((90.0, 600.0))), "an end past the file is cut at the file");
        s.start.clear();
        s.end = "30".into();
        assert_eq!(s.range(), Ok(Some((0.0, 30.0))));
    }

    #[test]
    fn bad_times_say_which_box_is_wrong() {
        let mut s = src(Some(60.0));
        s.start = "abc".into();
        assert_eq!(s.range(), Err(crate::strings::RANGE_BAD_START));
        s.start = "5".into();
        s.end = "1:xx".into();
        assert_eq!(s.range(), Err(crate::strings::RANGE_BAD_END));
    }

    #[test]
    fn all_tracks_start_selected_and_the_selection_is_by_position() {
        let mut s = src(None);
        assert_eq!(s.audio_selection(), vec![0, 1]);
        s.audio_on[0] = false;
        assert_eq!(s.audio_selection(), vec![1]);
    }
}
