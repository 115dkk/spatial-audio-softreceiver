// SPDX-License-Identifier: GPL-3.0-or-later
#![deny(unsafe_code)]
//! Private SDK/COM and Windows boundary. No SDK type is exposed to the DLL policy.
mod com;
mod host;
#[cfg(windows)]
mod platform;
pub mod probe_report;
mod sdk;
pub use host::{Host, HostBuffers};
#[cfg(windows)]
pub use platform::{ControlMapping, Event, Mmcss, Peer, RingMapping, qpc_nanos, spawn_worker};
#[cfg(all(windows, feature = "probe"))]
pub mod probe;

pub use inventory::submit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NotPresent,
    Hardware,
    InvalidParameter,
    InvalidMode,
    NotAdvancing,
    NoClock,
    NoMemory,
}
pub type Result<T = ()> = std::result::Result<T, Error>;
#[derive(Clone, Copy, Debug, Default)]
pub struct Position {
    pub frames: u64,
    pub nanos: u64,
}
#[derive(Clone, Debug)]
pub struct Channel {
    pub name: String,
    pub active: bool,
}

/// All methods may be reentered by the host. Control methods must not hold a
/// driver mutex while invoking Host; output_ready and position must not lock.
/// HostBuffers owns storage, and only Host::switch can hand its samples to Rust.
pub trait AsioDriver: Send + Sync + std::fmt::Debug + 'static {
    fn name(&self) -> &'static str;
    fn init(&self) -> bool;
    fn error_code(&self) -> &'static str;
    /// Cached when COM constructs the object, before any real-time callback.
    fn panic_message(&self) -> &'static str;
    fn channels(&self) -> u32;
    fn latency(&self) -> u32;
    fn preferred_frames(&self) -> u32;
    fn sample_rate(&self) -> u32;
    fn can_sample_rate(&self, rate: u32) -> bool;
    fn set_sample_rate(&self, rate: u32) -> Result;
    fn position(&self) -> Position;
    fn channel(&self, index: u32) -> Result<Channel>;
    fn create_buffers(
        &self,
        channels: &[u32],
        frames: u32,
        host: Host,
        buffers: HostBuffers,
    ) -> Result;
    fn dispose_buffers(&self) -> Result;
    fn start(&self) -> Result;
    fn stop(&self) -> Result;
    fn output_ready(&self) -> Result;
}

/// Link-time factory registration. inventory's constructor only registers this
/// function pointer; it never initializes the driver or performs IPC in DllMain.
#[derive(Debug)]
pub struct Factory(pub fn() -> std::sync::Arc<dyn AsioDriver>);
inventory::collect!(Factory);

/// Retain the sys object containing the COM exports in the final cdylib.
/// The app build script additionally exports both names through the MSVC linker.
pub fn retain_exports() -> usize {
    com::export_anchor()
}

/// Prevents COM unloading while a worker (possibly quarantined) still runs.
#[derive(Debug)]
pub struct WorkerGuard {
    _private: (),
}
impl WorkerGuard {
    pub fn new() -> Self {
        com::LIVE.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Self { _private: () }
    }
}
impl Default for WorkerGuard {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        com::LIVE.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}
