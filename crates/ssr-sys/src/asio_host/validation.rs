//! Pure admission checks, also exercised on Linux. Peer lengths never determine
//! a mapped slice's length. Shared ring bytes are exclusively AtomicU32.

use asio_proto::{Control, Layout, Lease};
use streamring::Geometry;

/// Called only after exclusively creating the owner object. Every old control
/// Arc retains that object, so same-process restart cannot overlap an old owner.
pub(super) fn owner_takeover_allowed(previous_pid: u32, current_pid: u32, dead: bool) -> bool {
    previous_pid == current_pid || dead
}

/// One process identity for the exact observed lease, including while the ring
/// is unpublished or the output is not ready. Unknown opens are retried; an
/// acquired handle is never reopened by PID until the lease changes.
#[derive(Debug)]
pub(super) struct LeaseMonitor<H> {
    observed: Option<(Lease, H)>,
}

impl<H> LeaseMonitor<H> {
    pub(super) fn new() -> Self {
        Self { observed: None }
    }

    pub(super) fn observe(
        &mut self,
        lease: Lease,
        open: impl FnOnce(u32) -> Option<H>,
    ) -> Option<&H> {
        if self.observed.as_ref().is_some_and(|(old, _)| *old != lease) {
            self.observed = None;
        }
        if lease.is_vacant() {
            return None;
        }
        if self.observed.is_none() {
            self.observed = open(lease.pid).map(|handle| (lease, handle));
        }
        self.observed.as_ref().map(|(_, handle)| handle)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputConfig {
    pub sample_rate: u32,
    pub dac_period_frames: u32,
    pub layout: Layout,
    pub latency_frames: u32,
}

impl OutputConfig {
    pub(super) fn requires_reset(self, next: Self) -> bool {
        self.sample_rate != next.sample_rate
            || self.dac_period_frames != next.dac_period_frames
            || self.layout != next.layout
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            dac_period_frames: 480,
            layout: Layout::Surround714,
            latency_frames: 0,
        }
    }
}

pub(super) fn client_geometry(control: Control<'_>, config: OutputConfig) -> Option<Geometry> {
    if control.validate().is_err()
        || control.sample_rate() != config.sample_rate
        || control.layout() != Some(config.layout)
        || control.dac_period_frames() != config.dac_period_frames
        || control.latency_frames() != config.latency_frames
        || !asio_proto::accepts_buffer(control.buffer_frames())
    {
        return None;
    }
    Some(Geometry {
        lanes: 1,
        input_channels: config.layout.channels(),
        output_channels: 2,
        max_frames: control.buffer_frames(),
        sample_rate: config.sample_rate,
        latency_bound_frames: config.latency_frames,
    })
}

/// Latency reported by getLatencies can change without resizing the ring. The
/// header retains its creation latency; compare that separately against itself.
pub(super) fn same_ring_geometry(left: Geometry, right: Geometry) -> bool {
    Geometry {
        latency_bound_frames: right.latency_bound_frames,
        ..left
    } == right
}

pub(super) fn validate_header(
    header: &[u32; streamring::HEADER_BYTES / 4],
    expected: Geometry,
    pid: u32,
    mapped_bytes: usize,
) -> bool {
    let Ok(total) = expected.total_bytes() else {
        return false;
    };
    let slot_bytes =
        (expected.samples_per_slot() * 4).div_ceil(streamring::CACHE_LINE) * streamring::CACHE_LINE;
    header[0] == streamring::MAGIC
        && header[1] == streamring::LAYOUT_VERSION
        && header[2] as usize == total
        && mapped_bytes == total
        && header[3] == expected.lanes
        && header[4] == expected.input_channels
        && header[5] == expected.output_channels
        && header[6] == expected.max_frames
        && header[7] == expected.sample_rate
        && header[8] == expected.latency_bound_frames
        && (header[9] == streamring::State::Announced as u32
            || header[9] == streamring::State::Ready as u32)
        && header[11] == pid
        && header[16] as usize == streamring::HEADER_BYTES
        && header[17] as usize == streamring::HEADER_BYTES + slot_bytes
        && header[18] as usize == slot_bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use asio_proto::ControlBuf;
    use std::cell::Cell;

    #[test]
    fn latency_change_does_not_request_buffer_reset() {
        let old = OutputConfig::default();
        assert!(!old.requires_reset(OutputConfig {
            latency_frames: 999,
            ..old
        }));
        assert!(old.requires_reset(OutputConfig {
            sample_rate: 96000,
            ..old
        }));
        assert!(old.requires_reset(OutputConfig {
            dac_period_frames: 64,
            ..old
        }));
        assert!(old.requires_reset(OutputConfig {
            layout: Layout::Stereo,
            ..old
        }));
    }
    #[test]
    fn exclusive_owner_allows_same_process_restart_but_requires_other_owner_death() {
        assert!(owner_takeover_allowed(42, 42, false));
        assert!(owner_takeover_allowed(42, 42, true));
        assert!(!owner_takeover_allowed(41, 42, false));
        assert!(owner_takeover_allowed(41, 42, true));
        // PID zero and failed/access-denied queries supply no proof of death.
        assert!(!owner_takeover_allowed(0, 42, false));
    }

    #[test]
    fn lease_monitor_keeps_observed_process_identity_after_pid_reuse() {
        let lease = Lease::new(11, 7).unwrap();
        let original_dead = Cell::new(false);
        let replacement_dead = Cell::new(false);
        let mut monitor = LeaseMonitor::new();
        assert!(
            !monitor
                .observe(lease, |_| Some(&original_dead))
                .unwrap()
                .get()
        );
        original_dead.set(true);
        // The PID now names another live process. Never reopen it for this lease.
        assert!(
            monitor
                .observe(lease, |_| Some(&replacement_dead))
                .unwrap()
                .get()
        );

        let memory = configured();
        let control = memory.control();
        control.acquire_lease(lease).unwrap();
        assert!(control.release_lease(lease));
        let replacement = Lease::new(11, 8).unwrap();
        control.acquire_lease(replacement).unwrap();
        assert!(!control.release_lease(lease));
        assert_eq!(control.lease(), replacement);
        assert!(
            !monitor
                .observe(replacement, |_| Some(&replacement_dead))
                .unwrap()
                .get()
        );
    }

    #[test]
    fn lease_monitor_retries_unknown_and_retires_on_vacancy() {
        let lease = Lease::new(11, 7).unwrap();
        let mut monitor = LeaseMonitor::<u32>::new();
        assert!(monitor.observe(lease, |_| None).is_none());
        assert_eq!(monitor.observe(lease, Some), Some(&11));
        assert_eq!(
            monitor.observe(lease, |_| panic!("must retain handle")),
            Some(&11)
        );
        assert!(
            monitor
                .observe(Lease::VACANT, |_| panic!("vacant PID"))
                .is_none()
        );
        assert!(monitor.observed.is_none());
        let replacement = Lease::new(12, 8).unwrap();
        assert_eq!(monitor.observe(replacement, Some), Some(&12));
        // Failure to open a different lease must not reuse the old identity.
        assert!(monitor.observe(lease, |_| None).is_none());
        assert!(monitor.observed.is_none());
    }

    fn configured() -> ControlBuf {
        let memory = ControlBuf::new();
        let control = memory.control();
        let config = OutputConfig::default();
        control.initialize(42).unwrap();
        control.set_sample_rate(config.sample_rate);
        control.set_layout(config.layout);
        control.set_dac_period_frames(config.dac_period_frames);
        control.set_latency_frames(config.latency_frames);
        control.set_buffer_frames(512);
        memory
    }

    #[test]
    fn admission_uses_local_configuration_not_peer_geometry() {
        let memory = configured();
        let config = OutputConfig::default();
        let expected = client_geometry(memory.control(), config).unwrap();
        assert_eq!(expected.max_frames, 512);
        assert_eq!(expected.input_channels, 12);
        assert_eq!(expected.output_channels, 2);
        memory.control().set_sample_rate(96_000);
        assert!(client_geometry(memory.control(), config).is_none());
        memory.control().set_sample_rate(48_000);
        memory.control().set_buffer_frames(u32::MAX);
        assert!(client_geometry(memory.control(), config).is_none());
    }

    #[test]
    fn admission_does_not_reset_output_or_lease_counters() {
        let memory = configured();
        let control = memory.control();
        let lease = Lease::new(11, 7).unwrap();
        control.acquire_lease(lease).unwrap();
        control.set_dac_generation(99);
        control.set_demand(10_000);
        control.set_loaded_frames(960);
        control.set_completed_frames(480);
        let before = control.snapshot().unwrap();
        assert!(client_geometry(control, OutputConfig::default()).is_some());
        assert_eq!(control.snapshot().unwrap(), before);
        control.set_completed_frames(961);
        control.set_client_running(true);
        control.set_ready(false);
        control.set_client_generation_ack(98);
        let transitioning = control.snapshot().unwrap();
        // DLL loaded and app completed reset independently across the ACK. Ring
        // admission is necessary to drain it, not permission to render a block.
        assert!(client_geometry(control, OutputConfig::default()).is_some());
        assert_eq!(control.snapshot().unwrap(), transitioning);
    }

    #[test]
    fn only_reported_latency_can_change_without_reopening_ring() {
        let original = client_geometry(configured().control(), OutputConfig::default()).unwrap();
        assert!(same_ring_geometry(
            original,
            Geometry {
                latency_bound_frames: 999,
                ..original
            }
        ));
        for changed in [
            Geometry {
                sample_rate: 96_000,
                ..original
            },
            Geometry {
                input_channels: 2,
                ..original
            },
            Geometry {
                max_frames: 64,
                ..original
            },
        ] {
            assert!(!same_ring_geometry(original, changed));
        }
    }

    #[test]
    fn exact_header_geometry_and_bounds_are_required() {
        let memory = configured();
        let geometry = client_geometry(memory.control(), OutputConfig::default()).unwrap();
        let bytes = geometry.total_bytes().unwrap();
        let slot = (geometry.samples_per_slot() * 4).div_ceil(64) * 64;
        let mut header = [0; streamring::HEADER_BYTES / 4];
        header[..12].copy_from_slice(&[
            streamring::MAGIC,
            streamring::LAYOUT_VERSION,
            bytes as u32,
            1,
            12,
            2,
            512,
            48_000,
            0,
            streamring::State::Announced as u32,
            0,
            11,
        ]);
        header[16..19].copy_from_slice(&[512, (512 + slot) as u32, slot as u32]);
        assert!(validate_header(&header, geometry, 11, bytes));
        for index in [0, 1, 2, 3, 4, 5, 6, 7, 8, 11, 16, 17, 18] {
            let mut bad = header;
            bad[index] = bad[index].wrapping_add(1);
            assert!(
                !validate_header(&bad, geometry, 11, bytes),
                "header {index}"
            );
        }
        assert!(!validate_header(&header, geometry, 11, bytes - 1));
        header[9] = streamring::State::Closing as u32;
        assert!(!validate_header(&header, geometry, 11, bytes));
    }
}
