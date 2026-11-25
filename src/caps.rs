//! Which encoders this ffmpeg has. Read once after ffmpeg is found, so the menus can hide what is missing.

use crate::model::{Rate, VideoCodec};
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Where VA-API encodes when no render node has been tried yet.
const FIRST_RENDER_NODE: &str = "/dev/dri/renderD128";

/// The render node the last probe found working. A global because the queue's jobs are saved to disk
/// and a graphics chip belongs to this PC, not to a job.
static VAAPI_DEVICE: Mutex<Option<String>> = Mutex::new(None);

/// The render node to give ffmpeg for VA-API.
pub fn vaapi_device() -> String {
    VAAPI_DEVICE.lock().ok().and_then(|d| d.clone()).unwrap_or_else(|| FIRST_RENDER_NODE.into())
}

/// The graphics devices that can encode, in order: `/dev/dri/renderD128`, `renderD129`...
fn render_nodes() -> Vec<String> {
    let mut nodes: Vec<String> = std::fs::read_dir("/dev/dri")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("renderD"))
        .map(|n| format!("/dev/dri/{n}"))
        .collect();
    nodes.sort();
    nodes
}

/// Picks the node on which most of `listed` pass a test; the first one wins a tie. With none passing, the first
/// node is kept so the logged reasons are those of the usual device.
fn pick_node(nodes: &[String], listed: &[VideoCodec], works: &(impl Fn(VideoCodec, &str) -> Result<(), String> + Sync)) -> String {
    let mut best: Option<(&String, usize)> = None;
    for node in nodes {
        let passed = run_tests(listed, &|c| works(c, node)).iter().filter(|(_, r)| r.is_ok()).count();
        if passed > 0 && best.is_none_or(|(_, b)| passed > b) {
            best = Some((node, passed));
        }
    }
    best.map(|(n, _)| n.clone()).or_else(|| nodes.first().cloned()).unwrap_or_else(|| FIRST_RENDER_NODE.into())
}

#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    encoders: HashSet<String>,
    /// First line of `ffmpeg -version`, for the options card.
    pub version: String,
    /// Hardware encoders that ffmpeg lists but that failed their test here, and what ffmpeg said.
    pub rejected: Vec<(String, String)>,
    /// Encoders that passed a test encode in 10-bit, and those that did not with what ffmpeg said.
    ten_bit: HashSet<String>,
    ten_bit_rejected: Vec<(String, String)>,
    /// Hardware encoders that took a test encode steered by a bitrate, and those that did not.
    bitrate: HashSet<String>,
    bitrate_rejected: Vec<(String, String)>,
    /// The render node the VA-API encoders were tried on, when this ffmpeg has any.
    pub vaapi_device: Option<String>,
}

/// Runs `works` for each codec, side by side, as failing ones take a moment.
fn run_tests(codecs: &[VideoCodec], works: &(impl Fn(VideoCodec) -> Result<(), String> + Sync)) -> Vec<(VideoCodec, Result<(), String>)> {
    std::thread::scope(|scope| {
        let runs: Vec<_> = codecs.iter().map(|&c| scope.spawn(move || (c, works(c)))).collect();
        runs.into_iter().filter_map(|r| r.join().ok()).collect()
    })
}

impl Capabilities {
    pub fn has(&self, encoder: &str) -> bool {
        self.encoders.contains(encoder)
    }

    /// True when a test encode in 10-bit worked with this encoder.
    pub fn has_ten_bit(&self, encoder: &str) -> bool {
        self.ten_bit.contains(encoder)
    }

    /// True when the encoder can be steered by a bitrate: the software ones always, a graphics chip once a test encode worked.
    pub fn has_bitrate(&self, codec: VideoCodec) -> bool {
        !codec.is_hardware() || self.bitrate.contains(codec.encoder())
    }

    /// Reads the list printed by `ffmpeg -encoders`: a header, a "------" line, then
    /// "flags name description" rows such as ` V....D libx264   libx264 H.264`.
    pub fn parse(listing: &str, version: &str) -> Capabilities {
        let mut encoders = HashSet::new();
        let mut in_rows = false;
        for line in listing.lines() {
            if !in_rows {
                in_rows = line.trim().starts_with("------");
                continue;
            }
            let mut it = line.split_whitespace();
            if let (Some(flags), Some(name)) = (it.next(), it.next()) {
                if flags.len() == 6 {
                    encoders.insert(name.to_string());
                }
            }
        }
        Capabilities { encoders, version: version.to_string(), ..Default::default() }
    }

    /// Drops the hardware encoders that fail `works`. The listing only says ffmpeg was built with them,
    /// not that this PC has the chip and the driver.
    fn keep_working(&mut self, works: impl Fn(VideoCodec) -> Result<(), String> + Sync) {
        let listed: Vec<VideoCodec> = VideoCodec::ALL.into_iter().filter(|c| c.is_hardware() && self.has(c.encoder())).collect();
        for (c, result) in run_tests(&listed, &works) {
            if let Err(why) = result {
                self.encoders.remove(c.encoder());
                self.rejected.push((c.encoder().to_string(), why));
            }
        }
    }

    /// Finds out which of the encoders still here can do 10-bit. Some builds lack it, and graphics chips differ.
    fn find_ten_bit(&mut self, works: impl Fn(VideoCodec) -> Result<(), String> + Sync) {
        let listed: Vec<VideoCodec> = VideoCodec::ALL.into_iter().filter(|c| c.can_ten_bit() && self.has(c.encoder())).collect();
        for (c, result) in run_tests(&listed, &works) {
            match result {
                Ok(()) => {
                    self.ten_bit.insert(c.encoder().to_string());
                }
                Err(why) => self.ten_bit_rejected.push((c.encoder().to_string(), why)),
            }
        }
    }

    /// Finds out which graphics chips accept a bitrate: their options for it differ from one maker to the next.
    fn find_bitrate(&mut self, works: impl Fn(VideoCodec) -> Result<(), String> + Sync) {
        let listed: Vec<VideoCodec> = VideoCodec::ALL.into_iter().filter(|c| c.is_hardware() && self.has(c.encoder())).collect();
        for (c, result) in run_tests(&listed, &works) {
            match result {
                Ok(()) => {
                    self.bitrate.insert(c.encoder().to_string());
                }
                Err(why) => self.bitrate_rejected.push((c.encoder().to_string(), why)),
            }
        }
    }

    /// Plain text on which encoders work here, in 8 and 10 bit, and why the others do not; kept in the logs
    /// folder so a missing option can be traced.
    pub fn report(&self) -> String {
        let mut out = format!("{}\n\n", self.version);
        if let Some(d) = &self.vaapi_device {
            out += &format!("VA-API device: {d}\n\n");
        }
        for c in VideoCodec::ALL {
            let name = c.encoder();
            if self.has(name) {
                out += &format!("{name}: works\n");
            } else if let Some((_, why)) = self.rejected.iter().find(|(n, _)| n == name) {
                out += &format!("{name}: not usable here: {why}\n");
                continue;
            } else {
                out += &format!("{name}: not in this ffmpeg\n");
                continue;
            }
            if !c.can_ten_bit() {
                out += &format!("{name} 10-bit: not offered for this encoder\n");
            } else if self.has_ten_bit(name) {
                out += &format!("{name} 10-bit: works\n");
            } else if let Some((_, why)) = self.ten_bit_rejected.iter().find(|(n, _)| n == name) {
                out += &format!("{name} 10-bit: not usable here: {why}\n");
            }
            if c.is_hardware() {
                if self.has_bitrate(c) {
                    out += &format!("{name} bitrate: works\n");
                } else if let Some((_, why)) = self.bitrate_rejected.iter().find(|(n, _)| n == name) {
                    out += &format!("{name} bitrate: not usable here: {why}\n");
                }
            }
        }
        out
    }

    pub fn probe(ffmpeg: &Path) -> Result<Capabilities, String> {
        let run = |args: &[&str]| -> Result<String, String> {
            let mut cmd = Command::new(ffmpeg);
            cmd.args(args);
            crate::deps::quiet(&mut cmd);
            let out = cmd.output().map_err(|e| format!("ffmpeg could not start: {e}"))?;
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        };
        let listing = run(&["-hide_banner", "-encoders"])?;
        let version = crate::deps::version_words(run(&["-hide_banner", "-version"])?.lines().next().unwrap_or(""));
        let mut caps = Capabilities::parse(&listing, &version);
        if caps.encoders.is_empty() {
            return Err("ffmpeg did not list any encoder".into());
        }
        let vaapi: Vec<VideoCodec> = VideoCodec::ALL.into_iter().filter(|c| c.is_vaapi() && caps.has(c.encoder())).collect();
        let device = if vaapi.is_empty() {
            FIRST_RENDER_NODE.to_string()
        } else {
            let node = pick_node(&render_nodes(), &vaapi, &|c, node| test_encode(ffmpeg, c, 8, false, node));
            caps.vaapi_device = Some(node.clone());
            if let Ok(mut d) = VAAPI_DEVICE.lock() {
                *d = Some(node.clone());
            }
            node
        };
        caps.keep_working(|c| test_encode(ffmpeg, c, 8, false, &device));
        caps.find_ten_bit(|c| test_encode(ffmpeg, c, 10, false, &device));
        caps.find_bitrate(|c| test_encode(ffmpeg, c, 8, true, &device));
        Ok(caps)
    }
}

/// Encodes a few black frames with the encoder, using the options a real encode would use.
/// The error is the first lines ffmpeg printed, which name the real cause.
/// The test picture: colour bars, a gradient and things that move, so a broken encoder cannot pass by
/// producing something flat. 8 frames of it.
const TEST_PICTURE: &str = "testsrc2=s=1280x720:r=25:d=0.32";

/// Below this the picture is damaged: a sound encode is far above (35 dB and more), garbage is under 10.
const MIN_PSNR_DB: f64 = 15.0;

/// Runs a command and waits up to 20 seconds. A broken driver can hang; then that encoder is given up on.
/// Gives the exit status (None when it was killed) and what the command said on stderr.
fn run_limited(mut cmd: Command) -> Result<(Option<std::process::ExitStatus>, String), String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    crate::deps::quiet(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < Duration::from_secs(20) => std::thread::sleep(Duration::from_millis(40)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let mut said = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut said);
    }
    Ok((status, said))
}

fn first_lines(said: &str) -> String {
    said.lines().filter(|l| !l.trim().is_empty()).take(2).collect::<Vec<_>>().join(" | ")
}

/// A short encode of the test picture. A graphics chip's encode is written to a real MP4 and read back, because
/// an encoder can finish without any error and still write a picture that is garbage; the others only have to
/// finish.
fn test_encode(ffmpeg: &Path, codec: VideoCodec, bits: u8, bitrate: bool, device: &str) -> Result<(), String> {
    let rate = if bitrate { Rate::Kbps(1000) } else { Rate::Quality(codec.default_crf()) };
    let check = codec.is_vaapi().then(|| std::env::temp_dir().join(format!("stiva-probe-{}-{}.mp4", std::process::id(), codec.encoder())));
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-v", "error", "-nostdin", "-y"]);
    if codec.is_vaapi() {
        cmd.args(["-vaapi_device", device]);
    }
    cmd.args(["-f", "lavfi", "-i", TEST_PICTURE, "-frames:v", "8"]);
    if let Some(upload) = codec.upload_filter(bits) {
        cmd.args(["-vf", upload]);
    }
    cmd.args(codec.args(rate, codec.default_preset(), bits));
    match &check {
        Some(file) => {
            if let Some(tag) = codec.mp4_tag() {
                cmd.args(["-tag:v", tag]);
            }
            cmd.args(["-f", "mp4"]).arg(file);
        }
        None => {
            cmd.args(["-f", "null", "-"]);
        }
    }
    let result = match run_limited(cmd)? {
        (Some(s), _) if s.success() => Ok(()),
        (Some(_), said) => Err(first_lines(&said)),
        (None, _) => Err("no answer within 20 seconds".into()),
    };
    let result = result.and_then(|()| check.as_deref().map_or(Ok(()), |file| picture_is_sound(ffmpeg, file)));
    if let Some(file) = &check {
        let _ = std::fs::remove_file(file);
    }
    result
}

/// Reads the encoded test file back and compares it with the test picture, frame by frame.
fn picture_is_sound(ffmpeg: &Path, file: &Path) -> Result<(), String> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-i"]).arg(file);
    cmd.args(["-f", "lavfi", "-i", TEST_PICTURE, "-lavfi"]);
    // Frames are paired by their position, so a container that keeps one frame more or less cannot fool it.
    cmd.arg("[0:v:0]settb=AVTB,setpts=N*1000,format=yuv420p[a];[1:v:0]settb=AVTB,setpts=N*1000,format=yuv420p[b];[a][b]psnr");
    cmd.args(["-f", "null", "-"]);
    let (status, said) = run_limited(cmd)?;
    if status.is_none() {
        return Err("reading the test picture back took too long".into());
    }
    match psnr_of(&said) {
        Some(db) if db >= MIN_PSNR_DB => Ok(()),
        Some(db) => Err(format!("it finishes, but the picture is garbage ({db:.1} dB against the test picture)")),
        None => Err(format!("could not read the test picture back: {}", first_lines(&said))),
    }
}

/// The average PSNR in dB that ffmpeg's psnr filter prints at the end ("inf" when the pictures are identical).
fn psnr_of(said: &str) -> Option<f64> {
    let line = said.lines().rev().find(|l| l.contains("PSNR") && l.contains("average:"))?;
    line.split("average:").nth(1)?.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "Encoders:\n V..... = Video\n A..... = Audio\n ------\n V....D libx264              libx264 H.264 / AVC\n V....D libsvtav1            SVT-AV1\n A....D aac                  AAC (Advanced Audio Coding)\n S..... srt                  SubRip subtitle\n";

    #[test]
    fn hardware_that_fails_its_test_is_dropped_and_software_is_left_alone() {
        let listing =
            format!("{LISTING} V....D h264_amf  AMD AMF H.264\n V....D hevc_nvenc  NVIDIA NVENC hevc\n V....D av1_qsv  Intel QSV av1\n");
        let mut c = Capabilities::parse(&listing, "ffmpeg version 7.1");
        assert!(c.has("h264_amf") && c.has("hevc_nvenc") && c.has("av1_qsv"));
        c.keep_working(|codec| if codec == VideoCodec::H264Amf { Ok(()) } else { Err("No device".to_string()) });
        assert!(c.has("h264_amf"));
        assert!(!c.has("hevc_nvenc") && !c.has("av1_qsv"));
        let report = c.report();
        assert!(report.contains("h264_amf: works") && report.contains("hevc_nvenc: not usable here: No device"));
        assert!(report.contains("av1_amf: not in this ffmpeg"));
        assert!(c.has("libx264") && c.has("libsvtav1") && c.has("aac"));
    }

    #[test]
    fn ten_bit_is_found_per_encoder_and_the_reason_is_kept() {
        let listing =
            format!("{LISTING} V....D libx265  libx265 H.265 / HEVC\n V....D hevc_amf  AMD AMF HEVC\n V....D h264_amf  AMD AMF H.264\n");
        let mut c = Capabilities::parse(&listing, "ffmpeg version 7.1");
        c.find_ten_bit(|codec| if codec == VideoCodec::X265 { Ok(()) } else { Err("no 10-bit here".to_string()) });
        assert!(c.has_ten_bit("libx265") && !c.has_ten_bit("libx264") && !c.has_ten_bit("hevc_amf"));
        assert!(!c.has_ten_bit("h264_amf"), "H.264 on the graphics chip is not tested in 10-bit at all");
        let report = c.report();
        assert!(report.contains("libx265 10-bit: works") && report.contains("hevc_amf 10-bit: not usable here: no 10-bit here"));
        assert!(report.contains("h264_amf 10-bit: not offered"));
    }

    #[test]
    fn the_render_node_with_most_working_vaapi_encoders_is_picked() {
        use VideoCodec::{Av1Vaapi, H264Vaapi, HevcVaapi};
        const A: &str = "/dev/dri/renderD128";
        const B: &str = "/dev/dri/renderD129";
        const C: &str = "/dev/dri/renderD130";
        let nodes: Vec<String> = [A, B, C].map(String::from).to_vec();
        let listed = [H264Vaapi, HevcVaapi, Av1Vaapi];
        // Which codecs pass on which node.
        let pick = |good: Vec<(&str, Vec<VideoCodec>)>| {
            pick_node(&nodes, &listed, &|c, node| {
                if good.iter().any(|(n, ok)| *n == node && ok.contains(&c)) { Ok(()) } else { Err("no".to_string()) }
            })
        };
        // The second chip does more than the first.
        assert_eq!(pick(vec![(A, vec![H264Vaapi]), (B, listed.to_vec())]), B);
        // A tie goes to the first node that passes.
        assert_eq!(pick(vec![(B, vec![H264Vaapi]), (C, vec![HevcVaapi])]), B);
        // Nothing works anywhere: the first node stays, so the reasons logged are its own.
        assert_eq!(pick(vec![]), A);
        // No render node at all: the usual name, which ffmpeg will say it cannot open.
        assert_eq!(pick_node(&[], &listed, &|_, _| Err("no".to_string())), FIRST_RENDER_NODE);
    }

    #[test]
    fn the_vaapi_device_is_named_in_the_report() {
        let mut caps = Capabilities::parse(LISTING, "ffmpeg 7.1");
        assert!(!caps.report().contains("VA-API device"));
        caps.vaapi_device = Some("/dev/dri/renderD129".into());
        assert!(caps.report().contains("VA-API device: /dev/dri/renderD129\n"), "{}", caps.report());
    }

    #[test]
    fn the_psnr_is_read_from_the_last_line_ffmpeg_prints() {
        let said = "[Parsed_psnr_2 @ 0x55] PSNR y:41.20 u:44.00 v:45.10 average:42.35 min:40.10 max:48.00\n";
        assert_eq!(psnr_of(said), Some(42.35));
        assert_eq!(psnr_of("PSNR y:inf u:inf v:inf average:inf min:inf max:inf"), Some(f64::INFINITY));
        assert_eq!(psnr_of("Conversion failed!"), None);
    }

    /// Needs an ffmpeg with libx264 on the PATH; where there is none the test has nothing to run.
    fn ffmpeg_with_x264() -> Option<&'static Path> {
        let out = Command::new("ffmpeg").args(["-hide_banner", "-encoders"]).output().ok()?;
        String::from_utf8_lossy(&out.stdout).contains("libx264").then_some(Path::new("ffmpeg"))
    }

    fn write_mp4(ffmpeg: &Path, source: &str, file: &Path) {
        let ok = Command::new(ffmpeg)
            .args(["-hide_banner", "-v", "error", "-nostdin", "-y", "-f", "lavfi", "-i", source, "-t", "0.32", "-frames:v", "8"])
            .args(["-c:v", "libx264", "-qp", "26", "-f", "mp4"])
            .arg(file)
            .status()
            .map(|s| s.success());
        assert_eq!(ok.ok(), Some(true), "the test file could not be written");
    }

    #[test]
    fn an_encode_that_finishes_with_the_wrong_picture_is_refused() {
        let Some(ffmpeg) = ffmpeg_with_x264() else { return };
        let dir = std::env::temp_dir();
        let good = dir.join(format!("stiva-test-good-{}.mp4", std::process::id()));
        let bad = dir.join(format!("stiva-test-bad-{}.mp4", std::process::id()));
        write_mp4(ffmpeg, TEST_PICTURE, &good);
        write_mp4(ffmpeg, "mandelbrot=s=1280x720:r=25", &bad);
        let sound = picture_is_sound(ffmpeg, &good);
        let garbage = picture_is_sound(ffmpeg, &bad);
        let _ = std::fs::remove_file(&good);
        let _ = std::fs::remove_file(&bad);
        assert_eq!(sound, Ok(()));
        assert!(garbage.as_ref().is_err_and(|e| e.contains("garbage")), "{garbage:?}");
    }

    #[test]
    fn rows_after_the_dashes_are_read() {
        let c = Capabilities::parse(LISTING, "ffmpeg version 7.1");
        assert!(c.has("libx264") && c.has("libsvtav1") && c.has("aac"));
        assert!(!c.has("libx265"));
        assert!(!c.has("="), "the legend above the dashes is not an encoder");
        assert_eq!(c.version, "ffmpeg version 7.1");
    }
}
