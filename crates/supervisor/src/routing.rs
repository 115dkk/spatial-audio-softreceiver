//! Per-application entrance policy (ADR 0028).
//!
//! This is control-thread policy only. It polls Windows at most once per second,
//! caches the complete view for snapshots, and remembers successful decisions
//! for the supervisor's lifetime so a user's later Windows change is not
//! overwritten. Process identity is limited to the `(pid, exe)` pair exposed by
//! the host; a PID reused by the same executable cannot be distinguished.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::{Duration, Instant};

use ssr_types::app::{
    AppRule, DEFAULT_APP_RULES, Entrance, RoutedApp, RoutingSettings, RoutingView, RuleView,
    built_in_rule,
};
use ssr_types::{AudioApp, DeviceId, DeviceState, Host, HostError, OutputDevice};

use crate::text::Message;

const POLL_PERIOD: Duration = Duration::from_secs(1);

pub(crate) struct Routing {
    available: bool,
    last_poll: Option<Instant>,
    probed: bool,
    handled: HashSet<(u32, String)>,
    /// Processes for which this supervisor lifetime actually wrote a target.
    moved: HashSet<(u32, String)>,
    apps: Vec<RoutedApp>,
}

impl Routing {
    pub(crate) fn new() -> Self {
        Self {
            available: true,
            last_poll: None,
            probed: false,
            handled: HashSet::new(),
            moved: HashSet::new(),
            apps: Vec::new(),
        }
    }

    pub(crate) fn view(&self, settings: &RoutingSettings) -> RoutingView {
        let rules = effective_rule_map(settings);
        let mut apps = self.apps.clone();
        for app in &mut apps {
            app.rule = normalize_exe(&app.exe)
                .as_ref()
                .and_then(|exe| rules.get(exe))
                .copied();
        }
        let names: BTreeSet<String> = DEFAULT_APP_RULES
            .iter()
            .map(|(exe, _)| (*exe).to_owned())
            .chain(
                settings
                    .rules
                    .iter()
                    .filter_map(|rule| normalize_exe(&rule.exe)),
            )
            .collect();
        RoutingView {
            available: self.available,
            enabled: settings.enabled,
            rules: names
                .into_iter()
                .map(|exe| RuleView {
                    entrance: rules.get(&exe).copied(),
                    built_in: built_in_rule(&exe),
                    exe,
                })
                .filter(|row| row.entrance.is_some() || row.built_in.is_some())
                .collect(),
            apps,
        }
    }

    pub(crate) fn set_rule(
        &mut self,
        settings: &mut RoutingSettings,
        mut rule: AppRule,
    ) -> Result<(), ()> {
        rule.exe = normalize_exe(&rule.exe).ok_or(())?;
        let before = effective_rule_map(settings).get(&rule.exe).copied();
        settings
            .rules
            .retain(|saved| normalize_exe(&saved.exe).as_deref() != Some(rule.exe.as_str()));
        let exe = rule.exe.clone();
        // Keep only what differs from the built-in rule; `None` for an
        // executable without one is the same as no rule.
        if rule.entrance != built_in_rule(&exe) {
            settings.rules.push(rule);
        }
        let after = effective_rule_map(settings).get(&exe).copied();
        if before != after {
            self.clear_exe(&exe);
        }
        Ok(())
    }

    pub(crate) fn remove_rule(
        &mut self,
        settings: &mut RoutingSettings,
        exe: &str,
    ) -> Result<(), ()> {
        let exe = normalize_exe(exe).ok_or(())?;
        let before = effective_rule_map(settings).get(&exe).copied();
        settings
            .rules
            .retain(|saved| normalize_exe(&saved.exe).as_deref() != Some(exe.as_str()));
        let after = effective_rule_map(settings).get(&exe).copied();
        if before != after {
            self.clear_exe(&exe);
        }
        Ok(())
    }

    fn clear_exe(&mut self, exe: &str) {
        self.handled.retain(|(_, handled_exe)| handled_exe != exe);
    }

    pub(crate) fn tick<H: Host>(
        &mut self,
        host: &mut H,
        settings: &RoutingSettings,
        now: Instant,
    ) -> Vec<Message> {
        if !settings.enabled || !self.available {
            return Vec::new();
        }
        if self
            .last_poll
            .is_some_and(|last| now.saturating_duration_since(last) < POLL_PERIOD)
        {
            return Vec::new();
        }
        self.last_poll = Some(now);

        let sessions = host.audio_apps();
        if !self.probed {
            match host.app_entrance(std::process::id()) {
                Ok(_) => self.probed = true,
                Err(HostError::Unsupported) => {
                    self.probed = true;
                    self.available = false;
                    self.apps.clear();
                    return vec![Message::EntranceRoutingUnavailable];
                }
                // A process may exit while Windows answers. Retry this harmless
                // read on a later poll; only Unsupported is a capability verdict.
                Err(_) => {}
            }
        }

        let outputs = host.outputs();
        let entrances = Entrances::from_outputs(&outputs);
        let rules = effective_rule_map(settings);
        let mut attempted = HashSet::new();
        let mut notes = Vec::new();

        for app in &sessions {
            if !app.active {
                continue;
            }
            let exe = normalize_exe(&app.exe).unwrap_or_else(|| app.exe.clone());
            let key = (app.pid, exe.clone());
            if self.handled.contains(&key) {
                continue;
            }
            let rule = rules.get(&exe).copied();
            let target = rule.or_else(|| {
                (entrances.of_device(&app.device) == Some(Entrance::Pass)).then_some(Entrance::Main)
            });
            let Some(target) = target else {
                continue;
            };
            // An unruled process may have several sessions. Do not let an
            // earlier Main session hide a later PASS session that needs repair.
            if !attempted.insert(key.clone()) {
                continue;
            }
            let Some(target_id) = entrances.id(target) else {
                continue;
            };
            match host.app_entrance(app.pid) {
                Ok(current) if current.as_ref() == Some(target_id) => {
                    self.handled.insert(key);
                }
                Ok(_) => match host.set_app_entrance(app.pid, Some(target_id)) {
                    Ok(()) => {
                        self.handled.insert(key.clone());
                        self.moved.insert(key);
                        if rule.is_none() {
                            notes.push(Message::AppEntranceMoved {
                                app: display_name(app),
                            });
                        }
                    }
                    Err(HostError::Unsupported) => {
                        self.available = false;
                        notes.push(Message::EntranceRoutingUnavailable);
                        break;
                    }
                    Err(HostError::Conflict) => {
                        // A concurrent preference edit belongs to the user. Mark
                        // this rule decision handled without claiming a move.
                        self.moved.remove(&key);
                        self.handled.insert(key);
                    }
                    Err(_) => {}
                },
                Err(HostError::Unsupported) => {
                    self.available = false;
                    notes.push(Message::EntranceRoutingUnavailable);
                    break;
                }
                Err(_) => {}
            }
        }

        self.apps = sessions
            .into_iter()
            .map(|app| {
                let exe = normalize_exe(&app.exe).unwrap_or(app.exe);
                let key = (app.pid, exe.clone());
                RoutedApp {
                    rule: rules.get(&exe).copied(),
                    entrance_now: entrances.of_device(&app.device),
                    moved: self.moved.contains(&key),
                    exe,
                    display_name: app.display_name,
                }
            })
            .collect();
        notes
    }
}

fn display_name(app: &AudioApp) -> String {
    if app.display_name.is_empty() {
        app.exe.clone()
    } else {
        app.display_name.clone()
    }
}

struct Entrances {
    main: Option<DeviceId>,
    music: Option<DeviceId>,
    pass: Option<DeviceId>,
}

impl Entrances {
    fn from_outputs(outputs: &[OutputDevice]) -> Self {
        let find = |endpoint| {
            outputs
                .iter()
                .find(|output| {
                    output.endpoint == Some(endpoint) && output.state == DeviceState::Active
                })
                .map(|output| output.id.clone())
        };
        Self {
            main: find(0),
            music: find(1),
            pass: find(2),
        }
    }

    fn id(&self, entrance: Entrance) -> Option<&DeviceId> {
        match entrance {
            Entrance::Main => self.main.as_ref(),
            Entrance::Music => self.music.as_ref(),
            Entrance::Pass => self.pass.as_ref(),
        }
    }

    fn of_device(&self, device: &DeviceId) -> Option<Entrance> {
        [Entrance::Main, Entrance::Music, Entrance::Pass]
            .into_iter()
            .find(|entrance| self.id(*entrance) == Some(device))
    }
}

/// ASCII case is insignificant, but an executable rule is one filename rather
/// than a path. Invalid saved rules are ignored; invalid intents are refused.
pub(crate) fn normalize_exe(exe: &str) -> Option<String> {
    if exe.is_empty() || exe == "." || exe == ".." || exe.contains(['/', '\\', '\0']) {
        return None;
    }
    Some(exe.to_ascii_lowercase())
}

fn effective_rule_map(settings: &RoutingSettings) -> BTreeMap<String, Entrance> {
    let mut rules: BTreeMap<_, _> = DEFAULT_APP_RULES
        .iter()
        .map(|(exe, entrance)| ((*exe).to_owned(), *entrance))
        .collect();
    for rule in &settings.rules {
        if let Some(exe) = normalize_exe(&rule.exe) {
            match rule.entrance {
                Some(entrance) => rules.insert(exe, entrance),
                None => rules.remove(&exe),
            };
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_rules_override_defaults_last_wins_and_invalid_rules_are_ignored() {
        let settings = RoutingSettings {
            enabled: true,
            rules: vec![
                AppRule {
                    exe: "KODI.EXE".into(),
                    entrance: Some(Entrance::Music),
                },
                AppRule {
                    exe: "folder/player.exe".into(),
                    entrance: Some(Entrance::Pass),
                },
                AppRule {
                    exe: "kodi.exe".into(),
                    entrance: Some(Entrance::Main),
                },
                AppRule {
                    exe: "foobar2000.exe".into(),
                    entrance: None,
                },
            ],
        };
        let rules = effective_rule_map(&settings);
        assert_eq!(rules.get("kodi.exe"), Some(&Entrance::Main));
        assert!(!rules.contains_key("folder/player.exe"));
        assert!(!rules.contains_key("foobar2000.exe"));
        assert_eq!(rules.get("mpc-be.exe"), Some(&Entrance::Pass));
    }

    #[test]
    fn a_built_in_rule_can_be_turned_off_and_restored_and_only_differences_are_saved() {
        let mut routing = Routing::new();
        let mut settings = RoutingSettings::default();
        let row = |routing: &Routing, settings: &RoutingSettings, exe: &str| {
            routing
                .view(settings)
                .rules
                .into_iter()
                .find(|row| row.exe == exe)
        };

        routing
            .set_rule(
                &mut settings,
                AppRule {
                    exe: "Kodi.exe".into(),
                    entrance: None,
                },
            )
            .unwrap();
        assert_eq!(
            row(&routing, &settings, "kodi.exe"),
            Some(RuleView {
                exe: "kodi.exe".into(),
                entrance: None,
                built_in: Some(Entrance::Pass),
            })
        );

        routing
            .set_rule(
                &mut settings,
                AppRule {
                    exe: "kodi.exe".into(),
                    entrance: Some(Entrance::Pass),
                },
            )
            .unwrap();
        assert!(settings.rules.is_empty());

        routing
            .set_rule(
                &mut settings,
                AppRule {
                    exe: "player.exe".into(),
                    entrance: Some(Entrance::Music),
                },
            )
            .unwrap();
        routing
            .set_rule(
                &mut settings,
                AppRule {
                    exe: "player.exe".into(),
                    entrance: None,
                },
            )
            .unwrap();
        assert!(settings.rules.is_empty());
        assert_eq!(row(&routing, &settings, "player.exe"), None);
    }
}
