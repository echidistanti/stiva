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
#[cfg(windows)]
pub const SETUP_BODY: &str = "Stiva converts video with ffmpeg. It is downloaded once, about 75 MB, and checked before it is used.";
#[cfg(not(windows))]
pub const SETUP_BODY: &str = "Stiva converts video with ffmpeg. It is downloaded once, about 150 MB, and checked before it is used.";
#[cfg(not(windows))]
pub const SETUP_FOUND_TITLE: &str = "ffmpeg is already installed";
#[cfg(not(windows))]
pub const SETUP_FOUND_BODY: &str = "Stiva can work with the copy this system has. Builds from a distribution sometimes leave encoders out; the full build that Stiva downloads has them all.";
#[cfg(not(windows))]
pub const SETUP_USE_THIS: &str = "Use this one";
#[cfg(not(windows))]
pub const SETUP_DOWNLOAD_FULL: &str = "Download the full build (about 150 MB)";
#[cfg(not(windows))]
pub const SETUP_NONE_TITLE: &str = "ffmpeg is not installed";
#[cfg(not(windows))]
pub const SETUP_NONE_BODY: &str = "Stiva needs ffmpeg and ffprobe. It can download a full build, about 150 MB, checked before it is used. Or install ffmpeg with your package manager and press Try again.";
#[cfg(not(windows))]
pub const SETUP_DOWNLOAD: &str = "Download ffmpeg (about 150 MB)";
pub const SETUP_FAILED: &str = "ffmpeg is not ready";
pub const RETRY: &str = "Try again";
pub const CHOOSE_FOLDER: &str = "Choose a folder with ffmpeg";

// Source
pub const DROP_TITLE: &str = "Drop a video here";
pub const DROP_BODY: &str = "or press Open file. Any video or audio file that ffmpeg can read works.";
// Where a file cannot be dropped on the window (native Wayland), the same card without the invitation.
pub const OPEN_TITLE: &str = "Open a video";
pub const OPEN_BODY: &str = "Press Open file or Ctrl+O. Any video or audio file that ffmpeg can read works.";
pub const DROP_RELEASE: &str = "Release to open it";
pub const READING: &str = "Reading";
pub const CLOSE_SOURCE: &str = "Close this file";
pub const AUDIO_TRACKS: &str = "Audio tracks";
pub const NO_AUDIO: &str = "This file has no audio.";
pub const RANGE_LABEL: &str = "Encode from";
pub const RANGE_TO: &str = "to";
pub const RANGE_HINT: &str = "Leave both empty for the whole file. Write seconds (90) or 1:30 or 1:02:03.";
pub const RANGE_BAD_START: &str = "The start time is not valid. Use 90, 1:30 or 1:02:03.";
pub const RANGE_BAD_END: &str = "The end time is not valid. Use 90, 1:30 or 1:02:03.";

// Settings tabs
pub const TAB_CONTAINER: &str = "Container";
pub const TAB_SIZING: &str = "Sizing";
pub const TAB_FILTERS: &str = "Filters";
pub const TAB_VIDEO: &str = "Video";
pub const TAB_AUDIO: &str = "Audio";
pub const FORMAT: &str = "Format";
pub const FORMAT_NOTE_MP4: &str = "Plays nearly everywhere. The index goes at the start, so it also streams.";
pub const FORMAT_NOTE_MKV: &str = "Holds any codec, several audio tracks and chapters.";
pub const FORMAT_NOTE_WEBM: &str = "For the web: VP9 or AV1 video with Opus or Vorbis audio.";
pub const FORMAT_NOTE_AUDIO: &str = "Only the sound is kept.";
pub const VIDEO_CODEC: &str = "Codec";
pub const VIDEO_QUALITY_HINT_HW: &str = "Lower is better and bigger. The scale is the chip's own, not the same as x264.";
pub const HARDWARE_NOTE: &str =
    "Runs on the graphics chip: much faster, but the file comes out bigger than with x264 or x265 at the same look.";
pub const VIDEO_QUALITY_HINT: &str = "Lower is better and bigger. Around 18 looks like the source, 28 is small.";
pub const VIDEO_SPEED: &str = "Speed";
pub const VIDEO_SPEED_HINT: &str = "Slower takes longer and makes a smaller file at the same quality.";
pub const SVT_TUNE: &str = "Tune";
pub const SVT_TUNE_HINT: &str = "What the encoder aims at. Visual quality is for watching; PSNR and SSIM are for measuring.";
pub const FILM_GRAIN: &str = "Film grain";
pub const FILM_GRAIN_OFF: &str = "Off";
pub const FILM_GRAIN_HINT: &str = "Leaves the grain out of the file and makes it again when playing: much smaller for grainy film. 4 to 10 suits most; the player must support AV1 film grain.";
pub const GRAIN_DENOISE: &str = "Take the grain out before encoding";
pub const EXTRA_VIDEO: &str = "Extra options";
pub const EXTRA_VIDEO_HINT: &str =
    "For ffmpeg, as on its command line, after the options above (later ones win). Wrong ones make the encode fail; the log says why.";
pub const EXTRA_VIDEO_EXAMPLE: &str = "e.g. -svtav1-params fast-decode=1";
pub const AV1_HW_NOTE: &str = "Tune and film grain belong to the software encoder (SVT-AV1). A graphics chip keeps its own settings: pass any your card has in Extra options.";
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
#[cfg(not(windows))]
pub const OPT_FFMPEG_SYSTEM: &str = "Installed on this system";
#[cfg(not(windows))]
pub const OPT_USE_SYSTEM: &str = "Use the one installed on the system";
#[cfg(not(windows))]
pub const OPT_DOWNLOAD: &str = "Download ffmpeg";
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
pub const ASK_CANCEL: &str = "Skip this file";

// Closing while an encode runs
pub const QUIT_TITLE: &str = "An encode is still running";
pub const QUIT_BODY: &str = "If you quit now, it stops and the unfinished file is deleted.";
pub const QUIT_STOP: &str = "Stop and quit";
pub const QUIT_KEEP: &str = "Keep encoding";

// Bit depth
pub const VIDEO_BITS: &str = "Bit depth";
pub const BITS_8: &str = "8-bit";
pub const BITS_10: &str = "10-bit";
pub const BITS_HINT: &str = "10-bit keeps gradients smooth and costs a little more space. Some TVs and phones cannot play it.";
pub const BITS_ONLY_8: &str = "8-bit only";
pub const BITS_ONLY_8_HINT: &str = "This encoder does not do 10-bit.";
pub const BITS_FAILED_HINT: &str = "10-bit did not pass its test on this PC. encoders.txt in the logs folder says why.";

// Sizing
pub const SIZE_LABEL: &str = "Largest size";
pub const SIZE_SOURCE: &str = "As the source";
pub const SIZE_CUSTOM: &str = "Custom";
pub const SIZE_HINT: &str = "Bigger pictures are scaled down to fit. Smaller ones are left alone, never enlarged.";
pub const SIZE_FIT: &str = "Fit inside";
pub const SIZE_FIT_HINT: &str = "Width and height of a landscape picture. A portrait one is fitted the same way, turned.";

// Queue
pub const ADD_TO_QUEUE: &str = "Add to queue";
pub const PAUSE: &str = "Pause";
pub const RESUME: &str = "Resume";
pub const PAUSED: &str = "Paused";
pub const TO_QUEUE: &str = "Queue";
pub const TO_EDITOR: &str = "Back to the file";
pub const TAB_QUEUED: &str = "Queued";
pub const TAB_COMPLETED: &str = "Completed";
pub const QUEUE_EMPTY: &str = "Nothing is waiting. Open a file, set it up and press Add to queue, or drop several files on the window.";
pub const QUEUE_EMPTY_NO_DROP: &str = "Nothing is waiting. Open a file, set it up and press Add to queue, or choose several files at once.";
pub const COMPLETED_EMPTY: &str = "Nothing has finished yet.";
pub const CLEAR_COMPLETED: &str = "Clear completed";
pub const WAITING: &str = "Waiting";
pub const ADDED: &str = "Added to the queue";
pub const VIEW_QUEUE: &str = "View queue";
pub const REMOVE: &str = "Take off the queue";
pub const RUN_AGAIN: &str = "Run again";
pub const SKIPPED_EXISTS: &str = "Skipped, the file already exists";
pub const MORE_WAITING: &str = "more waiting";

// Presets
pub const PRESET: &str = "Preset";
pub const PRESET_CUSTOM: &str = "Custom";
pub const PRESET_BUILTIN: &str = "Built in";
pub const PRESET_YOURS: &str = "Yours";
pub const PRESET_MODIFIED: &str = "Modified";
pub const PRESET_SAVE: &str = "Save";
pub const PRESET_SAVE_AS: &str = "Save as...";
pub const PRESET_MORE: &str = "More";
pub const PRESET_EXPORT: &str = "Export to a file...";
pub const PRESET_IMPORT: &str = "Import from a file...";
pub const PRESET_DELETE: &str = "Delete this preset";
pub const PRESET_SAVE_TITLE: &str = "Save the settings as a preset";
pub const PRESET_NAME_HINT: &str = "Name of the preset";
pub const PRESET_REPLACES: &str = "This replaces your preset of the same name.";
pub const PRESET_DELETE_TITLE: &str = "Delete this preset?";
pub const PRESET_DELETE_YES: &str = "Delete";
pub const PRESET_DELETE_NO: &str = "Keep it";
pub const CANCEL: &str = "Cancel";
pub const STOPPED_SHORT: &str = "Stopped";

// Rate
pub const RATE_MODE: &str = "Rate control";
pub const RATE_QUALITY: &str = "Quality";
pub const RATE_BITRATE: &str = "Video bitrate";
pub const RATE_SIZE: &str = "File size";
pub const RATE_BITRATE_HINT: &str = "The average the video aims at. Higher is better and bigger.";
pub const RATE_SIZE_HINT: &str =
    "The video bitrate is worked out from the length of the file and the audio, so the result comes close to this size.";
pub const RATE_SIZE_NEEDS_FILE: &str = "Open a file to see the bitrate this gives.";
pub const RATE_NEEDS_CHIP_TEST: &str = "Only constant quality works with this encoder on this PC.";
pub const TWO_PASS: &str = "Two passes";
pub const TWO_PASS_HINT: &str = "Reads the file twice: slower, and the bits go where the picture needs them.";

// Filters
pub const FILTER_CROP: &str = "Crop";
pub const FILTER_CROP_NONE: &str = "No crop";
pub const FILTER_CROP_AUTO: &str = "Automatic (cut the black bars)";
pub const FILTER_CROP_MANUAL: &str = "By hand";
pub const FILTER_CROP_AUTO_HINT: &str =
    "When the encode starts, a few seconds are read at several points of the file to find black bars. What is not black everywhere stays.";
pub const FILTER_CROP_MANUAL_HINT: &str = "Pixels cut from each side of the original picture.";
pub const FILTER_LEFT: &str = "Left";
pub const FILTER_TOP: &str = "Top";
pub const FILTER_RIGHT: &str = "Right";
pub const FILTER_BOTTOM: &str = "Bottom";
pub const FILTER_ROTATE: &str = "Rotate";
pub const FILTER_FLIP_H: &str = "Flip left to right";
pub const FILTER_FLIP_V: &str = "Flip top to bottom";
pub const FILTER_DEINTERLACE: &str = "Deinterlace";
pub const FILTER_DEINTERLACE_HINT: &str = "For old TV recordings and DVDs, where moving edges show comb-like lines.";
pub const FILTER_DETELECINE: &str = "Detelecine";
pub const FILTER_DETELECINE_ON: &str = "Undo the 3:2 pulldown";
pub const FILTER_DETELECINE_HINT: &str = "For film that was shown on TV. It drops one picture in five, so use it only on such files.";
pub const FILTER_DENOISE: &str = "Denoise";
pub const FILTER_DENOISE_HINT: &str = "Smooths grain and noise. Strong also softens detail.";
pub const FILTER_SHARPEN: &str = "Sharpen";
pub const FILTER_GRAYSCALE: &str = "Black and white";
pub const FILTER_CUSTOM: &str = "Your own filter";
pub const FILTER_CUSTOM_HINT: &str = "ffmpeg video filters, separated by commas, added after the others. Example: eq=brightness=0.05";
pub const FILTER_ORDER_NOTE: &str =
    "They run in this order: detelecine, deinterlace, crop, rotate, denoise, sharpen, black and white, the size limit, your own.";

// Subtitles
pub const SUBTITLE_TRACKS: &str = "Subtitles";
pub const SUBTITLE_NONE_HERE: &str = "Subtitles are not kept in this format.";
pub const SUBTITLE_PICTURES: &str = "does not fit MP4";
pub const SUBTITLE_HINT_MP4: &str = "MP4 keeps text subtitles only; the picture kind from DVDs and Blu-rays needs MKV.";

// Shortcuts and tips
pub const TIP_OPEN: &str = "Open a file (Ctrl+O)";
pub const TIP_ENCODE: &str = "Start encoding (Ctrl+Enter)";
pub const TIP_PAUSE: &str = "Pause (Space)";
pub const TIP_RESUME: &str = "Resume (Space)";

// Files that were not added
pub const REFUSED_ONE: &str = "1 file was not added";
pub const REFUSED_MANY: &str = "files were not added";
pub const REFUSED_MORE: &str = "more";

// A file without a picture
pub const NO_PICTURE: &str = "This file has no picture, so it can only become an audio file.";
pub const MAKE_MP3: &str = "Make it MP3";
pub const MAKE_M4A: &str = "Make it M4A";
pub const MAKE_OPUS: &str = "Make it Opus";

// Undo
pub const UNDO: &str = "Undo";
pub const REMOVED_ONE: &str = "Removed from the queue";
pub const REMOVED_MANY: &str = "cleared";
pub const CLOSED_FILE: &str = "File closed";

// Small things
pub const DETAILS: &str = "Details";
pub const BETTER: &str = "Better";
pub const SMALLER: &str = "Smaller";
pub const SOURCE_MORE: &str = "Tracks and range";

// Queue and Add to queue
pub const NOT_ADDED: &str = "Not added";
pub const TIP_ADD: &str = "Put this file in the queue with the settings as they are, and set up another";
pub const TIP_ADD_DISABLED: &str = "Open a file first";
pub const TIP_QUEUE: &str = "The jobs that wait, the one that runs and the ones that are done";

// When done
pub const WHEN_DONE: &str = "When done";
pub const POWER_SHUTDOWN: &str = "The PC will shut down in";
pub const POWER_SLEEP: &str = "The PC will go to sleep in";
pub const POWER_BODY: &str = "Encoding is finished. Press Cancel to keep the PC on.";
pub const POWER_CANCEL: &str = "Cancel";
pub const POWER_NOW_SHUTDOWN: &str = "Shut down now";
pub const POWER_NOW_SLEEP: &str = "Sleep now";
