//! Which encoders this ffmpeg has. Read once after ffmpeg is found, so the menus can hide what is missing.

use crate::model::VideoCodec;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

    /// Plain text on which encoders work here, in 8 and 10 bit, and why the others do not; kept in the logs
    /// folder so a missing option can be traced.
    pub fn report(&self) -> String {
        let mut out = format!("{}\n\n", self.version);
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
        // "ffmpeg version 7.1.1-essentials_build-www.gyan.dev Copyright (c) 2000-2025 ..." -> the first three words.
        let version =
            run(&["-hide_banner", "-version"])?.lines().next().unwrap_or("").split_whitespace().take(3).collect::<Vec<_>>().join(" ");
        let mut caps = Capabilities::parse(&listing, &version);
        if caps.encoders.is_empty() {
            return Err("ffmpeg did not list any encoder".into());
        }
        caps.keep_working(|c| test_encode(ffmpeg, c, 8));
        caps.find_ten_bit(|c| test_encode(ffmpeg, c, 10));
        Ok(caps)
    }
}

/// Encodes a few black frames with the encoder, using the options a real encode would use.
/// The error is the first lines ffmpeg printed, which name the real cause.
fn test_encode(ffmpeg: &Path, codec: VideoCodec, bits: u8) -> Result<(), String> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-v", "error", "-nostdin", "-f", "lavfi", "-i", "color=c=black:s=640x360:r=25:d=0.5", "-frames:v", "8"]);
    cmd.args(codec.args(codec.default_crf(), codec.default_preset(), bits));
    cmd.args(["-f", "null", "-"]);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    crate::deps::quiet(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    // A broken driver can hang; give it a while and then give up on that encoder.
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
    match status {
        Some(s) if s.success() => Ok(()),
        Some(_) => Err(said.lines().filter(|l| !l.trim().is_empty()).take(2).collect::<Vec<_>>().join(" | ")),
        None => Err("no answer within 20 seconds".into()),
    }
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
    fn rows_after_the_dashes_are_read() {
        let c = Capabilities::parse(LISTING, "ffmpeg version 7.1");
        assert!(c.has("libx264") && c.has("libsvtav1") && c.has("aac"));
        assert!(!c.has("libx265"));
        assert!(!c.has("="), "the legend above the dashes is not an encoder");
        assert_eq!(c.version, "ffmpeg version 7.1");
    }
}
