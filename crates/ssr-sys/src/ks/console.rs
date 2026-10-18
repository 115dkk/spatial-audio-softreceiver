#![allow(unsafe_code)]
use ssr_types::HostError;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::{
    Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler},
    core::BOOL,
};
static INSTALLED: AtomicBool = AtomicBool::new(false);
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Ctrl+C/Break cancellation only. Closing/killing the process still requires
/// an external guard; this callback never performs restoration or blocks.
#[derive(Debug)]
pub struct ConsoleCancel;
unsafe extern "system" fn handler(kind: u32) -> BOOL {
    if matches!(kind, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        CANCELLED.store(true, Ordering::Release);
        BOOL(1)
    } else {
        BOOL(0)
    }
}
impl ConsoleCancel {
    pub fn install() -> Result<Self, HostError> {
        if INSTALLED.swap(true, Ordering::AcqRel) {
            return Err(HostError::Failed(
                "console handler already installed".into(),
            ));
        }
        CANCELLED.store(false, Ordering::Release);
        // SAFETY: static function has the Windows callback ABI and uses only
        // process-lifetime atomics; no borrowed state is captured.
        if let Err(e) = unsafe { SetConsoleCtrlHandler(Some(handler), true) } {
            INSTALLED.store(false, Ordering::Release);
            return Err(HostError::Failed(e.to_string()));
        }
        Ok(Self)
    }
    pub fn cancelled(&self) -> bool {
        CANCELLED.load(Ordering::Acquire)
    }
}
impl Drop for ConsoleCancel {
    fn drop(&mut self) {
        // SAFETY: unregister the same process-lifetime callback we installed.
        let _ = unsafe { SetConsoleCtrlHandler(Some(handler), false) };
        INSTALLED.store(false, Ordering::Release);
    }
}
