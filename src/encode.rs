//! Turns the user's choices into an ffmpeg command line, runs it, and reports progress.
//! ffmpeg writes to `name.ext.part`; the file takes its real name only when the run ends well.

use crate::filters::Crop;
use crate::model::{AudioCodec, Container, EncodeSettings, Rate, RateMode};
use crate::probe::SourceInfo;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Job {
    pub src: SourceInfo,
    /// Which audio tracks to keep, as positions in `src.audio`.
    pub audio_sel: Vec<usize>,
    /// Which subtitle tracks to keep, as positions in `src.subs`.
    #[serde(default)]
    pub sub_sel: Vec<usize>,
    pub settings: EncodeSettings,
    /// Start and end in seconds, when only a part of the file is wanted.
    pub range: Option<(f64, f64)>,
    pub out: PathBuf,
}

impl Job {
    pub fn part_path(&self) -> PathBuf {
        let mut s = self.out.as_os_str().to_owned();
        s.push(".part");
        PathBuf::from(s)
    }

    /// Seconds of media that will be encoded, when known.
    pub fn span(&self) -> Option<f64> {
        match self.range {
            Some((a, b)) => Some(b - a),
            None => self.src.duration,
        }
    }

    /// The audio tracks that go into the file, as positions in `src.audio`.
    fn kept_audio(&self) -> Vec<usize> {
        if self.settings.audio_codec == AudioCodec::None {
            vec![]
        } else {
            self.audio_sel.iter().copied().filter(|&p| p < self.src.audio.len()).collect()
        }
    }

    /// The subtitle tracks that go into the file: only those the container can hold.
    fn kept_subs(&self) -> Vec<usize> {
        let c = self.settings.container;
        if c.audio_only() || self.src.video.is_none() {
            return vec![];
        }
        self.sub_sel.iter().copied().filter(|&p| self.src.subs.get(p).is_some_and(|t| t.fits(c))).collect()
    }

    /// Kilobits a second all the kept audio takes, as far as it can be told before the encode.
    fn audio_kbps_total(&self) -> u32 {
        let st = &self.settings;
        self.kept_audio()
            .iter()
            .map(|&p| match st.audio_codec {
                c if c.has_bitrate() => st.audio_kbps,
                AudioCodec::Flac => self.src.audio[p].bit_rate.map_or(800, |b| (b / 1000) as u32),
                _ => self.src.audio[p].bit_rate.map_or(192, |b| (b / 1000) as u32),
            })
            .sum()
    }

    /// The video bitrate that brings the file to the wanted size: the size spread over the length, minus the
    /// audio and a little for the container.
    pub fn target_kbps(&self) -> Result<u32, String> {
        let secs =
            self.span().filter(|s| *s > 0.0).ok_or("The length of this file is not known, so a size cannot be aimed at. Use a bitrate.")?;
        let total_kbps = f64::from(self.settings.target_mb) * 8192.0 * 0.97 / secs;
        let video = total_kbps - f64::from(self.audio_kbps_total());
        if video < 50.0 {
            return Err("The size is too small for this length with the audio it has. Raise the size or lower the audio bitrate.".into());
        }
        Ok(video.min(200_000.0) as u32)
    }

    /// How the video is steered for this job.
    pub fn video_rate(&self) -> Result<Rate, String> {
        let target = (self.settings.rate == RateMode::TargetSize).then(|| self.target_kbps()).transpose()?;
        Ok(self.settings.rate_with(target))
    }

    /// The reasons this job cannot run, in words for the user.
    pub fn validate(&self) -> Result<(), String> {
        let st = &self.settings;
        let wants_audio = st.audio_codec != AudioCodec::None;
        if st.container.audio_only() {
            if self.src.audio.is_empty() {
                return Err("This file has no audio track, so it cannot become an audio-only file.".into());
            }
            if self.audio_sel.is_empty() {
                return Err("Choose at least one audio track.".into());
            }
        } else if self.src.video.is_none() {
            return Err("This file has no video: pick an audio-only format.".into());
        } else if wants_audio && !self.src.audio.is_empty() && self.audio_sel.is_empty() {
            return Err("Choose at least one audio track, or set the audio to \"No audio\".".into());
        }
        if !st.container.audio_only() && self.src.video.is_some() {
            self.video_rate()?;
        }
        if let Some((a, b)) = self.range {
            if !(a >= 0.0 && b > a) {
                return Err("The end of the range has to be after its start.".into());
            }
            if let Some(d) = self.src.duration {
                if a >= d {
                    return Err("The start of the range is past the end of the file.".into());
                }
            }
        }
        Ok(())
    }
}

/// A scale filter that fits the picture inside `long` x `short`, whichever way the picture is turned.
/// It never enlarges, and both sides stay even, which the encoders need.
pub fn scale_filter((a, b): (u32, u32)) -> String {
    let (long, short) = (a.max(b), a.min(b));
    let f = format!("min(1,min({long}/max(iw,ih),{short}/min(iw,ih)))");
    format!("scale=w='trunc(iw*{f}/2)*2':h='trunc(ih*{f}/2)*2':flags=lanczos")
}

fn secs(v: f64) -> String {
    format!("{v:.3}")
}

/// Which run over the file the arguments are for. With two passes the first only studies the picture, and
/// both name the same statistics file.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pass<'a> {
    Single,
    First(&'a str),
    Second(&'a str),
}

/// The arguments of a single-pass run, without the program name.
#[cfg(test)]
pub fn build_args(job: &Job) -> Vec<OsString> {
    args_for(job, Pass::Single)
}

fn args_for(job: &Job, pass: Pass) -> Vec<OsString> {
    let st = &job.settings;
    let first = matches!(pass, Pass::First(_));
    let mut a: Vec<OsString> = Vec::new();
    let mut o = |items: &[&str]| a.extend(items.iter().map(OsString::from));

    o(&["-hide_banner", "-nostdin", "-nostats", "-y", "-loglevel", "info"]);
    if let Some((start, _)) = job.range {
        if start > 0.0 {
            o(&["-ss", &secs(start)]);
        }
    }
    drop(o);
    a.push("-i".into());
    a.push(job.src.path.clone().into_os_string());
    let mut o = |items: &[&str]| a.extend(items.iter().map(OsString::from));
    if let Some((start, end)) = job.range {
        o(&["-t", &secs(end - start)]);
    }

    let audio_only = st.container.audio_only();
    let video = !audio_only && job.src.video.is_some();
    let keep = if first { vec![] } else { job.kept_audio() };
    let subs = if first { vec![] } else { job.kept_subs() };

    if video {
        o(&["-map", "0:v:0"]);
    }
    for pos in &keep {
        o(&["-map", &format!("0:a:{pos}")]);
    }
    for pos in &subs {
        o(&["-map", &format!("0:s:{pos}")]);
    }
    if first {
        o(&["-an", "-sn"]);
    } else {
        o(&["-map_metadata", "0"]);
        o(&["-map_chapters", if audio_only { "-1" } else { "0" }]);
    }

    if video {
        let codec = st.video_codec;
        if let Some(chain) = st.filters.chain(st.max_size.map(scale_filter)) {
            o(&["-vf", &chain]);
        }
        let rate = job.video_rate().unwrap_or(Rate::Quality(st.crf));
        let mut args = codec.args(rate, &st.preset, st.bit_depth);
        match pass {
            Pass::Single => {}
            Pass::First(log) => args.extend(["-pass".to_string(), "1".to_string(), "-passlogfile".to_string(), log.to_string()]),
            Pass::Second(log) => args.extend(["-pass".to_string(), "2".to_string(), "-passlogfile".to_string(), log.to_string()]),
        }
        if codec.is_hevc() && st.container == Container::Mp4 {
            // Without this tag Apple players and Windows' own player do not recognise H.265 in MP4.
            args.extend(["-tag:v".to_string(), "hvc1".to_string()]);
        }
        o(&args.iter().map(String::as_str).collect::<Vec<_>>());
    }

    if !first {
        match st.audio_codec {
            AudioCodec::None => o(&["-an"]),
            AudioCodec::Copy => o(&["-c:a", "copy"]),
            codec => {
                if let Some(enc) = codec.encoder() {
                    o(&["-c:a", enc]);
                }
                if codec.has_bitrate() {
                    o(&["-b:a", &format!("{}k", st.audio_kbps)]);
                }
                // Never more channels than the track has: a stereo track is not stretched to 5.1.
                if let Some(n) = st.mixdown.channels() {
                    for (k, pos) in keep.iter().enumerate() {
                        if job.src.audio[*pos].channels > n {
                            o(&[&format!("-ac:a:{k}"), &n.to_string()]);
                        }
                    }
                }
                if st.gain_db.abs() > 0.001 {
                    o(&["-af", &format!("volume={}dB", st.gain_db)]);
                }
            }
        }
        if !subs.is_empty() {
            // MP4 holds only its own kind of text subtitles; MKV takes them as they are.
            o(&["-c:s", if st.container == Container::Mp4 { "mov_text" } else { "copy" }]);
        }
        if matches!(st.container, Container::Mp4 | Container::M4a) {
            o(&["-movflags", "+faststart"]);
        }
        o(&["-f", st.container.muxer()]);
    } else {
        o(&["-f", "null"]);
    }
    o(&["-progress", "pipe:1", "-stats_period", "0.5"]);
    drop(o);
    a.push(if first { OsString::from("-") } else { job.part_path().into_os_string() });
    a
}

/// The command as one line, for the log.
pub fn command_line(ffmpeg: &Path, args: &[OsString]) -> String {
    let q = |s: &std::ffi::OsStr| {
        let t = s.to_string_lossy();
        if t.contains(' ') { format!("\"{t}\"") } else { t.into_owned() }
    };
    std::iter::once(q(ffmpeg.as_os_str())).chain(args.iter().map(|x| q(x))).collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
    /// How much of the media has been written, in seconds.
    pub out_secs: f64,
    pub fps: f32,
    /// 1.0 is real time.
    pub speed: f32,
    pub size: u64,
    pub kbps: f32,
    pub frame: u64,
    pub ended: bool,
    /// Which run over the file this is, of how many; both 0 until a run says so.
    pub pass: u8,
    pub passes: u8,
}

impl Progress {
    /// 0..=1 of the whole job, or None when the length is not known. With two passes each is half of it.
    pub fn fraction(&self, span: Option<f64>) -> Option<f32> {
        let passes = f32::from(self.passes.max(1));
        let done = f32::from(self.pass.max(1) - 1);
        span.filter(|s| *s > 0.0).map(|s| ((done + ((self.out_secs / s) as f32).clamp(0.0, 1.0)) / passes).clamp(0.0, 1.0))
    }

    /// Seconds left at the current speed.
    pub fn eta(&self, span: Option<f64>) -> Option<f64> {
        let s = span?;
        let later = f64::from(self.passes.max(1) - self.pass.max(1)) * s;
        (self.speed > 0.01 && self.out_secs > 0.0).then(|| ((s - self.out_secs + later) / self.speed as f64).max(0.0))
    }
}

/// Reads the `key=value` lines of `-progress`. A block ends with `progress=continue` or `progress=end`.
#[derive(Default)]
pub struct ProgressParser {
    cur: Progress,
}

impl ProgressParser {
    pub fn feed(&mut self, line: &str) -> Option<Progress> {
        let (k, v) = line.trim().split_once('=')?;
        let v = v.trim();
        let p = &mut self.cur;
        match k {
            "frame" => p.frame = v.parse().unwrap_or(p.frame),
            "fps" => p.fps = v.parse().unwrap_or(p.fps),
            // Both of these are in microseconds, whatever the name says.
            "out_time_us" | "out_time_ms" => {
                if let Ok(us) = v.parse::<i64>() {
                    p.out_secs = (us.max(0) as f64) / 1e6;
                }
            }
            "total_size" => p.size = v.parse().unwrap_or(p.size),
            "bitrate" => p.kbps = v.trim_end_matches("kbits/s").trim().parse().unwrap_or(p.kbps),
            "speed" => p.speed = v.trim_end_matches('x').trim().parse().unwrap_or(p.speed),
            "progress" => {
                p.ended = v == "end";
                return Some(*p);
            }
            _ => {}
        }
        None
    }
}

#[derive(Debug)]
pub enum Outcome {
    Finished { out: PathBuf, bytes: u64, elapsed: Duration },
    Failed { code: Option<i32>, tail: Vec<String> },
    Cancelled,
}

pub enum Event {
    Progress(Progress),
    Log(String),
    Done(Outcome),
}

pub struct Handle {
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
}

impl Handle {
    /// Ends the run. The half-written file is deleted by the thread that owns the run.
    pub fn stop(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut g) = self.child.lock() {
            if let Some(c) = g.as_mut() {
                let _ = c.kill();
            }
        }
    }
}

impl Handle {
    /// Holds the run still without ending it: the ffmpeg process is suspended, so it uses no processor.
    pub fn pause(&self) -> bool {
        self.suspend(true)
    }

    pub fn resume(&self) -> bool {
        self.suspend(false)
    }

    fn suspend(&self, on: bool) -> bool {
        self.child.lock().ok().and_then(|g| g.as_ref().map(|c| suspend::set(c, on))).unwrap_or(false)
    }
}

#[cfg(windows)]
mod suspend {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtSuspendProcess(process: *mut c_void) -> i32;
        fn NtResumeProcess(process: *mut c_void) -> i32;
    }

    pub fn set(child: &Child, on: bool) -> bool {
        let handle = child.as_raw_handle();
        // SAFETY: the handle belongs to the child, which `Child` keeps open for as long as it is alive.
        let status = unsafe { if on { NtSuspendProcess(handle) } else { NtResumeProcess(handle) } };
        status >= 0
    }
}

/// Only for trying the pause on the build machine; the program itself is for Windows.
#[cfg(not(windows))]
mod suspend {
    use std::process::{Child, Command};

    pub fn set(child: &Child, on: bool) -> bool {
        Command::new("kill").args([if on { "-STOP" } else { "-CONT" }, &child.id().to_string()]).status().is_ok_and(|s| s.success())
    }
}

impl Handle {
    /// For the moment the program closes: kills ffmpeg, waits until it is gone and deletes the half file,
    /// because the thread that would do it may not get to run before the program ends.
    pub fn shutdown(&self, part: &Path) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut g) = self.child.lock() {
            if let Some(c) = g.as_mut() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        let _ = std::fs::remove_file(part);
    }
}

/// A folder for the statistics of a two-pass encode, removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Scratch> {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("stiva-pass-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How one run of ffmpeg ended.
struct Ended {
    success: bool,
    code: Option<i32>,
    /// The last lines ffmpeg said, which explain a failure.
    tail: Vec<String>,
}

/// Runs ffmpeg once, passing its progress and its words on through `emit`, and waits for it to end.
fn run_ffmpeg<E: Fn(Event) + Send + Sync + 'static>(
    ffmpeg: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    pass: (u8, u8),
    child: &Arc<Mutex<Option<Child>>>,
    cancelled: &AtomicBool,
    emit: &Arc<E>,
) -> Result<Ended, String> {
    emit(Event::Log(command_line(ffmpeg, args)));
    let mut cmd = Command::new(ffmpeg);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    crate::deps::quiet(&mut cmd);
    let mut proc = cmd.spawn().map_err(|e| format!("ffmpeg could not start: {e}"))?;
    let stdout = proc.stdout.take();
    let stderr = proc.stderr.take();
    *child.lock().unwrap() = Some(proc);
    // A stop that came while this run was starting found no process to end: end it now.
    if cancelled.load(Ordering::SeqCst) {
        if let Some(c) = child.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }

    // ffmpeg's own messages: kept for the log, and the last ones explain a failure.
    let err_emit = emit.clone();
    let err_thread = std::thread::spawn(move || {
        let mut tail: Vec<String> = Vec::new();
        if let Some(e) = stderr {
            for raw in BufReader::new(e).split(b'\n').map_while(Result::ok) {
                let line = String::from_utf8_lossy(&raw).trim_end().to_string();
                if line.is_empty() {
                    continue;
                }
                tail.push(line.clone());
                if tail.len() > 14 {
                    tail.remove(0);
                }
                err_emit(Event::Log(line));
            }
        }
        tail
    });

    let mut parser = ProgressParser::default();
    if let Some(o) = stdout {
        for raw in BufReader::new(o).split(b'\n').map_while(Result::ok) {
            if let Some(mut p) = parser.feed(&String::from_utf8_lossy(&raw)) {
                (p.pass, p.passes) = pass;
                emit(Event::Progress(p));
            }
        }
    }
    let tail = err_thread.join().unwrap_or_default();
    let status = child.lock().unwrap().as_mut().map(|c| c.wait());
    match status {
        Some(Ok(s)) => Ok(Ended { success: s.success(), code: s.code(), tail }),
        _ => Err("ffmpeg ended in an unknown way".into()),
    }
}

/// Starts the run on its own thread. Everything it has to say comes through `emit`.
pub fn start(job: Job, ffmpeg: PathBuf, emit: impl Fn(Event) + Send + Sync + 'static) -> Handle {
    let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let cancelled = Arc::new(AtomicBool::new(false));
    let handle = Handle { child: child.clone(), cancelled: cancelled.clone() };
    let emit = Arc::new(emit);
    std::thread::spawn(move || {
        let failed = |msg: String| emit(Event::Done(Outcome::Failed { code: None, tail: vec![msg] }));
        let mut job = job;
        // The statistics folder of a two-pass run is worked in, so absolute paths keep the files findable.
        job.src.path = std::path::absolute(&job.src.path).unwrap_or(job.src.path);
        job.out = std::path::absolute(&job.out).unwrap_or(job.out);
        let part = job.part_path();
        if let Some(dir) = part.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                return failed(format!("Cannot create the folder {}: {e}", dir.display()));
            }
        }
        let started = Instant::now();

        let video = !job.settings.container.audio_only() && job.src.video.is_some();
        if video && job.settings.filters.crop == Crop::Auto {
            emit(Event::Log("Looking for black bars...".into()));
            let frame = job.src.video.as_ref().map_or((0, 0), |v| (v.width, v.height));
            let found = crate::filters::find_black_bars(&ffmpeg, &job.src.path, job.src.duration, frame);
            emit(Event::Log(match found {
                Some(m) => format!("Black bars found: cutting {} left, {} top, {} right, {} bottom.", m.left, m.top, m.right, m.bottom),
                None => "No black bars found, the picture is left whole.".into(),
            }));
            job.settings.filters = job.settings.filters.with_crop_found(found);
        }

        let two = video && job.settings.two_pass && job.settings.rate != RateMode::Quality && job.settings.video_codec.can_two_pass();
        let scratch = if two {
            match Scratch::new() {
                Ok(s) => Some(s),
                Err(e) => return failed(format!("Cannot make a temporary folder for the two passes: {e}")),
            }
        } else {
            None
        };
        let log = "stiva2pass";
        let runs: Vec<(Pass, Option<&Path>)> = match &scratch {
            Some(s) => vec![(Pass::First(log), Some(s.0.as_path())), (Pass::Second(log), Some(s.0.as_path()))],
            None => vec![(Pass::Single, None)],
        };
        let total = runs.len() as u8;

        let mut last = None;
        for (n, (pass, cwd)) in runs.into_iter().enumerate() {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            if total > 1 {
                emit(Event::Log(format!("--- pass {} of {total} ---", n + 1)));
            }
            let ended = match run_ffmpeg(&ffmpeg, &args_for(&job, pass), cwd, (n as u8 + 1, total), &child, &cancelled, &emit) {
                Ok(e) => e,
                Err(msg) => {
                    let _ = std::fs::remove_file(&part);
                    return failed(msg);
                }
            };
            let ok = ended.success;
            last = Some(ended);
            if !ok {
                break;
            }
        }

        if cancelled.load(Ordering::SeqCst) {
            let _ = std::fs::remove_file(&part);
            return emit(Event::Done(Outcome::Cancelled));
        }
        match last {
            Some(e) if e.success => match std::fs::rename(&part, &job.out) {
                Ok(()) => {
                    let bytes = std::fs::metadata(&job.out).map(|m| m.len()).unwrap_or(0);
                    emit(Event::Done(Outcome::Finished { out: job.out.clone(), bytes, elapsed: started.elapsed() }));
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    failed(format!("The file was encoded but could not be named {}: {e}", job.out.display()));
                }
            },
            Some(e) => {
                let _ = std::fs::remove_file(&part);
                emit(Event::Done(Outcome::Failed { code: e.code, tail: e.tail }));
            }
            None => {
                let _ = std::fs::remove_file(&part);
                failed("ffmpeg did not run".into());
            }
        }
    });
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Mixdown, VideoCodec};
    use crate::probe::{AudioTrack, VideoInfo};

    fn source() -> SourceInfo {
        SourceInfo {
            path: PathBuf::from("/in/film.avi"),
            duration: Some(100.0),
            video: Some(VideoInfo { codec: "h264".into(), width: 1280, height: 720, fps: 25.0, ..Default::default() }),
            audio: vec![
                AudioTrack { index: 1, channels: 6, ..Default::default() },
                AudioTrack { index: 2, channels: 2, ..Default::default() },
            ],
            ..Default::default()
        }
    }

    fn job(settings: EncodeSettings) -> Job {
        Job { src: source(), audio_sel: vec![0, 1], sub_sel: vec![], settings, range: None, out: PathBuf::from("/out/film.mp4") }
    }

    fn line(j: &Job) -> String {
        build_args(j).iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn a_plain_mp4_encode() {
        let l = line(&job(EncodeSettings::default()));
        assert!(l.starts_with("-hide_banner -nostdin -nostats -y -loglevel info -i /in/film.avi "), "{l}");
        assert!(l.contains("-map 0:v:0 -map 0:a:0 -map 0:a:1 "), "{l}");
        assert!(l.contains("-c:v libx264 -preset medium -crf 22 -pix_fmt yuv420p"), "{l}");
        assert!(l.contains("-c:a aac -b:a 160k"), "{l}");
        assert!(l.contains("-movflags +faststart -f mp4 -progress pipe:1 -stats_period 0.5 /out/film.mp4.part"), "{l}");
        assert!(l.contains("-map_chapters 0"), "{l}");
    }

    #[test]
    fn the_range_cuts_before_and_after_the_input() {
        let mut j = job(EncodeSettings::default());
        j.range = Some((10.0, 70.5));
        let l = line(&j);
        assert!(l.contains("-ss 10.000 -i /in/film.avi -t 60.500"), "{l}");
        assert_eq!(j.span(), Some(60.5));
        j.range = Some((0.0, 20.0));
        assert!(!line(&j).contains("-ss"), "a start at zero needs no seek");
    }

    #[test]
    fn each_codec_gets_its_own_options() {
        let mut s = EncodeSettings::default();
        s.set_video_codec(VideoCodec::X265);
        assert!(
            line(&job(s.clone())).contains("-c:v libx265 -preset medium -crf 24 -pix_fmt yuv420p -x265-params log-level=error -tag:v hvc1")
        );
        s.container = Container::Mkv;
        assert!(!line(&job(s.clone())).contains("hvc1"), "the tag is for MP4 only");
        let mut w = EncodeSettings { container: Container::WebM, ..Default::default() };
        w.normalize();
        let l = line(&job(w));
        assert!(l.contains("-c:v libvpx-vp9 -crf 32 -b:v 0 -row-mt 1 -cpu-used 2"), "{l}");
        assert!(l.contains("-c:a libopus -b:a 128k") && l.contains("-f webm"), "{l}");
        let mut a = EncodeSettings::default();
        a.set_video_codec(VideoCodec::SvtAv1);
        assert!(line(&job(a)).contains("-c:v libsvtav1 -preset 6 -crf 30"));
    }

    #[test]
    fn audio_only_has_no_video_and_no_chapters() {
        let mut s = EncodeSettings { container: Container::Mp3, ..Default::default() };
        s.normalize();
        let mut j = job(s);
        j.out = PathBuf::from("/out/film.mp3");
        j.audio_sel = vec![1];
        let l = line(&j);
        assert!(!l.contains("0:v:0") && !l.contains("-c:v"), "{l}");
        assert!(l.contains("-map 0:a:1") && !l.contains("0:a:0"), "{l}");
        assert!(l.contains("-map_chapters -1") && l.contains("-c:a libmp3lame") && l.contains("-f mp3"), "{l}");
    }

    #[test]
    fn mixdown_never_adds_channels() {
        let l = line(&job(EncodeSettings { mixdown: Mixdown::Stereo, ..Default::default() }));
        assert!(l.contains("-ac:a:0 2"), "the 5.1 track is mixed down: {l}");
        assert!(!l.contains("-ac:a:1"), "the stereo track is left alone: {l}");
        let l = line(&job(EncodeSettings { mixdown: Mixdown::Surround51, ..Default::default() }));
        assert!(!l.contains("-ac:a"), "{l}");
        let l = line(&job(EncodeSettings { gain_db: -3.5, ..Default::default() }));
        assert!(l.contains("-af volume=-3.5dB"), "{l}");
    }

    #[test]
    fn no_audio_and_copy() {
        let l = line(&job(EncodeSettings { audio_codec: AudioCodec::None, ..Default::default() }));
        assert!(l.contains("-an") && !l.contains("0:a:"), "{l}");
        let l = line(&job(EncodeSettings { audio_codec: AudioCodec::Copy, ..Default::default() }));
        assert!(l.contains("-c:a copy") && !l.contains("-b:a"), "{l}");
    }

    #[test]
    fn the_selection_of_tracks_is_followed_and_stale_positions_are_dropped() {
        let mut j = job(EncodeSettings::default());
        j.audio_sel = vec![1, 7];
        let l = line(&j);
        assert!(l.contains("-map 0:a:1") && !l.contains("0:a:0") && !l.contains("0:a:7"), "{l}");
    }

    #[test]
    fn jobs_that_cannot_work_are_refused_with_a_reason() {
        let ok = job(EncodeSettings::default());
        assert!(ok.validate().is_ok());
        let mut j = ok.clone();
        j.range = Some((50.0, 40.0));
        assert!(j.validate().unwrap_err().contains("after its start"));
        j.range = Some((200.0, 300.0));
        assert!(j.validate().unwrap_err().contains("past the end"));
        let mut j = ok.clone();
        j.audio_sel.clear();
        assert!(j.validate().unwrap_err().contains("audio track"));
        let mut j = ok.clone();
        j.src.video = None;
        assert!(j.validate().unwrap_err().contains("no video"));
        let mut j = ok.clone();
        j.settings = EncodeSettings { container: Container::Flac, ..Default::default() };
        j.src.audio.clear();
        assert!(j.validate().unwrap_err().contains("no audio"));
    }

    #[test]
    fn progress_blocks_are_read() {
        let text = "frame=120\nfps=48.5\nstream_0_0_q=-1.0\nbitrate=  812.3kbits/s\ntotal_size=1048576\nout_time_us=4800000\nout_time_ms=4800000\nout_time=00:00:04.800000\ndup_frames=0\nspeed=1.92x\nprogress=continue\n";
        let mut p = ProgressParser::default();
        let mut got = vec![];
        for l in text.lines() {
            if let Some(x) = p.feed(l) {
                got.push(x);
            }
        }
        assert_eq!(got.len(), 1);
        let x = got[0];
        assert_eq!((x.frame, x.size), (120, 1_048_576));
        assert!((x.out_secs - 4.8).abs() < 1e-9 && (x.speed - 1.92).abs() < 1e-6 && (x.kbps - 812.3).abs() < 1e-3);
        assert!(!x.ended);
        assert_eq!(x.fraction(Some(9.6)), Some(0.5));
        assert_eq!(x.fraction(None), None);
        let eta = x.eta(Some(9.6)).unwrap();
        assert!((eta - 2.5).abs() < 1e-6);
        let end = p.feed("progress=end").unwrap();
        assert!(end.ended);
    }

    #[test]
    fn unreadable_values_keep_the_last_good_ones() {
        let mut p = ProgressParser::default();
        p.feed("out_time_us=2000000");
        p.feed("speed=N/A");
        p.feed("out_time_us=N/A");
        let x = p.feed("progress=continue").unwrap();
        assert!((x.out_secs - 2.0).abs() < 1e-9);
        assert_eq!(x.speed, 0.0);
        assert_eq!(x.eta(Some(10.0)), None, "no speed, no estimate");
    }

    #[test]
    fn part_files_sit_next_to_the_result() {
        assert_eq!(job(EncodeSettings::default()).part_path(), PathBuf::from("/out/film.mp4.part"));
    }

    // Runs the real ffmpeg on a tiny generated clip. Skipped when no ffmpeg can be found.
    // On the build machine: STIVA_FFDIR=/usr/bin cargo test
    #[test]
    fn real_encodes_produce_files_ffprobe_understands() {
        let Some(tools) = crate::deps::locate(None) else {
            eprintln!("no ffmpeg here, real encodes skipped");
            return;
        };
        let dir = std::env::temp_dir().join(format!("stiva-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let clip = dir.join("clip.mkv");
        let mut maker = Command::new(&tools.ffmpeg);
        maker
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x180:rate=25:duration=3",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:duration=3",
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map",
                "2:a",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-c:a",
                "aac",
                "-ac:a:0",
                "2",
            ])
            .arg(&clip);
        assert!(maker.status().unwrap().success(), "the test clip could not be made");
        let src = crate::probe::run(&tools.ffprobe, &clip).unwrap();
        assert_eq!(src.audio.len(), 2);
        assert!(src.duration.unwrap() > 2.5);

        let cases: Vec<(&str, Container, VideoCodec, AudioCodec, &str)> = vec![
            ("mp4-x264", Container::Mp4, VideoCodec::X264, AudioCodec::Aac, "h264"),
            ("mkv-x265", Container::Mkv, VideoCodec::X265, AudioCodec::Opus, "hevc"),
            ("mp4-x265", Container::Mp4, VideoCodec::X265, AudioCodec::Aac, "hevc"),
            ("webm-vp9", Container::WebM, VideoCodec::Vp9, AudioCodec::Opus, "vp9"),
            ("mkv-av1", Container::Mkv, VideoCodec::SvtAv1, AudioCodec::Flac, "av1"),
            ("mp4-copy", Container::Mp4, VideoCodec::X264, AudioCodec::Copy, "h264"),
        ];
        for (name, container, vc, ac, want_codec) in cases {
            let mut s = EncodeSettings { container, ..Default::default() };
            s.set_video_codec(vc);
            s.set_audio_codec(ac);
            s.preset = vc.presets()[vc.presets().len() - 1].to_string(); // the fastest, to keep the test short
            s.normalize();
            let out = dir.join(format!("{name}.{}", container.ext()));
            let job = Job { src: src.clone(), audio_sel: vec![0, 1], sub_sel: vec![], settings: s, range: None, out: out.clone() };
            job.validate().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let _h = start(job, tools.ffmpeg.clone(), move |e| {
                let _ = tx.send(e);
            });
            let mut saw_progress = false;
            let outcome = loop {
                match rx.recv_timeout(Duration::from_secs(120)).expect("ffmpeg did not finish") {
                    Event::Progress(_) => saw_progress = true,
                    Event::Log(_) => {}
                    Event::Done(o) => break o,
                }
            };
            match outcome {
                Outcome::Finished { bytes, .. } => assert!(bytes > 0, "{name}"),
                other => panic!("{name}: {other:?}"),
            }
            assert!(saw_progress, "{name}: no progress was reported");
            assert!(!job_part_exists(&out), "{name}: the .part file is gone");
            let got = crate::probe::run(&tools.ffprobe, &out).unwrap();
            assert_eq!(got.video.as_ref().map(|v| v.codec.as_str()), Some(want_codec), "{name}");
            assert_eq!(got.audio.len(), 2, "{name}: both audio tracks kept");
            assert!((got.duration.unwrap() - 3.0).abs() < 0.5, "{name}: duration {:?}", got.duration);
        }

        // A part of the file, as audio only, with the second track.
        let out = dir.join("part.mp3");
        let mut s = EncodeSettings { container: Container::Mp3, ..Default::default() };
        s.normalize();
        let job = Job { src: src.clone(), audio_sel: vec![1], sub_sel: vec![], settings: s, range: Some((1.0, 2.0)), out: out.clone() };
        let (tx, rx) = std::sync::mpsc::channel();
        start(job, tools.ffmpeg.clone(), move |e| {
            let _ = tx.send(e);
        });
        let outcome = loop {
            if let Event::Done(o) = rx.recv_timeout(Duration::from_secs(60)).unwrap() {
                break o;
            }
        };
        assert!(matches!(outcome, Outcome::Finished { .. }), "{outcome:?}");
        let got = crate::probe::run(&tools.ffprobe, &out).unwrap();
        assert!(got.video.is_none() && got.audio.len() == 1);
        assert!((got.duration.unwrap() - 1.0).abs() < 0.2, "{:?}", got.duration);

        // A broken input fails with ffmpeg's own words and leaves no file behind.
        let bad = dir.join("bad.mkv");
        std::fs::write(&bad, b"this is not a video").unwrap();
        let mut broken = src.clone();
        broken.path = bad;
        let out = dir.join("never.mp4");
        let job =
            Job { src: broken, audio_sel: vec![0], sub_sel: vec![], settings: EncodeSettings::default(), range: None, out: out.clone() };
        let (tx, rx) = std::sync::mpsc::channel();
        start(job, tools.ffmpeg.clone(), move |e| {
            let _ = tx.send(e);
        });
        let outcome = loop {
            if let Event::Done(o) = rx.recv_timeout(Duration::from_secs(60)).unwrap() {
                break o;
            }
        };
        match outcome {
            Outcome::Failed { tail, .. } => assert!(!tail.is_empty()),
            other => panic!("{other:?}"),
        }
        assert!(!out.exists() && !job_part_exists(&out));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stopping_ends_the_run_and_removes_the_part_file() {
        let Some(tools) = crate::deps::locate(None) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("stiva-stop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let clip = dir.join("long.mkv");
        // A few seconds of 720p, encoded with the slowest preset, so the run is still going when it is stopped.
        let mut maker = Command::new(&tools.ffmpeg);
        maker
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=1280x720:rate=30:duration=4",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
            ])
            .arg(&clip);
        assert!(maker.status().unwrap().success());
        let src = crate::probe::run(&tools.ffprobe, &clip).unwrap();
        let mut s = EncodeSettings::default();
        s.preset = "veryslow".into();
        s.crf = 10;
        let out = dir.join("out.mp4");
        let job = Job { src, audio_sel: vec![], sub_sel: vec![], settings: s, range: None, out: out.clone() };
        let (tx, rx) = std::sync::mpsc::channel();
        let h = start(job, tools.ffmpeg.clone(), move |e| {
            let _ = tx.send(e);
        });
        std::thread::sleep(Duration::from_millis(300));
        h.stop();
        let outcome = loop {
            if let Event::Done(o) = rx.recv_timeout(Duration::from_secs(60)).unwrap() {
                break o;
            }
        };
        // A very fast machine may have finished first; both ends are fine, but never a half file.
        assert!(matches!(outcome, Outcome::Cancelled | Outcome::Finished { .. }), "{outcome:?}");
        assert!(!job_part_exists(&out));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A generated clip of the given size, in `dir`.
    fn clip(tools: &crate::deps::Tools, dir: &Path, name: &str, size: &str, secs: u32) -> SourceInfo {
        let path = dir.join(name);
        let ok = Command::new(&tools.ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", &format!("testsrc2=size={size}:rate=25:duration={secs}")])
            .args(["-c:v", "libx264", "-preset", "ultrafast"])
            .arg(&path)
            .status()
            .unwrap()
            .success();
        assert!(ok, "the clip {name} could not be made");
        crate::probe::run(&tools.ffprobe, &path).unwrap()
    }

    fn run_to_the_end(tools: &crate::deps::Tools, job: Job) -> Outcome {
        let (tx, rx) = std::sync::mpsc::channel();
        start(job, tools.ffmpeg.clone(), move |e| {
            let _ = tx.send(e);
        });
        loop {
            if let Event::Done(o) = rx.recv_timeout(Duration::from_secs(120)).expect("ffmpeg did not finish") {
                break o;
            }
        }
    }

    fn pix_fmt(tools: &crate::deps::Tools, file: &Path) -> String {
        let out = Command::new(&tools.ffprobe)
            .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=pix_fmt", "-of", "csv=p=0"])
            .arg(file)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn the_scale_filter_fits_the_box_never_enlarges_and_follows_the_turn_of_the_picture() {
        let Some(tools) = crate::deps::locate(None) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("stiva-size-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // (clip size, expected size inside a 640 x 360 box: long side 640 at most, short side 360 at most, both even)
        for (size, want) in [("1280x720", (640, 360)), ("720x1280", (360, 640)), ("320x180", (320, 180)), ("1000x562", (640, 358))] {
            let src = clip(&tools, &dir, &format!("{size}.mkv"), size, 1);
            let mut s = EncodeSettings { max_size: Some((640, 360)), ..Default::default() };
            s.preset = "ultrafast".into();
            let out = dir.join(format!("{size}-out.mp4"));
            let outcome =
                run_to_the_end(&tools, Job { src, audio_sel: vec![], sub_sel: vec![], settings: s, range: None, out: out.clone() });
            assert!(matches!(outcome, Outcome::Finished { .. }), "{size}: {outcome:?}");
            let v = crate::probe::run(&tools.ffprobe, &out).unwrap().video.unwrap();
            assert_eq!((v.width, v.height), want, "{size}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ten_bit_encodes_come_out_in_ten_bit_and_eight_bit_ones_in_eight() {
        let Some(tools) = crate::deps::locate(None) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("stiva-bits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = clip(&tools, &dir, "in.mkv", "320x180", 1);
        for (vc, container) in
            [(VideoCodec::X264, Container::Mkv), (VideoCodec::X265, Container::Mp4), (VideoCodec::SvtAv1, Container::Mkv)]
        {
            for bits in [8u8, 10] {
                let mut s = EncodeSettings { container, bit_depth: bits, ..Default::default() };
                s.set_video_codec(vc);
                s.preset = vc.presets()[vc.presets().len() - 1].to_string();
                s.bit_depth = bits;
                s.normalize();
                let out = dir.join(format!("{vc:?}-{bits}.{}", container.ext()));
                let outcome = run_to_the_end(
                    &tools,
                    Job { src: src.clone(), audio_sel: vec![], sub_sel: vec![], settings: s, range: None, out: out.clone() },
                );
                assert!(matches!(outcome, Outcome::Finished { .. }), "{vc:?} {bits}: {outcome:?}");
                let want = if bits == 10 { "yuv420p10le" } else { "yuv420p" };
                assert_eq!(pix_fmt(&tools, &out), want, "{vc:?} {bits}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pausing_holds_the_run_still_and_resuming_lets_it_finish() {
        let Some(tools) = crate::deps::locate(None) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("stiva-pause-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = clip(&tools, &dir, "long.mkv", "1280x720", 6);
        let mut s = EncodeSettings::default();
        s.preset = "veryslow".into();
        let out = dir.join("out.mp4");
        let (tx, rx) = std::sync::mpsc::channel();
        let h = start(
            Job { src, audio_sel: vec![], sub_sel: vec![], settings: s, range: None, out: out.clone() },
            tools.ffmpeg.clone(),
            move |e| {
                let _ = tx.send(e);
            },
        );
        // Wait for the first progress, so the process surely exists, then hold it.
        let mut seen = 0.0;
        while seen == 0.0 {
            if let Event::Progress(p) = rx.recv_timeout(Duration::from_secs(60)).unwrap() {
                seen = p.out_secs;
            }
        }
        assert!(h.pause(), "the process could not be suspended");
        std::thread::sleep(Duration::from_millis(700)); // lets what was already written come through
        while rx.try_recv().is_ok() {}
        let before = std::fs::metadata(job_part(&out)).map(|m| m.len()).unwrap_or(0);
        std::thread::sleep(Duration::from_millis(1500));
        assert!(rx.try_recv().is_err(), "no progress while paused");
        assert_eq!(std::fs::metadata(job_part(&out)).map(|m| m.len()).unwrap_or(0), before, "the file does not grow while paused");
        assert!(h.resume());
        let outcome = loop {
            if let Event::Done(o) = rx.recv_timeout(Duration::from_secs(120)).unwrap() {
                break o;
            }
        };
        assert!(matches!(outcome, Outcome::Finished { .. }), "{outcome:?}");
        assert!(out.exists() && !job_part_exists(&out));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn job_part(out: &Path) -> PathBuf {
        let mut s = out.as_os_str().to_owned();
        s.push(".part");
        PathBuf::from(s)
    }

    fn job_part_exists(out: &Path) -> bool {
        let mut s = out.as_os_str().to_owned();
        s.push(".part");
        PathBuf::from(s).exists()
    }

    fn sub(codec: &str) -> crate::probe::SubTrack {
        crate::probe::SubTrack { codec: codec.into(), ..Default::default() }
    }

    #[test]
    fn filters_and_the_size_limit_form_one_chain() {
        let mut s = EncodeSettings::default();
        s.max_size = Some((1280, 720));
        s.filters.denoise = crate::filters::Strength::Light;
        s.filters.grayscale = true;
        let l = line(&job(s));
        assert!(l.contains("-vf hqdn3d=2:1:2:3,hue=s=0,scale=w="), "{l}");
        let none = line(&job(EncodeSettings::default()));
        assert!(!none.contains("-vf"), "no filter, no -vf: {none}");
    }

    #[test]
    fn an_average_bitrate_replaces_the_quality() {
        let mut s = EncodeSettings { rate: RateMode::Bitrate, video_kbps: 1500, ..Default::default() };
        s.normalize();
        let l = line(&job(s));
        assert!(l.contains("-c:v libx264 -preset medium -b:v 1500k -pix_fmt yuv420p"), "{l}");
        assert!(!l.contains("-crf") && !l.contains("-pass"), "{l}");
    }

    #[test]
    fn a_target_size_becomes_the_bitrate_left_after_the_audio() {
        // 100 s, 60 MB: about 4 770 kb/s in all, less two tracks of 160 kb/s.
        let mut s = EncodeSettings { rate: RateMode::TargetSize, target_mb: 60, ..Default::default() };
        s.normalize();
        let j = job(s.clone());
        let kbps = j.target_kbps().unwrap();
        assert!((4400..=4500).contains(&kbps), "{kbps}");
        assert!(line(&j).contains(&format!("-b:v {kbps}k")));
        // With one track of the two, more is left for the picture.
        let mut one = job(s.clone());
        one.audio_sel = vec![1];
        assert!(one.target_kbps().unwrap() > kbps);
        // A size the audio alone would eat is refused in words.
        s.target_mb = 1;
        assert!(job(s).validate().unwrap_err().contains("too small"));
        // Without a known length no size can be aimed at.
        let mut unknown = job(EncodeSettings { rate: RateMode::TargetSize, ..Default::default() });
        unknown.src.duration = None;
        assert!(unknown.validate().unwrap_err().contains("not known"));
    }

    #[test]
    fn two_passes_name_the_same_statistics_and_the_first_writes_nothing() {
        let mut s = EncodeSettings { rate: RateMode::Bitrate, two_pass: true, ..Default::default() };
        s.normalize();
        let j = job(s);
        let text = |args: Vec<OsString>| args.iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
        let one = text(args_for(&j, Pass::First("stats")));
        assert!(one.contains("-pass 1 -passlogfile stats") && one.contains("-an -sn"), "{one}");
        assert!(!one.contains("-map 0:a") && !one.contains("-c:a") && !one.contains("-movflags"), "{one}");
        assert!(one.ends_with("-f null -progress pipe:1 -stats_period 0.5 -"), "{one}");
        let two = text(args_for(&j, Pass::Second("stats")));
        assert!(two.contains("-pass 2 -passlogfile stats") && two.contains("-map 0:a:0") && two.ends_with("/out/film.mp4.part"), "{two}");
    }

    #[test]
    fn subtitles_follow_what_the_container_can_hold() {
        let mut src = source();
        src.subs = vec![sub("subrip"), sub("hdmv_pgs_subtitle"), sub("ass")];
        let mut mp4 = job(EncodeSettings::default());
        mp4.src = src.clone();
        mp4.sub_sel = vec![0, 1, 2, 9];
        let l = line(&mp4);
        assert!(l.contains("-map 0:s:0") && l.contains("-map 0:s:2") && !l.contains("-map 0:s:1") && !l.contains("0:s:9"), "{l}");
        assert!(l.contains("-c:s mov_text"), "{l}");
        let mut mkv = mp4.clone();
        mkv.settings.container = Container::Mkv;
        let l = line(&mkv);
        assert!(l.contains("-map 0:s:1") && l.contains("-c:s copy"), "pictures of text fit MKV: {l}");
        let mut webm = mp4.clone();
        webm.settings = EncodeSettings { container: Container::WebM, ..Default::default() };
        webm.settings.normalize();
        assert!(!line(&webm).contains("-map 0:s"), "WebM takes none of these");
        let mut audio = mp4.clone();
        audio.settings = EncodeSettings { container: Container::Mp3, ..Default::default() };
        audio.settings.normalize();
        assert!(!line(&audio).contains("-map 0:s") && !line(&audio).contains("-c:s"));
        let mut none = mp4.clone();
        none.sub_sel.clear();
        assert!(!line(&none).contains("-c:s"), "no track, no subtitle codec");
    }

    #[test]
    fn two_passes_make_each_half_of_the_progress() {
        let p = |pass, passes, secs| Progress { pass, passes, out_secs: secs, ..Default::default() };
        assert_eq!(p(0, 0, 5.0).fraction(Some(10.0)), Some(0.5), "a single run is as before");
        assert_eq!(p(1, 2, 5.0).fraction(Some(10.0)), Some(0.25));
        assert_eq!(p(2, 2, 5.0).fraction(Some(10.0)), Some(0.75));
        assert_eq!(p(2, 2, 10.0).fraction(Some(10.0)), Some(1.0));
    }

    /// Runs `job` to the end and returns the file it made.
    fn made(tools: &crate::deps::Tools, job: Job) -> SourceInfo {
        let out = job.out.clone();
        let outcome = run_to_the_end(tools, job);
        assert!(matches!(outcome, Outcome::Finished { .. }), "{outcome:?}");
        assert!(!job_part_exists(&out), "no .part file is left");
        crate::probe::run(&tools.ffprobe, &out).unwrap()
    }

    #[test]
    fn real_filters_change_the_picture_as_asked() {
        let Some(tools) = crate::deps::locate(None) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("stiva-filters-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 640x360 picture with a black bar of 60 above and below it: 640x480 in all.
        let barred = dir.join("barred.mp4");
        let ok = Command::new(&tools.ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=25:duration=14"])
            .args(["-vf", "pad=640:480:0:60,format=yuv420p", "-c:v", "libx264", "-preset", "ultrafast"])
            .arg(&barred)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let src = crate::probe::run(&tools.ffprobe, &barred).unwrap();
        let size = |i: &SourceInfo| i.video.as_ref().map(|v| (v.width, v.height)).unwrap();
        assert_eq!(size(&src), (640, 480));
        let fast = |s: &mut EncodeSettings| {
            s.preset = "ultrafast".into();
            s.normalize();
        };
        let run = |name: &str, filters: crate::filters::Filters, max: Option<(u32, u32)>| {
            let mut s = EncodeSettings { filters, max_size: max, ..Default::default() };
            fast(&mut s);
            made(&tools, Job { src: src.clone(), audio_sel: vec![], sub_sel: vec![], settings: s, range: None, out: dir.join(name) })
        };
        use crate::filters::{Crop, Deinterlace, Filters, Margins, Rotate, Strength};

        // The bars are found and cut: 640x360 is left.
        let auto = run("auto.mp4", Filters { crop: Crop::Auto, ..Default::default() }, None);
        assert_eq!(size(&auto), (640, 360), "automatic crop");
        // A crop by hand.
        let hand = Filters { crop: Crop::Manual(Margins { left: 10, top: 20, right: 30, bottom: 40 }), ..Default::default() };
        assert_eq!(size(&run("hand.mp4", hand, None)), (600, 420), "manual crop");
        // Turned on its side, the picture is 480x640; the size limit follows the turn.
        let turned = Filters { rotate: Rotate::Clockwise, ..Default::default() };
        assert_eq!(size(&run("turned.mp4", turned.clone(), None)), (480, 640));
        assert_eq!(size(&run("turned-small.mp4", turned, Some((320, 180)))), (180, 240), "turned and limited");
        // Everything else just has to run through ffmpeg and give a picture of the same size.
        let busy = Filters {
            deinterlace: Deinterlace::Bwdif,
            denoise: Strength::Medium,
            sharpen: Strength::Strong,
            grayscale: true,
            flip_h: true,
            flip_v: true,
            custom: "eq=brightness=0.05".into(),
            ..Default::default()
        };
        assert_eq!(size(&run("busy.mp4", busy, None)), (640, 480));
        let yadif = Filters { deinterlace: Deinterlace::Yadif, ..Default::default() };
        assert_eq!(size(&run("yadif.mp4", yadif, None)), (640, 480));
        // Detelecine drops one picture in five, so the clip keeps its length with fewer frames.
        let tele = Filters { detelecine: true, ..Default::default() };
        let t = run("tele.mp4", tele, None);
        assert_eq!(size(&t), (640, 480));
        assert!((t.duration.unwrap() - 14.0).abs() < 1.0, "{:?}", t.duration);
        // A custom filter that does not exist fails with ffmpeg's words and leaves nothing behind.
        let bad = Filters { custom: "nosuchfilter=1".into(), ..Default::default() };
        let mut s = EncodeSettings { filters: bad, ..Default::default() };
        fast(&mut s);
        let out = dir.join("bad.mp4");
        let outcome = run_to_the_end(
            &tools,
            Job { src: src.clone(), audio_sel: vec![], sub_sel: vec![], settings: s, range: None, out: out.clone() },
        );
        assert!(matches!(outcome, Outcome::Failed { .. }), "{outcome:?}");
        assert!(!out.exists() && !job_part_exists(&out));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_bitrates_sizes_and_two_passes_land_near_what_was_asked() {
        let Some(tools) = crate::deps::locate(None) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("stiva-rate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = clip(&tools, &dir, "src.mp4", "640x360", 10);
        let kbps = |i: &SourceInfo, out: &Path| (std::fs::metadata(out).unwrap().len() as f64 * 8.0 / i.duration.unwrap() / 1000.0) as u32;
        for (name, codec, container, two) in [
            ("x264-1", VideoCodec::X264, Container::Mp4, false),
            ("x264-2", VideoCodec::X264, Container::Mp4, true),
            ("x265-2", VideoCodec::X265, Container::Mkv, true),
            ("vp9-2", VideoCodec::Vp9, Container::WebM, true),
            ("av1-2", VideoCodec::SvtAv1, Container::Mkv, true),
        ] {
            let mut s = EncodeSettings {
                container,
                rate: RateMode::Bitrate,
                video_kbps: 600,
                two_pass: two,
                audio_codec: AudioCodec::None,
                ..Default::default()
            };
            s.set_video_codec(codec);
            s.preset = codec.presets()[codec.presets().len() - 1].to_string();
            s.normalize();
            assert_eq!(s.two_pass, two, "{name}");
            let out = dir.join(format!("{name}.{}", container.ext()));
            let job = Job { src: src.clone(), audio_sel: vec![], sub_sel: vec![], settings: s, range: None, out: out.clone() };
            job.validate().unwrap();
            let got = made(&tools, job);
            let k = kbps(&got, &out);
            assert!((300..=1200).contains(&k), "{name}: asked 600 kb/s, got {k}");
            assert!(
                std::fs::read_dir(std::env::temp_dir())
                    .unwrap()
                    .filter_map(Result::ok)
                    .all(|e| !e.file_name().to_string_lossy().starts_with(&format!("stiva-pass-{}", std::process::id()))),
                "{name}: the statistics folder is removed"
            );
        }
        // A size: about 0.5 MB for ten seconds, with no audio.
        let mut s = EncodeSettings {
            rate: RateMode::TargetSize,
            target_mb: 1,
            two_pass: true,
            audio_codec: AudioCodec::None,
            ..Default::default()
        };
        s.normalize();
        let out = dir.join("size.mp4");
        let got = made(&tools, Job { src: src.clone(), audio_sel: vec![], sub_sel: vec![], settings: s, range: None, out: out.clone() });
        let mb = std::fs::metadata(&out).unwrap().len() as f64 / 1_048_576.0;
        assert!((0.5..=1.5).contains(&mb), "asked 1 MB, got {mb:.2} ({got:?})", got = got.duration);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_subtitles_and_chapters_come_along() {
        let Some(tools) = crate::deps::locate(None) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("stiva-subs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let srt = dir.join("a.srt");
        std::fs::write(&srt, "1\n00:00:00,500 --> 00:00:02,000\nCiao\n\n2\n00:00:02,500 --> 00:00:04,000\nHello\n").unwrap();
        let meta = dir.join("meta.txt");
        std::fs::write(&meta, ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=3000\ntitle=One\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=3000\nEND=6000\ntitle=Two\n").unwrap();
        let film = dir.join("film.mkv");
        let ok = Command::new(&tools.ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x180:rate=25:duration=6", "-i"])
            .arg(&srt)
            .arg("-i")
            .arg(&srt)
            .arg("-i")
            .arg(&meta)
            .args([
                "-map",
                "0:v",
                "-map",
                "1:0",
                "-map",
                "2:0",
                "-map_chapters",
                "3",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-c:s",
                "srt",
            ])
            .args(["-metadata:s:s:0", "language=ita", "-metadata:s:s:1", "language=eng"])
            .arg(&film)
            .status()
            .unwrap()
            .success();
        assert!(ok, "the test film could not be made");
        let src = crate::probe::run(&tools.ffprobe, &film).unwrap();
        assert_eq!((src.subs.len(), src.chapters.len()), (2, 2));
        for (container, want_subs) in [(Container::Mkv, vec!["ita", "eng"]), (Container::Mp4, vec!["ita", "eng"])] {
            let mut s = EncodeSettings { container, ..Default::default() };
            s.preset = "ultrafast".into();
            s.audio_codec = AudioCodec::None;
            s.normalize();
            let out = dir.join(format!("out.{}", container.ext()));
            let got = made(&tools, Job { src: src.clone(), audio_sel: vec![], sub_sel: vec![0, 1], settings: s, range: None, out });
            assert_eq!(got.subs.iter().map(|t| t.lang.as_str()).collect::<Vec<_>>(), want_subs, "{container:?}");
            assert_eq!(got.chapters.len(), 2, "{container:?}: chapters are carried over");
            assert_eq!(got.chapters[1].title, "Two");
        }
        // Only the second track.
        let mut s = EncodeSettings { audio_codec: AudioCodec::None, ..Default::default() };
        s.preset = "ultrafast".into();
        s.normalize();
        let got = made(
            &tools,
            Job { src: src.clone(), audio_sel: vec![], sub_sel: vec![1], settings: s.clone(), range: None, out: dir.join("one.mp4") },
        );
        assert_eq!(got.subs.iter().map(|t| t.lang.as_str()).collect::<Vec<_>>(), ["eng"]);
        // None.
        let got = made(&tools, Job { src, audio_sel: vec![], sub_sel: vec![], settings: s, range: None, out: dir.join("none.mp4") });
        assert!(got.subs.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
