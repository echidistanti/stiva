//! Finding ffmpeg and ffprobe, and downloading them on the first run.
//! They are never shipped with the program: the app fetches a build of its own, checks its SHA-256 and keeps
//! it under the user's data folder. On Windows that is the full build from gyan.dev; on Linux the GPL build
//! of the newest release from the FFmpeg-Builds project. A folder chosen by the user wins.

use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

/// The "full" build, not "essentials": only the full one has SVT-AV1. It comes as .7z only.
#[cfg_attr(not(windows), allow(dead_code))]
const FF_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-full.7z";
#[cfg_attr(not(windows), allow(dead_code))]
const FF_NAME: &str = "ffmpeg-release-full.7z";

/// Where the Linux builds are kept. The `latest` release holds the current build of every branch, with a
/// `checksums.sha256` next to them.
#[cfg_attr(windows, allow(dead_code))]
const LINUX_BASE: &str = "https://github.com/FFmpeg-Builds/FFmpeg-Builds/releases/download/latest";

#[cfg(windows)]
const EXE: &str = ".exe";
#[cfg(not(windows))]
const EXE: &str = "";

/// `%LOCALAPPDATA%\Stiva` on Windows, `~/.local/share/stiva` on Linux, where folders are lower case by habit.
pub fn data_dir() -> PathBuf {
    let name = if cfg!(windows) { "Stiva" } else { "stiva" };
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join(name)
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

/// Looks, in this order, in the folder the user chose, in the app's own folder (the copy Stiva downloaded)
/// and in the folder named by STIVA_FFDIR. The ffmpeg the system has is not in the list: Stiva asks before it
/// takes that one, or before it downloads another (`system`).
pub fn locate(custom: Option<&Path>) -> Option<Tools> {
    if let Some(t) = custom.and_then(in_dir) {
        return Some(t);
    }
    if let Some(t) = downloaded() {
        return Some(t);
    }
    std::env::var_os("STIVA_FFDIR").map(PathBuf::from).and_then(|d| in_dir(&d))
}

/// The copy Stiva downloaded, if it is there.
pub fn downloaded() -> Option<Tools> {
    in_dir(&bin_dir())
}

/// The ffmpeg and ffprobe the system has: both in one folder of the PATH. On Windows nothing is looked for there.
#[cfg(not(windows))]
pub fn system() -> Option<Tools> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|d| in_dir(&d))
}

/// The first three words of the line `ffmpeg -version` starts with: "ffmpeg version 7.1.1-essentials_build-www.gyan.dev
/// Copyright (c) 2000-2025 ..." becomes "ffmpeg version 7.1.1-essentials_build-www.gyan.dev".
pub fn version_words(line: &str) -> String {
    line.split_whitespace().take(3).collect::<Vec<_>>().join(" ")
}

/// What `ffmpeg -version` says, in a few words; empty when ffmpeg does not start.
#[cfg_attr(windows, allow(dead_code))]
pub fn version(ffmpeg: &Path) -> String {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-version"]);
    quiet(&mut cmd);
    cmd.output().map(|o| version_words(String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or(""))).unwrap_or_default()
}

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

/// Takes `ffmpeg.exe` and `ffprobe.exe` out of a gyan.dev archive, whatever the version folder is called.
#[cfg_attr(not(windows), allow(dead_code))]
fn unpack(archive: Vec<u8>, bin: &Path) -> Result<(), String> {
    use sevenz_rust2::{ArchiveReader, Error, Password};
    let mut ar = ArchiveReader::new(Cursor::new(archive), Password::empty()).map_err(|e| e.to_string())?;
    ar.for_each_entries(|entry, reader| {
        for tool in ["ffmpeg.exe", "ffprobe.exe"] {
            if entry.name().ends_with(&format!("/bin/{tool}")) {
                write_atomic(&bin.join(tool), |out| std::io::copy(reader, out).map(|_| ())).map_err(|e| Error::Other(e.into()))?;
                break;
            }
        }
        Ok(true)
    })
    .map_err(|e| e.to_string())
}

/// Which Linux build to take from the list of checksums: the GPL build (static, with SVT-AV1, x265 and the
/// hardware encoders) of the newest release branch, `ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz` and the like.
/// The branches come and go, so the name is read from the list; the build of the development branch is
/// the fallback when no release branch is there. Returns the file name and its SHA-256.
#[cfg_attr(windows, allow(dead_code))]
fn pick_linux_build(sums: &str) -> Option<(String, String)> {
    let version = |name: &str| -> Option<(u32, u32)> {
        let rest = name.strip_prefix("ffmpeg-n")?.strip_suffix(".tar.xz")?;
        let (branch, tail) = rest.split_once("-latest-linux64-gpl-")?;
        if branch != tail {
            return None;
        }
        let (major, minor) = branch.split_once('.')?;
        Some((major.parse().ok()?, minor.parse().ok()?))
    };
    let lines: Vec<(&str, &str)> = sums
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            Some((w.next()?, w.next()?.trim_start_matches('*')))
        })
        .collect();
    let best = lines.iter().filter_map(|(h, n)| version(n).map(|v| (v, *h, *n))).max_by_key(|(v, _, _)| *v);
    let (hash, name) = match best {
        Some((_, h, n)) => (h, n),
        None => lines.iter().copied().find(|(_, n)| *n == "ffmpeg-master-latest-linux64-gpl.tar.xz")?,
    };
    Some((name.to_string(), hash.to_lowercase()))
}

/// Takes `ffmpeg` and `ffprobe` out of a .tar.xz, whatever the folder inside it is called, and makes them
/// runnable. Stops reading as soon as it has both.
#[cfg_attr(windows, allow(dead_code))]
fn unpack_tar_xz(archive: Vec<u8>, bin: &Path) -> Result<(), String> {
    const TOOLS: [&str; 2] = ["ffmpeg", "ffprobe"];
    let mut tar = tar::Archive::new(lzma_rust2::XzReader::new(Cursor::new(archive), true));
    let mut found = [false; 2];
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let in_bin = path.parent().is_some_and(|d| d.ends_with("bin"));
        let Some(i) = TOOLS.iter().position(|t| in_bin && path.file_name().is_some_and(|f| f == *t)) else { continue };
        write_atomic(&bin.join(TOOLS[i]), |out| {
            std::io::copy(&mut entry, out)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                out.set_permissions(std::fs::Permissions::from_mode(0o755))?;
            }
            Ok(())
        })?;
        found[i] = true;
        if found == [true, true] {
            return Ok(());
        }
    }
    Err("ffmpeg or ffprobe was not in the archive".into())
}

/// True for the old "essentials" build, which has no SVT-AV1. Earlier versions of Stiva downloaded that one,
/// so a copy found in the app's folder is replaced by the full build.
pub fn is_essentials(tools: &Tools) -> bool {
    let mut cmd = std::process::Command::new(&tools.ffmpeg);
    cmd.arg("-version");
    quiet(&mut cmd);
    cmd.output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).lines().next().is_some_and(|l| l.contains("essentials_build")))
}

/// Downloads ffmpeg and ffprobe into the app's folder, replacing any older copy.
/// `status` gets a message and the fraction done, when known.
#[cfg(windows)]
pub fn download(status: &mut dyn FnMut(&str, Option<f32>)) -> Result<Tools, String> {
    let bin = bin_dir();
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    let msg = "Downloading ffmpeg (about 75 MB)...";
    status(msg, Some(0.0));
    let archive = fetch(FF_URL, &mut |f| status(msg, f))?;
    status("Checking the download...", None);
    let sums = String::from_utf8_lossy(&fetch(&format!("{FF_URL}.sha256"), &mut |_| {})?).into_owned();
    let want = expected_hash(&sums, FF_NAME).ok_or("checksum not found")?;
    if sha256_hex(&archive) != want {
        return Err("The download does not match its checksum, so it was discarded. Try again.".into());
    }
    status("Unpacking (this takes a few seconds)...", None);
    unpack(archive, &bin)?;
    in_dir(&bin).ok_or_else(|| "ffmpeg.exe or ffprobe.exe was not in the archive".to_string())
}

#[cfg(not(windows))]
pub fn download(status: &mut dyn FnMut(&str, Option<f32>)) -> Result<Tools, String> {
    let bin = bin_dir();
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    status("Looking for the current ffmpeg build...", None);
    let sums = String::from_utf8_lossy(&fetch(&format!("{LINUX_BASE}/checksums.sha256"), &mut |_| {})?).into_owned();
    let (name, want) = pick_linux_build(&sums).ok_or("There is no Linux build in the list of ffmpeg builds")?;
    let msg = "Downloading ffmpeg (about 150 MB)...";
    status(msg, Some(0.0));
    let archive = fetch(&format!("{LINUX_BASE}/{name}"), &mut |f| status(msg, f))?;
    status("Checking the download...", None);
    if sha256_hex(&archive) != want {
        return Err("The download does not match its checksum, so it was discarded. Try again.".into());
    }
    status("Unpacking (this takes about half a minute)...", None);
    unpack_tar_xz(archive, &bin)?;
    in_dir(&bin).ok_or_else(|| "ffmpeg or ffprobe was not in the archive".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Runs only when STIVA_TEST_7Z names a real gyan.dev archive (about 75 MB), so it stays out of ordinary runs.
    #[test]
    fn the_real_gyan_archive_gives_both_programs() {
        let Some(file) = std::env::var_os("STIVA_TEST_7Z") else { return };
        let dir = std::env::temp_dir().join(format!("stiva-7z-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        unpack(std::fs::read(file).unwrap(), &dir).unwrap();
        for tool in ["ffmpeg.exe", "ffprobe.exe"] {
            let data = std::fs::read(dir.join(tool)).unwrap();
            assert!(data.len() > 10_000_000, "{tool} is too small: {}", data.len());
            assert_eq!(&data[..2], b"MZ", "{tool} is not a Windows program");
        }
        let ffmpeg = std::fs::read(dir.join("ffmpeg.exe")).unwrap();
        let has = |name: &[u8]| ffmpeg.windows(name.len()).any(|w| w == name);
        assert!(has(b"libsvtav1") && has(b"libx265") && has(b"h264_amf"), "the build lacks an encoder Stiva offers");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2, "only the two programs are written");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The first lines of the real list at FFmpeg-Builds, with a branch that is older and one that is only
    /// the development branch among them.
    const LIST: &str = "\
405ea9c76429c4b4ae777c6bc6748658b789e3bd6ff2541f52d54fd1cd2e605b  ffmpeg-master-latest-linux64-gpl-shared.tar.xz
231bf775a8800a6d148777b4be81a6f7e21b095d8f7e76b4fe02585806682cb6  ffmpeg-master-latest-linux64-gpl.tar.xz
83cee2d58418745840e991d6521e331f94c79cba138f5cf3dc623466958142ca  ffmpeg-master-latest-linux64-lgpl.tar.xz
6941b3ca1e60af55a5abd48bab758c29bd71d77389fea066e8ffd9a7bf37456f  ffmpeg-n8.1-latest-linux64-gpl-8.1.tar.xz
feb78e06d2d4d8e6e2944e67c6f5ec9d7cbe924559b77f3532ec1ffac13437c4  ffmpeg-n8.1-latest-linux64-gpl-shared-8.1.tar.xz
952E08967D724A632F9BD3D0CD765F771985831934100EACF92650713F61D54F  ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz
f5a03389ec684b51fddb9f9c66e3e7e25437a3d013007a01af45eefcb16d9888  ffmpeg-n9.0-latest-linux64-gpl-shared-9.0.tar.xz
af5e4d9df419a247aa7d9a666bc5a4a10263d442998f5da235423ff8e687bbd5  ffmpeg-n9.0-latest-linux64-lgpl-9.0.tar.xz
1111111111111111111111111111111111111111111111111111111111111111  ffmpeg-n10.0-latest-win64-gpl-10.0.zip
";

    #[test]
    fn the_newest_gpl_release_branch_is_picked_from_the_list() {
        let (name, hash) = pick_linux_build(LIST).unwrap();
        assert_eq!(name, "ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz", "not shared, not lgpl, not the older branch");
        assert_eq!(hash, "952e08967d724a632f9bd3d0cd765f771985831934100eacf92650713f61d54f", "lower case");
    }

    #[test]
    fn branch_numbers_compare_as_numbers_not_as_text() {
        let sums = "aa  ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz\nbb  ffmpeg-n10.1-latest-linux64-gpl-10.1.tar.xz\ncc  ffmpeg-n10.0-latest-linux64-gpl-10.0.tar.xz\n";
        assert_eq!(pick_linux_build(sums).unwrap().0, "ffmpeg-n10.1-latest-linux64-gpl-10.1.tar.xz");
    }

    #[test]
    fn without_a_release_branch_the_development_build_is_used() {
        let sums = "aa  ffmpeg-master-latest-linux64-gpl.tar.xz\nbb  ffmpeg-master-latest-win64-gpl.zip\n";
        assert_eq!(pick_linux_build(sums), Some(("ffmpeg-master-latest-linux64-gpl.tar.xz".into(), "aa".into())));
        assert_eq!(pick_linux_build("bb  ffmpeg-master-latest-win64-gpl.zip\n"), None);
        assert_eq!(pick_linux_build(""), None);
    }

    /// A tiny archive built like the real one: the two programs in `bin/`, next to a third program, a manual
    /// and a file with the same name somewhere else.
    #[test]
    fn the_two_programs_come_out_of_a_tar_xz_and_can_run() {
        let dir = std::env::temp_dir().join(format!("stiva-txz-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        unpack_tar_xz(include_bytes!("../testdata/ffmpeg-like.tar.xz").to_vec(), &dir).unwrap();
        assert_eq!(std::fs::read(dir.join("ffmpeg")).unwrap(), b"not really ffmpeg\n", "the one in bin/, not the one in include/");
        assert_eq!(std::fs::read(dir.join("ffprobe")).unwrap(), b"not really ffprobe\n");
        let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        assert_eq!(names, ["ffmpeg", "ffprobe"], "nothing else is written, and no .part is left");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join("ffmpeg")).unwrap().permissions().mode() & 0o111, 0o111, "runnable");
        }
        assert!(in_dir(&dir).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_without_the_programs_is_an_error_and_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("stiva-txz-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Not an xz file at all, as a half download or an error page would be.
        assert!(unpack_tar_xz(b"<html>Not found</html>".to_vec(), &dir).is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Runs only when STIVA_TEST_TXZ names a real FFmpeg-Builds archive (about 150 MB).
    #[test]
    fn the_real_linux_archive_gives_both_programs() {
        let Some(file) = std::env::var_os("STIVA_TEST_TXZ") else { return };
        let dir = std::env::temp_dir().join(format!("stiva-txz-real-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let t = std::time::Instant::now();
        unpack_tar_xz(std::fs::read(file).unwrap(), &dir).unwrap();
        eprintln!("unpacked in {:?}", t.elapsed());
        for tool in ["ffmpeg", "ffprobe"] {
            let data = std::fs::read(dir.join(tool)).unwrap();
            assert!(data.len() > 10_000_000, "{tool} is too small: {}", data.len());
            assert_eq!(&data[..4], b"\x7fELF", "{tool} is not a Linux program");
        }
        let ffmpeg = std::fs::read(dir.join("ffmpeg")).unwrap();
        let has = |name: &[u8]| ffmpeg.windows(name.len()).any(|w| w == name);
        assert!(has(b"libsvtav1") && has(b"libx265") && has(b"h264_nvenc"), "the build lacks an encoder Stiva offers");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2, "only the two programs are written");
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
