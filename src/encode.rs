//! Turns the user's choices into an ffmpeg command line, runs it, and reports progress.
//! ffmpeg writes to `name.ext.part`; the file takes its real name only when the run ends well.

use crate::model::{AudioCodec, Container, EncodeSettings, VideoCodec};
use crate::probe::SourceInfo;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Job {
    pub src: SourceInfo,
    /// Which audio tracks to keep, as positions in `src.audio`.
    pub audio_sel: Vec<usize>,
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

fn secs(v: f64) -> String {
    format!("{v:.3}")
}

/// The arguments of the ffmpeg run, without the program name.
pub fn build_args(job: &Job) -> Vec<OsString> {
    let st = &job.settings;
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
    let keep: Vec<usize> = if st.audio_codec == AudioCodec::None {
        vec![]
    } else {
        job.audio_sel.iter().copied().filter(|&p| p < job.src.audio.len()).collect()
    };

    if video {
        o(&["-map", "0:v:0"]);
    }
    for pos in &keep {
        o(&["-map", &format!("0:a:{pos}")]);
    }
    o(&["-map_metadata", "0"]);
    o(&["-map_chapters", if audio_only { "-1" } else { "0" }]);

    if video {
        let codec = st.video_codec;
        let (crf, preset) = (st.crf.to_string(), st.preset.as_str());
        match codec {
            VideoCodec::X264 => o(&["-c:v", "libx264", "-preset", preset, "-crf", &crf, "-pix_fmt", "yuv420p"]),
            VideoCodec::X265 => {
                o(&["-c:v", "libx265", "-preset", preset, "-crf", &crf, "-pix_fmt", "yuv420p", "-x265-params", "log-level=error"]);
                if st.container == Container::Mp4 {
                    // Without this tag Apple players and Windows' own player do not recognise H.265 in MP4.
                    o(&["-tag:v", "hvc1"]);
                }
            }
            VideoCodec::SvtAv1 => o(&["-c:v", "libsvtav1", "-preset", preset, "-crf", &crf, "-pix_fmt", "yuv420p"]),
            VideoCodec::Vp9 => {
                o(&["-c:v", "libvpx-vp9", "-crf", &crf, "-b:v", "0", "-row-mt", "1", "-cpu-used", preset, "-pix_fmt", "yuv420p"])
            }
        }
    }

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

    if matches!(st.container, Container::Mp4 | Container::M4a) {
        o(&["-movflags", "+faststart"]);
    }
    o(&["-f", st.container.muxer()]);
    o(&["-progress", "pipe:1", "-stats_period", "0.5"]);
    drop(o);
    a.push(job.part_path().into_os_string());
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
}

impl Progress {
    /// 0..=1, or None when the length is not known.
    pub fn fraction(&self, span: Option<f64>) -> Option<f32> {
        span.filter(|s| *s > 0.0).map(|s| ((self.out_secs / s) as f32).clamp(0.0, 1.0))
    }

    /// Seconds left at the current speed.
    pub fn eta(&self, span: Option<f64>) -> Option<f64> {
        let s = span?;
        (self.speed > 0.01 && self.out_secs > 0.0).then(|| ((s - self.out_secs) / self.speed as f64).max(0.0))
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

/// Starts the run on its own thread. Everything it has to say comes through `emit`.
pub fn start(job: Job, ffmpeg: PathBuf, emit: impl Fn(Event) + Send + Sync + 'static) -> Handle {
    let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let cancelled = Arc::new(AtomicBool::new(false));
    let handle = Handle { child: child.clone(), cancelled: cancelled.clone() };
    let emit = Arc::new(emit);
    std::thread::spawn(move || {
        let failed = |msg: String| emit(Event::Done(Outcome::Failed { code: None, tail: vec![msg] }));
        let part = job.part_path();
        if let Some(dir) = part.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                return failed(format!("Cannot create the folder {}: {e}", dir.display()));
            }
        }
        let args = build_args(&job);
        emit(Event::Log(command_line(&ffmpeg, &args)));
        let mut cmd = Command::new(&ffmpeg);
        cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        crate::deps::quiet(&mut cmd);
        let mut proc = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return failed(format!("ffmpeg could not start: {e}")),
        };
        let stdout = proc.stdout.take();
        let stderr = proc.stderr.take();
        *child.lock().unwrap() = Some(proc);
        let started = Instant::now();

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
                if let Some(p) = parser.feed(&String::from_utf8_lossy(&raw)) {
                    emit(Event::Progress(p));
                }
            }
        }
        let tail = err_thread.join().unwrap_or_default();
        let status = child.lock().unwrap().as_mut().map(|c| c.wait());

        if cancelled.load(Ordering::SeqCst) {
            let _ = std::fs::remove_file(&part);
            return emit(Event::Done(Outcome::Cancelled));
        }
        match status {
            Some(Ok(s)) if s.success() => match std::fs::rename(&part, &job.out) {
                Ok(()) => {
                    let bytes = std::fs::metadata(&job.out).map(|m| m.len()).unwrap_or(0);
                    emit(Event::Done(Outcome::Finished { out: job.out.clone(), bytes, elapsed: started.elapsed() }));
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    failed(format!("The file was encoded but could not be named {}: {e}", job.out.display()));
                }
            },
            Some(Ok(s)) => {
                let _ = std::fs::remove_file(&part);
                emit(Event::Done(Outcome::Failed { code: s.code(), tail }));
            }
            _ => {
                let _ = std::fs::remove_file(&part);
                failed("ffmpeg ended in an unknown way".into());
            }
        }
    });
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Mixdown;
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
        Job { src: source(), audio_sel: vec![0, 1], settings, range: None, out: PathBuf::from("/out/film.mp4") }
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
            let job = Job { src: src.clone(), audio_sel: vec![0, 1], settings: s, range: None, out: out.clone() };
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
        let job = Job { src: src.clone(), audio_sel: vec![1], settings: s, range: Some((1.0, 2.0)), out: out.clone() };
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
        let job = Job { src: broken, audio_sel: vec![0], settings: EncodeSettings::default(), range: None, out: out.clone() };
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
        let job = Job { src, audio_sel: vec![], settings: s, range: None, out: out.clone() };
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

    fn job_part_exists(out: &Path) -> bool {
        let mut s = out.as_os_str().to_owned();
        s.push(".part");
        PathBuf::from(s).exists()
    }
}
