//! Control-thread state for the driverless application-capture entrance.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use ssr_types::app::{
    AppCaptureSettings, AppCaptureView, CaptureApp, CaptureAppView, CaptureLaneView, CaptureState,
    OutputChoice,
};
use ssr_types::{AudioApp, Host};

use crate::routing::normalize_exe;

const POLL_PERIOD: Duration = Duration::from_secs(1);
const RESTART_PERIOD: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ResolveError {
    NotRunning(String),
    Ambiguous { app: String, pids: Vec<u32> },
}

#[derive(Default)]
pub(crate) struct AppCaptureMonitor {
    last_poll: Option<Instant>,
    sessions: Vec<AudioApp>,
    capturing: [Option<u32>; 2],
    waiting: [bool; 2],
    restart_pending: [bool; 2],
    waiting_noted: [bool; 2],
    last_restart: Option<Instant>,
}

impl AppCaptureMonitor {
    pub(crate) fn refresh(&mut self, host: &mut impl Host) {
        self.sessions = deduplicate(host.audio_apps());
    }

    pub(crate) fn tick_due(&mut self, host: &mut impl Host, now: Instant) -> bool {
        if self
            .last_poll
            .is_some_and(|last| now.saturating_duration_since(last) < POLL_PERIOD)
        {
            return false;
        }
        self.last_poll = Some(now);
        self.refresh(host);
        true
    }

    pub(crate) fn sessions(&self) -> &[AudioApp] {
        &self.sessions
    }

    pub(crate) fn resolve(&self, selected: &CaptureApp) -> Result<u32, ResolveError> {
        let Some(exe) = normalize_exe(&selected.exe) else {
            return Err(ResolveError::NotRunning(selected.display_name.clone()));
        };
        let mut pids: Vec<_> = self
            .sessions
            .iter()
            .filter(|app| app.exe == exe && app.pid != std::process::id())
            .map(|app| app.pid)
            .collect();
        pids.sort_unstable();
        pids.dedup();
        if pids.is_empty() {
            return Err(ResolveError::NotRunning(selected.display_name.clone()));
        }
        if pids.len() == 1 {
            return Ok(pids[0]);
        }
        let mut active: Vec<_> = self
            .sessions
            .iter()
            .filter(|app| app.exe == exe && app.active && app.pid != std::process::id())
            .map(|app| app.pid)
            .collect();
        active.sort_unstable();
        active.dedup();
        match active.as_slice() {
            [pid] => Ok(*pid),
            _ => Err(ResolveError::Ambiguous {
                app: selected.display_name.clone(),
                pids,
            }),
        }
    }

    pub(crate) fn set_capturing(&mut self, targets: &[(u16, u32)]) {
        self.capturing = [None, None];
        self.waiting = [false, false];
        self.restart_pending = [false, false];
        self.waiting_noted = [false, false];
        for &(lane, pid) in targets {
            if let Some(slot) = self.capturing.get_mut(usize::from(lane)) {
                *slot = Some(pid);
            }
        }
    }

    pub(crate) fn clear_capturing(&mut self) {
        self.capturing = [None, None];
        self.waiting = [false, false];
        self.restart_pending = [false, false];
        self.waiting_noted = [false, false];
        self.last_restart = None;
    }

    pub(crate) fn capture_ended(&mut self) {
        for lane in 0..2 {
            if self.capturing[lane].is_some() {
                self.waiting[lane] = true;
                self.restart_pending[lane] = true;
            }
        }
    }

    pub(crate) fn captured_pid(&self, lane: usize) -> Option<u32> {
        self.capturing.get(lane).copied().flatten()
    }

    pub(crate) fn restart_pending(&self, lane: usize) -> bool {
        self.restart_pending.get(lane).copied().unwrap_or(false)
    }

    pub(crate) fn restart_allowed(&self, now: Instant) -> bool {
        self.last_restart
            .is_none_or(|last| now.saturating_duration_since(last) >= RESTART_PERIOD)
    }

    pub(crate) fn mark_restarted(&mut self, now: Instant) {
        self.last_restart = Some(now);
    }

    pub(crate) fn mark_waiting(&mut self, lane: usize) -> bool {
        self.waiting[lane] = true;
        self.restart_pending[lane] = true;
        if self.waiting_noted[lane] {
            false
        } else {
            self.waiting_noted[lane] = true;
            true
        }
    }

    pub(crate) fn mark_resumed(&mut self, lane: usize) {
        if !self.restart_pending[lane] {
            self.waiting[lane] = false;
            self.waiting_noted[lane] = false;
        }
    }

    pub(crate) fn view(
        &self,
        settings: &AppCaptureSettings,
        supported: bool,
        min_build: u32,
        discards: Vec<OutputChoice>,
        running: bool,
    ) -> AppCaptureView {
        AppCaptureView {
            supported,
            min_build,
            discard: settings.discard.clone(),
            discards,
            main: settings
                .main
                .as_ref()
                .map(|app| self.lane_view(app, 0, running)),
            music: settings
                .music
                .as_ref()
                .map(|app| self.lane_view(app, 1, running)),
            apps: app_views(&self.sessions),
            exclusive: settings.exclusive,
        }
    }

    fn lane_view(&self, selected: &CaptureApp, lane: usize, running: bool) -> CaptureLaneView {
        let present = self.sessions.iter().any(|app| app.exe == selected.exe);
        let state = if running && self.waiting[lane] {
            CaptureState::Waiting
        } else if running && self.capturing[lane].is_some() {
            CaptureState::Capturing
        } else if present {
            CaptureState::Running
        } else {
            CaptureState::NotRunning
        };
        CaptureLaneView {
            exe: selected.exe.clone(),
            display_name: selected.display_name.clone(),
            state,
        }
    }
}

fn deduplicate(sessions: Vec<AudioApp>) -> Vec<AudioApp> {
    let mut by_process = BTreeMap::<(String, u32), AudioApp>::new();
    for mut app in sessions {
        if app.pid == std::process::id() {
            continue;
        }
        app.exe = normalize_exe(&app.exe).unwrap_or(app.exe);
        by_process
            .entry((app.exe.clone(), app.pid))
            .and_modify(|saved| {
                saved.active |= app.active;
                if saved.display_name.is_empty() {
                    saved.display_name.clone_from(&app.display_name);
                }
            })
            .or_insert(app);
    }
    by_process.into_values().collect()
}

fn app_views(sessions: &[AudioApp]) -> Vec<CaptureAppView> {
    let mut by_exe = BTreeMap::<String, CaptureAppView>::new();
    for app in sessions {
        by_exe
            .entry(app.exe.clone())
            .and_modify(|saved| {
                saved.active |= app.active;
                if saved.display_name.is_empty() {
                    saved.display_name.clone_from(&app.display_name);
                }
            })
            .or_insert_with(|| CaptureAppView {
                exe: app.exe.clone(),
                display_name: if app.display_name.is_empty() {
                    app.exe.clone()
                } else {
                    app.display_name.clone()
                },
                active: app.active,
            });
    }
    let mut apps: Vec<_> = by_exe.into_values().collect();
    apps.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.exe.cmp(&right.exe))
    });
    apps
}
