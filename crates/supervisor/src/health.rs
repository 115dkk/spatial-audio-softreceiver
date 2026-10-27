//! Minute-by-minute health history for the console skin (ADR 0041).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ssr_types::app::{HealthHistory, HealthIncident, HealthMinute, HealthPart, MinuteState, Phase};
use ssr_types::{DriverState, EngineFault, EngineStatus};

const MINUTE: Duration = Duration::from_secs(60);
const WINDOW: usize = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PluginHealth {
    Idle,
    Starting,
    Running,
    Fallback,
    Failed,
}

pub(crate) struct Observation<'a> {
    pub phase: &'a Phase,
    pub driver: Option<&'a DriverState>,
    pub engine: Option<&'a EngineStatus>,
    pub plugins: &'a [PluginHealth],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counters {
    underruns: u64,
    output_restarts: u64,
    output_reconnects: u64,
    output_late_resyncs: u64,
    output_cadence_faults: u64,
    dropped_frames: u64,
    lane_underrun_frames: u64,
    decode_refused: u64,
}

impl From<&EngineStatus> for Counters {
    fn from(status: &EngineStatus) -> Self {
        Self {
            underruns: status.underruns,
            output_restarts: status.output_restarts,
            output_reconnects: status.output_reconnects,
            output_late_resyncs: status.output_late_resyncs,
            output_cadence_faults: status.output_cadence_faults,
            dropped_frames: status.dropped_frames,
            lane_underrun_frames: status.lane_underrun_frames,
            decode_refused: status.decode_refused,
        }
    }
}

impl Counters {
    fn increases(self, previous: Option<Self>) -> Self {
        let increase = |current, old| {
            if current >= old {
                current - old
            } else {
                current
            }
        };
        let previous = previous.unwrap_or_default();
        Self {
            underruns: increase(self.underruns, previous.underruns),
            output_restarts: increase(self.output_restarts, previous.output_restarts),
            output_reconnects: increase(self.output_reconnects, previous.output_reconnects),
            output_late_resyncs: increase(self.output_late_resyncs, previous.output_late_resyncs),
            output_cadence_faults: increase(
                self.output_cadence_faults,
                previous.output_cadence_faults,
            ),
            dropped_frames: increase(self.dropped_frames, previous.dropped_frames),
            lane_underrun_frames: increase(
                self.lane_underrun_frames,
                previous.lane_underrun_frames,
            ),
            decode_refused: increase(self.decode_refused, previous.decode_refused),
        }
    }

    fn engine_degraded(self) -> bool {
        self.underruns != 0
            || self.dropped_frames != 0
            || self.lane_underrun_frames != 0
            || self.decode_refused != 0
    }

    fn output_degraded(self) -> bool {
        self.output_restarts != 0
            || self.output_reconnects != 0
            || self.output_late_resyncs != 0
            || self.output_cadence_faults != 0
    }
}

pub(crate) struct HealthLog {
    started: Option<Instant>,
    minute: u64,
    minutes: VecDeque<HealthMinute>,
    previous: Option<Counters>,
}

impl Default for HealthLog {
    fn default() -> Self {
        Self {
            started: None,
            minute: 0,
            minutes: VecDeque::with_capacity(WINDOW),
            previous: None,
        }
    }
}

impl HealthLog {
    /// Observes one supervisor tick. The return value says a snapshot should be
    /// sent at once: a minute began, or the current minute became worse.
    pub(crate) fn observe(&mut self, now: Instant, observation: Observation<'_>) -> bool {
        let started = *self.started.get_or_insert(now);
        let minute = now.saturating_duration_since(started).as_secs() / MINUTE.as_secs();
        let current = observation.engine.map(Counters::from);
        let increases = current.map(|counters| counters.increases(self.previous));
        self.previous = current;
        let observed = observed_minute(observation, increases.unwrap_or_default());

        if self.minutes.is_empty() {
            self.minute = minute;
            self.push(observed);
            return true;
        }

        if minute > self.minute {
            for _ in (self.minute + 1)..minute {
                self.push(no_record());
            }
            self.push(observed);
            self.minute = minute;
            return true;
        }

        let current = self
            .minutes
            .back_mut()
            .expect("a nonempty health log has a current minute");
        let worsened = worsen(&mut current.driver, observed.driver)
            | worsen(&mut current.engine, observed.engine)
            | worsen(&mut current.output, observed.output)
            | worsen(&mut current.plugins, observed.plugins);
        current.underruns = current.underruns.saturating_add(observed.underruns);
        current.output_recoveries = current
            .output_recoveries
            .saturating_add(observed.output_recoveries);
        worsened || observed.underruns != 0 || observed.output_recoveries != 0
    }

    pub(crate) fn history(&self) -> HealthHistory {
        let minutes: Vec<_> = self.minutes.iter().cloned().collect();
        let incidents = incidents(&minutes);
        HealthHistory { minutes, incidents }
    }

    fn push(&mut self, minute: HealthMinute) {
        if self.minutes.len() == WINDOW {
            self.minutes.pop_front();
        }
        self.minutes.push_back(minute);
    }
}

fn observed_minute(observation: Observation<'_>, increases: Counters) -> HealthMinute {
    let running = *observation.phase == Phase::Running;
    let faulted = *observation.phase == Phase::Faulted;
    let fault = observation.engine.and_then(|status| status.fault.as_ref());

    let driver = match observation.driver {
        None => MinuteState::Idle,
        Some(DriverState::Installed {
            problem_code: 0, ..
        }) if !matches!(fault, Some(EngineFault::CaptureLost(_))) => MinuteState::Good,
        Some(_) => MinuteState::Down,
    };

    let engine = if faulted || running && fault.is_some() {
        MinuteState::Down
    } else if !running {
        MinuteState::Idle
    } else if increases.engine_degraded() {
        MinuteState::Degraded
    } else {
        MinuteState::Good
    };

    let output = if faulted || matches!(fault, Some(EngineFault::OutputLost(_))) {
        MinuteState::Down
    } else if !running {
        MinuteState::Idle
    } else if increases.output_degraded()
        || observation
            .engine
            .is_some_and(|status| status.output_fell_back)
    {
        MinuteState::Degraded
    } else {
        MinuteState::Good
    };

    let plugins = if observation.plugins.contains(&PluginHealth::Failed) {
        MinuteState::Down
    } else if observation.plugins.contains(&PluginHealth::Fallback) {
        MinuteState::Degraded
    } else if observation
        .plugins
        .iter()
        .any(|state| matches!(state, PluginHealth::Starting | PluginHealth::Running))
    {
        MinuteState::Good
    } else {
        MinuteState::Idle
    };

    HealthMinute {
        driver,
        engine,
        output,
        plugins,
        underruns: to_u32(increases.underruns),
        output_recoveries: to_u32(
            increases
                .output_restarts
                .saturating_add(increases.output_reconnects),
        ),
    }
}

fn no_record() -> HealthMinute {
    HealthMinute {
        driver: MinuteState::NoRecord,
        engine: MinuteState::NoRecord,
        output: MinuteState::NoRecord,
        plugins: MinuteState::NoRecord,
        underruns: 0,
        output_recoveries: 0,
    }
}

fn to_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn rank(state: MinuteState) -> u8 {
    match state {
        MinuteState::NoRecord => 0,
        MinuteState::Idle => 1,
        MinuteState::Good => 2,
        MinuteState::Degraded => 3,
        MinuteState::Down => 4,
    }
}

fn worsen(current: &mut MinuteState, observed: MinuteState) -> bool {
    if rank(observed) > rank(*current) {
        *current = observed;
        true
    } else {
        false
    }
}

fn incidents(minutes: &[HealthMinute]) -> Vec<HealthIncident> {
    let mut incidents = Vec::new();
    for part in [
        HealthPart::Driver,
        HealthPart::Engine,
        HealthPart::Output,
        HealthPart::Plugins,
    ] {
        let mut first = None;
        let mut worst = MinuteState::NoRecord;
        let mut underruns = 0_u32;
        let mut output_recoveries = 0_u32;
        for (index, minute) in minutes.iter().enumerate() {
            let state = part_state(minute, part);
            if matches!(state, MinuteState::Degraded | MinuteState::Down) {
                first.get_or_insert(index);
                if rank(state) > rank(worst) {
                    worst = state;
                }
                match part {
                    HealthPart::Engine => {
                        underruns = underruns.saturating_add(minute.underruns);
                    }
                    HealthPart::Output => {
                        output_recoveries =
                            output_recoveries.saturating_add(minute.output_recoveries);
                    }
                    HealthPart::Driver | HealthPart::Plugins => {}
                }
            } else if let Some(first) = first.take() {
                incidents.push(HealthIncident {
                    part,
                    worst,
                    first: first as u32,
                    last: (index - 1) as u32,
                    open: false,
                    underruns,
                    output_recoveries,
                });
                worst = MinuteState::NoRecord;
                underruns = 0;
                output_recoveries = 0;
            }
        }
        if let Some(first) = first {
            incidents.push(HealthIncident {
                part,
                worst,
                first: first as u32,
                last: minutes.len().saturating_sub(1) as u32,
                open: true,
                underruns,
                output_recoveries,
            });
        }
    }
    incidents.sort_by_key(|incident| incident.first);
    incidents
}

fn part_state(minute: &HealthMinute, part: HealthPart) -> MinuteState {
    match part {
        HealthPart::Driver => minute.driver,
        HealthPart::Engine => minute.engine,
        HealthPart::Output => minute.output,
        HealthPart::Plugins => minute.plugins,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssr_types::ports::PolicyState;

    fn installed() -> DriverState {
        DriverState::Installed {
            problem_code: 0,
            service_running: true,
            exclusive_policy: PolicyState::Applied,
        }
    }

    fn observe(
        log: &mut HealthLog,
        at: Instant,
        phase: &Phase,
        driver: Option<&DriverState>,
        engine: Option<&EngineStatus>,
        plugins: &[PluginHealth],
    ) -> bool {
        log.observe(
            at,
            Observation {
                phase,
                driver,
                engine,
                plugins,
            },
        )
    }

    #[test]
    fn minute_rollover_keeps_oldest_first() {
        let start = Instant::now();
        let mut log = HealthLog::default();
        let driver = installed();
        let status = EngineStatus::default();
        assert!(observe(
            &mut log,
            start,
            &Phase::Running,
            Some(&driver),
            Some(&status),
            &[]
        ));
        assert!(observe(
            &mut log,
            start + MINUTE,
            &Phase::Ready,
            Some(&driver),
            None,
            &[]
        ));
        let history = log.history();
        assert_eq!(history.minutes.len(), 2);
        assert_eq!(history.minutes[0].engine, MinuteState::Good);
        assert_eq!(history.minutes[1].engine, MinuteState::Idle);
    }

    #[test]
    fn worst_state_wins_within_a_minute() {
        let start = Instant::now();
        let mut log = HealthLog::default();
        let driver = installed();
        let good = EngineStatus::default();
        let mut degraded = good.clone();
        degraded.underruns = 1;
        let mut down = degraded.clone();
        down.fault = Some(EngineFault::OutputLost("gone".into()));
        observe(
            &mut log,
            start,
            &Phase::Running,
            Some(&driver),
            Some(&good),
            &[],
        );
        assert!(observe(
            &mut log,
            start + Duration::from_secs(1),
            &Phase::Running,
            Some(&driver),
            Some(&degraded),
            &[],
        ));
        assert!(observe(
            &mut log,
            start + Duration::from_secs(2),
            &Phase::Running,
            Some(&driver),
            Some(&down),
            &[],
        ));
        assert!(!observe(
            &mut log,
            start + Duration::from_secs(3),
            &Phase::Running,
            Some(&driver),
            Some(&good),
            &[],
        ));
        let minute = &log.history().minutes[0];
        assert_eq!(minute.engine, MinuteState::Down);
        assert_eq!(minute.output, MinuteState::Down);
    }

    #[test]
    fn five_skipped_minutes_are_no_record() {
        let start = Instant::now();
        let mut log = HealthLog::default();
        observe(&mut log, start, &Phase::Ready, None, None, &[]);
        observe(&mut log, start + MINUTE * 6, &Phase::Ready, None, None, &[]);
        let history = log.history();
        assert_eq!(history.minutes.len(), 7);
        assert!(
            history.minutes[1..6]
                .iter()
                .all(|minute| minute.driver == MinuteState::NoRecord)
        );
    }

    #[test]
    fn window_caps_at_ninety_and_drops_oldest() {
        let start = Instant::now();
        let mut log = HealthLog::default();
        let driver = installed();
        observe(&mut log, start, &Phase::Ready, None, None, &[]);
        for minute in 1..=90 {
            observe(
                &mut log,
                start + MINUTE * minute,
                &Phase::Ready,
                Some(&driver),
                None,
                &[],
            );
        }
        let history = log.history();
        assert_eq!(history.minutes.len(), 90);
        assert_eq!(history.minutes[0].driver, MinuteState::Good);
    }

    #[test]
    fn counter_reset_counts_the_new_session_value() {
        let start = Instant::now();
        let mut log = HealthLog::default();
        let driver = installed();
        let mut old = EngineStatus {
            underruns: 10,
            output_restarts: 7,
            output_reconnects: 4,
            ..EngineStatus::default()
        };
        observe(
            &mut log,
            start,
            &Phase::Running,
            Some(&driver),
            Some(&old),
            &[],
        );
        old.underruns = 2;
        old.output_restarts = 1;
        old.output_reconnects = 3;
        observe(
            &mut log,
            start + MINUTE,
            &Phase::Running,
            Some(&driver),
            Some(&old),
            &[],
        );
        let minute = &log.history().minutes[1];
        assert_eq!(minute.underruns, 2);
        assert_eq!(minute.output_recoveries, 4);
    }

    #[test]
    fn incidents_close_and_clamp_to_the_retained_start() {
        let start = Instant::now();
        let mut log = HealthLog::default();
        let driver = installed();
        let mut status = EngineStatus::default();
        observe(
            &mut log,
            start,
            &Phase::Running,
            Some(&driver),
            Some(&status),
            &[],
        );
        for minute in 1..=91 {
            status.underruns += 1;
            observe(
                &mut log,
                start + MINUTE * minute,
                &Phase::Running,
                Some(&driver),
                Some(&status),
                &[],
            );
        }
        let open = log.history();
        assert_eq!(open.incidents[0].part, HealthPart::Engine);
        assert_eq!(open.incidents[0].first, 0);
        assert!(open.incidents[0].open);

        observe(
            &mut log,
            start + MINUTE * 92,
            &Phase::Running,
            Some(&driver),
            Some(&status),
            &[],
        );
        let closed = log.history();
        assert_eq!(closed.incidents[0].first, 0);
        assert_eq!(closed.incidents[0].last, 88);
        assert!(!closed.incidents[0].open);
    }

    #[test]
    fn driver_rule_marks_missing_problem_and_capture_fault() {
        let start = Instant::now();
        let mut log = HealthLog::default();
        observe(&mut log, start, &Phase::Ready, None, None, &[]);
        let missing = DriverState::NotInstalled;
        observe(
            &mut log,
            start + Duration::from_secs(1),
            &Phase::Ready,
            Some(&missing),
            None,
            &[],
        );
        let driver = installed();
        let status = EngineStatus {
            fault: Some(EngineFault::CaptureLost("gone".into())),
            ..EngineStatus::default()
        };
        observe(
            &mut log,
            start + MINUTE,
            &Phase::Running,
            Some(&driver),
            Some(&status),
            &[],
        );
        let history = log.history();
        assert_eq!(history.minutes[0].driver, MinuteState::Down);
        assert_eq!(history.minutes[1].driver, MinuteState::Down);
    }

    #[test]
    fn engine_rule_distinguishes_idle_degraded_and_faulted() {
        let start = Instant::now();
        let mut log = HealthLog::default();
        let driver = installed();
        observe(&mut log, start, &Phase::Ready, Some(&driver), None, &[]);
        let status = EngineStatus {
            dropped_frames: 1,
            ..EngineStatus::default()
        };
        observe(
            &mut log,
            start + MINUTE,
            &Phase::Running,
            Some(&driver),
            Some(&status),
            &[],
        );
        observe(
            &mut log,
            start + MINUTE * 2,
            &Phase::Faulted,
            Some(&driver),
            None,
            &[],
        );
        let history = log.history();
        assert_eq!(history.minutes[0].engine, MinuteState::Idle);
        assert_eq!(history.minutes[1].engine, MinuteState::Degraded);
        assert_eq!(history.minutes[2].engine, MinuteState::Down);
    }

    #[test]
    fn output_rule_distinguishes_recovery_fallback_and_loss() {
        let start = Instant::now();
        let mut log = HealthLog::default();
        let driver = installed();
        let recovered = EngineStatus {
            output_restarts: 1,
            ..EngineStatus::default()
        };
        observe(
            &mut log,
            start,
            &Phase::Running,
            Some(&driver),
            Some(&recovered),
            &[],
        );
        let fallback = EngineStatus {
            output_fell_back: true,
            ..recovered.clone()
        };
        observe(
            &mut log,
            start + MINUTE,
            &Phase::Running,
            Some(&driver),
            Some(&fallback),
            &[],
        );
        let lost = EngineStatus {
            fault: Some(EngineFault::OutputLost("gone".into())),
            ..fallback
        };
        observe(
            &mut log,
            start + MINUTE * 2,
            &Phase::Running,
            Some(&driver),
            Some(&lost),
            &[],
        );
        let history = log.history();
        assert_eq!(history.minutes[0].output, MinuteState::Degraded);
        assert_eq!(history.minutes[1].output, MinuteState::Degraded);
        assert_eq!(history.minutes[2].output, MinuteState::Down);
        let incident = history
            .incidents
            .iter()
            .find(|incident| incident.part == HealthPart::Output)
            .unwrap();
        assert_eq!(incident.output_recoveries, 1);
        assert_eq!(incident.underruns, 0);
    }

    #[test]
    fn plugin_rule_uses_the_worst_endpoint() {
        let start = Instant::now();
        let mut log = HealthLog::default();
        observe(&mut log, start, &Phase::Ready, None, None, &[]);
        observe(
            &mut log,
            start + MINUTE,
            &Phase::Ready,
            None,
            None,
            &[PluginHealth::Starting, PluginHealth::Running],
        );
        observe(
            &mut log,
            start + MINUTE * 2,
            &Phase::Ready,
            None,
            None,
            &[PluginHealth::Running, PluginHealth::Fallback],
        );
        observe(
            &mut log,
            start + MINUTE * 3,
            &Phase::Ready,
            None,
            None,
            &[PluginHealth::Fallback, PluginHealth::Failed],
        );
        let history = log.history();
        assert_eq!(history.minutes[0].plugins, MinuteState::Idle);
        assert_eq!(history.minutes[1].plugins, MinuteState::Good);
        assert_eq!(history.minutes[2].plugins, MinuteState::Degraded);
        assert_eq!(history.minutes[3].plugins, MinuteState::Down);
    }
}
