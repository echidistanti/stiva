//! What the user chooses for one encode: container, video codec and quality, audio codec.
//! Plain data with the rules about which choices go together; no I/O and no drawing.

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
            Container::Mp4 | Container::Mkv => {
                &[X264, X265, SvtAv1, Vp9, H264Amf, HevcAmf, Av1Amf, H264Nvenc, HevcNvenc, Av1Nvenc, H264Qsv, HevcQsv, Av1Qsv]
            }
            Container::WebM => &[Vp9, SvtAv1, Av1Amf, Av1Nvenc, Av1Qsv],
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
}

impl VideoCodec {
    pub const ALL: [VideoCodec; 13] = {
        use VideoCodec::*;
        [X264, X265, SvtAv1, Vp9, H264Amf, HevcAmf, Av1Amf, H264Nvenc, HevcNvenc, Av1Nvenc, H264Qsv, HevcQsv, Av1Qsv]
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
        }
    }

    pub fn is_hardware(self) -> bool {
        !matches!(self, VideoCodec::X264 | VideoCodec::X265 | VideoCodec::SvtAv1 | VideoCodec::Vp9)
    }

    pub fn is_hevc(self) -> bool {
        matches!(self, VideoCodec::X265 | VideoCodec::HevcAmf | VideoCodec::HevcNvenc | VideoCodec::HevcQsv)
    }

    /// Lowest and highest value of the quality scale: lower is better and bigger.
    pub fn crf_range(self) -> (u32, u32) {
        use VideoCodec::*;
        match self {
            X264 | X265 | H264Amf | HevcAmf | H264Nvenc | HevcNvenc | Av1Nvenc => (0, 51),
            SvtAv1 | Vp9 => (0, 63),
            Av1Amf => (0, 255),
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
        }
    }

    /// The ffmpeg options for this codec at a given quality and speed. The test encode that decides whether a
    /// hardware encoder is offered uses these same options, so what passes the test is what the real encode runs.
    pub fn args(self, crf: u32, preset: &str) -> Vec<String> {
        use VideoCodec::*;
        let crf = crf.to_string();
        let mut a: Vec<&str> = vec!["-c:v", self.encoder()];
        match self {
            X264 => a.extend(["-preset", preset, "-crf", &crf, "-pix_fmt", "yuv420p"]),
            X265 => a.extend(["-preset", preset, "-crf", &crf, "-pix_fmt", "yuv420p", "-x265-params", "log-level=error"]),
            SvtAv1 => a.extend(["-preset", preset, "-crf", &crf, "-pix_fmt", "yuv420p"]),
            Vp9 => a.extend(["-crf", &crf, "-b:v", "0", "-row-mt", "1", "-cpu-used", preset, "-pix_fmt", "yuv420p"]),
            // Constant quantizer; the other rate modes come with the bitrate options.
            H264Amf | HevcAmf | Av1Amf => a.extend(["-quality", preset, "-rc", "cqp", "-qp_i", &crf, "-qp_p", &crf, "-pix_fmt", "nv12"]),
            // Variable bitrate steered by a quality level, with no bitrate cap.
            H264Nvenc | HevcNvenc | Av1Nvenc => {
                a.extend(["-preset", preset, "-tune", "hq", "-rc", "vbr", "-cq", &crf, "-b:v", "0", "-pix_fmt", "nv12"])
            }
            H264Qsv | HevcQsv | Av1Qsv => a.extend(["-preset", preset, "-global_quality", &crf, "-pix_fmt", "nv12"]),
        }
        a.into_iter().map(String::from).collect()
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
        }
    }
}

impl EncodeSettings {
    /// Brings the choices back to a combination that works after one of them changed:
    /// a codec the container cannot hold is replaced, a quality out of range is pulled in.
    /// Returns true when something was replaced, so the window can say so.
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
        changed
    }

    /// The user picked another video codec: start from that codec's usual quality and speed.
    pub fn set_video_codec(&mut self, codec: VideoCodec) {
        if self.video_codec != codec {
            self.video_codec = codec;
            self.crf = codec.default_crf();
            self.preset = codec.default_preset().into();
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
            let args = c.args(c.default_crf(), c.default_preset());
            assert_eq!(&args[..2], ["-c:v", c.encoder()], "{c:?}");
        }
    }

    #[test]
    fn hardware_is_named_as_such_and_av1_alone_fits_webm() {
        assert!(!VideoCodec::X264.is_hardware() && VideoCodec::HevcAmf.is_hardware());
        assert!(VideoCodec::HevcQsv.is_hevc() && !VideoCodec::Av1Qsv.is_hevc());
        let webm: Vec<_> = Container::WebM.video_codecs().iter().filter(|c| c.is_hardware()).collect();
        assert_eq!(webm, [&VideoCodec::Av1Amf, &VideoCodec::Av1Nvenc, &VideoCodec::Av1Qsv]);
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
