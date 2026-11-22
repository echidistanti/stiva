//! Named sets of encode settings: the ones that come with Stiva, and the user's own, one JSON file each.
//! The same file format is what Export writes and Import reads, so presets can be passed around.

use crate::model::{AudioCodec, Container, EncodeSettings, Mixdown, VideoCodec};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub settings: EncodeSettings,
}

fn make(name: &str, set: impl FnOnce(&mut EncodeSettings)) -> Preset {
    let mut settings = EncodeSettings::default();
    set(&mut settings);
    settings.normalize();
    Preset { name: name.into(), settings }
}

/// Settings for H.264 in MP4 with AAC stereo, the common ground of the web and storage presets.
fn h264(s: &mut EncodeSettings, crf: u32, speed: &str, kbps: u32, size: (u32, u32)) {
    s.container = Container::Mp4;
    s.set_video_codec(VideoCodec::X264);
    s.crf = crf;
    s.preset = speed.into();
    s.set_audio_codec(AudioCodec::Aac);
    s.audio_kbps = kbps;
    s.mixdown = Mixdown::Stereo;
    s.max_size = Some(size);
}

/// The presets that come with the program. They cannot be changed or deleted; "Save as" makes a copy.
pub fn builtin() -> Vec<Preset> {
    vec![
        make("Web 1080p", |s| h264(s, 22, "medium", 160, (1920, 1080))),
        make("Web 720p", |s| h264(s, 23, "medium", 128, (1280, 720))),
        make("Mobile 480p", |s| h264(s, 24, "fast", 96, (854, 480))),
        // Small H.264 files in the manner of the YIFY releases: stereo AAC, a slow preset and a high CRF.
        make("Storage 1080p", |s| h264(s, 27, "slow", 128, (1920, 1080))),
        make("Storage 720p", |s| h264(s, 26, "slow", 128, (1280, 720))),
        make("Archive H.265 10-bit", |s| {
            s.container = Container::Mkv;
            s.set_video_codec(VideoCodec::X265);
            s.crf = 20;
            s.preset = "slow".into();
            s.bit_depth = 10;
            s.set_audio_codec(AudioCodec::Copy);
        }),
        make("Small AV1 10-bit", |s| {
            s.container = Container::Mkv;
            s.set_video_codec(VideoCodec::SvtAv1);
            s.bit_depth = 10;
            s.set_audio_codec(AudioCodec::Opus);
            s.audio_kbps = 128;
            s.mixdown = Mixdown::Stereo;
        }),
        make("WebM VP9 1080p", |s| {
            s.container = Container::WebM;
            s.set_video_codec(VideoCodec::Vp9);
            s.set_audio_codec(AudioCodec::Opus);
            s.audio_kbps = 128;
            s.mixdown = Mixdown::Stereo;
            s.max_size = Some((1920, 1080));
        }),
        make("Music MP3 192", |s| {
            s.container = Container::Mp3;
            s.normalize();
            s.audio_kbps = 192;
        }),
        make("Music AAC 192", |s| {
            s.container = Container::M4a;
            s.normalize();
            s.audio_kbps = 192;
        }),
        make("Lossless FLAC", |s| {
            s.container = Container::Flac;
            s.normalize();
        }),
    ]
}

pub fn dir() -> PathBuf {
    crate::deps::data_dir().join("presets")
}

/// The name of a preset as a file name.
fn slug(name: &str) -> String {
    let s: String =
        name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') { c } else { '_' }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "preset".into() } else { s }
}

/// The file name Export suggests for a preset.
pub fn file_name(name: &str) -> String {
    format!("{}.json", slug(name))
}

/// Reads one preset file, as Import and the presets folder both do.
pub fn read_file(path: &Path) -> Result<Preset, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut p: Preset = serde_json::from_str(&text).map_err(|_| "This is not a Stiva preset file.".to_string())?;
    p.name = p.name.trim().to_string();
    if p.name.is_empty() {
        return Err("This preset file has no name.".into());
    }
    p.settings.normalize();
    Ok(p)
}

pub fn write_file(path: &Path, p: &Preset) -> Result<(), String> {
    let json = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    let part = path.with_extension("json.part");
    std::fs::write(&part, json).map_err(|e| e.to_string())?;
    std::fs::rename(&part, path).map_err(|e| e.to_string())
}

/// The user's presets in `dir`, by name. Files that cannot be read are left out.
pub fn load_dir(dir: &Path) -> Vec<(PathBuf, Preset)> {
    let mut found: Vec<(PathBuf, Preset)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| read_file(&p).ok().map(|preset| (p, preset)))
        .collect();
    found.sort_by_key(|(_, p)| p.name.to_lowercase());
    found
}

/// Saves `preset` in `dir`: over the file that already holds a preset of that name, or in a new file.
pub fn save_in(dir: &Path, preset: &Preset) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = match load_dir(dir).into_iter().find(|(_, p)| p.name == preset.name) {
        Some((path, _)) => path,
        None => crate::settings::unique_with(&dir.join(format!("{}.json", slug(&preset.name)))),
    };
    write_file(&path, preset)
}

pub fn delete_in(dir: &Path, name: &str) -> Result<(), String> {
    match load_dir(dir).into_iter().find(|(_, p)| p.name == name) {
        Some((path, _)) => std::fs::remove_file(path).map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

/// `wanted`, or `wanted (2)`, `wanted (3)`... when the name is taken. Names are compared without regard to case.
pub fn unique_name(taken: &[&str], wanted: &str) -> String {
    let is_taken = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !is_taken(wanted) {
        return wanted.to_string();
    }
    (2..).map(|n| format!("{wanted} ({n})")).find(|n| !is_taken(n)).expect("an endless range always ends")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stiva-presets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_built_in_presets_are_named_once_and_are_already_in_order() {
        let all = builtin();
        assert!(all.len() >= 10, "{}", all.len());
        let names: Vec<_> = all.iter().map(|p| p.name.to_lowercase()).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "a name is used twice");
        for p in &all {
            let mut again = p.settings.clone();
            assert!(!again.normalize() && again == p.settings, "{} needs fixing by normalize", p.name);
            let args = p.settings.video_codec.args(p.settings.crf, &p.settings.preset, p.settings.bit_depth);
            assert!(args.len() > 4, "{}", p.name);
        }
    }

    #[test]
    fn the_storage_presets_are_small_stereo_h264_in_mp4_with_a_size_limit() {
        let all = builtin();
        for (name, size) in [("Storage 720p", (1280, 720)), ("Storage 1080p", (1920, 1080))] {
            let p = all.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} is missing"));
            let s = &p.settings;
            assert_eq!((s.container, s.video_codec, s.audio_codec), (Container::Mp4, VideoCodec::X264, AudioCodec::Aac), "{name}");
            assert_eq!((s.mixdown, s.max_size, s.audio_kbps, s.bit_depth), (Mixdown::Stereo, Some(size), 128, 8), "{name}");
            assert!(s.crf >= 25 && s.preset == "slow", "{name}");
        }
    }

    #[test]
    fn ten_bit_presets_say_so_and_the_audio_ones_have_no_video() {
        let all = builtin();
        let get = |n: &str| all.iter().find(|p| p.name == n).unwrap().settings.clone();
        assert_eq!(get("Archive H.265 10-bit").bit_depth, 10);
        assert_eq!(get("Small AV1 10-bit").video_codec, VideoCodec::SvtAv1);
        assert!(get("Music MP3 192").container.audio_only() && get("Music MP3 192").audio_kbps == 192);
        assert_eq!(get("Lossless FLAC").audio_codec, AudioCodec::Flac);
    }

    #[test]
    fn a_saved_preset_comes_back_and_the_same_name_replaces_it() {
        let dir = temp("save");
        let mut p = Preset { name: "My phone".into(), settings: EncodeSettings::default() };
        p.settings.crf = 30;
        save_in(&dir, &p).unwrap();
        assert_eq!(load_dir(&dir).len(), 1);
        p.settings.crf = 18;
        save_in(&dir, &p).unwrap();
        let loaded = load_dir(&dir);
        assert_eq!(loaded.len(), 1, "same name, same file");
        assert_eq!(loaded[0].1.settings.crf, 18);
        let other = Preset { name: "My TV".into(), settings: EncodeSettings::default() };
        save_in(&dir, &other).unwrap();
        assert_eq!(load_dir(&dir).iter().map(|(_, p)| p.name.as_str()).collect::<Vec<_>>(), ["My phone", "My TV"]);
        delete_in(&dir, "My phone").unwrap();
        assert_eq!(load_dir(&dir).len(), 1);
        delete_in(&dir, "never was").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_that_differ_only_in_odd_characters_do_not_share_a_file() {
        let dir = temp("slug");
        for name in ["a/b", "a:b", "a?b"] {
            save_in(&dir, &Preset { name: name.into(), settings: EncodeSettings::default() }).unwrap();
        }
        assert_eq!(load_dir(&dir).len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_refuses_what_is_not_a_preset_and_fixes_what_is_out_of_range() {
        let dir = temp("import");
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("junk.json");
        std::fs::write(&junk, "{\"hello\": 1}").unwrap();
        assert!(read_file(&junk).is_err());
        std::fs::write(&junk, "not json at all").unwrap();
        assert!(read_file(&junk).is_err());
        let wild = dir.join("wild.json");
        std::fs::write(
            &wild,
            r#"{"name": " Wild ", "settings": {"container": "WebM", "video_codec": "X264", "crf": 999, "bit_depth": 12}}"#,
        )
        .unwrap();
        let p = read_file(&wild).unwrap();
        assert_eq!(p.name, "Wild");
        assert_eq!(p.settings.video_codec, VideoCodec::Vp9, "WebM cannot hold H.264");
        assert!(p.settings.crf <= 63 && p.settings.bit_depth == 10);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        assert_eq!(unique_name(&["Web 720p"], "Mine"), "Mine");
        assert_eq!(unique_name(&["Web 720p"], "web 720P"), "web 720P (2)");
        assert_eq!(unique_name(&["Mine", "Mine (2)"], "Mine"), "Mine (3)");
    }
}
