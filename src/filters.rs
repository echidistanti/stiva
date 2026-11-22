//! The picture filters of the Filters tab: what the user chose, the ffmpeg filter chain it becomes,
//! and the search for black bars. Plain data and text, apart from the search, which runs ffmpeg.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::{Command, Stdio};

/// Pixels to cut away from each side.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Margins {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl Margins {
    pub fn is_zero(&self) -> bool {
        *self == Margins::default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Crop {
    #[default]
    None,
    /// Black bars are looked for in the file when the encode starts.
    Auto,
    Manual(Margins),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Rotate {
    #[default]
    None,
    Clockwise,
    CounterClockwise,
    Half,
}

impl Rotate {
    pub const ALL: [Rotate; 4] = [Rotate::None, Rotate::Clockwise, Rotate::CounterClockwise, Rotate::Half];

    pub fn label(self) -> &'static str {
        match self {
            Rotate::None => "No rotation",
            Rotate::Clockwise => "90° clockwise",
            Rotate::CounterClockwise => "90° counter-clockwise",
            Rotate::Half => "180°",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Deinterlace {
    #[default]
    Off,
    Yadif,
    Bwdif,
}

impl Deinterlace {
    pub const ALL: [Deinterlace; 3] = [Deinterlace::Off, Deinterlace::Yadif, Deinterlace::Bwdif];

    pub fn label(self) -> &'static str {
        match self {
            Deinterlace::Off => "Off",
            Deinterlace::Yadif => "Yadif (fast)",
            Deinterlace::Bwdif => "Bwdif (cleaner, slower)",
        }
    }
}

/// How hard a denoise or a sharpen works.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Strength {
    #[default]
    Off,
    Light,
    Medium,
    Strong,
}

impl Strength {
    pub const ALL: [Strength; 4] = [Strength::Off, Strength::Light, Strength::Medium, Strength::Strong];

    pub fn label(self) -> &'static str {
        match self {
            Strength::Off => "Off",
            Strength::Light => "Light",
            Strength::Medium => "Medium",
            Strength::Strong => "Strong",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Filters {
    pub crop: Crop,
    pub rotate: Rotate,
    pub flip_h: bool,
    pub flip_v: bool,
    pub deinterlace: Deinterlace,
    /// Undoes the 3:2 pulldown of film shown on TV, which leaves 24 pictures a second out of 30.
    pub detelecine: bool,
    pub denoise: Strength,
    pub sharpen: Strength,
    pub grayscale: bool,
    /// A filter chain typed by the user, added at the end as it is.
    pub custom: String,
}

/// The largest margin the controls allow.
pub const MAX_MARGIN: u32 = 4000;

impl Filters {
    pub fn normalize(&mut self) {
        if let Crop::Manual(m) = &mut self.crop {
            for v in [&mut m.left, &mut m.top, &mut m.right, &mut m.bottom] {
                *v = (*v).min(MAX_MARGIN);
            }
        }
        self.custom = self.custom.trim().to_string();
    }

    /// The names of what is switched on, for the queue's one-line summary.
    pub fn active(&self) -> Vec<&'static str> {
        let mut v = vec![];
        match self.crop {
            Crop::None => {}
            Crop::Auto => v.push("auto crop"),
            Crop::Manual(m) if !m.is_zero() => v.push("crop"),
            Crop::Manual(_) => {}
        }
        if self.rotate != Rotate::None || self.flip_h || self.flip_v {
            v.push("rotate");
        }
        if self.detelecine {
            v.push("detelecine");
        }
        if self.deinterlace != Deinterlace::Off {
            v.push("deinterlace");
        }
        if self.denoise != Strength::Off {
            v.push("denoise");
        }
        if self.sharpen != Strength::Off {
            v.push("sharpen");
        }
        if self.grayscale {
            v.push("grayscale");
        }
        if !self.custom.trim().is_empty() {
            v.push("custom filter");
        }
        v
    }

    /// The `-vf` value: the filters in the order that gives the best picture, then `scale` (the size limit),
    /// then the user's own text. None when there is nothing to apply.
    pub fn chain(&self, scale: Option<String>) -> Option<String> {
        let mut f: Vec<String> = vec![];
        if self.detelecine {
            f.push("fieldmatch,yadif=deint=interlaced,decimate".into());
        }
        match self.deinterlace {
            Deinterlace::Off => {}
            Deinterlace::Yadif => f.push("yadif=mode=send_frame:parity=auto:deint=all".into()),
            Deinterlace::Bwdif => f.push("bwdif=mode=send_frame:parity=auto:deint=all".into()),
        }
        if let Crop::Manual(m) = self.crop {
            if !m.is_zero() {
                // Both sides stay even, which the encoders need.
                f.push(format!(
                    "crop=w='trunc((iw-{})/2)*2':h='trunc((ih-{})/2)*2':x={}:y={}",
                    m.left + m.right,
                    m.top + m.bottom,
                    m.left,
                    m.top
                ));
            }
        }
        match self.rotate {
            Rotate::None => {}
            Rotate::Clockwise => f.push("transpose=1".into()),
            Rotate::CounterClockwise => f.push("transpose=2".into()),
            Rotate::Half => f.push("hflip,vflip".into()),
        }
        if self.flip_h {
            f.push("hflip".into());
        }
        if self.flip_v {
            f.push("vflip".into());
        }
        match self.denoise {
            Strength::Off => {}
            Strength::Light => f.push("hqdn3d=2:1:2:3".into()),
            Strength::Medium => f.push("hqdn3d=3:2:2:3".into()),
            Strength::Strong => f.push("hqdn3d=7:7:5:5".into()),
        }
        match self.sharpen {
            Strength::Off => {}
            Strength::Light => f.push("unsharp=5:5:0.4:5:5:0".into()),
            Strength::Medium => f.push("unsharp=5:5:0.8:5:5:0".into()),
            Strength::Strong => f.push("unsharp=5:5:1.2:5:5:0".into()),
        }
        if self.grayscale {
            f.push("hue=s=0".into());
        }
        f.extend(scale);
        let custom = self.custom.trim().trim_matches(',');
        if !custom.is_empty() {
            f.push(custom.to_string());
        }
        (!f.is_empty()).then(|| f.join(","))
    }

    /// The same filters with the automatic crop replaced by what was found.
    pub fn with_crop_found(&self, found: Option<Margins>) -> Filters {
        let mut f = self.clone();
        if f.crop == Crop::Auto {
            f.crop = found.map_or(Crop::None, Crop::Manual);
        }
        f
    }
}

/// Reads the `crop=W:H:X:Y` that ffmpeg's cropdetect prints, keeping the last one: with `reset=0`
/// that is the largest picture area seen so far.
pub fn last_crop(log: &str) -> Option<(u32, u32, u32, u32)> {
    log.lines().rev().find_map(|l| {
        let rest = &l[l.find("crop=")? + 5..];
        let mut it = rest.split_whitespace().next()?.split(':').map(|n| n.parse::<u32>().ok());
        Some((it.next()??, it.next()??, it.next()??, it.next()??))
    })
}

/// Margins from a found area, or None when there is nothing worth cutting (a couple of pixels is noise).
pub fn margins_of((w, h, x, y): (u32, u32, u32, u32), frame: (u32, u32)) -> Option<Margins> {
    let (fw, fh) = frame;
    if w == 0 || h == 0 || x + w > fw || y + h > fh {
        return None;
    }
    let m = Margins { left: x, top: y, right: fw - w - x, bottom: fh - h - y };
    (m.left + m.right > 2 || m.top + m.bottom > 2).then_some(m)
}

/// Looks for black bars: a few seconds are read at several points of the file, and only what is black
/// in all of them is cut, so a dark scene cannot make the picture lose its edges.
pub fn find_black_bars(ffmpeg: &Path, file: &Path, duration: Option<f64>, frame: (u32, u32)) -> Option<Margins> {
    let points: Vec<f64> = match duration {
        Some(d) if d > 60.0 => [0.12, 0.35, 0.6, 0.85].iter().map(|p| d * p).collect(),
        Some(d) if d > 12.0 => vec![d * 0.3, d * 0.7],
        _ => vec![0.0],
    };
    let found: Vec<Margins> = std::thread::scope(|scope| {
        let runs: Vec<_> = points.iter().map(|&at| scope.spawn(move || sample(ffmpeg, file, at, frame))).collect();
        runs.into_iter().filter_map(|r| r.join().ok().flatten()).collect()
    });
    if found.len() < points.len().div_ceil(2) {
        return None; // too few samples read to trust
    }
    let least = |pick: fn(&Margins) -> u32| found.iter().map(pick).min().unwrap_or(0);
    let m = Margins { left: least(|m| m.left), top: least(|m| m.top), right: least(|m| m.right), bottom: least(|m| m.bottom) };
    (m.left + m.right > 2 || m.top + m.bottom > 2).then_some(m)
}

/// The bars seen in about three seconds starting at `at`; None when the file could not be read there.
/// A sample with no bars gives zero margins, which keeps the other samples from cropping.
fn sample(ffmpeg: &Path, file: &Path, at: f64, frame: (u32, u32)) -> Option<Margins> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-ss", &format!("{at:.3}"), "-i"]).arg(file);
    cmd.args(["-t", "3", "-vf", "cropdetect=limit=24:round=2:reset=0", "-an", "-sn", "-f", "null", "-"]);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    crate::deps::quiet(&mut cmd);
    let out = cmd.output().ok()?;
    let log = String::from_utf8_lossy(&out.stderr);
    let found = last_crop(&log)?;
    Some(margins_of(found, frame).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_switched_on_gives_no_chain_and_the_scale_stands_alone() {
        let f = Filters::default();
        assert_eq!(f.chain(None), None);
        assert_eq!(f.chain(Some("scale=1:1".into())).as_deref(), Some("scale=1:1"));
        assert!(f.active().is_empty());
    }

    #[test]
    fn the_filters_come_in_the_order_that_serves_the_picture() {
        let f = Filters {
            crop: Crop::Manual(Margins { left: 4, top: 60, right: 4, bottom: 60 }),
            rotate: Rotate::Clockwise,
            flip_h: true,
            deinterlace: Deinterlace::Bwdif,
            detelecine: true,
            denoise: Strength::Medium,
            sharpen: Strength::Light,
            grayscale: true,
            custom: "  eq=brightness=0.1,  ".into(),
            ..Default::default()
        };
        let chain = f.chain(Some("scale=640:-2".into())).unwrap();
        let at = |needle: &str| chain.find(needle).unwrap_or_else(|| panic!("{needle} missing in {chain}"));
        let order = ["fieldmatch", "bwdif", "crop=", "transpose=1", "hflip", "hqdn3d=3:2:2:3", "unsharp", "hue=s=0", "scale=640", "eq="];
        assert!(order.windows(2).all(|w| at(w[0]) < at(w[1])), "{chain}");
        assert!(chain.contains("x=4:y=60") && chain.contains("iw-8") && chain.contains("ih-120"));
        assert!(chain.ends_with("eq=brightness=0.1"), "{chain}");
    }

    #[test]
    fn a_half_turn_is_two_flips_and_zero_margins_cut_nothing() {
        let f = Filters { rotate: Rotate::Half, crop: Crop::Manual(Margins::default()), ..Default::default() };
        assert_eq!(f.chain(None).as_deref(), Some("hflip,vflip"));
        assert_eq!(f.active(), vec!["rotate"]);
    }

    #[test]
    fn auto_crop_is_resolved_with_what_was_found() {
        let f = Filters { crop: Crop::Auto, ..Default::default() };
        let m = Margins { left: 0, top: 60, right: 0, bottom: 60 };
        assert_eq!(f.with_crop_found(Some(m)).crop, Crop::Manual(m));
        assert_eq!(f.with_crop_found(None).crop, Crop::None);
        let manual = Filters { crop: Crop::Manual(m), ..Default::default() };
        assert_eq!(manual.with_crop_found(None).crop, Crop::Manual(m), "a manual crop is not touched");
        assert_eq!(f.chain(None), None, "an automatic crop that was not resolved does nothing");
    }

    #[test]
    fn cropdetect_output_is_read_and_turned_into_margins() {
        let log = "[Parsed_cropdetect_0 @ 0x1] x1:0 x2:639 y1:60 y2:419 w:640 h:360 x:0 y:60 pts:1 t:0.04 limit:0.094118 crop=640:360:0:60\n[Parsed_cropdetect_0 @ 0x1] crop=640:352:0:64\nframe=3\n";
        assert_eq!(last_crop(log), Some((640, 352, 0, 64)));
        assert_eq!(last_crop("nothing here"), None);
        assert_eq!(margins_of((640, 360, 0, 60), (640, 480)), Some(Margins { left: 0, top: 60, right: 0, bottom: 60 }));
        assert_eq!(margins_of((640, 478, 0, 1), (640, 480)), None, "a pixel or two is noise");
        assert_eq!(margins_of((700, 360, 0, 60), (640, 480)), None, "an area outside the picture is refused");
    }

    #[test]
    fn old_files_without_filters_still_load() {
        let f: Filters = serde_json::from_str("{}").unwrap();
        assert_eq!(f, Filters::default());
        let g: Filters = serde_json::from_str(r#"{"crop":{"Manual":{"left":1,"top":2,"right":3,"bottom":4}},"denoise":"Strong"}"#).unwrap();
        assert_eq!(g.crop, Crop::Manual(Margins { left: 1, top: 2, right: 3, bottom: 4 }));
        assert_eq!(g.denoise, Strength::Strong);
    }
}
