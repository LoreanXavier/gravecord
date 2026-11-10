//! Later launches wake the existing window before opening credentials or starting a connection.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{Instance, claim};

pub enum Launch {
    Primary(Option<Instance>),
    Forwarded,
}

#[cfg(not(windows))]
pub struct Instance;
#[cfg(not(windows))]
impl Instance {
    pub fn take_requests(&mut self) -> Option<tokio::sync::mpsc::Receiver<()>> {
        None
    }
}
#[cfg(not(windows))]
pub fn claim() -> anyhow::Result<Launch> {
    Ok(Launch::Primary(None))
}
