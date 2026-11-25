//! The state of the window and what happens when the user does something.
//! Work that takes time (finding ffmpeg, reading a file, encoding) runs on threads that report
//! through a channel; the window drains it at the start of every frame.

use crate::after::{self, Batch, TaskbarState, WhenDone};
use crate::caps::Capabilities;
use crate::deps::{self, Tools};
use crate::encode::{self, Event, Handle, Job, Outcome, Progress};
use crate::model::{Container, EncodeSettings, VideoCodec};
use crate::platform;
use crate::presets::{self, Preset};
use crate::probe::{self, SourceInfo};
use crate::queue::{Item, Queue, State};
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
    /// Nothing is chosen or downloaded yet: the ffmpeg the system has, with its version, or none.
    #[cfg(not(windows))]
    Choose(Option<(Tools, String)>),
    Setup(String, Option<f32>),
    Caps(Result<Capabilities, String>),
    Picked(Vec<PathBuf>),
    PickedFolder(PathBuf),
    PickedToolsDir(PathBuf),
    Probed(PathBuf, Result<SourceInfo, String>),
    /// One file of several dropped or chosen together, read, with the settings at that moment.
    Batch(PathBuf, Result<SourceInfo, String>, Box<EncodeSettings>),
    PresetImported(Result<Preset, String>),
    /// A short message for the amber line under the settings.
    Said(String),
    Enc(u64, Event),
}

pub enum Setup {
    Checking,
    Downloading {
        status: String,
        frac: Option<f32>,
    },
    /// Outside Windows nothing is downloaded before the user has said so. Holds the ffmpeg the system has, if any.
    #[cfg(not(windows))]
    Choose {
        system: Option<(Tools, String)>,
    },
    Ready(Tools),
    Failed(String),
}

pub struct Source {
    pub info: SourceInfo,
    pub audio_on: Vec<bool>,
    /// Which subtitle tracks to keep; those the container cannot hold are left out when the job is made.
    pub subs_on: Vec<bool>,
    pub start: String,
    pub end: String,
}

impl Source {
    fn new(info: SourceInfo) -> Source {
        let (n, m) = (info.audio.len(), info.subs.len());
        Source { info, audio_on: vec![true; n], subs_on: vec![true; m], start: String::new(), end: String::new() }
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

    pub fn subtitle_selection(&self) -> Vec<usize> {
        self.subs_on.iter().enumerate().filter(|(_, on)| **on).map(|(i, _)| i).collect()
    }
}

pub enum Src {
    Empty,
    Scanning(PathBuf),
    Failed(PathBuf, String),
    Ready(Source),
}

/// What the main area shows: the file being set up, or the list of jobs.
#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Editor,
    Queue,
}

#[derive(Clone, Copy, PartialEq)]
pub enum QueueTab {
    Queued,
    Completed,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Container,
    Sizing,
    Filters,
    Video,
    Audio,
}

pub struct Run {
    pub id: u64,
    /// The job in the queue this run belongs to.
    pub item: u64,
    pub handle: Handle,
    pub job: Job,
    pub progress: Option<Progress>,
    pub started: Instant,
    pub log: Vec<String>,
    pub paused_at: Option<Instant>,
    pub paused_before: Duration,
}

impl Run {
    pub fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    /// Time spent encoding, without the time spent paused.
    pub fn active(&self) -> Duration {
        let paused = self.paused_before + self.paused_at.map_or(Duration::ZERO, |t| t.elapsed());
        self.started.elapsed().saturating_sub(paused)
    }

    /// Seconds left: by the pace of the whole run so far, which pauses do not upset, or ffmpeg's own speed at the start.
    pub fn eta(&self) -> Option<f64> {
        let span = self.job.span();
        let p = self.progress?;
        match p.fraction(span) {
            Some(f) if f >= 0.02 => Some(self.active().as_secs_f64() * (1.0 - f as f64) / f as f64),
            _ => p.eta(span),
        }
    }
}

pub enum Report {
    Done {
        out: PathBuf,
        /// Size of the source, to show what the encode did to it.
        from: u64,
        bytes: u64,
        elapsed: Duration,
    },
    Failed {
        code: Option<i32>,
        tail: Vec<String>,
    },
    Cancelled,
    Skipped(PathBuf),
}

/// A line over the progress strip that says what a click just did: a file joined the queue, or could not.
/// It has a life of its own, so it shows while an encode is running too.
pub struct Toast {
    pub title: String,
    pub detail: String,
    pub warn: bool,
    /// Offers a button that opens the queue.
    pub view_queue: bool,
    pub until: Instant,
}

/// How long a toast stays.
pub const TOAST_FOR: Duration = Duration::from_secs(9);
/// How long the queue button glows after a job joined.
pub const FLASH_FOR: Duration = Duration::from_millis(1800);

pub struct Last {
    pub report: Report,
    pub name: String,
    pub log: Vec<String>,
}

/// A sleep or a shutdown that is counting down, so the user can still say no.
pub struct Power {
    pub what: WhenDone,
    pub at: Instant,
}

/// Something the user took away and can have back for a few seconds.
pub enum Undo {
    Item(Item, usize),
    Finished(Vec<(Item, usize)>),
    Source(Box<Src>),
}

/// How long the Undo line stays.
pub const UNDO_FOR: Duration = Duration::from_secs(10);

pub struct App {
    pub ctx: egui::Context,
    pub tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub set: Settings,
    pub setup: Setup,
    pub caps: Option<Capabilities>,
    pub src: Src,
    pub tab: Tab,
    pub page: Page,
    pub qtab: QueueTab,
    pub queue: Queue,
    /// The queue is being worked through: when a job ends, the next one starts.
    pub processing: bool,
    pub builtin: Vec<Preset>,
    pub user_presets: Vec<Preset>,
    /// The name being typed in the "Save as" box, while it is open.
    pub preset_name: Option<String>,
    pub preset_delete: Option<String>,
    pub run: Option<Run>,
    pub last: Option<Last>,
    next_run: u64,
    pub show_options: bool,
    /// The job in the queue that waits for the user to say what to do with a file that exists.
    pub ask: Option<u64>,
    /// The window was asked to close during an encode, and the question is on screen.
    pub confirm_quit: bool,
    pub quit_confirmed: bool,
    /// A line in amber under the settings, about something the program changed by itself.
    pub note: Option<String>,
    /// Files of the last batch that could not be added, with the reason; shown until dismissed.
    pub refused: Vec<(String, String)>,
    /// The last thing taken away, and until when it can be put back.
    pub undo: Option<(Undo, Instant)>,
    pub toast: Option<Toast>,
    /// What to do when the queue runs dry. Not saved: it starts as "Do nothing" every time.
    pub when_done: WhenDone,
    /// What the current run of the queue has done so far.
    pub batch: Batch,
    pub power: Option<Power>,
    pub taskbar: platform::Taskbar,
    /// The ffmpeg the system has, found at start and at each look for ffmpeg, for the options.
    #[cfg(not(windows))]
    pub system_tools: Option<Tools>,
    /// When a job last joined the queue; the queue button glows for a moment after.
    pub queue_flash: Option<Instant>,
    geom: Option<WindowGeom>,
    dirty: bool,
    last_save: Instant,
    /// Files named on the command line (Open with, Send to), opened as soon as ffmpeg is ready.
    pending_open: Vec<PathBuf>,
    /// Debug builds only: start the encode as soon as the file is read, to look at the progress.
    auto_encode: bool,
    /// Debug builds only: open this file once an encode is running, to look at adding during one.
    debug_second: Option<PathBuf>,
}

/// What the file picker offers first. Any other file can still be chosen: ffprobe decides.
const MEDIA_EXTS: [&str; 24] = [
    "mp4", "m4v", "mkv", "webm", "avi", "mov", "wmv", "flv", "ts", "m2ts", "mts", "mpg", "mpeg", "ogv", "3gp", "mp3", "m4a", "aac", "flac",
    "wav", "ogg", "opus", "wma", "mka",
];

impl App {
    pub fn new(ctx: egui::Context, mut set: Settings, open: Vec<PathBuf>) -> App {
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
                (true, Ok("sizing")) => Tab::Sizing,
                (true, Ok("filters")) => Tab::Filters,
                _ => Tab::Video,
            },
            page: Page::Editor,
            qtab: QueueTab::Queued,
            queue: Queue::load(&queue_file()),
            processing: false,
            builtin: presets::builtin(),
            user_presets: presets::load_dir(&presets::dir()).into_iter().map(|(_, p)| p).collect(),
            preset_name: None,
            preset_delete: None,
            run: None,
            last: None,
            next_run: 1,
            show_options: cfg!(debug_assertions) && std::env::var_os("STIVA_SHOW_OPTIONS").is_some(),
            ask: None,
            confirm_quit: false,
            quit_confirmed: false,
            note: None,
            refused: vec![],
            undo: None,
            toast: None,
            // Debug builds only: STIVA_WHENDONE=sleep|shutdown|close|folder, to look at the choices without clicking.
            when_done: if cfg!(debug_assertions) {
                match std::env::var("STIVA_WHENDONE").as_deref() {
                    Ok("sleep") => WhenDone::Sleep,
                    Ok("shutdown") => WhenDone::Shutdown,
                    Ok("close") => WhenDone::CloseApp,
                    Ok("folder") => WhenDone::OpenFolder,
                    _ => WhenDone::default(),
                }
            } else {
                WhenDone::default()
            },
            batch: Batch::default(),
            power: None,
            taskbar: platform::Taskbar::default(),
            #[cfg(not(windows))]
            system_tools: deps::system(),
            queue_flash: None,
            dirty: false,
            last_save: Instant::now(),
            pending_open: open,
            auto_encode: cfg!(debug_assertions) && std::env::var_os("STIVA_AUTOENCODE").is_some(),
            debug_second: if cfg!(debug_assertions) { std::env::var_os("STIVA_SECOND").map(PathBuf::from) } else { None },
        };
        app.start_setup(false);
        app
    }

    /// Finds ffmpeg, or downloads it. `force` downloads even when a copy is there.
    pub fn start_setup(&mut self, force: bool) {
        self.setup = Setup::Checking;
        #[cfg(not(windows))]
        {
            self.system_tools = deps::system();
        }
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let custom = self.set.ffmpeg_dir.clone();
        std::thread::spawn(move || {
            let found = if force { None } else { deps::locate(custom.as_deref()) };
            // The copy an earlier Stiva downloaded was the "essentials" build, which has no SVT-AV1: replace it.
            let stale = cfg!(windows) && custom.is_none() && found.as_ref().is_some_and(deps::is_essentials);
            let result = match found {
                Some(t) if !stale => Ok(t),
                old => {
                    // Outside Windows nothing is downloaded before the user has said so: ask, offering the ffmpeg the
                    // system has. "Download" comes back here with `force` set.
                    #[cfg(not(windows))]
                    if !force && old.is_none() {
                        let system = deps::system().map(|t| {
                            let version = deps::version(&t.ffmpeg);
                            (t, version)
                        });
                        let _ = tx.send(Msg::Choose(system));
                        ctx.request_repaint();
                        return;
                    }
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
        if self.run.is_some() && matches!(self.src, Src::Empty) {
            if let Some(p) = self.debug_second.take() {
                self.load_source(p);
            }
        }
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Setup(status, frac) => self.setup = Setup::Downloading { status, frac },
                #[cfg(not(windows))]
                Msg::Choose(system) => self.setup = Setup::Choose { system },
                Msg::Tools(Ok(t)) => {
                    let (tx, ctx, ff) = (self.tx.clone(), self.ctx.clone(), t.ffmpeg.clone());
                    std::thread::spawn(move || {
                        let _ = tx.send(Msg::Caps(Capabilities::probe(&ff)));
                        ctx.request_repaint();
                    });
                    self.setup = Setup::Ready(t);
                    let paths = std::mem::take(&mut self.pending_open);
                    self.open_files(paths);
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
                    if enc.bit_depth == 10 && !c.has_ten_bit(enc.video_codec.encoder()) {
                        enc.bit_depth = 8;
                        self.dirty = true;
                    }
                    if enc.rate != crate::model::RateMode::Quality && !c.has_bitrate(enc.video_codec) {
                        enc.rate = crate::model::RateMode::Quality;
                        enc.normalize();
                        self.dirty = true;
                    }
                    // Which encoders passed their test, in 8 and 10 bit, and why the others did not.
                    let logs = deps::data_dir().join("logs");
                    let _ = std::fs::create_dir_all(&logs);
                    let _ = std::fs::write(logs.join("encoders.txt"), c.report());
                    self.caps = Some(c);
                }
                Msg::Caps(Err(_)) => {}
                Msg::Picked(paths) => self.open_files(paths),
                Msg::Batch(path, res, settings) => self.batch_arrived(path, res, *settings),
                Msg::PresetImported(res) => match res {
                    Ok(p) => self.take_imported(p),
                    Err(e) => self.note = Some(e),
                },
                Msg::Said(text) => self.note = Some(text),
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
                let stopped = matches!(outcome, Outcome::Cancelled);
                let (report, state) = match outcome {
                    Outcome::Finished { out, bytes, elapsed } => {
                        self.batch.finished_one(out.clone(), run.job.src.size, bytes);
                        (
                            Report::Done { out: out.clone(), from: run.job.src.size, bytes, elapsed },
                            State::Done { out, bytes, secs: run.active().as_secs_f64() },
                        )
                    }
                    Outcome::Failed { code, tail } => {
                        self.batch.failed_one();
                        let why = tail.last().cloned().unwrap_or_else(|| format!("ffmpeg stopped with code {}", code.unwrap_or(-1)));
                        (Report::Failed { code, tail }, State::Failed(why))
                    }
                    Outcome::Cancelled => (Report::Cancelled, State::Stopped),
                };
                if let Some(item) = self.queue.get_mut(run.item) {
                    item.state = state;
                }
                self.queue_save();
                save_log(&name, &run.log);
                self.last = Some(Last { report, name, log: run.log });
                if stopped {
                    self.processing = false;
                } else {
                    self.start_next();
                }
            }
        }
    }

    /// The queue ran dry by itself: tell the user if they are elsewhere, and do what they chose for this moment.
    /// Nothing but the notification happens when every job failed, so a PC does not switch itself off over errors nobody saw.
    fn finish_batch(&mut self) {
        let b = std::mem::take(&mut self.batch);
        if b.ended() == 0 {
            return;
        }
        let power = self.when_done.grace().is_some();
        if !power && !platform::focused(&self.ctx) {
            let (title, text) = b.summary();
            platform::notify(&title, &text);
        }
        if b.done == 0 {
            return;
        }
        match self.when_done {
            WhenDone::Nothing => {}
            WhenDone::OpenFolder => {
                if let Some(dir) = b.last_out.as_deref().and_then(Path::parent) {
                    open_path(dir);
                }
            }
            WhenDone::CloseApp => {
                self.quit_confirmed = true;
                self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            what @ (WhenDone::Sleep | WhenDone::Shutdown) => {
                let wait = what.grace().unwrap_or_default();
                self.power = Some(Power { what, at: Instant::now() + wait });
            }
        }
    }

    /// Carries out a sleep or shutdown whose time has come.
    pub fn tick_power(&mut self) {
        let Some(p) = &self.power else { return };
        if Instant::now() < p.at {
            self.ctx.request_repaint_after(Duration::from_millis(250));
            return;
        }
        let what = p.what;
        self.power = None;
        let ok = match what {
            WhenDone::Sleep => platform::sleep(),
            WhenDone::Shutdown => platform::shutdown(),
            _ => true,
        };
        if !ok {
            self.toast(&format!("Could not {}", what.label().to_lowercase()), "The system did not accept it.", true, false);
        }
    }

    /// Seconds left before the sleep or shutdown, for the window that counts them.
    pub fn power_left(&self) -> Option<(WhenDone, u64)> {
        self.power.as_ref().map(|p| (p.what, p.at.saturating_duration_since(Instant::now()).as_secs() + 1))
    }

    /// Puts the progress of the run on the taskbar button.
    pub fn update_taskbar(&mut self, frame: &eframe::Frame) {
        let state = match &self.run {
            Some(run) => {
                let fraction = run.progress.and_then(|p| p.fraction(run.job.span()));
                after::taskbar_state(fraction, run.is_paused(), self.batch.ended(), self.queue.waiting())
            }
            None => TaskbarState::None,
        };
        self.taskbar.set(frame, state);
    }

    pub fn open_picker(&self) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let pick = rfd::FileDialog::new().add_filter("Video and audio", &MEDIA_EXTS).add_filter("All files", &["*"]).pick_files();
            if let Some(p) = pick.filter(|p| !p.is_empty()) {
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

    /// One file opens in the window to be set up; several go straight into the queue with the settings as they are.
    pub fn open_files(&mut self, paths: Vec<PathBuf>) {
        match paths.as_slice() {
            [] => {}
            [one] => self.load_source(one.clone()),
            _ => self.add_many(paths),
        }
    }

    fn add_many(&mut self, paths: Vec<PathBuf>) {
        let Some(t) = self.tools().cloned() else {
            return;
        };
        let settings = self.set.encode.clone();
        self.refused.clear();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            for path in paths {
                let r = probe::run(&t.ffprobe, &path);
                let _ = tx.send(Msg::Batch(path, r, Box::new(settings.clone())));
                ctx.request_repaint();
            }
        });
    }

    fn batch_arrived(&mut self, path: PathBuf, res: Result<SourceInfo, String>, settings: EncodeSettings) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let job = res.and_then(|info| {
            let (audio_sel, sub_sel) = ((0..info.audio.len()).collect(), (0..info.subs.len()).collect());
            self.job_for(&info, audio_sel, sub_sel, None, &settings)
        });
        match job {
            Ok(job) => {
                self.queue.add(job);
                self.queue_save();
                self.flash_queue();
                self.page = Page::Queue;
                self.qtab = QueueTab::Queued;
            }
            Err(e) => self.refused.push((name, e)),
        }
    }

    /// Why this PC or this ffmpeg cannot do what the settings ask, if it cannot.
    fn unsupported(&self, st: &EncodeSettings) -> Option<String> {
        let c = self.caps.as_ref()?;
        if !st.container.audio_only() {
            let (enc, label) = (st.video_codec.encoder(), st.video_codec.label());
            if !c.has(enc) {
                return Some(format!("{label} is not available here. Pick another video codec."));
            }
            if st.bit_depth >= 10 && !c.has_ten_bit(enc) {
                return Some(format!("10-bit does not work with {label} on this PC. Choose 8-bit or another codec."));
            }
            if st.rate != crate::model::RateMode::Quality && !c.has_bitrate(st.video_codec) {
                return Some(format!("{label} cannot aim for a bitrate or a size on this PC. Use constant quality or another codec."));
            }
        }
        match st.audio_codec.encoder() {
            Some(enc) if !c.has(enc) => Some(format!("{} is not available here. Pick another audio codec.", st.audio_codec.label())),
            _ => None,
        }
    }

    /// A job for `info` with the given choices, or the reason there can be none.
    fn job_for(
        &self,
        info: &SourceInfo,
        audio_sel: Vec<usize>,
        sub_sel: Vec<usize>,
        range: Option<(f64, f64)>,
        st: &EncodeSettings,
    ) -> Result<Job, String> {
        if let Some(why) = self.unsupported(st) {
            return Err(why);
        }
        let out = self.set.output_path_for(&info.path, st.container);
        let job = Job { src: info.clone(), audio_sel, sub_sel, settings: st.clone(), range, out };
        job.validate()?;
        Ok(job)
    }

    /// Puts the file that is open in the window into the queue, and empties the window for the next one.
    pub fn add_current(&mut self, first: bool) -> bool {
        let Src::Ready(src) = &self.src else { return false };
        let name = src.info.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let job = src
            .range()
            .map_err(str::to_string)
            .and_then(|r| self.job_for(&src.info, src.audio_selection(), src.subtitle_selection(), r, &self.set.encode));
        match job {
            Ok(job) => {
                if first {
                    self.queue.add_first(job);
                } else {
                    self.queue.add(job);
                    self.toast(crate::strings::ADDED, &name, false, true);
                    self.flash_queue();
                }
                self.queue_save();
                self.src = Src::Empty;
                true
            }
            Err(why) => {
                self.toast(crate::strings::NOT_ADDED, &format!("{name}: {why}"), true, false);
                false
            }
        }
    }

    /// How many files the Encode button would work through: the open one and everything waiting.
    pub fn to_encode(&self) -> usize {
        self.queue.waiting() + usize::from(matches!(self.src, Src::Ready(_)))
    }

    /// The Encode button: the file in the window goes to the front of the queue, so it is the first one done.
    pub fn encode(&mut self) {
        if self.tools().is_none() || self.run.is_some() {
            return;
        }
        if matches!(self.src, Src::Ready(_)) && !self.add_current(true) {
            return;
        }
        if !self.processing {
            self.batch = Batch::default();
        }
        self.power = None;
        self.processing = true;
        self.start_next();
    }

    /// Starts the job at the top of the queue, unless one is running or a question is open.
    pub fn start_next(&mut self) {
        if !self.processing || self.run.is_some() || self.ask.is_some() {
            return;
        }
        let Some(id) = self.queue.next_waiting() else {
            self.processing = false;
            self.finish_batch();
            return;
        };
        let Some(item) = self.queue.get(id) else { return };
        let job = item.job.clone();
        let refuse = |app: &mut App, why: String| {
            app.batch.failed_one();
            if let Some(item) = app.queue.get_mut(id) {
                item.state = State::Failed(why.clone());
            }
            let name = job.src.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            app.last = Some(Last { report: Report::Failed { code: None, tail: vec![why] }, name, log: vec![] });
            app.queue_save();
            app.start_next();
        };
        if let Err(why) = job.validate() {
            return refuse(self, why);
        }
        if let Some(why) = self.unsupported(&job.settings) {
            return refuse(self, why);
        }
        match settings::resolve(self.set.on_exists, job.out.clone(), &job.src.path) {
            Resolution::Use(p) => {
                self.queue.get_mut(id).expect("just read").job.out = p;
                self.launch(id);
            }
            Resolution::Ask(p) => {
                self.queue.get_mut(id).expect("just read").job.out = p;
                self.ask = Some(id);
            }
            Resolution::Skip => {
                if let Some(item) = self.queue.get_mut(id) {
                    item.state = State::Skipped(job.out.clone());
                }
                let name = job.src.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.last = Some(Last { report: Report::Skipped(job.out.clone()), name, log: vec![] });
                self.queue_save();
                self.start_next();
            }
        }
    }

    pub fn launch(&mut self, item: u64) {
        let Some(tools) = self.tools().cloned() else {
            return;
        };
        let Some(entry) = self.queue.get_mut(item) else { return };
        entry.state = State::Running;
        let job = entry.job.clone();
        let id = self.next_run;
        self.next_run += 1;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let handle = encode::start(job.clone(), tools.ffmpeg, move |e| {
            let _ = tx.send(Msg::Enc(id, e));
            ctx.request_repaint();
        });
        self.last = None;
        self.queue_save();
        self.run = Some(Run {
            id,
            item,
            handle,
            job,
            progress: None,
            started: Instant::now(),
            log: vec![],
            paused_at: None,
            paused_before: Duration::ZERO,
        });
    }

    /// Ends the current job and stops working through the queue; what is still waiting stays in line.
    pub fn stop(&mut self) {
        self.processing = false;
        if let Some(r) = &self.run {
            if r.is_paused() {
                r.handle.resume(); // a suspended process cannot act on being told to end
            }
            r.handle.stop();
        }
    }

    pub fn pause(&mut self) {
        if let Some(r) = &mut self.run {
            if !r.is_paused() && r.handle.pause() {
                r.paused_at = Some(Instant::now());
            }
        }
    }

    pub fn resume(&mut self) {
        if let Some(r) = &mut self.run {
            if let Some(at) = r.paused_at.take() {
                r.paused_before += at.elapsed();
                r.handle.resume();
            }
        }
    }

    /// The program is closing: end the run and make sure no half file is left. The job waits for the next time.
    pub fn shutdown(&mut self) {
        if let Some(r) = self.run.take() {
            r.handle.shutdown(&r.job.part_path());
            if let Some(item) = self.queue.get_mut(r.item) {
                item.state = State::Waiting;
            }
        }
        self.queue_save();
    }

    // ---- presets

    pub fn preset(&self, name: &str) -> Option<&Preset> {
        self.builtin.iter().chain(self.user_presets.iter()).find(|p| p.name == name)
    }

    pub fn is_builtin(&self, name: &str) -> bool {
        self.builtin.iter().any(|p| p.name == name)
    }

    /// The settings have moved away from the preset they started from.
    pub fn preset_modified(&self) -> bool {
        self.set.preset.as_deref().and_then(|n| self.preset(n)).is_some_and(|p| p.settings != self.set.encode)
    }

    pub fn apply_preset(&mut self, name: &str) {
        let Some(p) = self.preset(name).cloned() else { return };
        self.set.encode = p.settings;
        self.set.preset = Some(p.name);
        // A preset made on another PC may ask for an encoder this one does not have.
        self.note = self.unsupported(&self.set.encode);
        self.dirty = true;
    }

    fn reload_presets(&mut self) {
        self.user_presets = presets::load_dir(&presets::dir()).into_iter().map(|(_, p)| p).collect();
    }

    /// Why the name typed in the "Save as" box cannot be used, if it cannot.
    pub fn preset_error(&self, name: &str) -> Option<String> {
        let name = name.trim();
        if name.is_empty() {
            return Some("Give the preset a name.".into());
        }
        self.builtin
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(name))
            .then(|| "A preset that comes with Stiva has this name. Pick another.".into())
    }

    /// Keeps the settings in the window as a preset of the user's own, and selects it.
    pub fn save_preset_as(&mut self, name: &str) -> Result<(), String> {
        if let Some(why) = self.preset_error(name) {
            return Err(why);
        }
        let name = name.trim();
        // The same name in other letters is the same preset: file names on Windows do not tell the two apart.
        let name = self.user_presets.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map_or(name.to_string(), |p| p.name.clone());
        presets::save_in(&presets::dir(), &Preset { name: name.clone(), settings: self.set.encode.clone() })?;
        self.reload_presets();
        self.set.preset = Some(name);
        self.dirty = true;
        Ok(())
    }

    /// The selected preset of the user's own takes the settings now in the window.
    pub fn save_preset_over(&mut self) {
        let Some(name) = self.set.preset.clone().filter(|n| !self.is_builtin(n)) else { return };
        if let Err(e) = presets::save_in(&presets::dir(), &Preset { name, settings: self.set.encode.clone() }) {
            self.note = Some(e);
        }
        self.reload_presets();
    }

    pub fn delete_preset(&mut self, name: &str) {
        if self.is_builtin(name) {
            return;
        }
        if let Err(e) = presets::delete_in(&presets::dir(), name) {
            self.note = Some(e);
        }
        self.reload_presets();
        if self.set.preset.as_deref() == Some(name) {
            self.set.preset = None;
            self.dirty = true;
        }
    }

    pub fn export_preset(&self) {
        let name = self.set.preset.clone().unwrap_or_else(|| "My preset".into());
        let preset = Preset { name: name.clone(), settings: self.set.encode.clone() };
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let pick = rfd::FileDialog::new().add_filter("Stiva preset", &["json"]).set_file_name(presets::file_name(&name)).save_file();
            if let Some(path) = pick {
                let said = match presets::write_file(&path, &preset) {
                    Ok(()) => "The preset was saved to the file.".to_string(),
                    Err(e) => format!("The preset could not be saved: {e}"),
                };
                let _ = tx.send(Msg::Said(said));
                ctx.request_repaint();
            }
        });
    }

    pub fn import_preset(&self) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            if let Some(path) = rfd::FileDialog::new().add_filter("Stiva preset", &["json"]).pick_file() {
                let _ = tx.send(Msg::PresetImported(presets::read_file(&path)));
                ctx.request_repaint();
            }
        });
    }

    /// A preset read from a file joins the user's own, under a free name, and is selected.
    fn take_imported(&mut self, mut p: Preset) {
        let name = {
            let taken: Vec<&str> = self.builtin.iter().chain(&self.user_presets).map(|q| q.name.as_str()).collect();
            presets::unique_name(&taken, &p.name)
        };
        p.name = name.clone();
        match presets::save_in(&presets::dir(), &p) {
            Ok(()) => {
                self.reload_presets();
                self.apply_preset(&name);
            }
            Err(e) => self.note = Some(e),
        }
    }

    pub fn queue_save(&self) {
        self.queue.save(&queue_file());
    }

    pub fn can_encode(&self) -> bool {
        self.tools().is_some() && self.run.is_none() && (matches!(self.src, Src::Ready(_)) || self.queue.waiting() > 0)
    }

    /// Files dropped on the window: one is opened to be set up, several go into the queue.
    pub fn accept_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if self.tools().is_none() {
            return;
        }
        self.open_files(dropped.into_iter().filter(|p| p.is_file()).collect());
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

    /// True when the folder the user chose is the one the system's ffmpeg is in.
    #[cfg(not(windows))]
    pub fn using_system_tools(&self) -> bool {
        let dir = self.set.ffmpeg_dir.as_deref();
        dir.is_some() && self.system_tools.as_ref().and_then(|t| t.ffmpeg.parent()) == dir
    }

    pub fn using_custom_tools(&self) -> bool {
        self.set.ffmpeg_dir.as_deref().and_then(|d| deps::locate(Some(d))).is_some_and(|t| self.tools() == Some(&t))
    }

    /// Keeps what was taken away for a few seconds, and asks for a repaint when they are over.
    fn keep_for_undo(&mut self, what: Undo) {
        self.undo = Some((what, Instant::now() + UNDO_FOR));
        self.ctx.request_repaint_after(UNDO_FOR + Duration::from_millis(50));
    }

    /// Drops the undo and the toast once their time is up.
    pub fn expire_notices(&mut self) {
        let now = Instant::now();
        if self.undo.as_ref().is_some_and(|(_, until)| now >= *until) {
            self.undo = None;
        }
        if self.toast.as_ref().is_some_and(|t| now >= t.until) {
            self.toast = None;
        }
    }

    pub fn toast(&mut self, title: &str, detail: &str, warn: bool, view_queue: bool) {
        self.toast = Some(Toast { title: title.into(), detail: detail.into(), warn, view_queue, until: Instant::now() + TOAST_FOR });
        self.ctx.request_repaint_after(TOAST_FOR + Duration::from_millis(50));
    }

    /// Makes the queue button glow, so the eye finds where the job went.
    pub fn flash_queue(&mut self) {
        self.queue_flash = Some(Instant::now());
        self.ctx.request_repaint();
    }

    /// How much the queue button glows now, from 1 down to 0.
    pub fn flash_level(&self) -> f32 {
        let Some(at) = self.queue_flash else { return 0.0 };
        let t = at.elapsed().as_secs_f32() / FLASH_FOR.as_secs_f32();
        if t >= 1.0 { 0.0 } else { (1.0 - t) * (1.0 - t) }
    }

    /// What the Undo line says, if there is one.
    pub fn undo_text(&self) -> Option<String> {
        self.undo.as_ref().map(|(u, _)| match u {
            Undo::Item(..) => crate::strings::REMOVED_ONE.to_string(),
            Undo::Finished(v) => {
                format!("{} finished {} {}", v.len(), if v.len() == 1 { "job" } else { "jobs" }, crate::strings::REMOVED_MANY)
            }
            Undo::Source(_) => crate::strings::CLOSED_FILE.to_string(),
        })
    }

    pub fn remove_item(&mut self, id: u64) {
        if let Some((item, at)) = self.queue.take(id) {
            self.queue_save();
            self.keep_for_undo(Undo::Item(item, at));
        }
    }

    pub fn clear_finished(&mut self) {
        let taken = self.queue.take_finished();
        if taken.is_empty() {
            return;
        }
        self.queue_save();
        self.keep_for_undo(Undo::Finished(taken));
    }

    /// Closes the file in the window. A file that was read can be got back; a failure has nothing to keep.
    pub fn close_source(&mut self) {
        let old = std::mem::replace(&mut self.src, Src::Empty);
        self.reset_note();
        if matches!(old, Src::Ready(_)) {
            self.keep_for_undo(Undo::Source(Box::new(old)));
        }
    }

    pub fn undo(&mut self) {
        let Some((what, _)) = self.undo.take() else { return };
        match what {
            Undo::Item(item, at) => {
                self.queue.put_back(item, at);
                self.queue_save();
            }
            Undo::Finished(taken) => {
                for (item, at) in taken {
                    self.queue.put_back(item, at);
                }
                self.queue_save();
            }
            Undo::Source(src) => {
                // Only if the window is still empty: a file opened since is not thrown away.
                if matches!(self.src, Src::Empty) {
                    self.src = *src;
                }
            }
        }
    }

    pub fn reset_note(&mut self) {
        self.note = None;
    }

    pub fn dest_mode_next(&self) -> bool {
        self.set.dest_mode == DestMode::NextToSource
    }
}

fn queue_file() -> PathBuf {
    deps::data_dir().join("queue.json")
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
    crate::platform::reveal(p);
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
