use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use supervisor::guard::{Action, Guard, Observation, Progress, Reason};

use crate::heartbeat;
use crate::marker::{Marker, MarkerReason, MarkerScope};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeout;

pub trait AppProcess {
    /// `Ok(Some(line))` is a line, `Ok(None)` is EOF, and `Err(Timeout)` means
    /// no line arrived before `wait`.
    fn next_line(&mut self, wait: Duration) -> Result<Option<String>, Timeout>;
    fn kill(&mut self);
    /// The process id, which a restore point the app wrote names as its
    /// owner.
    fn id(&self) -> u32;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recovery {
    pub notes: Vec<String>,
    pub scope: MarkerScope,
}

pub trait Platform {
    fn spawn(&mut self, resume: bool) -> Result<Box<dyn AppProcess>, String>;
    /// Gives the roles back from the restore point when there is one and
    /// [`restore_allowed`] accepts it for `app`, the process this guard
    /// launched: `None` when nothing was restored (no restore point, or
    /// another instance's), otherwise the notes.
    fn restore(&mut self, app: u32) -> Option<Recovery>;
    /// The owner of a restore point or an app-output record. Parked app-output
    /// records have no owner and do not start the takeover-silence clock.
    fn recovery_owner(&self) -> Option<u32>;
    fn write_marker(&mut self, marker: &Marker);
    fn now(&self) -> Instant;
}

/// How long an app that holds the roles (its restore point exists) but has
/// not said it is Running may stay silent, counted from the first silent
/// look that found it holding them after its last heartbeat. The control
/// thread takes the roles and then starts the engine in one step, and gives
/// them back in one step when quitting; a heartbeat that does not come for
/// this long means that step hangs. Longer than [`Guard::STALL_AFTER`]
/// because a device takes a while to open.
pub const TAKEOVER_SILENCE: Duration = Duration::from_secs(15);

/// Whether a restore point whose `app_pid` is `owner` (0 when unknown) may
/// be used on behalf of `app`, the process this guard launched. Another
/// process's restore point belongs to a running instance this guard does
/// not own, and is left alone. A guard that launched nothing restores
/// nothing: it cannot tell a dead instance's leftover from a live one's.
pub fn restore_allowed(owner: u32, app: u32) -> bool {
    owner == 0 || owner == app
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Clean,
    Restored(MarkerReason),
}

/// What the loop knows about the current app.
struct Watch {
    guard: Guard,
    /// When the app last said it is Running; `None` until the current app
    /// says so, which is also how a slow start is not a stall.
    running_since: Option<Instant>,
    /// The first silent look, since the last heartbeat, that found the app
    /// holding the roles.
    held_since: Option<Instant>,
    takeover_silence: Duration,
}

/// Drives ADR 0033's pipe protocol and delegates the stall decisions to
/// [`Guard`].
pub fn run(
    platform: &mut impl Platform,
    stall_after: Duration,
    takeover_silence: Duration,
) -> Exit {
    let Ok(mut child) = platform.spawn(false) else {
        // Nothing was launched, so no restore point is this guard's to use.
        return Exit::Restored(MarkerReason::RestartFailed);
    };
    let mut watch = Watch {
        guard: Guard::new(stall_after),
        running_since: None,
        held_since: None,
        takeover_silence,
    };

    loop {
        match child.next_line(Duration::from_secs(1)) {
            Ok(Some(line)) => {
                let Some(heartbeat) = heartbeat::parse(&line) else {
                    if let Some(exit) = silence(platform, &mut child, &mut watch) {
                        return exit;
                    }
                    continue;
                };
                watch.held_since = None;
                if !heartbeat.running {
                    watch.guard.app_idle();
                    watch.running_since = None;
                    continue;
                }
                watch.running_since.get_or_insert_with(|| platform.now());
                let action = watch.guard.observe(
                    platform.now(),
                    Observation {
                        app_alive: true,
                        progress: Some(Progress {
                            output_periods: heartbeat.output_periods,
                            capture_reads: heartbeat.capture_reads,
                        }),
                    },
                );
                if let Some(exit) = act(platform, &mut child, &mut watch, action) {
                    return exit;
                }
            }
            Ok(None) => {
                if let Some(recovery) = platform.restore(child.id()) {
                    platform.write_marker(&marker(
                        recovery.scope,
                        MarkerReason::AppExited,
                        recovery.notes,
                    ));
                    return Exit::Restored(MarkerReason::AppExited);
                }
                return Exit::Clean;
            }
            Err(Timeout) => {
                if let Some(exit) = silence(platform, &mut child, &mut watch) {
                    return exit;
                }
            }
        }
    }
}

/// No heartbeat (or no readable one) arrived. While the app is Running that
/// is an observation for the guard. Before it says so, silence matters only
/// once the app holds the roles: then the step that took them hangs, and
/// [`TAKEOVER_SILENCE`] after the roles were first seen held, the roles go
/// back without a restart, since the app never delivered audio under them.
/// The time the step spent before taking the roles does not count.
fn silence(
    platform: &mut impl Platform,
    child: &mut Box<dyn AppProcess>,
    watch: &mut Watch,
) -> Option<Exit> {
    if watch.running_since.is_some() {
        let action = watch.guard.observe(
            platform.now(),
            Observation {
                app_alive: true,
                progress: None,
            },
        );
        return act(platform, child, watch, action);
    }
    let holds_the_roles = platform
        .recovery_owner()
        .is_some_and(|owner| restore_allowed(owner, child.id()));
    if !holds_the_roles {
        watch.held_since = None;
        return None;
    }
    let held_since = *watch.held_since.get_or_insert_with(|| platform.now());
    if platform.now().saturating_duration_since(held_since) < watch.takeover_silence {
        return None;
    }
    child.kill();
    let recovery = platform.restore(child.id()).unwrap_or_default();
    platform.write_marker(&marker(recovery.scope, MarkerReason::Stuck, recovery.notes));
    Some(Exit::Restored(MarkerReason::Stuck))
}

fn act(
    platform: &mut impl Platform,
    child: &mut Box<dyn AppProcess>,
    watch: &mut Watch,
    action: Action,
) -> Option<Exit> {
    match action {
        Action::Nothing => None,
        Action::RestartApp => {
            let killed = child.id();
            child.kill();
            let recovery = platform.restore(killed).unwrap_or_default();
            platform.write_marker(&marker(
                recovery.scope,
                MarkerReason::StallRestarted,
                recovery.notes.clone(),
            ));
            match platform.spawn(true) {
                Ok(new_child) => {
                    *child = new_child;
                    watch.guard.app_restarted();
                    watch.running_since = None;
                    watch.held_since = None;
                    None
                }
                Err(_) => {
                    // Only the killed app's leftover, if the first restore
                    // left one, is this guard's to use.
                    let recovery = platform.restore(killed).unwrap_or(recovery);
                    platform.write_marker(&marker(
                        recovery.scope,
                        MarkerReason::RestartFailed,
                        recovery.notes,
                    ));
                    Some(Exit::Restored(MarkerReason::RestartFailed))
                }
            }
        }
        Action::RestoreAndNotify { reason } => {
            child.kill();
            let recovery = platform.restore(child.id()).unwrap_or_default();
            let reason = MarkerReason::from(reason);
            platform.write_marker(&marker(recovery.scope, reason, recovery.notes));
            Some(Exit::Restored(reason))
        }
    }
}

impl From<Reason> for MarkerReason {
    fn from(reason: Reason) -> Self {
        match reason {
            Reason::AppExited => Self::AppExited,
            Reason::RestartFailed => Self::RestartFailed,
        }
    }
}

fn marker(scope: MarkerScope, reason: MarkerReason, notes: Vec<String>) -> Marker {
    let at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Marker::new_in(scope, reason, at_unix, notes)
}
