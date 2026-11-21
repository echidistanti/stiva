//! What the program remembers between runs, and where the encoded file goes.

use crate::model::EncodeSettings;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum DestMode {
    /// One folder for everything: the chosen one, or Videos\Stiva.
    #[default]
    Folder,
    NextToSource,
}

/// What to do when the file to write is already there.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum OnExists {
    #[default]
    Rename,
    Overwrite,
    Skip,
    Ask,
}

impl OnExists {
    pub const ALL: [OnExists; 4] = [OnExists::Rename, OnExists::Overwrite, OnExists::Skip, OnExists::Ask];

    pub fn label(self) -> &'static str {
        match self {
            OnExists::Rename => "Add a number to the name",
            OnExists::Overwrite => "Overwrite it",
            OnExists::Skip => "Skip the file",
            OnExists::Ask => "Ask me",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WindowGeom {
    pub pos: Option<[f32; 2]>,
    pub size: [f32; 2],
    pub maximized: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub dest_mode: DestMode,
    pub dest_dir: Option<PathBuf>,
    pub on_exists: OnExists,
    /// Folder holding ffmpeg and ffprobe, when the user does not want the downloaded ones.
    pub ffmpeg_dir: Option<PathBuf>,
    pub encode: EncodeSettings,
    pub window: Option<WindowGeom>,
}

fn file() -> PathBuf {
    crate::deps::data_dir().join("settings.json")
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let path = file();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(json) = serde_json::to_string_pretty(self) else {
            return;
        };
        let part = path.with_extension("json.part");
        if std::fs::write(&part, json).is_ok() {
            let _ = std::fs::rename(&part, &path);
        }
    }

    /// The folder used when files go to one place.
    pub fn out_dir(&self) -> PathBuf {
        self.dest_dir
            .clone()
            .or_else(|| dirs::video_dir().map(|d| d.join("Stiva")))
            .or_else(|| dirs::home_dir().map(|d| d.join("Stiva")))
            .unwrap_or_else(|| PathBuf::from("Stiva"))
    }

    /// Where the file for `src` would be written, before any check about existing files.
    pub fn output_path(&self, src: &Path) -> PathBuf {
        let dir = match self.dest_mode {
            DestMode::NextToSource => src.parent().map(Path::to_path_buf).unwrap_or_else(|| self.out_dir()),
            DestMode::Folder => self.out_dir(),
        };
        let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "video".into());
        dir.join(format!("{stem}.{}", self.encode.container.ext()))
    }
}

#[derive(Debug, PartialEq)]
pub enum Resolution {
    Use(PathBuf),
    /// The file is there and the user wants to be asked.
    Ask(PathBuf),
    Skip,
}

fn same_file(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let p = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let s = p.to_string_lossy().into_owned();
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    norm(a) == norm(b)
}

/// "name.mp4" -> "name (1).mp4", "name (2).mp4", ... the first one that is free.
pub fn unique(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    (1..).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).expect("an endless range always ends")
}

/// Applies the choice about existing files. The source file is never the target: when the output
/// would land on the file being read (next to the source, same extension), it gets a number instead,
/// whatever the choice is.
pub fn resolve(policy: OnExists, desired: PathBuf, src: &Path) -> Resolution {
    if same_file(&desired, src) {
        return Resolution::Use(unique(&desired));
    }
    if !desired.exists() {
        return Resolution::Use(desired);
    }
    match policy {
        OnExists::Rename => Resolution::Use(unique(&desired)),
        OnExists::Overwrite => Resolution::Use(desired),
        OnExists::Skip => Resolution::Skip,
        OnExists::Ask => Resolution::Ask(desired),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Container;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stiva-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_name_follows_the_source_and_the_container() {
        let mut s = Settings { dest_dir: Some(PathBuf::from("/out")), ..Default::default() };
        assert_eq!(s.output_path(Path::new("/in/Film.avi")), PathBuf::from("/out/Film.mp4"));
        s.encode.container = Container::Mkv;
        s.dest_mode = DestMode::NextToSource;
        assert_eq!(s.output_path(Path::new("/in/Film.avi")), PathBuf::from("/in/Film.mkv"));
    }

    #[test]
    fn existing_files_follow_the_choice() {
        let d = scratch("exists");
        let src = d.join("src.avi");
        let out = d.join("a.mp4");
        std::fs::write(&out, b"x").unwrap();
        assert_eq!(resolve(OnExists::Rename, out.clone(), &src), Resolution::Use(d.join("a (1).mp4")));
        std::fs::write(d.join("a (1).mp4"), b"x").unwrap();
        assert_eq!(resolve(OnExists::Rename, out.clone(), &src), Resolution::Use(d.join("a (2).mp4")));
        assert_eq!(resolve(OnExists::Overwrite, out.clone(), &src), Resolution::Use(out.clone()));
        assert_eq!(resolve(OnExists::Skip, out.clone(), &src), Resolution::Skip);
        assert_eq!(resolve(OnExists::Ask, out.clone(), &src), Resolution::Ask(out.clone()));
        assert_eq!(resolve(OnExists::Skip, d.join("free.mp4"), &src), Resolution::Use(d.join("free.mp4")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_source_is_never_overwritten() {
        let d = scratch("source");
        let src = d.join("movie.mp4");
        std::fs::write(&src, b"original").unwrap();
        for policy in OnExists::ALL {
            match resolve(policy, src.clone(), &src) {
                Resolution::Use(p) => assert_eq!(p, d.join("movie (1).mp4"), "{policy:?}"),
                other => panic!("{policy:?} gave {other:?}"),
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn old_and_partial_settings_files_still_load() {
        let s: Settings = serde_json::from_str(r#"{"on_exists":"Skip"}"#).unwrap();
        assert_eq!(s.on_exists, OnExists::Skip);
        assert_eq!(s.dest_mode, DestMode::Folder);
        assert!(s.window.is_none());
    }
}
