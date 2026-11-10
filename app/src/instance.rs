//! Later launches wake the existing window before opening credentials or starting a connection.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{Instance, claim};

pub enum Launch {
    Primary(Option<Instance>),
    /// The running instance restored its window and said so.
    Forwarded,
    /// The request was sent but the running instance did not answer in time.
    Unresponsive,
}

/// Shown by a later launch when the first one is running but does not answer.
pub const NOT_RESPONDING: &str =
    "gravecord is already running but is not responding. Wait a moment and start it again.";

#[cfg(not(windows))]
pub struct Instance;
#[cfg(not(windows))]
#[derive(Clone)]
pub struct Acknowledger;
#[cfg(not(windows))]
impl Acknowledger {
    pub fn acknowledge(&self) {}
}
#[cfg(not(windows))]
impl Instance {
    pub fn take_requests(&mut self) -> Option<tokio::sync::mpsc::Receiver<()>> {
        None
    }
    pub fn acknowledger(&self) -> Acknowledger {
        Acknowledger
    }
}
#[cfg(not(windows))]
pub fn claim() -> anyhow::Result<Launch> {
    Ok(Launch::Primary(None))
}
