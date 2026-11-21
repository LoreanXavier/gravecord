//! Game detection. The official desktop client hands the detectable games list to a native process observer
//! (`setObservedGamesCallback`, module 253400) that matches running processes against each game's `executables`;
//! the web side then sorts the matches (module 397369 `RunningGameStore`). The native matcher is not part of the
//! snapshot, so [`matcher`] follows the web side's own rules for the same data: lower-cased paths with `/`
//! separators, an executable name matches when the path ends with it at a path boundary (`eU` in 397369, `PQ` in
//! module 973522), the OS must be the running one, and the blocklist of `shouldBlock` (module 760751) applies.
//!
//! [`processes`] reads the running process list on Windows. Nothing is opened for writing and no process memory is
//! read: only the image path of each process this user may query.

pub mod matcher;
pub mod processes;

pub use matcher::{
    Blocklist, Candidate, DetectableGame, Detectables, Detected, Executable, Index, Kind, Scan,
    normalize_path,
};
pub use processes::{
    Process, foreground_pid, is_current_process_elevated, is_process_elevated, main_window,
    process_age, running_processes, visible_window_pids,
};
