//! Position unwrapping never invents progress from wall time. If polling missed
//! an entire ring, its wrap count is ambiguous and the stream must reopen.
#[derive(Debug, Default)]
pub(super) struct RingPosition {
    last: u64,
    total: u64,
    last_qpc: Option<u64>,
    accuracy_bytes: u64,
}
impl RingPosition {
    pub fn with_accuracy(accuracy_bytes: u64) -> Self {
        Self {
            accuracy_bytes,
            ..Self::default()
        }
    }

    pub fn update(
        &mut self,
        position: u64,
        ring_bytes: u64,
        now: u64,
        frequency: u64,
        bytes_per_second: u64,
    ) -> Option<u64> {
        if ring_bytes == 0
            || position >= ring_bytes
            || frequency == 0
            || bytes_per_second == 0
            || self.accuracy_bytes >= ring_bytes
        {
            return None;
        }
        if let Some(last) = self.last_qpc {
            let elapsed = now.checked_sub(last)?;
            // A coarse register can hide progress at both observations. Use a
            // conservative two-reading error bound, never invent a wrap count.
            // https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ksmedia/ns-ksmedia-ksrtaudio_hwregister
            if u128::from(elapsed) * u128::from(bytes_per_second)
                + 2 * u128::from(self.accuracy_bytes) * u128::from(frequency)
                >= u128::from(ring_bytes) * u128::from(frequency)
            {
                return None;
            }
        }
        let delta = if position >= self.last {
            position - self.last
        } else {
            ring_bytes - self.last + position
        };
        self.total = self.total.checked_add(delta)?;
        self.last = position;
        self.last_qpc = Some(now);
        Some(self.total)
    }
}
/// A write is safe only when its entire span is ahead of both the hardware
/// guard and the driver's reserved write cursor, without lapping the hardware.
#[derive(Clone, Copy, Debug)]
pub(super) struct WaveRtWrite {
    pub ring_bytes: u64,
    pub period_bytes: u64,
    pub frame_bytes: u64,
    pub guard_bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct WritePlan {
    pub write_bytes: u64,
    pub ready: bool,
    pub underrun: bool,
}

impl WaveRtWrite {
    /// Even a byte-granular hardware cursor must leave room for a full period
    /// after strict frame alignment. Dynamic driver reservations may still wait.
    pub fn guard_fits(self) -> bool {
        self.frame_bytes != 0
            && self
                .guard_bytes
                .checked_add(self.frame_bytes)
                .and_then(|lead| lead.checked_add(self.period_bytes))
                .is_some_and(|needed| needed <= self.ring_bytes)
    }

    pub fn plan(
        self,
        write_bytes: u64,
        consumed: u64,
        reserved_write: Option<u64>,
    ) -> Option<WritePlan> {
        let Self {
            ring_bytes,
            period_bytes,
            frame_bytes,
            guard_bytes,
        } = self;
        if frame_bytes == 0
            || ring_bytes == 0
            || period_bytes == 0
            || !ring_bytes.is_multiple_of(frame_bytes)
            || !period_bytes.is_multiple_of(frame_bytes)
            || !write_bytes.is_multiple_of(frame_bytes)
            || period_bytes > ring_bytes
            || guard_bytes >= ring_bytes
        {
            return None;
        }
        let end = consumed.checked_add(ring_bytes)?;
        if write_bytes > end {
            return None;
        }
        let mut protected = consumed.checked_add(guard_bytes)?;
        if let Some(reserved) = reserved_write {
            if reserved >= ring_bytes {
                return None;
            }
            let play = consumed % ring_bytes;
            let ahead = if reserved >= play {
                reserved - play
            } else {
                ring_bytes - play + reserved
            };
            protected = protected.max(consumed.checked_add(ahead)?);
        }
        // Both cursors can point at a byte still in use. Keep the first whole
        // frame strictly beyond them, including when the guard is zero.
        let safe = protected
            .checked_div(frame_bytes)?
            .checked_add(1)?
            .checked_mul(frame_bytes)?;
        let underrun = write_bytes < safe;
        let write_bytes = write_bytes.max(safe);
        let ready = write_bytes.checked_add(period_bytes)? <= end
            && reserved_write.is_none_or(|reserved| {
                super::selection::writable(
                    consumed % ring_bytes,
                    reserved,
                    write_bytes % ring_bytes,
                    period_bytes,
                    ring_bytes,
                )
            });
        Some(WritePlan {
            write_bytes,
            ready,
            underrun,
        })
    }
}

/// A known underrun changes where the next block will play, not the hardware
/// clock. Re-anchor even a small gap instead of smoothing it over four periods,
/// while keeping the last returned timestamp as a monotonic lower bound.
#[derive(Debug, Default)]
pub(super) struct PlaybackTiming {
    clock: crate::playback_clock::PlaybackClock,
    last_qpc: Option<u64>,
}
impl PlaybackTiming {
    #[allow(clippy::too_many_arguments)]
    pub fn next(
        &mut self,
        first_frame: u64,
        reading: Option<crate::playback_clock::ClockReading>,
        started: u64,
        rate: u32,
        frequency: u64,
        period_frames: usize,
        resynced: bool,
    ) -> u64 {
        if resynced {
            self.clock = crate::playback_clock::PlaybackClock::default();
        }
        let next = self.clock.next(
            first_frame,
            reading,
            started,
            rate,
            u64::from(rate),
            frequency,
            period_frames,
        );
        let next = self
            .last_qpc
            .map_or(next, |last| next.max(last.saturating_add(1)));
        self.last_qpc = Some(next);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const WRITER: WaveRtWrite = WaveRtWrite {
        ring_bytes: 400,
        period_bytes: 200,
        frame_bytes: 4,
        guard_bytes: 12,
    };

    #[test]
    fn ks_late_write_realigns_beyond_guard_and_reserved_cursor() {
        assert_eq!(
            WRITER.plan(400, 420, None),
            Some(WritePlan {
                write_bytes: 436,
                ready: true,
                underrun: true,
            })
        );
        // The driver can reserve farther ahead than the fixed FIFO guard.
        assert_eq!(
            WRITER.plan(400, 396, Some(25)),
            Some(WritePlan {
                write_bytes: 428,
                ready: true,
                underrun: true,
            })
        );
        // Byte positions must not be rounded down before guard calculation.
        assert_eq!(WRITER.plan(400, 391, None).unwrap().write_bytes, 404);
        assert!(WRITER.plan(400, 388, None).unwrap().underrun);
        assert!(!WRITER.plan(400, 387, None).unwrap().underrun);
        let zero_guard = WaveRtWrite {
            guard_bytes: 0,
            ..WRITER
        };
        assert_eq!(zero_guard.plan(400, 400, None).unwrap().write_bytes, 404);
    }

    #[test]
    fn ks_write_waits_for_whole_span_and_rejects_invalid_positions() {
        let early = WRITER.plan(400, 199, None).unwrap();
        assert!(!early.ready);
        assert!(!early.underrun);
        assert!(WRITER.plan(400, 200, None).unwrap().ready);
        // A safe start alone is insufficient: the period would lap play.
        let reserved = WRITER.plan(400, 420, Some(240)).unwrap();
        assert!(reserved.underrun);
        assert!(!reserved.ready);
        assert!(WRITER.plan(400, 440, Some(44)).unwrap().ready);
        assert_eq!(WRITER.plan(400, 420, Some(400)), None);
        assert_eq!(WRITER.plan(804, 400, None), None);
        assert_eq!(WRITER.plan(401, 400, None), None);
        assert_eq!(WRITER.plan(u64::MAX - 3, u64::MAX - 3, None), None);
    }

    #[test]
    fn ks_guard_admission_leaves_a_period_for_every_byte_alignment() {
        for guard_bytes in 0..200 {
            let writer = WaveRtWrite {
                guard_bytes,
                ..WRITER
            };
            assert_eq!(writer.guard_fits(), guard_bytes <= 196);
            if writer.guard_fits() {
                for consumed in 400..404 {
                    assert!(writer.plan(400, consumed, None).unwrap().ready);
                }
            }
        }
        assert!(
            !WaveRtWrite {
                frame_bytes: 0,
                ..WRITER
            }
            .guard_fits()
        );
        assert!(
            !WaveRtWrite {
                guard_bytes: u64::MAX,
                ..WRITER
            }
            .guard_fits()
        );
    }

    #[test]
    fn ks_resync_planning_and_clock_do_not_allocate() {
        use crate::playback_clock::ClockReading;
        let mut clock = PlaybackTiming::default();
        assert_no_alloc::assert_no_alloc(|| {
            for consumed in (400..800).step_by(4) {
                let plan = WRITER.plan(consumed, consumed, None).unwrap();
                assert!(plan.underrun && plan.ready);
                clock.next(
                    plan.write_bytes / 4,
                    Some(ClockReading {
                        position: consumed / 4,
                        qpc_100ns: consumed * 2500,
                    }),
                    0,
                    1000,
                    10_000_000,
                    50,
                    true,
                );
            }
        });
    }

    #[test]
    fn ks_write_plans_never_touch_protected_or_lap_play() {
        let writer = WaveRtWrite {
            ring_bytes: 40,
            period_bytes: 20,
            frame_bytes: 4,
            guard_bytes: 3,
        };
        for consumed in 0..120 {
            for write in (0..=consumed + 40).step_by(4) {
                for reserved in 0..40 {
                    let plan = writer.plan(write, consumed, Some(reserved)).unwrap();
                    if plan.ready {
                        let distance = (reserved + 40 - consumed % 40) % 40;
                        assert!(plan.write_bytes > consumed + 3);
                        assert!(plan.write_bytes > consumed + distance);
                        assert!(plan.write_bytes + 20 <= consumed + 40);
                        assert!(plan.write_bytes >= write);
                        assert_eq!(plan.write_bytes % 4, 0);
                        assert!(super::super::selection::writable(
                            consumed % 40,
                            reserved,
                            plan.write_bytes % 40,
                            20,
                            40,
                        ));
                    }
                }
            }
        }
    }

    #[test]
    fn ks_playback_timestamps_and_played_frames_survive_resync_wrap_and_fill_preemption() {
        use crate::playback_clock::{ClockReading, scale};
        for frequency in [10_000_000, 24_000_000] {
            let rate = 1000;
            let mut position = RingPosition::default();
            let mut clock = PlaybackTiming::default();
            let mut write = 400;
            let mut last_stamp = 0;
            let mut last_played = 0;
            let mut underruns = 0;
            // Absolute hardware bytes, before/after fill. All observations are
            // less than a ring apart; the second and fourth fill are preempted.
            let rows = [
                (200, 208),
                (604, 620),
                (820, 820),
                (1032, 1244),
                (1444, 1444),
            ];
            position.update(0, 400, 0, frequency, 4000).unwrap();
            for (before, after) in rows {
                let mut resynced = false;
                let mut fresh = 0;
                for absolute in [before, after] {
                    let now = scale(absolute, frequency, 4000);
                    fresh = position
                        .update(absolute % 400, 400, now, frequency, 4000)
                        .unwrap();
                    assert_eq!(fresh, absolute);
                    let plan = WRITER.plan(write, fresh, None).unwrap();
                    assert!(plan.ready);
                    write = plan.write_bytes;
                    resynced |= plan.underrun;
                }
                underruns += u64::from(resynced);
                let played = fresh / 4;
                assert!(played >= last_played);
                let stamp = clock.next(
                    write / 4,
                    Some(ClockReading {
                        position: played,
                        qpc_100ns: scale(after, 10_000_000, 4000),
                    }),
                    0,
                    rate,
                    frequency,
                    50,
                    resynced,
                );
                assert!(stamp > last_stamp);
                // Hardware-correlated time, including the realigned queue gap.
                assert_eq!(stamp, scale(write / 4, frequency, u64::from(rate)));
                last_stamp = stamp;
                last_played = played;
                write += 200;
            }
            assert_eq!(underruns, 2);
        }
    }

    #[test]
    fn ks_clock_reanchors_small_resync_without_losing_monotonicity_or_drifting() {
        use crate::playback_clock::ClockReading;
        let mut clock = PlaybackTiming::default();
        let mut last = 0;
        for period in 0..1000 {
            let correlation = if period == 1 {
                0
            } else {
                1_000_000 + period * 100_000
            };
            let stamp = clock.next(
                period * 480 + 480,
                Some(ClockReading {
                    position: period * 480,
                    qpc_100ns: correlation,
                }),
                0,
                48_000,
                10_000_000,
                480,
                period <= 1,
            );
            assert!(stamp > last);
            if period > 128 {
                assert!(stamp.abs_diff(correlation + 100_000) < 8);
            }
            last = stamp;
        }
    }

    #[test]
    fn ks_position_unwraps_hardware_not_elapsed_frames() {
        let mut p = RingPosition::default();
        assert_eq!(p.update(80, 100, 0, 1000, 1000), Some(80));
        assert_eq!(p.update(10, 100, 30, 1000, 1000), Some(110));
        assert_eq!(p.update(10, 100, 60, 1000, 1000), Some(110));
    }
    #[test]
    fn ks_coarse_position_rejects_a_hidden_wrap_before_nominal_ring_time() {
        let mut p = RingPosition::with_accuracy(16);
        assert_eq!(p.update(0, 400, 0, 1000, 4000), Some(0));
        // Both register reads can be zero despite a full wrap: 98 ms plus
        // register uncertainty exceeds the 100 ms ring's unambiguous window.
        assert_eq!(p.update(0, 400, 98, 1000, 4000), None);
        assert_eq!(p.update(320, 400, 80, 1000, 4000), Some(320));
        assert_eq!(p.update(0, 400, 100, 1000, 4000), Some(400));
    }

    #[test]
    fn ks_position_rejects_ambiguous_wrap_and_invalid_bounds() {
        let mut p = RingPosition::default();
        assert_eq!(p.update(0, 100, 0, 1000, 1000), Some(0));
        assert_eq!(p.update(1, 100, 100, 1000, 1000), None);
        assert_eq!(p.update(100, 100, 1, 1000, 1000), None);
        assert_eq!(p.update(0, 0, 1, 1000, 1000), None);
    }
}
