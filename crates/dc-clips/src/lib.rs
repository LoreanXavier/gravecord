//! The Clips engine: a rolling buffer of the Go Live stream's encoded video and audio, a cut of the newest window, the
//! MP4 file it is written to with the clip metadata inside, and the reader that loads a clips folder back. No UI and no
//! network; the stream's sender feeds [`Tap`], the app asks [`Recorder::snapshot`] when the save keybind fires.
pub mod audio;
pub mod buffer;
pub mod library;
pub mod metadata;
pub mod mp4;
pub mod preview;
pub mod recorder;
pub mod sps;

pub use buffer::now_ms;
pub use library::{Clip, load_directory, load_file};
pub use metadata::{ClipMetadata, file_name};
pub use recorder::{
    Recorder, Snapshot, SnapshotError, SourceInfo, Tap, TapGuard, Track, recorder, standalone,
};
