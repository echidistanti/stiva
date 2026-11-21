//! Finding ffmpeg and ffprobe, and downloading them on the first run.
//! They are never shipped with the program: the app fetches the Windows build from gyan.dev,
//! checks its SHA-256 and keeps it under the user's data folder. A folder chosen by the user wins.

use sha2::{Digest, Sha256};
#[cfg(windows)]
use std::io::Cursor;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

#[cfg_attr(not(windows), allow(dead_code))]
const FF_ZIP: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
#[cfg_attr(not(windows), allow(dead_code))]
const FF_ZIP_NAME: &str = "ffmpeg-release-essentials.zip";

#[cfg(windows)]
const EXE: &str = ".exe";
#[cfg(not(windows))]
const EXE: &str = "";

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Stiva")
}

pub fn bin_dir() -> PathBuf {
    data_dir().join("bin")
}

/// No console window for the programs started from the windowed app.
pub fn quiet(c: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

fn in_dir(dir: &Path) -> Option<Tools> {
    let t = Tools { ffmpeg: dir.join(format!("ffmpeg{EXE}")), ffprobe: dir.join(format!("ffprobe{EXE}")) };
    (t.ffmpeg.is_file() && t.ffprobe.is_file()).then_some(t)
}

/// Looks, in this order, in the folder the user chose, in the app's own folder, in the folder named by
/// STIVA_FFDIR, and (outside Windows, where nothing is downloaded) on the PATH.
pub fn locate(custom: Option<&Path>) -> Option<Tools> {
    if let Some(t) = custom.and_then(in_dir) {
        return Some(t);
    }
    if let Some(t) = in_dir(&bin_dir()) {
        return Some(t);
    }
    if let Some(t) = std::env::var_os("STIVA_FFDIR").map(PathBuf::from).and_then(|d| in_dir(&d)) {
        return Some(t);
    }
    #[cfg(not(windows))]
    {
        let path = std::env::var_os("PATH")?;
        return std::env::split_paths(&path).find_map(|d| in_dir(&d));
    }
    #[cfg(windows)]
    None
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads a hash out of a checksum file: either a bare hash or "hash  filename" lines.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn expected_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let t: Vec<&str> = l.split_whitespace().collect();
        match t.as_slice() {
            [h] => Some(h.to_lowercase()),
            [h, f] if f.trim_start_matches('*') == name => Some(h.to_lowercase()),
            _ => None,
        }
    })
}

/// Downloads `url`, reporting the fraction done (0..1) when the server says how big the file is.
#[cfg_attr(not(windows), allow(dead_code))]
fn fetch(url: &str, progress: &mut dyn FnMut(Option<f32>)) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let total = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<u64>().ok());
    let mut reader = resp.into_body().into_with_config().limit(400 << 20).reader();
    let mut data = Vec::with_capacity(total.unwrap_or(32 << 20) as usize);
    let mut buf = vec![0u8; 64 * 1024];
    let mut last = 0.0f32;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("{url}: {e}"))?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        let frac = total.map(|t| (data.len() as f32 / t as f32).min(1.0));
        // Reporting every few percent is enough and keeps the window from redrawing for each chunk.
        if frac.is_none_or(|f| f - last >= 0.01) {
            last = frac.unwrap_or(0.0);
            progress(frac);
        }
    }
    Ok(data)
}

/// Writes `path` through a `.part` file and a rename, so a closed app or a power cut never leaves
/// half a file under the real name (the next start would trust it, because it exists).
#[cfg_attr(not(windows), allow(dead_code))]
fn write_atomic(path: &Path, fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>) -> Result<(), String> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let r: std::io::Result<()> = (|| {
        let mut f = std::fs::File::create(&part)?;
        fill(&mut f)?;
        f.sync_all()?;
        drop(f); // closed before the rename, which Windows requires
        std::fs::rename(&part, path)
    })();
    if let Err(e) = r {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(())
}

/// Downloads ffmpeg and ffprobe into the app's folder, replacing any older copy.
/// `status` gets a message and the fraction done, when known.
#[cfg(windows)]
pub fn download(status: &mut dyn FnMut(&str, Option<f32>)) -> Result<Tools, String> {
    let bin = bin_dir();
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    status("Downloading ffmpeg (about 30 MB)...", Some(0.0));
    let zip = fetch(FF_ZIP, &mut |f| status("Downloading ffmpeg (about 30 MB)...", f))?;
    status("Checking the download...", None);
    let sums = String::from_utf8_lossy(&fetch(&format!("{FF_ZIP}.sha256"), &mut |_| {})?).into_owned();
    let want = expected_hash(&sums, FF_ZIP_NAME).ok_or("checksum not found")?;
    if sha256_hex(&zip) != want {
        return Err("The download does not match its checksum, so it was discarded. Try again.".into());
    }
    status("Unpacking...", None);
    let mut ar = zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| e.to_string())?;
    for i in 0..ar.len() {
        let mut f = ar.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = f.name().map(|n| n.to_string()).ok() else {
            continue;
        };
        for tool in ["ffmpeg.exe", "ffprobe.exe"] {
            if name.ends_with(&format!("/bin/{tool}")) {
                write_atomic(&bin.join(tool), |out| std::io::copy(&mut f, out).map(|_| ()))?;
                break;
            }
        }
    }
    in_dir(&bin).ok_or_else(|| "ffmpeg.exe or ffprobe.exe was not in the archive".to_string())
}

#[cfg(not(windows))]
pub fn download(_status: &mut dyn FnMut(&str, Option<f32>)) -> Result<Tools, String> {
    // The builds on gyan.dev are for Windows. Elsewhere the program uses what is installed.
    Err("Automatic download is only available on Windows. Install ffmpeg and ffprobe, or choose the folder that has them in the options."
        .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn sha256_known_vector() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn a_failed_write_leaves_neither_the_file_nor_a_part() {
        let dir = std::env::temp_dir().join(format!("stiva-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("tool.exe");
        let e = write_atomic(&target, |f| {
            f.write_all(b"half of it")?;
            Err(std::io::Error::other("connection lost"))
        });
        assert!(e.unwrap_err().contains("connection lost"));
        assert!(!target.exists(), "a half file must never take the real name");
        assert!(!dir.join("tool.exe.part").exists());
        write_atomic(&target, |f| f.write_all(b"v1")).unwrap();
        write_atomic(&target, |f| f.write_all(b"v2")).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"v2");
        assert!(!dir.join("tool.exe.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hash_parsing() {
        assert_eq!(expected_hash("AbC123\n", "x.zip").as_deref(), Some("abc123"));
        let sums = "aaa  other.exe\nbbb *yt-dlp.exe\n";
        assert_eq!(expected_hash(sums, "yt-dlp.exe").as_deref(), Some("bbb"));
        assert_eq!(expected_hash(sums, "missing.exe"), None);
    }

    #[test]
    fn a_folder_needs_both_programs_to_count() {
        let dir = std::env::temp_dir().join(format!("stiva-tools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("ffmpeg{EXE}")), b"x").unwrap();
        assert!(in_dir(&dir).is_none(), "ffprobe is missing");
        std::fs::write(dir.join(format!("ffprobe{EXE}")), b"x").unwrap();
        assert!(in_dir(&dir).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
