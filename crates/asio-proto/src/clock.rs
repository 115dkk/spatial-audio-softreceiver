use std::time::Duration;

pub const MIN_BUFFER_FRAMES: u32 = 64;
pub const MAX_BUFFER_FRAMES: u32 = 2048;

/// Cumulative counters are high-water marks for the same frame timeline. They
/// are alternatives, not quantities to add together.
pub const fn deficit(demand: u64, requested: u64, loaded: u64, processing: u64) -> u64 {
    let supplied = max_u64(requested, max_u64(loaded, processing));
    demand.saturating_sub(supplied)
}

/// Next modular sequence. Keep the last published sequence across stop/start
/// and generation changes.
pub const fn next_sequence(last_published: u32) -> u32 {
    last_published.wrapping_add(1)
}

/// Mirrors streamring's two-slot modular reuse rule.
pub const fn can_publish(completed: u32, next: u32) -> bool {
    streamring::sequence_at_least(completed, next.wrapping_sub(2))
}

pub const fn accepts_buffer(frames: u32) -> bool {
    frames >= MIN_BUFFER_FRAMES && frames <= MAX_BUFFER_FRAMES && frames.is_power_of_two()
}

/// Returns the nearest power of two in the supported range. An exact tie picks
/// the smaller buffer to avoid adding latency.
pub const fn preferred_buffer(dac_period_frames: u32) -> u32 {
    let clamped = if dac_period_frames < MIN_BUFFER_FRAMES {
        MIN_BUFFER_FRAMES
    } else if dac_period_frames > MAX_BUFFER_FRAMES {
        MAX_BUFFER_FRAMES
    } else {
        dac_period_frames
    };
    let mut lower = MIN_BUFFER_FRAMES;
    while lower.saturating_mul(2) <= clamped {
        lower *= 2;
    }
    let upper = lower.saturating_mul(2);
    if upper > MAX_BUFFER_FRAMES || clamped - lower <= upper - clamped {
        lower
    } else {
        upper
    }
}

/// Preserve the clock phase, but cap catch-up at four callbacks per wake.
pub fn advance_deadline(due: u64, now: u64, period: u64) -> u64 {
    let period = period.max(1);
    due.saturating_add(period)
        .max(now.saturating_sub(period.saturating_mul(3)))
}

/// Finite Windows millisecond waits round up, never spin on a fractional ms.
pub fn wait_millis(timeout: Duration) -> u32 {
    timeout
        .as_nanos()
        .div_ceil(1_000_000)
        .min(u128::from(u32::MAX - 1)) as u32
}

/// Maximum of four DAW buffers, four DAC periods, and 50 milliseconds.
pub fn clock_fault_timeout(
    sample_rate: u32,
    daw_buffer_frames: u32,
    dac_period_frames: u32,
) -> Duration {
    const MINIMUM: Duration = Duration::from_millis(50);
    if sample_rate == 0 {
        return MINIMUM;
    }
    let frames = u64::from(daw_buffer_frames.max(dac_period_frames)).saturating_mul(4);
    let nanos = frames
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::from(sample_rate) - 1)
        / u64::from(sample_rate);
    MINIMUM.max(Duration::from_nanos(nanos))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockFault {
    PeerGone,
    DemandStalled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockEvent {
    Unchanged,
    DemandAdvanced,
    GenerationChanged,
    Faulted(ClockFault),
    Recovered,
}

/// Tracks the DAC generation and its cumulative demand high-water mark.
///
/// `stalled_for` is supplied by the caller's monotonic clock. Once faulted, a
/// demand advance in the faulted generation is ignored. Recovery requires first
/// observing a different generation and then observing demand advance in that
/// generation while the peer is alive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockTracker {
    generation: u64,
    demand: u64,
    fault: Option<ClockFault>,
    recovery_generation: Option<u64>,
}

impl ClockTracker {
    pub const fn new(generation: u64, demand: u64) -> Self {
        Self {
            generation,
            demand,
            fault: None,
            recovery_generation: None,
        }
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn demand(self) -> u64 {
        self.demand
    }

    pub const fn fault(self) -> Option<ClockFault> {
        self.fault
    }

    pub fn observe(
        &mut self,
        generation: u64,
        demand: u64,
        peer_alive: bool,
        stalled_for: Duration,
        timeout: Duration,
    ) -> ClockEvent {
        if !peer_alive {
            self.fault = Some(ClockFault::PeerGone);
            self.recovery_generation = None;
            return ClockEvent::Faulted(ClockFault::PeerGone);
        }

        if generation != self.generation {
            self.generation = generation;
            self.demand = demand;
            if self.fault.is_some() {
                self.recovery_generation = Some(generation);
            }
            return ClockEvent::GenerationChanged;
        }

        let advanced = counter_advanced(demand, self.demand);
        if advanced {
            self.demand = demand;
            if self.fault.is_some() && self.recovery_generation == Some(generation) {
                self.fault = None;
                self.recovery_generation = None;
                return ClockEvent::Recovered;
            }
            return if self.fault.is_some() {
                ClockEvent::Unchanged
            } else {
                ClockEvent::DemandAdvanced
            };
        }

        if self.fault.is_none() && stalled_for >= timeout {
            self.fault = Some(ClockFault::DemandStalled);
            self.recovery_generation = None;
            ClockEvent::Faulted(ClockFault::DemandStalled)
        } else {
            ClockEvent::Unchanged
        }
    }
}

const fn max_u64(left: u64, right: u64) -> u64 {
    if left > right { left } else { right }
}

const fn counter_advanced(value: u64, reference: u64) -> bool {
    value > reference
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        ClockEvent, ClockFault, ClockTracker, accepts_buffer, can_publish, clock_fault_timeout,
        deficit, next_sequence, preferred_buffer,
    };

    #[test]
    fn fallback_deadlines_keep_phase_bound_catchup_and_round_waits_up() {
        assert_eq!(super::advance_deadline(100, 105, 10), 110);
        assert_eq!(super::advance_deadline(100, 1000, 10), 970);
        assert_eq!(super::wait_millis(Duration::from_nanos(1)), 1);
        assert_eq!(super::wait_millis(Duration::from_micros(1333)), 2);
        assert_eq!(super::wait_millis(Duration::ZERO), 0);
    }
    #[test]
    fn deficit_uses_largest_cumulative_watermark() {
        assert_eq!(deficit(1_000, 700, 800, 750), 200);
        assert_eq!(deficit(1_000, 1_100, 800, 900), 0);
        assert_eq!(deficit(1_000, 900, 1_100, 800), 0);
        assert_eq!(deficit(1_000, 900, 800, 1_100), 0);
        assert_eq!(deficit(5, u64::MAX, u64::MAX - 1, 0), 0);
        assert_eq!(deficit(u64::MAX, 0, 0, 0), u64::MAX);
    }

    #[test]
    fn publish_gate_uses_modular_two_slot_sequences() {
        assert_eq!(next_sequence(0), 1);
        assert_eq!(next_sequence(u32::MAX), 0);
        assert!(can_publish(u32::MAX - 1, u32::MAX));
        assert!(can_publish(u32::MAX, 0));
        assert!(can_publish(u32::MAX - 1, 0));
        assert!(!can_publish(u32::MAX - 1, 1));
        assert!(can_publish(0, 1));
        assert!(can_publish(0, 2));
        assert!(!can_publish(0, 3));
    }

    #[test]
    fn publish_gate_matches_streamring() {
        for completed in [0, 1, u32::MAX - 1, u32::MAX] {
            for next in [0, 1, 2, 3, u32::MAX - 1, u32::MAX] {
                assert_eq!(
                    can_publish(completed, next),
                    streamring::sequence_at_least(completed, next.wrapping_sub(2))
                );
            }
        }
    }

    #[test]
    fn only_power_of_two_buffers_are_accepted() {
        for frames in 0..=4096 {
            assert_eq!(
                accepts_buffer(frames),
                [64, 128, 256, 512, 1024, 2048].contains(&frames)
            );
        }
        assert!(accepts_buffer(64));
        assert!(!accepts_buffer(480));
        assert!(accepts_buffer(2048));
        assert!(!accepts_buffer(63));
        assert!(!accepts_buffer(2049));
        assert_eq!(preferred_buffer(1), 64);
        assert_eq!(preferred_buffer(480), 512);
        assert_eq!(preferred_buffer(768), 512);
        assert_eq!(preferred_buffer(769), 1024);
        assert_eq!(preferred_buffer(5_000), 2048);
    }

    #[test]
    fn timeout_uses_largest_bound_and_rounds_up() {
        assert_eq!(
            clock_fault_timeout(48_000, 64, 480),
            Duration::from_millis(50)
        );
        assert_eq!(
            clock_fault_timeout(48_000, 1024, 480),
            Duration::from_nanos(85_333_334)
        );
        assert_eq!(
            clock_fault_timeout(0, 2048, 2048),
            Duration::from_millis(50)
        );
    }

    #[test]
    fn tracker_detects_stall_peer_loss_and_generation_change() {
        let timeout = Duration::from_millis(50);
        let mut tracker = ClockTracker::new(10, 100);
        assert_eq!(
            tracker.observe(10, 101, true, Duration::ZERO, timeout),
            ClockEvent::DemandAdvanced
        );
        assert_eq!(
            tracker.observe(10, 101, true, timeout, timeout),
            ClockEvent::Faulted(ClockFault::DemandStalled)
        );
        assert_eq!(
            tracker.observe(10, 102, true, Duration::ZERO, timeout),
            ClockEvent::Unchanged
        );
        assert_eq!(
            tracker.observe(11, 0, true, Duration::ZERO, timeout),
            ClockEvent::GenerationChanged
        );
        assert_eq!(tracker.fault(), Some(ClockFault::DemandStalled));
        assert_eq!(
            tracker.observe(11, 1, true, Duration::ZERO, timeout),
            ClockEvent::Recovered
        );
        assert_eq!(tracker.fault(), None);
        assert_eq!(
            tracker.observe(11, 1, false, Duration::ZERO, timeout),
            ClockEvent::Faulted(ClockFault::PeerGone)
        );
    }

    #[test]
    fn dead_peer_new_ready_owner_still_needs_new_epoch_and_demand_advance() {
        let timeout = Duration::from_millis(50);
        let mut tracker = ClockTracker::new(10, 1000);
        assert_eq!(
            tracker.observe(10, 1000, false, Duration::ZERO, timeout),
            ClockEvent::Faulted(ClockFault::PeerGone)
        );
        // ACK/drain can proceed before the replacement process handle is opened.
        assert_eq!(
            tracker.observe(11, 128, false, Duration::ZERO, timeout),
            ClockEvent::Faulted(ClockFault::PeerGone)
        );
        assert_eq!(
            tracker.observe(11, 128, true, Duration::ZERO, timeout),
            ClockEvent::GenerationChanged
        );
        assert_eq!(
            tracker.observe(11, 128, true, Duration::ZERO, timeout),
            ClockEvent::Unchanged
        );
        assert_eq!(
            tracker.observe(11, 192, true, Duration::ZERO, timeout),
            ClockEvent::Recovered
        );
    }

    #[test]
    fn tracker_does_not_treat_counter_rewind_as_advance() {
        let timeout = Duration::from_millis(50);
        let mut tracker = ClockTracker::new(4, u64::MAX);
        assert_eq!(
            tracker.observe(4, 0, true, Duration::ZERO, timeout),
            ClockEvent::Unchanged
        );
    }
}
