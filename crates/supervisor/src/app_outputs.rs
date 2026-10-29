//! Recovery record for applications temporarily sent to the discard output.

use serde::{Deserialize, Serialize};
use ssr_types::{DeviceId, Host};

use crate::text::Message;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppOutputs {
    pub app_pid: Option<u32>,
    pub apps: Vec<SentApp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentApp {
    pub exe: String,
    pub pid: u32,
    pub discard: DeviceId,
    pub previous: Option<DeviceId>,
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
    let sessions = host.audio_apps();
    let mut notes = Vec::new();
    record.apps.retain(|sent| {
        let pid = sessions
            .iter()
            .find(|app| app.pid == sent.pid && app.exe == sent.exe)
            .or_else(|| sessions.iter().find(|app| app.exe == sent.exe))
            .map(|app| app.pid);
        let Some(pid) = pid else {
            notes.push(Message::AppOutputUndoPending {
                app: sent.exe.clone(),
            });
            return true;
        };
        match host.app_entrance(pid) {
            Ok(current) if current.as_ref() == Some(&sent.discard) => {
                match host.set_app_entrance(pid, sent.previous.as_ref()) {
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
            Ok(_) => false,
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
        current: Option<DeviceId>,
        sets: Vec<Option<DeviceId>>,
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
        fn app_entrance(&mut self, _: u32) -> Result<Option<DeviceId>, HostError> {
            Ok(self.current.clone())
        }
        fn set_app_entrance(&mut self, _: u32, device: Option<&DeviceId>) -> Result<(), HostError> {
            self.current = device.cloned();
            self.sets.push(device.cloned());
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

    fn app(pid: u32) -> AudioApp {
        AudioApp {
            pid,
            exe: "game.exe".into(),
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
                pid: 7,
                discard: DeviceId("discard".into()),
                previous: Some(DeviceId("previous".into())),
            }],
        }
    }

    #[test]
    fn undo_restores_only_while_the_discard_is_still_selected() {
        let mut host = Fake {
            apps: vec![app(7)],
            current: Some(DeviceId("discard".into())),
            sets: Vec::new(),
        };
        let mut saved = record();
        let notes = undo(&mut saved, &mut host);
        assert!(saved.apps.is_empty());
        assert_eq!(host.sets, [Some(DeviceId("previous".into()))]);
        assert!(matches!(
            notes.as_slice(),
            [Message::AppOutputRestored { .. }]
        ));

        host.current = Some(DeviceId("user-choice".into()));
        host.sets.clear();
        let mut saved = record();
        assert!(undo(&mut saved, &mut host).is_empty());
        assert!(saved.apps.is_empty());
        assert!(host.sets.is_empty());
    }

    #[test]
    fn undo_waits_for_the_executable_and_uses_its_new_pid() {
        let mut host = Fake {
            apps: Vec::new(),
            current: Some(DeviceId("discard".into())),
            sets: Vec::new(),
        };
        let mut saved = record();
        assert!(matches!(
            undo(&mut saved, &mut host).as_slice(),
            [Message::AppOutputUndoPending { .. }]
        ));
        assert_eq!(saved.apps.len(), 1);

        host.apps.push(app(9));
        assert!(matches!(
            undo(&mut saved, &mut host).as_slice(),
            [Message::AppOutputRestored { .. }]
        ));
        assert!(saved.apps.is_empty());
    }
}
