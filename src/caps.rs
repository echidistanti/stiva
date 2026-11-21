//! Which encoders this ffmpeg has. Read once after ffmpeg is found, so the menus can hide what is missing.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    encoders: HashSet<String>,
    /// First line of `ffmpeg -version`, for the options card.
    pub version: String,
}

impl Capabilities {
    pub fn has(&self, encoder: &str) -> bool {
        self.encoders.contains(encoder)
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
        Capabilities { encoders, version: version.to_string() }
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
        let caps = Capabilities::parse(&listing, &version);
        if caps.encoders.is_empty() {
            return Err("ffmpeg did not list any encoder".into());
        }
        Ok(caps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "Encoders:\n V..... = Video\n A..... = Audio\n ------\n V....D libx264              libx264 H.264 / AVC\n V....D libsvtav1            SVT-AV1\n A....D aac                  AAC (Advanced Audio Coding)\n S..... srt                  SubRip subtitle\n";

    #[test]
    fn rows_after_the_dashes_are_read() {
        let c = Capabilities::parse(LISTING, "ffmpeg version 7.1");
        assert!(c.has("libx264") && c.has("libsvtav1") && c.has("aac"));
        assert!(!c.has("libx265"));
        assert!(!c.has("="), "the legend above the dashes is not an encoder");
        assert_eq!(c.version, "ffmpeg version 7.1");
    }
}
