//! Recovery record for applications temporarily sent to the discard output.

use serde::{Deserialize, Serialize};
use ssr_types::{AppRoleOutputs, DeviceId, Host};

use crate::text::Message;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppOutputs {
    pub app_pid: Option<u32>,
    pub apps: Vec<SentApp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentApp {
    pub exe: String,
    pub path: String,
    pub pid: u32,
    pub discard: DeviceId,
    pub previous: AppRoleOutputs,
}

#[derive(Serialize, Deserialize)]
struct AppOutputsFile {
    version: u32,
    app_pid: Option<u32>,
    apps: Vec<SentApp>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AppOutputsError {
    #[error("the app-output record is not JSON: {0}")]
    Json(String),
    #[error("the app-output record has version {0}, this build reads version 1")]
    Version(u32),
}

impl AppOutputs {
    pub const VERSION: u32 = 1;

    pub fn to_json(&self) -> String {
        serde_json::to_string(&AppOutputsFile {
            version: Self::VERSION,
            app_pid: self.app_pid,
            apps: self.apps.clone(),
        })
        .expect("app-output recovery fields serialize")
    }

    pub fn from_json(text: &str) -> Result<Self, AppOutputsError> {
        let file: AppOutputsFile =
            serde_json::from_str(text).map_err(|error| AppOutputsError::Json(error.to_string()))?;
        if file.version != Self::VERSION {
            return Err(AppOutputsError::Version(file.version));
        }
        Ok(Self {
            app_pid: file.app_pid,
            apps: file.apps,
        })
    }
}

/// Reverts every entry that can be handled now and removes finished entries.
/// An application that is not running, or whose policy call fails, stays in the
/// record so a later tick can retry it.
pub fn undo(record: &mut AppOutputs, host: &mut impl Host) -> Vec<Message> {
    undo_matching(record, host, |_| true)
}

/// Reverts entries other than the processes used by the active capture.
pub(crate) fn undo_except_pids(
    record: &mut AppOutputs,
    host: &mut impl Host,
    excluded: &[u32],
) -> Vec<Message> {
    undo_matching(record, host, |sent| !excluded.contains(&sent.pid))
}

/// Reverts the pending entry for one exact executable image path.
pub(crate) fn undo_path(record: &mut AppOutputs, host: &mut impl Host, path: &str) -> Vec<Message> {
    undo_matching(record, host, |sent| sent.path == path)
}

fn undo_matching(
    record: &mut AppOutputs,
    host: &mut impl Host,
    selected: impl Fn(&SentApp) -> bool,
) -> Vec<Message> {
    let sessions = host.audio_apps();
    let mut notes = Vec::new();
    record.apps.retain(|sent| {
        if !selected(sent) {
            return true;
        }
        let pid = sessions
            .iter()
            .find(|app| app.pid == sent.pid && app.path == sent.path)
            .or_else(|| sessions.iter().find(|app| app.path == sent.path))
            .map(|app| app.pid);
        let Some(pid) = pid else {
            notes.push(Message::AppOutputUndoPending {
                app: sent.exe.clone(),
            });
            return true;
        };
        match host.app_entrance_roles(pid) {
            Ok(current) => {
                let mut restored = current.clone();
                let mut changed = false;
                if current.console.as_ref() == Some(&sent.discard) {
                    restored.console.clone_from(&sent.previous.console);
                    changed = true;
                }
                if current.multimedia.as_ref() == Some(&sent.discard) {
                    restored.multimedia.clone_from(&sent.previous.multimedia);
                    changed = true;
                }
                if !changed {
                    return false;
                }
                match host.set_app_entrance_roles(pid, &restored) {
                    Ok(()) => {
                        notes.push(Message::AppOutputRestored {
                            app: sent.exe.clone(),
                        });
                        false
                    }
                    Err(error) => {
                        eprintln!("app-output undo failed for {}: {error}", sent.exe);
                        notes.push(Message::AppOutputUndoFailed {
                            app: sent.exe.clone(),
                        });
                        true
                    }
                }
            }
            Err(error) => {
                eprintln!("app-output read failed for {}: {error}", sent.exe);
                notes.push(Message::AppOutputUndoFailed {
                    app: sent.exe.clone(),
                });
                true
            }
        }
    });
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssr_types::{
        Arch, AudioApp, DefaultRoles, DriverState, HostError, OutputDevice, OutputLayout, Platform,
        Role, SecureBoot, SetupOutcome, SetupTask,
    };

    struct Fake {
        apps: Vec<AudioApp>,
        current: AppRoleOutputs,
        sets: Vec<AppRoleOutputs>,
        set_error: Option<HostError>,
    }

    impl Host for Fake {
        fn platform(&mut self) -> Platform {
            Platform {
                computer_name: String::new(),
                os_build: 0,
                arch: Arch::X64,
                test_signing: false,
                secure_boot: SecureBoot::Disabled,
            }
        }
        fn driver(&mut self) -> DriverState {
            DriverState::NotInstalled
        }
        fn outputs(&mut self) -> Vec<OutputDevice> {
            Vec::new()
        }
        fn default_roles(&mut self) -> DefaultRoles {
            DefaultRoles::default()
        }
        fn set_default_role(&mut self, _: Role, _: &DeviceId) -> Result<(), HostError> {
            Ok(())
        }
        fn app_entrance_roles(&mut self, _: u32) -> Result<AppRoleOutputs, HostError> {
            Ok(self.current.clone())
        }
        fn set_app_entrance_roles(
            &mut self,
            _: u32,
            roles: &AppRoleOutputs,
        ) -> Result<(), HostError> {
            if let Some(error) = self.set_error.clone() {
                return Err(error);
            }
            self.current = roles.clone();
            self.sets.push(roles.clone());
            Ok(())
        }
        fn supports_layout(&mut self, _: &DeviceId, _: OutputLayout) -> Result<bool, HostError> {
            Ok(true)
        }
        fn run_setup(&mut self, _: &SetupTask) -> Result<SetupOutcome, HostError> {
            Err(HostError::Unsupported)
        }
        fn write_restore_point(&mut self, _: Option<&str>) -> Result<(), HostError> {
            Ok(())
        }
        fn audio_apps(&mut self) -> Vec<AudioApp> {
            self.apps.clone()
        }
    }

    fn app(pid: u32, path: &str) -> AudioApp {
        AudioApp {
            pid,
            exe: "game.exe".into(),
            path: path.into(),
            display_name: "Game".into(),
            device: DeviceId("output".into()),
            active: true,
        }
    }

    fn record() -> AppOutputs {
        AppOutputs {
            app_pid: Some(1),
            apps: vec![SentApp {
                exe: "game.exe".into(),
                path: r"c:\games\game.exe".into(),
                pid: 7,
                discard: DeviceId("discard".into()),
                previous: AppRoleOutputs {
                    console: Some(DeviceId("console".into())),
                    multimedia: Some(DeviceId("multimedia".into())),
                },
            }],
        }
    }

    fn fake(apps: Vec<AudioApp>, current: AppRoleOutputs) -> Fake {
        Fake {
            apps,
            current,
            sets: Vec::new(),
            set_error: None,
        }
    }

    #[test]
    fn undo_restores_each_role_to_its_own_previous_output() {
        let mut host = fake(
            vec![app(7, r"c:\games\game.exe")],
            AppRoleOutputs {
                console: Some(DeviceId("discard".into())),
                multimedia: Some(DeviceId("discard".into())),
            },
        );
        let mut saved = record();
        let notes = undo(&mut saved, &mut host);
        assert!(saved.apps.is_empty());
        assert_eq!(host.sets, [record().apps[0].previous.clone()]);
        assert!(matches!(
            notes.as_slice(),
            [Message::AppOutputRestored { .. }]
        ));
    }

    #[test]
    fn undo_restores_only_roles_that_still_point_to_the_discard() {
        let mut host = fake(
            vec![app(7, r"c:\games\game.exe")],
            AppRoleOutputs {
                console: Some(DeviceId("discard".into())),
                multimedia: Some(DeviceId("user-choice".into())),
            },
        );
        let mut saved = record();
        undo(&mut saved, &mut host);
        assert!(saved.apps.is_empty());
        assert_eq!(
            host.sets,
            [AppRoleOutputs {
                console: Some(DeviceId("console".into())),
                multimedia: Some(DeviceId("user-choice".into())),
            }]
        );
    }

    #[test]
    fn undo_keeps_the_entry_when_a_role_write_fails() {
        let mut host = fake(
            vec![app(7, r"c:\games\game.exe")],
            AppRoleOutputs {
                console: Some(DeviceId("discard".into())),
                multimedia: Some(DeviceId("discard".into())),
            },
        );
        host.set_error = Some(HostError::Failed("scripted".into()));
        let mut saved = record();
        assert!(matches!(
            undo(&mut saved, &mut host).as_slice(),
            [Message::AppOutputUndoFailed { .. }]
        ));
        assert_eq!(saved.apps.len(), 1);
    }

    #[test]
    fn undo_matches_the_full_image_path_not_only_the_basename() {
        let mut host = fake(
            vec![app(9, r"d:\other\game.exe")],
            AppRoleOutputs {
                console: Some(DeviceId("discard".into())),
                multimedia: Some(DeviceId("discard".into())),
            },
        );
        let mut saved = record();
        assert!(matches!(
            undo(&mut saved, &mut host).as_slice(),
            [Message::AppOutputUndoPending { .. }]
        ));
        assert_eq!(saved.apps.len(), 1);
        assert!(host.sets.is_empty());
    }

    #[test]
    fn undo_waits_for_the_same_image_and_uses_its_new_pid() {
        let mut host = fake(
            Vec::new(),
            AppRoleOutputs {
                console: Some(DeviceId("discard".into())),
                multimedia: Some(DeviceId("discard".into())),
            },
        );
        let mut saved = record();
        assert!(matches!(
            undo(&mut saved, &mut host).as_slice(),
            [Message::AppOutputUndoPending { .. }]
        ));
        assert_eq!(saved.apps.len(), 1);

        host.apps.push(app(9, r"c:\games\game.exe"));
        assert!(matches!(
            undo(&mut saved, &mut host).as_slice(),
            [Message::AppOutputRestored { .. }]
        ));
        assert!(saved.apps.is_empty());
    }
}
