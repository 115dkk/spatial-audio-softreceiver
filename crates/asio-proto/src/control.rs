use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Layout;

pub const MAGIC: u64 = u64::from_le_bytes(*b"SSRACCTL");
// Version 2 adds a callback-quiescent generation acknowledgement. Version 1 was
// never released; mixed versions are rejected rather than interpreted together.
pub const CONTROL_VERSION: u64 = 2;
pub const CONTROL_WORDS: usize = 24;
pub const CONTROL_BYTES: usize = CONTROL_WORDS * size_of::<u64>();

const INITIALIZING: u64 = u64::from_le_bytes(*b"SSRACINI");
const MAGIC_I: usize = 0;
const VERSION_I: usize = 1;
const APP_PID_I: usize = 2;
const READY_I: usize = 3;
const SAMPLE_RATE_I: usize = 4;
const LAYOUT_I: usize = 5;
const BUFFER_FRAMES_I: usize = 6;
const DAC_PERIOD_FRAMES_I: usize = 7;
const LATENCY_FRAMES_I: usize = 8;
// Word 9 is reserved (zero). Unreleased v2 no longer requests DAC rate changes.
const LEASE_I: usize = 10;
const DEMAND_I: usize = 11;
const DAC_GENERATION_I: usize = 12;
const HEARTBEAT_I: usize = 13;
const LOADED_FRAMES_I: usize = 14;
const COMPLETED_FRAMES_I: usize = 15;
const UNDERRUNS_I: usize = 16;
const LAST_ERROR_I: usize = 17;
const RESET_SERIAL_I: usize = 18;
const CLIENT_RUNNING_I: usize = 19;
const CLOCK_FAULT_I: usize = 20;
const CLIENT_GENERATION_ACK_I: usize = 21;
const GENERATION_BARRIER_SEQUENCE_I: usize = 22;
const CLIENT_RUN_SERIAL_I: usize = 23;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlError {
    Size { expected: usize, actual: usize },
    InvalidOwner,
    AlreadyInitialized,
    Magic,
    Version { expected: u64, actual: u64 },
}

impl fmt::Display for ControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ControlError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Lease {
    pub pid: u32,
    pub serial: u32,
}

impl Lease {
    pub const VACANT: Self = Self { pid: 0, serial: 0 };

    pub const fn new(pid: u32, serial: u32) -> Option<Self> {
        if pid == 0 {
            None
        } else {
            Some(Self { pid, serial })
        }
    }

    pub const fn is_vacant(self) -> bool {
        self.pid == 0
    }

    pub const fn to_wire(self) -> u64 {
        (self.pid as u64) | ((self.serial as u64) << 32)
    }

    pub const fn from_wire(value: u64) -> Self {
        Self {
            pid: value as u32,
            serial: (value >> 32) as u32,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseError {
    Invalid,
    Occupied(Lease),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reclaim {
    Vacant,
    Alive(Lease),
    Reclaimed(Lease),
    Changed(Lease),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlSnapshot {
    pub app_pid: u32,
    pub ready: bool,
    pub sample_rate: u32,
    pub layout: Option<Layout>,
    pub buffer_frames: u32,
    pub dac_period_frames: u32,
    pub latency_frames: u32,
    pub lease: Lease,
    pub demand: u64,
    pub dac_generation: u64,
    pub heartbeat: u64,
    pub loaded_frames: u64,
    pub completed_frames: u64,
    pub underruns: u64,
    pub last_error: u64,
    pub reset_serial: u64,
    pub client_running: bool,
    pub clock_fault: bool,
    pub client_generation_ack: u64,
    pub generation_barrier_sequence: u32,
    pub client_run_serial: u64,
}

/// A borrowed ASIO control mapping.
///
/// The wire image is 24 little-endian `AtomicU64` words. Every field therefore
/// starts at an eight-byte boundary. Offsets are: magic 0, version 8, app PID
/// 16, ready 24, sample rate 32, layout 40, buffer frames 48, DAC period 56,
/// latency 64, reserved zero 72, lease 80, demand 88, DAC generation
/// 96, heartbeat 104, loaded frames 112, completed frames 120, underruns 128,
/// last error 136, reset serial 144, client running 152, clock fault 160,
/// client generation acknowledgement 168, generation barrier sequence 176
/// (a u32 value stored in a whole u64 word), and client run serial 184.
/// Total size is 192 bytes. The DLL increments run serial before each start;
/// output advances the epoch when it changes, even if a quick stop/start hides
/// the running flag's falling edge between DAC periods.
///
/// Writers own disjoint fields. The app output owner clears `ready`, resets its
/// demand timeline, and release-publishes a new nonzero generation. Only the DLL
/// callback worker, AFTER the preceding callback returns, records the last
/// actually published sequence, resets its loaded/requested frame counters, and
/// release-publishes `client_generation_ack`. The app consumer acquire-loads that
/// acknowledgement, discards ring work through the barrier (at most two slots),
/// resets DSP/queue and completed_frames, then release-publishes its captured
/// epoch with `set_ready_for_generation`. The ready word at offset 24 is an epoch
/// tag (zero means not ready), not a boolean in v2. A stale consumer can never
/// acknowledge a newer epoch. No new epoch audio may be published before that
/// ready acknowledgement. A callback's generation check alone cannot exclude a late
/// old-epoch publish, which is why the quiescent barrier is mandatory.
///
/// Initial createBuffers announces a fresh ring and acknowledges barrier zero
/// before waiting for streamring Ready; it does not need start/a callback. With
/// no running client demand stays at lead, not elapsed app uptime. Ring geometry
/// is immutable for a lease: rate/layout changes need a new ring, not a new view.
/// The ring's reserved heartbeat/error suffix is unused by ASIO; heartbeat and
/// errors live here, in the separate fixed-width control mapping.
///
/// Counters and flags use release stores and acquire loads. The lease is the
/// sole multi-writer CAS field. Generation/frame counters saturate or require a
/// fresh session at exhaustion; ring sequences never wrap within a lease.
#[derive(Clone, Copy, Debug)]
pub struct Control<'a> {
    words: &'a [AtomicU64],
}

impl<'a> Control<'a> {
    pub fn from_slice(words: &'a [AtomicU64]) -> Result<Self, ControlError> {
        if words.len() != CONTROL_WORDS {
            return Err(ControlError::Size {
                expected: CONTROL_WORDS,
                actual: words.len(),
            });
        }
        Ok(Self { words })
    }

    /// Initializes only a zeroed, unowned mapping. It never clears an existing
    /// mapping or lease, including one whose owner is no longer alive.
    pub fn initialize(self, owner_pid: u32) -> Result<(), ControlError> {
        if owner_pid == 0 {
            return Err(ControlError::InvalidOwner);
        }
        self.words[MAGIC_I]
            .compare_exchange(
                0,
                INITIALIZING.to_le(),
                Ordering::Acquire,
                Ordering::Acquire,
            )
            .map_err(|_| ControlError::AlreadyInitialized)?;
        for word in &self.words[VERSION_I..] {
            word.store(0, Ordering::Relaxed);
        }
        self.store(APP_PID_I, u64::from(owner_pid), Ordering::Relaxed);
        self.store(VERSION_I, CONTROL_VERSION, Ordering::Relaxed);
        self.store(MAGIC_I, MAGIC, Ordering::Release);
        Ok(())
    }

    pub fn validate(self) -> Result<(), ControlError> {
        if self.load(MAGIC_I, Ordering::Acquire) != MAGIC {
            return Err(ControlError::Magic);
        }
        let actual = self.load(VERSION_I, Ordering::Relaxed);
        if actual != CONTROL_VERSION {
            return Err(ControlError::Version {
                expected: CONTROL_VERSION,
                actual,
            });
        }
        Ok(())
    }

    pub fn snapshot(self) -> Result<ControlSnapshot, ControlError> {
        self.validate()?;
        Ok(ControlSnapshot {
            app_pid: self.app_pid(),
            ready: self.ready(),
            sample_rate: self.sample_rate(),
            layout: self.layout(),
            buffer_frames: self.buffer_frames(),
            dac_period_frames: self.dac_period_frames(),
            latency_frames: self.latency_frames(),
            lease: self.lease(),
            demand: self.demand(),
            dac_generation: self.dac_generation(),
            heartbeat: self.heartbeat(),
            loaded_frames: self.loaded_frames(),
            completed_frames: self.completed_frames(),
            underruns: self.underruns(),
            last_error: self.last_error(),
            reset_serial: self.reset_serial(),
            client_running: self.client_running(),
            clock_fault: self.clock_fault(),
            client_generation_ack: self.client_generation_ack(),
            generation_barrier_sequence: self.generation_barrier_sequence(),
            client_run_serial: self.client_run_serial(),
        })
    }

    pub fn app_pid(self) -> u32 {
        self.get_u32(APP_PID_I)
    }

    pub fn set_app_pid(self, value: u32) {
        self.set_u32(APP_PID_I, value);
    }

    pub fn ready(self) -> bool {
        let generation = self.ready_generation();
        generation != 0 && generation == self.dac_generation()
    }

    /// Convenience for synchronous configuration/bootstrap only. A consumer
    /// finishing an asynchronous drain must use set_ready_for_generation with its
    /// captured epoch, never stamp whatever generation happens to be current.
    pub fn set_ready(self, value: bool) {
        self.set_ready_for_generation(if value { self.dac_generation() } else { 0 });
    }

    pub fn ready_generation(self) -> u64 {
        self.get_u64(READY_I)
    }

    /// App consumer's acknowledgement, tied to the epoch it actually drained.
    /// A late store for an old generation cannot enable a newer generation.
    pub fn set_ready_for_generation(self, generation: u64) {
        self.set_u64(READY_I, generation);
    }

    pub fn sample_rate(self) -> u32 {
        self.get_u32(SAMPLE_RATE_I)
    }

    pub fn set_sample_rate(self, value: u32) {
        self.set_u32(SAMPLE_RATE_I, value);
    }

    pub fn layout(self) -> Option<Layout> {
        Layout::from_wire(self.get_u32(LAYOUT_I))
    }

    pub fn set_layout(self, value: Layout) {
        self.set_u32(LAYOUT_I, value as u32);
    }

    pub fn buffer_frames(self) -> u32 {
        self.get_u32(BUFFER_FRAMES_I)
    }

    pub fn set_buffer_frames(self, value: u32) {
        self.set_u32(BUFFER_FRAMES_I, value);
    }

    pub fn dac_period_frames(self) -> u32 {
        self.get_u32(DAC_PERIOD_FRAMES_I)
    }

    pub fn set_dac_period_frames(self, value: u32) {
        self.set_u32(DAC_PERIOD_FRAMES_I, value);
    }

    pub fn latency_frames(self) -> u32 {
        self.get_u32(LATENCY_FRAMES_I)
    }

    pub fn set_latency_frames(self, value: u32) {
        self.set_u32(LATENCY_FRAMES_I, value);
    }

    pub fn lease(self) -> Lease {
        Lease::from_wire(self.load(LEASE_I, Ordering::Acquire))
    }

    pub fn acquire_lease(self, lease: Lease) -> Result<(), LeaseError> {
        if lease.is_vacant() {
            return Err(LeaseError::Invalid);
        }
        self.words[LEASE_I]
            .compare_exchange(
                0_u64.to_le(),
                lease.to_wire().to_le(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|actual| LeaseError::Occupied(Lease::from_wire(u64::from_le(actual))))
    }

    pub fn release_lease(self, lease: Lease) -> bool {
        if lease.is_vacant() {
            return false;
        }
        self.words[LEASE_I]
            .compare_exchange(
                lease.to_wire().to_le(),
                0_u64.to_le(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    /// Reclaims the exact lease observed before `is_dead` runs. A replacement
    /// lease cannot be cleared by the stale liveness result.
    pub fn reclaim_lease(self, mut is_dead: impl FnMut(u32) -> bool) -> Reclaim {
        let observed = self.lease();
        if observed.is_vacant() {
            return Reclaim::Vacant;
        }
        if !is_dead(observed.pid) {
            return Reclaim::Alive(observed);
        }
        match self.words[LEASE_I].compare_exchange(
            observed.to_wire().to_le(),
            0_u64.to_le(),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Reclaim::Reclaimed(observed),
            Err(actual) => Reclaim::Changed(Lease::from_wire(u64::from_le(actual))),
        }
    }

    pub fn demand(self) -> u64 {
        self.get_u64(DEMAND_I)
    }

    pub fn set_demand(self, value: u64) {
        self.set_u64(DEMAND_I, value);
    }

    pub fn dac_generation(self) -> u64 {
        self.get_u64(DAC_GENERATION_I)
    }

    pub fn set_dac_generation(self, value: u64) {
        self.set_u64(DAC_GENERATION_I, value);
    }

    pub fn heartbeat(self) -> u64 {
        self.get_u64(HEARTBEAT_I)
    }

    pub fn set_heartbeat(self, value: u64) {
        self.set_u64(HEARTBEAT_I, value);
    }

    pub fn loaded_frames(self) -> u64 {
        self.get_u64(LOADED_FRAMES_I)
    }

    pub fn set_loaded_frames(self, value: u64) {
        self.set_u64(LOADED_FRAMES_I, value);
    }

    pub fn completed_frames(self) -> u64 {
        self.get_u64(COMPLETED_FRAMES_I)
    }

    pub fn set_completed_frames(self, value: u64) {
        self.set_u64(COMPLETED_FRAMES_I, value);
    }

    pub fn underruns(self) -> u64 {
        self.get_u64(UNDERRUNS_I)
    }

    pub fn set_underruns(self, value: u64) {
        self.set_u64(UNDERRUNS_I, value);
    }

    pub fn last_error(self) -> u64 {
        self.get_u64(LAST_ERROR_I)
    }

    pub fn set_last_error(self, value: u64) {
        self.set_u64(LAST_ERROR_I, value);
    }

    pub fn reset_serial(self) -> u64 {
        self.get_u64(RESET_SERIAL_I)
    }

    pub fn set_reset_serial(self, value: u64) {
        self.set_u64(RESET_SERIAL_I, value);
    }

    pub fn client_running(self) -> bool {
        self.get_bool(CLIENT_RUNNING_I)
    }

    pub fn set_client_running(self, value: bool) {
        self.set_bool(CLIENT_RUNNING_I, value);
    }

    pub fn clock_fault(self) -> bool {
        self.get_bool(CLOCK_FAULT_I)
    }

    pub fn set_clock_fault(self, value: bool) {
        self.set_bool(CLOCK_FAULT_I, value);
    }

    /// Acquire the DLL's callback-quiescent acknowledgement before reading its
    /// barrier/counters. Equality with dac_generation alone does not imply ready.
    pub fn client_generation_ack(self) -> u64 {
        self.get_u64(CLIENT_GENERATION_ACK_I)
    }

    /// DLL only, after the old callback returns and barrier/loaded are stored.
    pub fn set_client_generation_ack(self, value: u64) {
        self.set_u64(CLIENT_GENERATION_ACK_I, value);
    }

    pub fn generation_barrier_sequence(self) -> u32 {
        self.get_u32(GENERATION_BARRIER_SEQUENCE_I)
    }

    /// Last actually published ring sequence, not the next requested sequence.
    pub fn set_generation_barrier_sequence(self, value: u32) {
        self.set_u32(GENERATION_BARRIER_SEQUENCE_I, value);
    }

    pub fn client_run_serial(self) -> u64 {
        self.get_u64(CLIENT_RUN_SERIAL_I)
    }

    /// DLL only, before client_running=true. The output owner resets the epoch
    /// on any change, including a stop/start faster than its polling period.
    pub fn set_client_run_serial(self, value: u64) {
        self.set_u64(CLIENT_RUN_SERIAL_I, value);
    }

    fn get_u32(self, index: usize) -> u32 {
        self.load(index, Ordering::Acquire) as u32
    }

    fn set_u32(self, index: usize, value: u32) {
        self.store(index, u64::from(value), Ordering::Release);
    }

    fn get_u64(self, index: usize) -> u64 {
        self.load(index, Ordering::Acquire)
    }

    fn set_u64(self, index: usize, value: u64) {
        self.store(index, value, Ordering::Release);
    }

    fn get_bool(self, index: usize) -> bool {
        self.load(index, Ordering::Acquire) != 0
    }

    fn set_bool(self, index: usize, value: bool) {
        self.store(index, u64::from(value), Ordering::Release);
    }

    fn load(self, index: usize, ordering: Ordering) -> u64 {
        u64::from_le(self.words[index].load(ordering))
    }

    fn store(self, index: usize, value: u64, ordering: Ordering) {
        self.words[index].store(value.to_le(), ordering);
    }
}

#[derive(Debug)]
pub struct ControlBuf {
    words: Box<[AtomicU64]>,
}

impl ControlBuf {
    pub fn new() -> Self {
        Self {
            words: (0..CONTROL_WORDS).map(|_| AtomicU64::new(0)).collect(),
        }
    }

    pub fn control(&self) -> Control<'_> {
        Control::from_slice(&self.words).expect("ControlBuf has the fixed control image size")
    }
}

impl Default for ControlBuf {
    fn default() -> Self {
        Self::new()
    }
}

pub trait ControlMemory: Send + Sync {
    fn control(&self) -> Control<'_>;
}

impl ControlMemory for ControlBuf {
    fn control(&self) -> Control<'_> {
        self.control()
    }
}

impl<T: ControlMemory + ?Sized> ControlMemory for Arc<T> {
    fn control(&self) -> Control<'_> {
        T::control(self)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    use super::{
        CONTROL_VERSION, CONTROL_WORDS, Control, ControlBuf, ControlError, Lease, LeaseError,
        MAGIC, Reclaim,
    };
    use crate::Layout;

    #[test]
    fn rejects_bad_size_magic_and_version() {
        let short: Vec<_> = (0..CONTROL_WORDS - 1).map(|_| AtomicU64::new(0)).collect();
        assert!(matches!(
            Control::from_slice(&short),
            Err(ControlError::Size {
                expected: CONTROL_WORDS,
                actual
            }) if actual == CONTROL_WORDS - 1
        ));

        let memory = ControlBuf::new();
        let control = memory.control();
        assert_eq!(control.validate(), Err(ControlError::Magic));
        control.words[0].store(MAGIC.to_be(), Ordering::Release);
        assert_eq!(control.validate(), Err(ControlError::Magic));
        control.words[0].store(0, Ordering::Release);
        control.initialize(42).unwrap();
        assert_eq!(control.validate(), Ok(()));
        control.words[1].store((CONTROL_VERSION + 1).to_le(), Ordering::Relaxed);
        assert_eq!(
            control.validate(),
            Err(ControlError::Version {
                expected: CONTROL_VERSION,
                actual: CONTROL_VERSION + 1
            })
        );
        control.words[0].store((MAGIC + 1).to_le(), Ordering::Release);
        assert_eq!(control.validate(), Err(ControlError::Magic));
    }

    #[test]
    fn wire_words_are_little_endian_and_snapshot_is_typed() {
        let memory = ControlBuf::new();
        let control = memory.control();
        control.initialize(7).unwrap();
        control.set_dac_generation(1);
        control.set_ready(true);
        control.set_sample_rate(48_000);
        control.set_layout(Layout::Surround916);
        control.set_demand(0x0102_0304_0506_0708);
        assert_eq!(
            control.words[11].load(Ordering::Relaxed),
            0x0102_0304_0506_0708_u64.to_le()
        );
        let snapshot = control.snapshot().unwrap();
        assert_eq!(snapshot.app_pid, 7);
        assert!(snapshot.ready);
        assert_eq!(snapshot.sample_rate, 48_000);
        assert_eq!(snapshot.layout, Some(Layout::Surround916));
        assert_eq!(snapshot.demand, 0x0102_0304_0506_0708);
    }

    #[test]
    fn generation_handshake_uses_appended_whole_u64_words() {
        assert_eq!(super::CONTROL_BYTES, 192);
        assert_eq!(super::CLIENT_RUN_SERIAL_I * 8, 184);
        assert_eq!(super::CLIENT_GENERATION_ACK_I * 8, 168);
        assert_eq!(super::GENERATION_BARRIER_SEQUENCE_I * 8, 176);
        let memory = ControlBuf::new();
        let c = memory.control();
        c.initialize(1).unwrap();
        c.set_generation_barrier_sequence(u32::MAX - 1);
        c.set_client_generation_ack(0x0102_0304_0506_0708);
        let snapshot = c.snapshot().unwrap();
        assert_eq!(snapshot.client_generation_ack, 0x0102_0304_0506_0708);
        assert_eq!(snapshot.generation_barrier_sequence, u32::MAX - 1);
        assert_eq!(
            c.words[21].load(Ordering::Relaxed),
            0x0102_0304_0506_0708_u64.to_le()
        );
        assert_eq!(
            c.words[22].load(Ordering::Relaxed),
            u64::from(u32::MAX - 1).to_le()
        );
        c.words[1].store(1_u64.to_le(), Ordering::Relaxed);
        assert_eq!(
            c.validate(),
            Err(ControlError::Version {
                expected: 2,
                actual: 1
            })
        );
    }

    #[test]
    fn initialize_does_not_clear_live_lease() {
        let memory = ControlBuf::new();
        let control = memory.control();
        control.initialize(7).unwrap();
        let lease = Lease::new(9, 3).unwrap();
        control.acquire_lease(lease).unwrap();
        assert_eq!(control.initialize(8), Err(ControlError::AlreadyInitialized));
        assert_eq!(control.lease(), lease);
    }

    #[test]
    fn only_one_lease_racer_wins() {
        let memory = Arc::new(ControlBuf::new());
        memory.control().initialize(1).unwrap();
        let mut racers = Vec::new();
        for pid in 1..=8 {
            let memory = Arc::clone(&memory);
            racers.push(thread::spawn(move || {
                let lease = Lease::new(pid, 1).unwrap();
                (lease, memory.control().acquire_lease(lease))
            }));
        }
        let results: Vec<_> = racers
            .into_iter()
            .map(|racer| racer.join().unwrap())
            .collect();
        assert_eq!(
            results.iter().filter(|(_, result)| result.is_ok()).count(),
            1
        );
        let winner = results
            .iter()
            .find_map(|(lease, result)| result.is_ok().then_some(*lease))
            .unwrap();
        assert_eq!(memory.control().lease(), winner);
        assert!(results.iter().all(|(lease, result)| {
            *lease == winner
                || matches!(result, Err(LeaseError::Occupied(actual)) if *actual == winner)
        }));
    }

    #[test]
    fn stale_release_cannot_clear_replacement() {
        let memory = ControlBuf::new();
        let control = memory.control();
        control.initialize(1).unwrap();
        let first = Lease::new(10, 1).unwrap();
        let replacement = Lease::new(10, 2).unwrap();
        control.acquire_lease(first).unwrap();
        assert!(control.release_lease(first));
        control.acquire_lease(replacement).unwrap();
        assert!(!control.release_lease(first));
        assert_eq!(control.lease(), replacement);
    }

    #[test]
    fn reclaim_checks_liveness_and_cas_exact_observation() {
        let memory = ControlBuf::new();
        let control = memory.control();
        control.initialize(1).unwrap();
        let lease = Lease::new(12, 5).unwrap();
        control.acquire_lease(lease).unwrap();
        assert_eq!(control.reclaim_lease(|_| false), Reclaim::Alive(lease));
        assert_eq!(
            control.reclaim_lease(|pid| pid == 12),
            Reclaim::Reclaimed(lease)
        );
        assert_eq!(control.reclaim_lease(|_| true), Reclaim::Vacant);
    }

    #[test]
    fn stale_reclaim_cannot_clear_a_replacement() {
        let memory = ControlBuf::new();
        let control = memory.control();
        control.initialize(1).unwrap();
        let observed = Lease::new(12, 5).unwrap();
        let replacement = Lease::new(13, 6).unwrap();
        control.acquire_lease(observed).unwrap();
        assert_eq!(
            control.reclaim_lease(|_| {
                assert!(control.release_lease(observed));
                control.acquire_lease(replacement).unwrap();
                true
            }),
            Reclaim::Changed(replacement)
        );
        assert_eq!(control.lease(), replacement);
    }
}
