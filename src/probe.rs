//! Reads a media file with ffprobe: duration, video, audio tracks, subtitles, chapters.
//! The JSON parsing is separate from running the program, so it can be tested with a sample.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoInfo {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub pix_fmt: String,
    pub bit_rate: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioTrack {
    /// Number of the stream in the file.
    pub index: usize,
    pub codec: String,
    pub channels: u32,
    pub layout: String,
    pub sample_rate: u32,
    pub lang: String,
    pub title: String,
    pub bit_rate: Option<u64>,
    pub default: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubTrack {
    pub index: usize,
    pub codec: String,
    pub lang: String,
    pub title: String,
    pub forced: bool,
    pub default: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub start: f64,
    pub end: f64,
    pub title: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceInfo {
    pub path: PathBuf,
    pub container: String,
    pub duration: Option<f64>,
    pub size: u64,
    pub bit_rate: Option<u64>,
    pub video: Option<VideoInfo>,
    pub audio: Vec<AudioTrack>,
    pub subs: Vec<SubTrack>,
    pub chapters: Vec<Chapter>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Raw {
    format: RawFormat,
    streams: Vec<RawStream>,
    chapters: Vec<RawChapter>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawFormat {
    format_name: String,
    duration: Option<String>,
    size: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawStream {
    index: usize,
    codec_type: String,
    codec_name: String,
    width: u32,
    height: u32,
    pix_fmt: String,
    avg_frame_rate: String,
    r_frame_rate: String,
    bit_rate: Option<String>,
    channels: u32,
    channel_layout: String,
    sample_rate: Option<String>,
    tags: HashMap<String, String>,
    disposition: HashMap<String, i64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawChapter {
    start_time: Option<String>,
    end_time: Option<String>,
    tags: HashMap<String, String>,
}

fn tag(tags: &HashMap<String, String>, key: &str) -> String {
    tags.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.clone()).unwrap_or_default()
}

fn num<T: std::str::FromStr>(s: &Option<String>) -> Option<T> {
    s.as_deref().and_then(|v| v.trim().parse().ok())
}

/// "30000/1001" or "25/1" or "0/0".
fn fraction(s: &str) -> f32 {
    match s.split_once('/') {
        Some((a, b)) => {
            let (a, b) = (a.trim().parse::<f32>().unwrap_or(0.0), b.trim().parse::<f32>().unwrap_or(0.0));
            if b > 0.0 { a / b } else { 0.0 }
        }
        None => s.trim().parse().unwrap_or(0.0),
    }
}

pub fn parse(json: &str, path: &Path) -> Result<SourceInfo, String> {
    let raw: Raw = serde_json::from_str(json).map_err(|e| format!("ffprobe gave unreadable output: {e}"))?;
    let mut info = SourceInfo {
        path: path.to_path_buf(),
        container: raw.format.format_name.clone(),
        duration: num::<f64>(&raw.format.duration).filter(|d| d.is_finite() && *d > 0.0),
        size: num(&raw.format.size).unwrap_or(0),
        bit_rate: num(&raw.format.bit_rate),
        ..Default::default()
    };
    for s in &raw.streams {
        let flag = |name: &str| s.disposition.get(name).copied().unwrap_or(0) == 1;
        match s.codec_type.as_str() {
            "video" if !flag("attached_pic") && info.video.is_none() => {
                let fps = match fraction(&s.avg_frame_rate) {
                    f if f > 0.0 => f,
                    _ => fraction(&s.r_frame_rate),
                };
                info.video = Some(VideoInfo {
                    codec: s.codec_name.clone(),
                    width: s.width,
                    height: s.height,
                    fps,
                    pix_fmt: s.pix_fmt.clone(),
                    bit_rate: num(&s.bit_rate),
                });
            }
            "audio" => info.audio.push(AudioTrack {
                index: s.index,
                codec: s.codec_name.clone(),
                channels: s.channels,
                layout: s.channel_layout.clone(),
                sample_rate: num(&s.sample_rate).unwrap_or(0),
                lang: tag(&s.tags, "language"),
                title: tag(&s.tags, "title"),
                bit_rate: num(&s.bit_rate),
                default: flag("default"),
            }),
            "subtitle" => info.subs.push(SubTrack {
                index: s.index,
                codec: s.codec_name.clone(),
                lang: tag(&s.tags, "language"),
                title: tag(&s.tags, "title"),
                forced: flag("forced"),
                default: flag("default"),
            }),
            _ => {}
        }
    }
    for c in &raw.chapters {
        info.chapters.push(Chapter {
            start: num(&c.start_time).unwrap_or(0.0),
            end: num(&c.end_time).unwrap_or(0.0),
            title: tag(&c.tags, "title"),
        });
    }
    if info.video.is_none() && info.audio.is_empty() {
        return Err("This file has no video and no audio that ffmpeg can read.".into());
    }
    Ok(info)
}

pub fn run(ffprobe: &Path, file: &Path) -> Result<SourceInfo, String> {
    let mut cmd = Command::new(ffprobe);
    cmd.args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams", "-show_chapters", "-i"]).arg(file);
    crate::deps::quiet(&mut cmd);
    let out = cmd.output().map_err(|e| format!("ffprobe could not start: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("ffprobe could not read the file");
        return Err(first.trim().to_string());
    }
    parse(&String::from_utf8_lossy(&out.stdout), file)
}

/// "1:02:03" or "4:05"; fractions of a second are dropped.
pub fn human_duration(secs: f64) -> String {
    let t = secs.max(0.0).round() as u64;
    let (h, m, s) = (t / 3600, (t / 60) % 60, t % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[u])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

pub fn human_bitrate(bps: u64) -> String {
    if bps >= 1_000_000 { format!("{:.1} Mb/s", bps as f64 / 1e6) } else { format!("{} kb/s", bps / 1000) }
}

/// Reads "90", "1:30", "01:02:03.5" as seconds; None for anything else.
pub fn parse_time(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    let mut total = 0.0;
    for p in &parts {
        let v: f64 = p.trim().parse().ok().filter(|v: &f64| v.is_finite() && *v >= 0.0)?;
        total = total * 60.0 + v;
    }
    Some(total)
}

pub fn channel_name(channels: u32, layout: &str) -> String {
    match channels {
        1 => "mono".into(),
        2 => "stereo".into(),
        6 => "5.1".into(),
        8 => "7.1".into(),
        n if !layout.is_empty() => format!("{n} ch ({layout})"),
        n => format!("{n} ch"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "streams": [
        {"index":0,"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"pix_fmt":"yuv420p",
         "avg_frame_rate":"30000/1001","r_frame_rate":"30000/1001","bit_rate":"4000000","disposition":{"attached_pic":0}},
        {"index":1,"codec_type":"audio","codec_name":"ac3","channels":6,"channel_layout":"5.1(side)","sample_rate":"48000",
         "bit_rate":"448000","tags":{"language":"ita","title":"Italiano"},"disposition":{"default":1}},
        {"index":2,"codec_type":"audio","codec_name":"aac","channels":2,"channel_layout":"stereo","sample_rate":"44100",
         "tags":{"LANGUAGE":"eng"},"disposition":{"default":0}},
        {"index":3,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng"},"disposition":{"forced":1}},
        {"index":4,"codec_type":"video","codec_name":"mjpeg","width":600,"height":600,"disposition":{"attached_pic":1}}
      ],
      "chapters": [{"start_time":"0.000000","end_time":"60.5","tags":{"title":"Intro"}}],
      "format": {"format_name":"matroska,webm","duration":"5400.25","size":"3000000000","bit_rate":"4444444"}
    }"#;

    #[test]
    fn reads_streams_chapters_and_format() {
        let i = parse(SAMPLE, Path::new("/v/film.mkv")).unwrap();
        let v = i.video.as_ref().unwrap();
        assert_eq!((v.codec.as_str(), v.width, v.height), ("h264", 1920, 1080));
        assert!((v.fps - 29.97).abs() < 0.01);
        assert_eq!(i.audio.len(), 2);
        assert_eq!((i.audio[0].index, i.audio[0].channels, i.audio[0].lang.as_str()), (1, 6, "ita"));
        assert!(i.audio[0].default && !i.audio[1].default);
        assert_eq!(i.audio[1].lang, "eng", "tag keys are matched without regard to case");
        assert_eq!(i.audio[1].bit_rate, None);
        assert_eq!(i.subs.len(), 1);
        assert!(i.subs[0].forced);
        assert_eq!(i.chapters, vec![Chapter { start: 0.0, end: 60.5, title: "Intro".into() }]);
        assert_eq!(i.duration, Some(5400.25));
        assert_eq!(i.size, 3_000_000_000);
    }

    #[test]
    fn cover_art_is_not_the_video() {
        let json = r#"{"streams":[
          {"index":0,"codec_type":"audio","codec_name":"mp3","channels":2,"sample_rate":"44100"},
          {"index":1,"codec_type":"video","codec_name":"mjpeg","width":500,"height":500,"disposition":{"attached_pic":1}}],
          "format":{"format_name":"mp3","duration":"180.0"}}"#;
        let i = parse(json, Path::new("a.mp3")).unwrap();
        assert!(i.video.is_none());
        assert_eq!(i.audio.len(), 1);
    }

    #[test]
    fn a_file_without_media_is_refused_and_bad_json_is_an_error() {
        assert!(parse(r#"{"streams":[],"format":{}}"#, Path::new("x")).is_err());
        assert!(parse("not json", Path::new("x")).is_err());
    }

    #[test]
    fn times_are_read_and_written() {
        assert_eq!(parse_time("90"), Some(90.0));
        assert_eq!(parse_time("1:30"), Some(90.0));
        assert_eq!(parse_time("01:02:03.5"), Some(3723.5));
        assert_eq!(parse_time(" "), None);
        assert_eq!(parse_time("a:b"), None);
        assert_eq!(parse_time("-5"), None);
        assert_eq!(parse_time("1:2:3:4"), None);
        assert_eq!(human_duration(3723.4), "1:02:03");
        assert_eq!(human_duration(65.0), "1:05");
    }

    #[test]
    fn sizes_and_rates_read_well() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(3_000_000_000), "2.8 GB");
        assert_eq!(human_bitrate(448_000), "448 kb/s");
        assert_eq!(human_bitrate(4_500_000), "4.5 Mb/s");
        assert_eq!(channel_name(6, "5.1(side)"), "5.1");
        assert_eq!(channel_name(3, "2.1"), "3 ch (2.1)");
    }
}
