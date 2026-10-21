#![forbid(unsafe_code)]

//! Platform-neutral ASIO entrance names, control image, and clock algorithms.
//!
//! Platform adapters own shared memory and events. This crate defines their
//! fixed wire contract without depending on Windows or any `ssr-*` crate.

mod clock;
mod control;
mod layout;

pub use clock::{
    ClockEvent, ClockFault, ClockTracker, MAX_BUFFER_FRAMES, MIN_BUFFER_FRAMES, accepts_buffer,
    advance_deadline, can_publish, clock_fault_timeout, deficit, next_sequence, preferred_buffer,
    wait_millis,
};
pub use control::{
    CONTROL_BYTES, CONTROL_VERSION, CONTROL_WORDS, Control, ControlBuf, ControlError,
    ControlMemory, ControlSnapshot, Lease, LeaseError, MAGIC, Reclaim,
};
pub use layout::{Channel, Layout};

/// Allocation-free attachment identity; discovery metadata is not part of a tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientState {
    pub lease: Lease,
    pub generation: u64,
    pub peer_alive: bool,
}

pub const CLSID: &str = "{39D4F08A-7B4C-4B32-94A7-C074BB51BFF9}";
pub const CONTROL_NAME: &str = r"Local\SpatialSoftReceiver.Asio.Control";

pub fn ring_name(pid: u32, serial: u32) -> String {
    format!(r"Local\SpatialSoftReceiver.Asio.{pid}.{serial}")
}

pub fn work_event_name(pid: u32, serial: u32) -> String {
    format!(r"Local\SpatialSoftReceiver.Asio.Work.{pid}.{serial}")
}

pub fn demand_event_name(pid: u32, serial: u32) -> String {
    format!(r"Local\SpatialSoftReceiver.Asio.Demand.{pid}.{serial}")
}

pub fn done_event_name(pid: u32, serial: u32) -> String {
    format!(r"Local\SpatialSoftReceiver.Asio.Done.{pid}.{serial}")
}

#[cfg(test)]
mod tests {
    use super::{CONTROL_NAME, demand_event_name, done_event_name, ring_name, work_event_name};

    #[test]
    fn session_object_names_include_producer_identity() {
        assert_eq!(CONTROL_NAME, r"Local\SpatialSoftReceiver.Asio.Control");
        assert_eq!(ring_name(123, 9), r"Local\SpatialSoftReceiver.Asio.123.9");
        assert_eq!(
            work_event_name(123, 9),
            r"Local\SpatialSoftReceiver.Asio.Work.123.9"
        );
        assert_eq!(
            demand_event_name(123, 9),
            r"Local\SpatialSoftReceiver.Asio.Demand.123.9"
        );
        assert_eq!(
            done_event_name(123, 9),
            r"Local\SpatialSoftReceiver.Asio.Done.123.9"
        );
    }
}
