//! Every piece of text the window shows lives here, so a translation touches one file.
//! Text that depends on values (numbers, names) is built where it is shown, from these pieces.

pub const APP_NAME: &str = "Stiva";
pub const MINIMIZE: &str = "Minimize";
pub const MAXIMIZE: &str = "Maximize";
pub const RESTORE: &str = "Restore";

// Top bar
pub const OPEN_FILE: &str = "Open file";
pub const ENCODE: &str = "Encode";
pub const STOP: &str = "Stop";
pub const OPTIONS: &str = "Options";

// First run
pub const SETUP_TITLE: &str = "Setting things up";
pub const SETUP_BODY: &str = "Stiva converts video with ffmpeg. It is downloaded once, about 30 MB, and checked before it is used.";
pub const SETUP_FAILED: &str = "ffmpeg is not ready";
pub const RETRY: &str = "Try again";
pub const CHOOSE_FOLDER: &str = "Choose a folder with ffmpeg";

// Source
pub const DROP_TITLE: &str = "Drop a video here";
pub const DROP_BODY: &str = "or press Open file. Any video or audio file that ffmpeg can read works.";
pub const DROP_RELEASE: &str = "Release to open it";
pub const READING: &str = "Reading";
pub const CLOSE_SOURCE: &str = "Close this file";
pub const AUDIO_TRACKS: &str = "Audio tracks";
pub const NO_AUDIO: &str = "This file has no audio.";
pub const SUBTITLES_NOT_YET: &str = "Subtitles in the file are not carried over yet.";
pub const RANGE_LABEL: &str = "Encode from";
pub const RANGE_TO: &str = "to";
pub const RANGE_HINT: &str = "Leave both empty for the whole file. Write seconds (90) or 1:30 or 1:02:03.";
pub const RANGE_BAD_START: &str = "The start time is not valid. Use 90, 1:30 or 1:02:03.";
pub const RANGE_BAD_END: &str = "The end time is not valid. Use 90, 1:30 or 1:02:03.";

// Settings tabs
pub const TAB_CONTAINER: &str = "Container";
pub const TAB_VIDEO: &str = "Video";
pub const TAB_AUDIO: &str = "Audio";
pub const FORMAT: &str = "Format";
pub const FORMAT_NOTE_MP4: &str = "Plays nearly everywhere. The index goes at the start, so it also streams.";
pub const FORMAT_NOTE_MKV: &str = "Holds any codec, several audio tracks and chapters.";
pub const FORMAT_NOTE_WEBM: &str = "For the web: VP9 or AV1 video with Opus or Vorbis audio.";
pub const FORMAT_NOTE_AUDIO: &str = "Only the sound is kept.";
pub const VIDEO_CODEC: &str = "Codec";
pub const VIDEO_QUALITY: &str = "Quality (CRF)";
pub const VIDEO_QUALITY_HINT_HW: &str = "Lower is better and bigger. The scale is the chip's own, not the same as x264.";
pub const HARDWARE_NOTE: &str =
    "Runs on the graphics chip: much faster, but the file comes out bigger than with x264 or x265 at the same look.";
pub const VIDEO_QUALITY_HINT: &str = "Lower is better and bigger. Around 18 looks like the source, 28 is small.";
pub const VIDEO_SPEED: &str = "Speed";
pub const VIDEO_SPEED_HINT: &str = "Slower takes longer and makes a smaller file at the same quality.";
pub const NO_VIDEO_HERE: &str = "This format has no video.";
pub const NOT_IN_FFMPEG: &str = "not in this ffmpeg";
pub const AUDIO_CODEC: &str = "Codec";
pub const AUDIO_BITRATE: &str = "Bitrate";
pub const AUDIO_MIXDOWN: &str = "Channels";
pub const AUDIO_MIXDOWN_HINT: &str = "A track is only ever mixed down, never stretched to more channels.";
pub const AUDIO_GAIN: &str = "Volume";
pub const AUDIO_COPY_NOTE: &str = "The sound is copied as it is. The container has to accept its codec, or ffmpeg stops with an error.";
pub const AUDIO_NONE_NOTE: &str = "The file will have no sound.";
pub const CHANGED_FOR_FORMAT: &str = "The previous codecs do not fit this format, so they were changed.";

// Destination
pub const SAVE_TO: &str = "Save to";
pub const NEXT_TO_SOURCE: &str = "Next to the source";
pub const CHANGE: &str = "Change";
pub const OPEN_FOLDER: &str = "Open folder";

// Options card
pub const OPT_FOLDER: &str = "Folder";
pub const OPT_PLACE: &str = "Place";
pub const OPT_PLACE_NEXT: &str = "Save each file next to its source";
pub const OPT_EXISTS: &str = "If the file exists";
pub const OPT_FFMPEG: &str = "ffmpeg";
pub const OPT_FFMPEG_CUSTOM: &str = "From the folder you chose";
pub const OPT_FFMPEG_OWN: &str = "Downloaded by Stiva";
pub const OPT_USE_DOWNLOADED: &str = "Use the downloaded one";
pub const OPT_UPDATE: &str = "Download again";
pub const OPT_UPDATE_TIP: &str = "Replaces the downloaded ffmpeg with the current release";

// Encoding strip
pub const ENCODING: &str = "Encoding";
pub const LOG: &str = "Log";
pub const DONE: &str = "Done";
pub const FAILED: &str = "It did not work";
pub const STOPPED: &str = "Stopped. The unfinished file was removed.";
pub const SKIPPED: &str = "Skipped, the file is already there:";
pub const OPEN: &str = "Open";
pub const SHOW: &str = "Show";
pub const DISMISS: &str = "Dismiss";

// Existing file
pub const ASK_TITLE: &str = "This file already exists";
pub const ASK_OVERWRITE: &str = "Overwrite";
pub const ASK_RENAME: &str = "Add a number";
pub const ASK_CANCEL: &str = "Cancel";

// Closing while an encode runs
pub const QUIT_TITLE: &str = "An encode is still running";
pub const QUIT_BODY: &str = "If you quit now, it stops and the unfinished file is deleted.";
pub const QUIT_STOP: &str = "Stop and quit";
pub const QUIT_KEEP: &str = "Keep encoding";
