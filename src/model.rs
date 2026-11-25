//! What the user chooses for one encode: container, video codec and quality, audio codec.
//! Plain data with the rules about which choices go together; no I/O and no drawing.

use crate::filters::Filters;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Container {
    Mp4,
    Mkv,
    WebM,
    Mp3,
    M4a,
    Opus,
    Flac,
}

impl Container {
    pub const ALL: [Container; 7] =
        [Container::Mp4, Container::Mkv, Container::WebM, Container::Mp3, Container::M4a, Container::Opus, Container::Flac];

    pub fn label(self) -> &'static str {
        match self {
            Container::Mp4 => "MP4",
            Container::Mkv => "MKV",
            Container::WebM => "WebM",
            Container::Mp3 => "MP3 (audio only)",
            Container::M4a => "M4A (audio only)",
            Container::Opus => "Opus (audio only)",
            Container::Flac => "FLAC (audio only)",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "mkv",
            Container::WebM => "webm",
            Container::Mp3 => "mp3",
            Container::M4a => "m4a",
            Container::Opus => "opus",
            Container::Flac => "flac",
        }
    }

    /// The name ffmpeg's `-f` wants. It has to be given because the file is written as `name.ext.part`.
    pub fn muxer(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "matroska",
            Container::WebM => "webm",
            Container::Mp3 => "mp3",
            Container::M4a => "ipod",
            Container::Opus => "opus",
            Container::Flac => "flac",
        }
    }

    pub fn audio_only(self) -> bool {
        matches!(self, Container::Mp3 | Container::M4a | Container::Opus | Container::Flac)
    }

    pub fn video_codecs(self) -> &'static [VideoCodec] {
        use VideoCodec::*;
        match self {
            Container::Mp4 | Container::Mkv => &[
                X264, X265, SvtAv1, Vp9, H264Amf, HevcAmf, Av1Amf, H264Nvenc, HevcNvenc, Av1Nvenc, H264Qsv, HevcQsv, Av1Qsv, H264Vaapi,
                HevcVaapi, Av1Vaapi,
            ],
            Container::WebM => &[Vp9, SvtAv1, Av1Amf, Av1Nvenc, Av1Qsv, Av1Vaapi],
            _ => &[],
        }
    }

    pub fn audio_codecs(self) -> &'static [AudioCodec] {
        use AudioCodec::*;
        match self {
            Container::Mp4 => &[Aac, Mp3, Ac3, Opus, Copy, None],
            Container::Mkv => &[Aac, Mp3, Opus, Vorbis, Ac3, Flac, Copy, None],
            Container::WebM => &[Opus, Vorbis, None],
            Container::Mp3 => &[Mp3],
            Container::M4a => &[Aac],
            Container::Opus => &[Opus],
            Container::Flac => &[Flac],
        }
    }
}

/// How the video is steered: by a quality level, or towards an average bitrate in kilobits a second.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rate {
    Quality(u32),
    Kbps(u32),
}

/// What the user fixes in the Video tab: the quality, the average bitrate, or the size of the file.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum RateMode {
    #[default]
    Quality,
    Bitrate,
    TargetSize,
}

impl RateMode {
    pub const ALL: [RateMode; 3] = [RateMode::Quality, RateMode::Bitrate, RateMode::TargetSize];

    pub fn label(self) -> &'static str {
        match self {
            RateMode::Quality => "Constant quality",
            RateMode::Bitrate => "Average bitrate",
            RateMode::TargetSize => "Target file size",
        }
    }
}

/// What SVT-AV1 aims at when it decides where to spend bits.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum SvtTune {
    #[default]
    Visual,
    Psnr,
    Ssim,
}

impl SvtTune {
    pub const ALL: [SvtTune; 3] = [SvtTune::Visual, SvtTune::Psnr, SvtTune::Ssim];

    pub fn label(self) -> &'static str {
        match self {
            SvtTune::Visual => "Visual quality",
            SvtTune::Psnr => "PSNR (measurements)",
            SvtTune::Ssim => "SSIM (measurements)",
        }
    }

    /// The value SVT-AV1 knows it by.
    fn value(self) -> u8 {
        match self {
            SvtTune::Visual => 0,
            SvtTune::Psnr => 1,
            SvtTune::Ssim => 2,
        }
    }
}

/// Splits a line of options the way a command line would: spaces separate, quotes keep words together.
pub fn split_options(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum VideoCodec {
    X264,
    X265,
    SvtAv1,
    Vp9,
    // On the graphics chip. Shown only when a test encode on this PC works.
    H264Amf,
    HevcAmf,
    Av1Amf,
    H264Nvenc,
    HevcNvenc,
    Av1Nvenc,
    H264Qsv,
    HevcQsv,
    Av1Qsv,
    // VA-API, which is how the AMD and Intel graphics chips encode on Linux.
    H264Vaapi,
    HevcVaapi,
    Av1Vaapi,
}

impl VideoCodec {
    pub const ALL: [VideoCodec; 16] = {
        use VideoCodec::*;
        [
            X264, X265, SvtAv1, Vp9, H264Amf, HevcAmf, Av1Amf, H264Nvenc, HevcNvenc, Av1Nvenc, H264Qsv, HevcQsv, Av1Qsv, H264Vaapi,
            HevcVaapi, Av1Vaapi,
        ]
    };

    pub fn label(self) -> &'static str {
        match self {
            VideoCodec::X264 => "H.264 (x264)",
            VideoCodec::X265 => "H.265 (x265)",
            VideoCodec::SvtAv1 => "AV1 (SVT-AV1)",
            VideoCodec::Vp9 => "VP9",
            VideoCodec::H264Amf => "H.264 (AMD AMF)",
            VideoCodec::HevcAmf => "H.265 (AMD AMF)",
            VideoCodec::Av1Amf => "AV1 (AMD AMF)",
            VideoCodec::H264Nvenc => "H.264 (NVIDIA NVENC)",
            VideoCodec::HevcNvenc => "H.265 (NVIDIA NVENC)",
            VideoCodec::Av1Nvenc => "AV1 (NVIDIA NVENC)",
            VideoCodec::H264Qsv => "H.264 (Intel QSV)",
            VideoCodec::HevcQsv => "H.265 (Intel QSV)",
            VideoCodec::Av1Qsv => "AV1 (Intel QSV)",
            VideoCodec::H264Vaapi => "H.264 (VA-API)",
            VideoCodec::HevcVaapi => "H.265 (VA-API)",
            VideoCodec::Av1Vaapi => "AV1 (VA-API)",
        }
    }

    /// Name of the encoder in ffmpeg.
    pub fn encoder(self) -> &'static str {
        match self {
            VideoCodec::X264 => "libx264",
            VideoCodec::X265 => "libx265",
            VideoCodec::SvtAv1 => "libsvtav1",
            VideoCodec::Vp9 => "libvpx-vp9",
            VideoCodec::H264Amf => "h264_amf",
            VideoCodec::HevcAmf => "hevc_amf",
            VideoCodec::Av1Amf => "av1_amf",
            VideoCodec::H264Nvenc => "h264_nvenc",
            VideoCodec::HevcNvenc => "hevc_nvenc",
            VideoCodec::Av1Nvenc => "av1_nvenc",
            VideoCodec::H264Qsv => "h264_qsv",
            VideoCodec::HevcQsv => "hevc_qsv",
            VideoCodec::Av1Qsv => "av1_qsv",
            VideoCodec::H264Vaapi => "h264_vaapi",
            VideoCodec::HevcVaapi => "hevc_vaapi",
            VideoCodec::Av1Vaapi => "av1_vaapi",
        }
    }

    pub fn is_hardware(self) -> bool {
        !matches!(self, VideoCodec::X264 | VideoCodec::X265 | VideoCodec::SvtAv1 | VideoCodec::Vp9)
    }

    /// Whether 10-bit is worth testing: H.264 on a graphics chip is 8-bit only in practice.
    pub fn can_ten_bit(self) -> bool {
        !matches!(self, VideoCodec::H264Amf | VideoCodec::H264Nvenc | VideoCodec::H264Qsv | VideoCodec::H264Vaapi)
    }

    /// VA-API takes its pictures in the graphics chip's own memory, so it needs a device and an upload step.
    pub fn is_vaapi(self) -> bool {
        matches!(self, VideoCodec::H264Vaapi | VideoCodec::HevcVaapi | VideoCodec::Av1Vaapi)
    }

    /// Whether the encoder has a speed setting to offer. VA-API has none that every driver understands.
    pub fn has_speed(self) -> bool {
        !self.is_vaapi()
    }

    /// The filter that moves the picture to the graphics chip, put last in the chain; none for the others.
    pub fn upload_filter(self, bits: u8) -> Option<&'static str> {
        self.is_vaapi().then_some(if bits >= 10 { "format=p010le,hwupload" } else { "format=nv12,hwupload" })
    }

    /// Two passes need the encoder to keep statistics between runs, which the graphics chips cannot.
    pub fn can_two_pass(self) -> bool {
        !self.is_hardware()
    }

    pub fn is_av1(self) -> bool {
        matches!(self, VideoCodec::SvtAv1 | VideoCodec::Av1Amf | VideoCodec::Av1Nvenc | VideoCodec::Av1Qsv | VideoCodec::Av1Vaapi)
    }

    pub fn is_hevc(self) -> bool {
        matches!(self, VideoCodec::X265 | VideoCodec::HevcAmf | VideoCodec::HevcNvenc | VideoCodec::HevcQsv | VideoCodec::HevcVaapi)
    }

    /// Lowest and highest value of the quality scale: lower is better and bigger.
    pub fn crf_range(self) -> (u32, u32) {
        use VideoCodec::*;
        match self {
            X264 | X265 | H264Amf | HevcAmf | H264Nvenc | HevcNvenc | Av1Nvenc | H264Vaapi | HevcVaapi => (0, 51),
            SvtAv1 | Vp9 => (0, 63),
            Av1Amf | Av1Vaapi => (0, 255),
            H264Qsv | HevcQsv | Av1Qsv => (1, 51),
        }
    }

    pub fn default_crf(self) -> u32 {
        use VideoCodec::*;
        match self {
            X264 => 22,
            X265 => 24,
            SvtAv1 => 30,
            Vp9 => 32,
            H264Amf | HevcAmf => 24,
            Av1Amf => 110,
            H264Nvenc | HevcNvenc => 26,
            Av1Nvenc => 28,
            H264Qsv | HevcQsv | Av1Qsv => 24,
            H264Vaapi => 26,
            HevcVaapi => 28,
            Av1Vaapi => 110,
        }
    }

    /// Speed settings from the slowest (best compression) to the fastest.
    pub fn presets(self) -> &'static [&'static str] {
        use VideoCodec::*;
        match self {
            X264 | X265 => &["veryslow", "slower", "slow", "medium", "fast", "faster", "veryfast", "superfast", "ultrafast"],
            SvtAv1 => &["2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"],
            Vp9 => &["0", "1", "2", "3", "4", "5"],
            H264Amf | HevcAmf | Av1Amf => &["quality", "balanced", "speed"],
            H264Nvenc | HevcNvenc | Av1Nvenc => &["p7", "p6", "p5", "p4", "p3", "p2", "p1"],
            H264Qsv | HevcQsv | Av1Qsv => &["veryslow", "slower", "slow", "medium", "fast", "faster", "veryfast"],
            H264Vaapi | HevcVaapi | Av1Vaapi => &["default"],
        }
    }

    pub fn default_preset(self) -> &'static str {
        use VideoCodec::*;
        match self {
            X264 | X265 => "medium",
            SvtAv1 => "6",
            Vp9 => "2",
            H264Amf | HevcAmf | Av1Amf => "balanced",
            H264Nvenc | HevcNvenc | Av1Nvenc => "p5",
            H264Qsv | HevcQsv | Av1Qsv => "medium",
            H264Vaapi | HevcVaapi | Av1Vaapi => "default",
        }
    }

    /// The ffmpeg options for this codec at a given rate and speed. The test encode that decides whether a
    /// hardware encoder is offered uses these same options, so what passes the test is what the real encode runs.
    pub fn args(self, rate: Rate, preset: &str, bits: u8) -> Vec<String> {
        use VideoCodec::*;
        // Graphics chips take NV12 (8-bit) or P010 (10-bit); the software encoders take planar YUV.
        let pix = match (self.is_hardware(), bits >= 10) {
            (false, false) => "yuv420p",
            (false, true) => "yuv420p10le",
            (true, false) => "nv12",
            (true, true) => "p010le",
        };
        let mut a: Vec<String> = vec!["-c:v".into(), self.encoder().into()];
        let mut add = |items: &[&str]| a.extend(items.iter().map(|s| s.to_string()));
        match (rate, self) {
            (Rate::Quality(q), _) => {
                let q = q.to_string();
                match self {
                    X264 => add(&["-preset", preset, "-crf", &q]),
                    X265 => add(&["-preset", preset, "-crf", &q]),
                    SvtAv1 => add(&["-preset", preset, "-crf", &q]),
                    Vp9 => add(&["-crf", &q, "-b:v", "0", "-row-mt", "1", "-cpu-used", preset]),
                    // Constant quantizer; the average bitrate has its own options below.
                    H264Amf | HevcAmf | Av1Amf => add(&["-quality", preset, "-rc", "cqp", "-qp_i", &q, "-qp_p", &q]),
                    // Variable bitrate steered by a quality level, with no bitrate cap.
                    H264Nvenc | HevcNvenc | Av1Nvenc => add(&["-preset", preset, "-tune", "hq", "-rc", "vbr", "-cq", &q, "-b:v", "0"]),
                    H264Qsv | HevcQsv | Av1Qsv => add(&["-preset", preset, "-global_quality", &q]),
                    // A constant quantizer, as with AMF. The picture is moved to the chip by a filter, see `upload_filter`.
                    H264Vaapi | HevcVaapi | Av1Vaapi => add(&["-rc_mode", "CQP", "-qp", &q]),
                }
            }
            (Rate::Kbps(k), _) => {
                let (avg, peak) = (format!("{k}k"), format!("{}k", k + k / 2));
                match self {
                    X264 | X265 | SvtAv1 => add(&["-preset", preset, "-b:v", &avg]),
                    Vp9 => add(&["-b:v", &avg, "-row-mt", "1", "-cpu-used", preset]),
                    H264Amf | HevcAmf | Av1Amf => add(&["-quality", preset, "-rc", "vbr_peak", "-b:v", &avg, "-maxrate", &peak]),
                    H264Nvenc | HevcNvenc | Av1Nvenc => {
                        add(&["-preset", preset, "-tune", "hq", "-rc", "vbr", "-b:v", &avg, "-maxrate", &peak])
                    }
                    H264Qsv | HevcQsv | Av1Qsv => add(&["-preset", preset, "-b:v", &avg, "-maxrate", &peak]),
                    H264Vaapi | HevcVaapi | Av1Vaapi => add(&["-b:v", &avg, "-maxrate", &peak]),
                }
            }
        }
        // The pixel format of a VA-API picture is the chip's own; the upload filter has already set it.
        if !self.is_vaapi() {
            add(&["-pix_fmt", pix]);
        }
        if self == X265 {
            add(&["-x265-params", "log-level=error"]);
        }
        a
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AudioCodec {
    Aac,
    Mp3,
    Opus,
    Vorbis,
    Ac3,
    Flac,
    /// The sound is passed through untouched.
    Copy,
    /// No audio in the file.
    None,
}

impl AudioCodec {
    pub fn label(self) -> &'static str {
        match self {
            AudioCodec::Aac => "AAC",
            AudioCodec::Mp3 => "MP3",
            AudioCodec::Opus => "Opus",
            AudioCodec::Vorbis => "Vorbis",
            AudioCodec::Ac3 => "AC-3",
            AudioCodec::Flac => "FLAC (lossless)",
            AudioCodec::Copy => "Copy (passthrough)",
            AudioCodec::None => "No audio",
        }
    }

    pub fn encoder(self) -> Option<&'static str> {
        match self {
            AudioCodec::Aac => Some("aac"),
            AudioCodec::Mp3 => Some("libmp3lame"),
            AudioCodec::Opus => Some("libopus"),
            AudioCodec::Vorbis => Some("libvorbis"),
            AudioCodec::Ac3 => Some("ac3"),
            AudioCodec::Flac => Some("flac"),
            AudioCodec::Copy | AudioCodec::None => None,
        }
    }

    pub fn has_bitrate(self) -> bool {
        matches!(self, AudioCodec::Aac | AudioCodec::Mp3 | AudioCodec::Opus | AudioCodec::Vorbis | AudioCodec::Ac3)
    }

    pub fn default_kbps(self) -> u32 {
        match self {
            AudioCodec::Opus => 128,
            AudioCodec::Ac3 => 384,
            _ => 160,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Mixdown {
    Source,
    Mono,
    Stereo,
    Surround51,
}

impl Mixdown {
    pub const ALL: [Mixdown; 4] = [Mixdown::Source, Mixdown::Mono, Mixdown::Stereo, Mixdown::Surround51];

    pub fn label(self) -> &'static str {
        match self {
            Mixdown::Source => "As the source",
            Mixdown::Mono => "Mono",
            Mixdown::Stereo => "Stereo",
            Mixdown::Surround51 => "5.1 surround",
        }
    }

    pub fn channels(self) -> Option<u32> {
        match self {
            Mixdown::Source => None,
            Mixdown::Mono => Some(1),
            Mixdown::Stereo => Some(2),
            Mixdown::Surround51 => Some(6),
        }
    }
}

/// The sizes offered in the Sizing tab: the name and the longest and shortest side.
pub const SIZE_STEPS: [(&str, u32, u32); 7] = [
    ("2160p (4K)", 3840, 2160),
    ("1440p", 2560, 1440),
    ("1080p", 1920, 1080),
    ("720p", 1280, 720),
    ("576p", 1024, 576),
    ("480p", 854, 480),
    ("360p", 640, 360),
];

pub const AUDIO_KBPS: [u32; 8] = [64, 96, 128, 160, 192, 256, 320, 384];

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EncodeSettings {
    pub container: Container,
    pub video_codec: VideoCodec,
    pub crf: u32,
    pub preset: String,
    pub audio_codec: AudioCodec,
    pub audio_kbps: u32,
    pub mixdown: Mixdown,
    pub gain_db: f32,
    /// 8 or 10 bits per colour.
    pub bit_depth: u8,
    /// Longest and shortest side the picture may have; larger pictures are scaled down, never up.
    pub max_size: Option<(u32, u32)>,
    pub rate: RateMode,
    /// The average bitrate of the video, in kilobits a second, for `RateMode::Bitrate`.
    pub video_kbps: u32,
    /// The size the file should come to, in megabytes, for `RateMode::TargetSize`.
    pub target_mb: u32,
    /// Two runs over the file for a steadier result at the same bitrate; only with a bitrate or a size.
    pub two_pass: bool,
    /// SVT-AV1 only: what the encoder aims at.
    pub svt_tune: SvtTune,
    /// SVT-AV1 only: strength of the synthetic film grain, 0 for none (up to 50).
    pub film_grain: u32,
    /// SVT-AV1 only: take the grain out of the picture before encoding, when film grain is on.
    pub grain_denoise: bool,
    /// More options for the video encoder, written as on the ffmpeg command line.
    pub extra_video: String,
    pub filters: Filters,
}

impl Default for EncodeSettings {
    fn default() -> Self {
        EncodeSettings {
            container: Container::Mp4,
            video_codec: VideoCodec::X264,
            crf: VideoCodec::X264.default_crf(),
            preset: VideoCodec::X264.default_preset().into(),
            audio_codec: AudioCodec::Aac,
            audio_kbps: 160,
            mixdown: Mixdown::Source,
            gain_db: 0.0,
            bit_depth: 8,
            max_size: None,
            rate: RateMode::Quality,
            video_kbps: 2500,
            target_mb: 700,
            two_pass: false,
            svt_tune: SvtTune::Visual,
            film_grain: 0,
            grain_denoise: true,
            extra_video: String::new(),
            filters: Filters::default(),
        }
    }
}

impl EncodeSettings {
    /// Brings the choices back to a combination that works after one of them changed:
    /// a codec the container cannot hold is replaced, a quality out of range is pulled in.
    /// Returns true when something was replaced, so the window can say so.
    /// The options that come after the codec's own: SVT-AV1's tune and film grain, then whatever the user wrote.
    pub fn video_extras(&self) -> Vec<String> {
        let mut a = Vec::new();
        if self.video_codec == VideoCodec::SvtAv1 {
            let mut params: Vec<String> = Vec::new();
            if self.svt_tune != SvtTune::Visual {
                params.push(format!("tune={}", self.svt_tune.value()));
            }
            if self.film_grain > 0 {
                params.push(format!("film-grain={}", self.film_grain.min(50)));
                params.push(format!("film-grain-denoise={}", u8::from(self.grain_denoise)));
            }
            if !params.is_empty() {
                a.push("-svtav1-params".to_string());
                a.push(params.join(":"));
            }
        }
        a.extend(split_options(&self.extra_video));
        a
    }

    pub fn normalize(&mut self) -> bool {
        let mut changed = false;
        let vc = self.container.video_codecs();
        if !vc.is_empty() && !vc.contains(&self.video_codec) {
            self.video_codec = vc[0];
            self.crf = self.video_codec.default_crf();
            self.preset = self.video_codec.default_preset().into();
            changed = true;
        }
        let ac = self.container.audio_codecs();
        if !ac.contains(&self.audio_codec) {
            self.audio_codec = ac[0];
            self.audio_kbps = self.audio_codec.default_kbps();
            changed = true;
        }
        let (lo, hi) = self.video_codec.crf_range();
        self.crf = self.crf.clamp(lo, hi);
        if !self.video_codec.presets().contains(&self.preset.as_str()) {
            self.preset = self.video_codec.default_preset().into();
        }
        self.gain_db = self.gain_db.clamp(-30.0, 30.0);
        self.max_size = self.max_size.map(|(a, b)| (a.clamp(64, 16384), b.clamp(64, 16384)));
        self.bit_depth = if self.bit_depth >= 10 && self.video_codec.can_ten_bit() { 10 } else { 8 };
        self.video_kbps = self.video_kbps.clamp(50, 200_000);
        self.target_mb = self.target_mb.clamp(1, 200_000);
        self.film_grain = self.film_grain.min(50);
        self.two_pass = self.two_pass && self.rate != RateMode::Quality && self.video_codec.can_two_pass();
        self.filters.normalize();
        changed
    }

    /// How the video is steered, given the bitrate worked out for a target size when that is the mode.
    pub fn rate_with(&self, target_kbps: Option<u32>) -> Rate {
        match self.rate {
            RateMode::Quality => Rate::Quality(self.crf),
            RateMode::Bitrate => Rate::Kbps(self.video_kbps),
            RateMode::TargetSize => Rate::Kbps(target_kbps.unwrap_or(self.video_kbps)),
        }
    }

    /// The user picked another video codec: start from that codec's usual quality and speed.
    pub fn set_video_codec(&mut self, codec: VideoCodec) {
        if self.video_codec != codec {
            self.video_codec = codec;
            self.crf = codec.default_crf();
            self.preset = codec.default_preset().into();
            if !codec.can_ten_bit() {
                self.bit_depth = 8;
            }
        }
    }

    pub fn set_audio_codec(&mut self, codec: AudioCodec) {
        if self.audio_codec != codec {
            self.audio_codec = codec;
            self.audio_kbps = codec.default_kbps();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webm_replaces_what_it_cannot_hold() {
        let mut s = EncodeSettings::default(); // x264 + aac
        s.container = Container::WebM;
        assert!(s.normalize());
        assert_eq!(s.video_codec, VideoCodec::Vp9);
        assert_eq!(s.audio_codec, AudioCodec::Opus);
        assert_eq!(s.crf, 32);
        assert_eq!(s.preset, "2");
    }

    #[test]
    fn a_good_combination_is_left_alone() {
        let mut s = EncodeSettings::default();
        assert!(!s.normalize());
        assert_eq!(s, EncodeSettings::default());
    }

    #[test]
    fn audio_only_containers_force_their_codec() {
        let mut s = EncodeSettings::default();
        s.container = Container::Mp3;
        s.normalize();
        assert_eq!(s.audio_codec, AudioCodec::Mp3);
        s.container = Container::Flac;
        s.normalize();
        assert_eq!(s.audio_codec, AudioCodec::Flac);
    }

    #[test]
    fn quality_is_kept_inside_the_scale_of_the_codec() {
        let mut s = EncodeSettings::default();
        s.crf = 60;
        s.normalize();
        assert_eq!(s.crf, 51);
        s.set_video_codec(VideoCodec::SvtAv1);
        assert_eq!((s.crf, s.preset.as_str()), (30, "6"));
    }

    #[test]
    fn every_codec_keeps_its_defaults_inside_its_own_scales() {
        for c in VideoCodec::ALL {
            let (lo, hi) = c.crf_range();
            assert!((lo..=hi).contains(&c.default_crf()), "{c:?} quality");
            assert!(c.presets().contains(&c.default_preset()), "{c:?} speed");
            let args = c.args(Rate::Quality(c.default_crf()), c.default_preset(), 8);
            assert_eq!(&args[..2], ["-c:v", c.encoder()], "{c:?}");
        }
    }

    #[test]
    fn hardware_is_named_as_such_and_av1_alone_fits_webm() {
        assert!(!VideoCodec::X264.is_hardware() && VideoCodec::HevcAmf.is_hardware());
        assert!(VideoCodec::HevcQsv.is_hevc() && !VideoCodec::Av1Qsv.is_hevc());
        let webm: Vec<_> = Container::WebM.video_codecs().iter().filter(|c| c.is_hardware()).collect();
        assert_eq!(webm, [&VideoCodec::Av1Amf, &VideoCodec::Av1Nvenc, &VideoCodec::Av1Qsv, &VideoCodec::Av1Vaapi]);
    }

    #[test]
    fn vaapi_needs_the_upload_filter_and_no_pixel_format_of_its_own() {
        for c in [VideoCodec::H264Vaapi, VideoCodec::HevcVaapi, VideoCodec::Av1Vaapi] {
            assert!(c.is_vaapi() && c.is_hardware() && !c.has_speed() && !c.can_two_pass(), "{c:?}");
            assert_eq!(c.upload_filter(8), Some("format=nv12,hwupload"));
            assert_eq!(c.upload_filter(10), Some("format=p010le,hwupload"));
            let q = c.args(Rate::Quality(c.default_crf()), c.default_preset(), 8);
            assert!(!q.contains(&"-pix_fmt".to_string()) && !q.contains(&"-preset".to_string()), "{c:?}: {q:?}");
            assert_eq!(q[2..], ["-rc_mode", "CQP", "-qp", &c.default_crf().to_string()], "{c:?}");
            let b = c.args(Rate::Kbps(2000), c.default_preset(), 10);
            assert_eq!(b[2..], ["-b:v", "2000k", "-maxrate", "3000k"], "{c:?}");
        }
        assert!(VideoCodec::X264.upload_filter(8).is_none() && VideoCodec::H264Nvenc.upload_filter(10).is_none());
        assert!(VideoCodec::X264.has_speed() && VideoCodec::H264Nvenc.has_speed());
        assert!(VideoCodec::HevcVaapi.is_hevc() && VideoCodec::Av1Vaapi.is_av1() && !VideoCodec::H264Vaapi.can_ten_bit());
        assert!(
            Container::Mp4.video_codecs().contains(&VideoCodec::H264Vaapi)
                && Container::WebM.video_codecs().contains(&VideoCodec::Av1Vaapi)
        );
    }

    #[test]
    fn av1_on_the_graphics_chip_survives_a_switch_to_webm() {
        let mut s = EncodeSettings::default();
        s.set_video_codec(VideoCodec::Av1Amf);
        assert_eq!((s.crf, s.preset.as_str()), (110, "balanced"));
        s.container = Container::WebM;
        s.normalize(); // the audio changes to Opus, the video codec stays
        assert_eq!((s.video_codec, s.crf), (VideoCodec::Av1Amf, 110));
        s.set_video_codec(VideoCodec::HevcNvenc);
        s.normalize();
        assert_eq!(s.video_codec, VideoCodec::Vp9, "H.265 does not fit WebM");
    }

    #[test]
    fn a_bitrate_replaces_the_quality_in_every_codec() {
        for c in VideoCodec::ALL {
            let q = c.args(Rate::Quality(c.default_crf()), c.default_preset(), 8).join(" ");
            let b = c.args(Rate::Kbps(1500), c.default_preset(), 8).join(" ");
            assert!(b.contains("-b:v 1500k"), "{c:?}: {b}");
            assert!(!b.contains("-crf") && !b.contains("-cq ") && !b.contains("-qp_") && !b.contains("-global_quality"), "{c:?}: {b}");
            // VA-API sets the pixel format in the upload filter instead.
            assert_eq!(b.contains("-pix_fmt"), !c.is_vaapi(), "{c:?}: {b}");
            assert_ne!(q, b);
        }
        assert!(VideoCodec::X265.args(Rate::Kbps(900), "medium", 8).join(" ").contains("log-level=error"));
        assert!(VideoCodec::HevcAmf.args(Rate::Kbps(1000), "balanced", 8).join(" ").contains("-maxrate 1500k"));
    }

    #[test]
    fn two_passes_need_a_bitrate_and_a_software_encoder() {
        let mut s = EncodeSettings { two_pass: true, ..Default::default() };
        s.normalize();
        assert!(!s.two_pass, "quality mode has nothing to plan");
        s.rate = RateMode::Bitrate;
        s.two_pass = true;
        s.normalize();
        assert!(s.two_pass);
        s.set_video_codec(VideoCodec::HevcAmf);
        s.normalize();
        assert!(!s.two_pass, "a graphics chip cannot do two passes");
    }

    #[test]
    fn the_target_size_uses_the_bitrate_worked_out_for_it() {
        let mut s = EncodeSettings { rate: RateMode::TargetSize, ..Default::default() };
        assert_eq!(s.rate_with(Some(1234)), Rate::Kbps(1234));
        s.rate = RateMode::Bitrate;
        assert_eq!(s.rate_with(Some(1234)), Rate::Kbps(2500));
        s.rate = RateMode::Quality;
        assert_eq!(s.rate_with(None), Rate::Quality(22));
    }

    #[test]
    fn settings_survive_a_round_trip_and_old_files_still_load() {
        let mut s = EncodeSettings::default();
        s.container = Container::Mkv;
        s.gain_db = 3.5;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<EncodeSettings>(&json).unwrap(), s);
        // A file written by an older version with fewer fields.
        let old: EncodeSettings = serde_json::from_str(r#"{"container":"Mkv"}"#).unwrap();
        assert_eq!(old.container, Container::Mkv);
        assert_eq!(old.audio_codec, AudioCodec::Aac);
    }
}
