<div align="center">

<img src="assets/icon-128.png" alt="Stiva" width="96" height="96">

# Stiva

**A plain, fast video converter for Windows 11, built on ffmpeg.**

[![Release](https://img.shields.io/github/v/release/echidistanti/stiva?color=7c4dff)](https://github.com/echidistanti/stiva/releases/latest)
[![CI](https://github.com/echidistanti/stiva/actions/workflows/ci.yml/badge.svg)](https://github.com/echidistanti/stiva/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform: Windows 11](https://img.shields.io/badge/platform-Windows%2011-2b2d31.svg)](#install)
[![Rust](https://img.shields.io/badge/written%20in-Rust-b7410e.svg)](https://www.rust-lang.org)

<img src="docs/editor.png" alt="Stiva with a video open: the file on the left, the settings on the right" width="760">

</div>

I wanted a converter that stays close to ffmpeg without making me write command lines: open a file, pick a preset, press Encode. Stiva is that. The layout follows [VidCoder](https://github.com/RandomEngy/VidCoder), the look follows my other app [Camallo](https://github.com/echidistanti/camallo). It is a version 0.1 project, and the [test notes](#what-i-have-tested) say plainly what I have and have not tried.

## Features

- **Formats in and out.** Reads anything ffmpeg reads. Writes MP4, MKV, WebM, MP3, M4A, Opus and FLAC.
- **Video encoders.** x264, x265, SVT-AV1 and VP9 in software; AMD AMF, NVIDIA NVENC and Intel QSV in hardware (H.264, HEVC, AV1). 8-bit and 10-bit. Quality, bitrate or target file size, with two passes. SVT-AV1 has tune and film grain; every encoder, hardware included, takes extra ffmpeg options. Stiva tests each encoder on the machine and shows only those that really start.
- **Audio.** AAC, MP3, Opus, Vorbis, AC-3, FLAC, copy or none; mono, stereo or 5.1; choice of tracks.
- **Picture.** Size limit, crop (auto or manual), rotate, flip, deinterlace, detelecine, denoise, sharpen, grayscale and a free ffmpeg filter field.
- **Subtitles, chapters, time range.** Keep the tracks you want, chapters travel in MKV, encode only a stretch of the file.
- **Presets.** Eleven built in, your own saved as small JSON files you can export and import.
- **Queue.** Add files and leave. Pause, stop, run again. It survives closing the program.
- **When done.** Do nothing, open the folder, close Stiva, sleep or shut down.
- **Windows integration.** Notification at the end, progress on the taskbar button, drag and drop, per-user installer.

<table>
<tr>
<td><img src="docs/queue.png" alt="The queue with three files waiting" width="420"></td>
<td><img src="docs/encoding.png" alt="The first of three files being encoded" width="420"></td>
</tr>
</table>

<sub>Screenshots are rendered on Linux, where I build the program, with made-up file names. The Windows window is identical.</sub>

## Install

Download from the [latest release](https://github.com/echidistanti/stiva/releases/latest):

- **`stiva-<version>-windows-x64-setup.exe`**: installs for your user only, no administrator rights. Start menu entry, optional desktop shortcut. On uninstall it asks whether to remove Stiva's data too; it never touches your videos.
- **`stiva-<version>-windows-x64.7z`**: portable, a single `stiva.exe`.

On first start Stiva downloads ffmpeg from [gyan.dev](https://www.gyan.dev/ffmpeg/builds/), verifies its SHA-256 and unpacks it. To use your own ffmpeg, choose *Options → ffmpeg → From the folder you chose*.

**Requirements:** Windows 11, 64-bit, 900 × 600 window or larger, a few hundred MB for ffmpeg. Hardware encoding needs a card and driver that support it. Windows 10 may work; I have not checked.

Stiva is not code signed. See [Troubleshooting](#troubleshooting) if Windows or your antivirus complains.

## Using it

1. Open a file: *Open file*, `Ctrl+O`, drag and drop, or `stiva.exe file.mp4` from the command line.
2. Pick a preset and adjust the tabs (*Container, Sizing, Filters, Video, Audio*). Every option explains itself in a line.
3. Check *Save to* (default `Videos\Stiva`, or next to the source).
4. *Encode*, or *Add to queue* and open the next file. Several files at once go straight to the queue.

If the output name is taken, Stiva adds a number; *Options* can overwrite, skip or ask instead. Shortcuts: `Ctrl+O` open, `Ctrl+Enter` encode, `Space` pause and resume.

**Presets:** Web 1080p, Web 720p, Mobile 480p, Storage 1080p, Storage 720p, Archive H.265 10-bit, Small AV1 10-bit, WebM VP9 1080p, Music MP3 192, Music AAC 192, Lossless FLAC. *Save as…* keeps your own; *More* exports, imports or deletes them.

**When done:** sleep and shut down wait 15 and 60 seconds with *Cancel* and *Now*, run only if something was actually finished, and never force other programs. The choice is not remembered between runs.

## Data and privacy

Everything lives in `%LOCALAPPDATA%\Stiva`: `settings.json`, `presets\`, `queue.json`, `bin\` (ffmpeg), `logs\` (one per encode, plus `encoders.txt`). One registry key, `HKCU\Software\Classes\AppUserModelId\Stiva`, gives the notification its name and icon.

No accounts, no telemetry, no update check. The only network request is the ffmpeg download. Stiva starts ffmpeg directly (never through a shell) and no other program except `shutdown.exe` for *Shut down*.

## What I have tested

I use Stiva on my own Windows 11 computer (AMD Ryzen, Radeon graphics) and have tried there: the first start and ffmpeg download, presets, the queue, pause and stop, 8-bit and 10-bit with x264, x265, SVT-AV1 and VP9, filters, bitrate, target size, two passes, subtitles, the notification, taskbar progress, every *When done* choice, the installer, and the **AMD AMF** encoders.

**Not tried on real hardware: NVIDIA NVENC, Intel QSV and hardware AV1.** Stiva offers them only when ffmpeg can start them, but I have not looked at a file they made. Reports from such machines are the most useful thing you can send me.

About 90 automated tests run on Linux, including real ffmpeg encodes that check the command lines produce files with the right streams, size and length.

## Troubleshooting

- **SmartScreen or Defender warns, or removes the exe.** Stiva is new and unsigned, and it downloads and unpacks a program, which heuristics look at. SmartScreen: *More info → Run anyway*. Defender: *Protection history → restore → Allow on device*, then report a false positive at [microsoft.com/wdsi/filesubmission](https://www.microsoft.com/en-us/wdsi/filesubmission). The source is here and `cargo build --release` produces the same program.
- **ffmpeg will not download.** Check proxy or antivirus and retry, or point Stiva to your own ffmpeg folder (it needs `ffmpeg.exe` and `ffprobe.exe`).
- **A hardware encoder is missing, or "10-bit did not pass its test".** The driver or card does not offer it; `logs\encoders.txt` shows what each test said.
- **An encode failed.** The job's *Log* panel starts with the exact ffmpeg command and ends with what ffmpeg objected to. *Run again* retries it.

## Building from source

Rust 1.85 or newer.

```sh
cargo run                                              # debug build, Windows or Linux
STIVA_FFDIR=/usr/bin cargo test                        # tests; the folder must hold ffmpeg and ffprobe
cargo build --release                                  # on Windows (MSVC), as the releases are built
cargo build --release --target x86_64-pc-windows-gnu   # a Windows exe from Linux (mingw-w64)
```

Without ffmpeg, the tests that encode skip themselves. Linux runs the program with the system ffmpeg, for development only.

**Releases are built by GitHub.** Publishing a release with a new tag starts `release.yml`, which builds `stiva.exe` on a Windows runner, attaches the `.7z`, then `installer.yml` builds the setup with [Inno Setup](https://jrsoftware.org/isinfo.php) and attaches it. `ci.yml` builds on every push and checks the formatting.

## Project layout

| Path | Contents |
| --- | --- |
| `src/app.rs`, `view.rs`, `ui.rs` | state and queue runner, screens, widgets and colours (egui) |
| `src/encode.rs`, `model.rs`, `filters.rs` | the ffmpeg command line, the settings it comes from, the filter chain |
| `src/caps.rs`, `probe.rs` | which encoders really start; reading files with ffprobe |
| `src/presets.rs`, `queue.rs`, `settings.rs` | presets, queue and settings, as JSON |
| `src/after.rs`, `platform.rs` | *When done*; Windows notification, taskbar, sleep and shutdown |
| `src/deps.rs`, `strings.rs` | ffmpeg download and check; every user-visible text |
| `installer/`, `.github/workflows/` | the Inno Setup script; CI, release and installer workflows |

## Contributing

Bug reports are welcome. Please include the Stiva and Windows versions, the graphics card and driver, what you did and expected, and the job's log (*Log* panel, or `%LOCALAPPDATA%\Stiva\logs`; `encoders.txt` for a missing encoder), with anything private removed. For a pull request, keep it small and make sure `cargo fmt` and the tests pass, user-visible texts live in `src/strings.rs`, and Windows-only code sits behind `#[cfg(windows)]` with a stub elsewhere.

## Known limits

- Not code signed.
- English interface only (the installer also speaks Italian).
- Queue rows cannot be selected, so there is no `Delete` key; each row has its own remove button.
- Windows 11 and AMD AMF are the only system and hardware encoder I have tried.

## Credits

[ffmpeg](https://ffmpeg.org) does all the encoding; it is downloaded, not bundled, and has its own licence (GPL v3 for the full build used). Built with [egui](https://github.com/emilk/egui), [rfd](https://github.com/PolyMeilex/rfd), [ureq](https://github.com/algesten/ureq), [serde](https://serde.rs), [sevenz-rust2](https://github.com/hasenbanck/sevenz-rust2), [windows-rs](https://github.com/microsoft/windows-rs) and [Inno Setup](https://jrsoftware.org/isinfo.php). The layout is inspired by VidCoder; no code from it is used.

## Licence

MIT. See [LICENSE](LICENSE).
