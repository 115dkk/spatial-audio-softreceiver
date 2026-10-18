//! The restore point and the rule that gives the default roles back (ADR 0023
//! decision 4, ADR 0013's 2026-10-15 addendum).
//!
//! The supervisor writes the restore point before it touches any role and
//! removes it after it gave them back. `ssr-guard` reads it when the app died
//! or stalled and gives the roles back with the same [`restore_roles`] rule, so
//! the two can never disagree about what "giving back" means.

use serde_json::{Map, Value, json};
use ssr_types::app::Language;
use ssr_types::{DefaultRoles, DeviceId, DeviceState, Host, Role};

use crate::text::Message;

/// What the guard needs to give the roles back without the app: our main
/// endpoint, the device each role pointed at before, the device a role falls
/// back to when its previous one is unusable (the chosen output), and the app
/// to watch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestorePoint {
    pub ours: DeviceId,
    pub previous: DefaultRoles,
    pub fallback: Option<DeviceId>,
    /// A DAC endpoint hidden for kernel-streaming output.
    pub hidden: Option<DeviceId>,
    pub app_pid: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RestorePointError {
    #[error("the restore point is not JSON: {0}")]
    Json(String),
    #[error("the restore point has version {0}, this build reads versions 1 and 2")]
    Version(u64),
    #[error("the restore point field `{0}` is missing or has the wrong type")]
    Field(&'static str),
}

impl RestorePoint {
    pub const VERSION: u32 = 2;

    /// One line: `version`, `ours`, `previous` (`console`, `multimedia`,
    /// `communications`, each an id or null), `fallback` (an id or null) and
    /// `app_pid`.
    pub fn to_json(&self) -> String {
        let previous = Role::ALL
            .into_iter()
            .map(|role| {
                let id = self.previous.get(role).map(|id| id.0.clone());
                (role_key(role).to_owned(), id.into())
            })
            .collect::<Map<_, _>>();
        json!({
            "version": Self::VERSION,
            "ours": self.ours.0,
            "previous": previous,
            "fallback": self.fallback.as_ref().map(|id| id.0.clone()),
            "hidden": self.hidden.as_ref().map(|id| id.0.clone()),
            "app_pid": self.app_pid,
        })
        .to_string()
    }

    /// Reads restore-point versions 1 and 2. Version 1 had no hidden endpoint;
    /// unknown keys are ignored and later versions are refused.
    pub fn from_json(text: &str) -> Result<Self, RestorePointError> {
        let value: Value = serde_json::from_str(text)
            .map_err(|error| RestorePointError::Json(error.to_string()))?;
        let version = value["version"]
            .as_u64()
            .ok_or(RestorePointError::Field("version"))?;
        if !matches!(version, 1 | 2) {
            return Err(RestorePointError::Version(version));
        }
        let ours = value["ours"]
            .as_str()
            .ok_or(RestorePointError::Field("ours"))?;
        let previous_value = value["previous"]
            .as_object()
            .ok_or(RestorePointError::Field("previous"))?;
        let mut previous = DefaultRoles::default();
        for role in Role::ALL {
            let key = role_key(role);
            previous.set(role, optional_id(previous_value.get(key), key)?);
        }
        let fallback = optional_id(value.get("fallback"), "fallback")?;
        let hidden = if version == 1 {
            None
        } else {
            optional_id(value.get("hidden"), "hidden")?
        };
        let app_pid = value["app_pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .ok_or(RestorePointError::Field("app_pid"))?;
        Ok(Self {
            ours: DeviceId(ours.to_owned()),
            previous,
            fallback,
            hidden,
            app_pid,
        })
    }
}

/// A key that must be present and hold an id or null.
fn optional_id(
    value: Option<&Value>,
    key: &'static str,
) -> Result<Option<DeviceId>, RestorePointError> {
    match value {
        Some(Value::String(id)) => Ok(Some(DeviceId(id.clone()))),
        Some(Value::Null) => Ok(None),
        _ => Err(RestorePointError::Field(key)),
    }
}

/// Gives the roles of `point` back and removes the restore point. Returns the
/// notes for the user (see [`restore_roles`]).
pub fn restore(point: &RestorePoint, host: &mut impl Host) -> Vec<String> {
    restore_with_language(point, host, Language::Ko)
}

pub fn restore_with_language(
    point: &RestorePoint,
    host: &mut impl Host,
    language: Language,
) -> Vec<String> {
    restore_messages(point, host)
        .into_iter()
        .map(|message| message.render(language))
        .collect()
}

pub fn restore_messages(point: &RestorePoint, host: &mut impl Host) -> Vec<Message> {
    restore_messages_checked(point, host).0
}

/// Restores a point and reports whether every recovery action, including
/// removing the durable point, succeeded.
pub fn restore_messages_checked(
    point: &RestorePoint,
    host: &mut impl Host,
) -> (Vec<Message>, bool) {
    let mut notes = Vec::new();
    let mut restored = true;
    if let Some(hidden) = &point.hidden
        && let Err(error) = host.set_endpoint_visible(hidden, true)
    {
        notes.push(Message::RestoreEndpointFailed {
            detail: error.to_string(),
        });
        restored = false;
    }
    let (role_notes, roles_restored) =
        restore_roles_messages_checked(host, &point.ours, &point.previous, point.fallback.as_ref());
    notes.extend(role_notes);
    restored &= roles_restored;
    if restored && let Err(error) = host.write_restore_point(None) {
        notes.push(Message::RestorePointRemoveFailed {
            detail: error.to_string(),
        });
        restored = false;
    }
    (notes, restored)
}

/// The one rule for giving the default roles back (ADR 0023):
///
/// * a role that no longer points at `ours` is left alone: the user changed it
///   meanwhile, and that choice wins;
/// * a role that still points at `ours` goes back to its previous device when
///   that device is listed, active and not ours;
/// * otherwise (no previous device, a gone one, or ours, left by an earlier
///   run that never gave the roles back) the role goes to `fallback` when that
///   is listed and active, with a note: leaving it on our endpoint while the
///   app is off would make the PC silent;
/// * with no usable fallback either, the role is left for Windows to choose
///   and a note says so.
///
/// Returns the notes the user should see.
pub fn restore_roles(
    host: &mut impl Host,
    ours: &DeviceId,
    previous: &DefaultRoles,
    fallback: Option<&DeviceId>,
) -> Vec<String> {
    restore_roles_with_language(host, ours, previous, fallback, Language::Ko)
}

pub fn restore_roles_with_language(
    host: &mut impl Host,
    ours: &DeviceId,
    previous: &DefaultRoles,
    fallback: Option<&DeviceId>,
    language: Language,
) -> Vec<String> {
    restore_roles_messages(host, ours, previous, fallback)
        .into_iter()
        .map(|message| message.render(language))
        .collect()
}

pub fn restore_roles_messages(
    host: &mut impl Host,
    ours: &DeviceId,
    previous: &DefaultRoles,
    fallback: Option<&DeviceId>,
) -> Vec<Message> {
    restore_roles_messages_checked(host, ours, previous, fallback).0
}

fn restore_roles_messages_checked(
    host: &mut impl Host,
    ours: &DeviceId,
    previous: &DefaultRoles,
    fallback: Option<&DeviceId>,
) -> (Vec<Message>, bool) {
    let current = host.default_roles();
    // Endpoint visibility must be restored before this enumeration: the hidden
    // DAC is also the fallback and is otherwise absent from the active set.
    let outputs = host.outputs();
    // A device a role may go to: listed, active, and not ours.
    let usable = |id: &DeviceId| {
        outputs.iter().find(|output| {
            output.id == *id && output.id != *ours && output.state == DeviceState::Active
        })
    };
    let fallback = fallback.and_then(usable);
    let mut notes = Vec::new();
    let mut restored = true;
    for role in Role::ALL {
        if current.get(role) != Some(ours) {
            continue;
        }
        let (target, note) = match previous.get(role).and_then(usable) {
            Some(back) => (back, None),
            None => match fallback {
                Some(output) => (
                    output,
                    Some(Message::RestoreFallback {
                        role,
                        name: output.name.clone(),
                    }),
                ),
                None => {
                    notes.push(Message::RestoreLeftToWindows { role });
                    continue;
                }
            },
        };
        match host.set_default_role(role, &target.id) {
            Ok(()) => notes.extend(note),
            Err(error) => {
                restored = false;
                notes.push(Message::RestoreRoleFailed {
                    role,
                    detail: error.to_string(),
                });
            }
        }
    }
    (notes, restored)
}

fn role_key(role: Role) -> &'static str {
    match role {
        Role::Console => "console",
        Role::Multimedia => "multimedia",
        Role::Communications => "communications",
    }
}
