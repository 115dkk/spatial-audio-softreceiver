//! The private audio-policy API uses a device-interface path, not an IMMDevice id.
//!
//! Format verified against EarTrumpet's interoperability declarations (ABI facts only):
//! https://raw.githubusercontent.com/File-New-Project/EarTrumpet/master/EarTrumpet/DataModel/WindowsAudio/Internal/AudioPolicyConfigService.cs
//! Render: `\\?\SWD#MMDEVAPI#` + IMMDevice id + `#{e6327cad-dcec-4949-ae8a-991e976a79d2}`.

use ssr_types::{DeviceId, HostError};

const PREFIX: &str = r"\\?\SWD#MMDEVAPI#";
const RENDER_SUFFIX: &str = "#{e6327cad-dcec-4949-ae8a-991e976a79d2}";

fn valid_id(id: &str) -> bool {
    !id.is_empty() && !id.contains('\0') && !id.contains('#') && !id.contains('\\')
}

pub(crate) fn wrap(id: &DeviceId) -> Result<String, HostError> {
    if !valid_id(&id.0) {
        return Err(HostError::Failed("invalid render endpoint id".into()));
    }
    Ok(format!("{PREFIX}{}{RENDER_SUFFIX}", id.0))
}

pub(crate) fn unwrap(value: &str) -> Result<Option<DeviceId>, HostError> {
    if value.is_empty() {
        return Ok(None);
    }
    let id = value
        .get(..PREFIX.len())
        .filter(|start| start.eq_ignore_ascii_case(PREFIX))
        .and_then(|_| value.get(PREFIX.len()..))
        .and_then(|rest| {
            let split = rest.len().checked_sub(RENDER_SUFFIX.len())?;
            rest.get(split..)
                .filter(|end| end.eq_ignore_ascii_case(RENDER_SUFFIX))?;
            rest.get(..split)
        })
        // Some Windows revisions return an unwrapped IMMDevice id.
        .unwrap_or(value);
    if !valid_id(id) {
        return Err(HostError::Failed(
            "invalid persisted render endpoint id".into(),
        ));
    }
    Ok(Some(DeviceId(id.to_owned())))
}

/// None means no common override: either both unset or the roles disagree.
/// In particular a console-only match must not suppress a multimedia repair.
pub(crate) fn consistent_roles(
    console: Option<DeviceId>,
    multimedia: Option<DeviceId>,
) -> Option<DeviceId> {
    (console == multimedia).then_some(console).flatten()
}

pub(crate) fn executable_basename(path: &str) -> Option<String> {
    let name = path.rsplit(['\\', '/']).next()?;
    (!name.is_empty() && !name.contains('\0')).then(|| name.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_id_round_trips_and_empty_is_clear() {
        let id = DeviceId("{0.0.0.00000000}.{ABC}".into());
        assert_eq!(unwrap(&wrap(&id).unwrap()).unwrap(), Some(id.clone()));
        assert_eq!(unwrap(&id.0).unwrap(), Some(id));
        assert_eq!(unwrap("").unwrap(), None);
        assert_eq!(
            unwrap(&format!(
                "{}device{}",
                PREFIX.to_lowercase(),
                RENDER_SUFFIX.to_uppercase()
            ))
            .unwrap(),
            Some(DeviceId("device".into()))
        );
    }

    #[test]
    fn malformed_ids_are_not_forwarded() {
        for value in ["", "a\0b", "a#b", r"a\b"] {
            assert!(wrap(&DeviceId(value.into())).is_err());
        }
        for value in [
            "a\0b",
            &format!("{PREFIX}{RENDER_SUFFIX}"),
            &format!("{PREFIX}x#capture"),
            &format!("{PREFIX}x"),
            &format!("x{RENDER_SUFFIX}"),
        ] {
            assert!(unwrap(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn both_roles_must_agree() {
        let a = Some(DeviceId("a".into()));
        assert_eq!(consistent_roles(a.clone(), a.clone()), a);
        assert_eq!(consistent_roles(None, None), None);
        assert_eq!(consistent_roles(a.clone(), None), None);
        assert_eq!(consistent_roles(a, Some(DeviceId("b".into()))), None);
    }

    #[test]
    fn executable_names_are_lowercase_basenames() {
        assert_eq!(
            executable_basename(r"C:\Apps\Player.EXE"),
            Some("player.exe".into())
        );
        assert_eq!(executable_basename("Player.EXE"), Some("player.exe".into()));
        assert_eq!(executable_basename(""), None);
        assert_eq!(executable_basename("x\0.exe"), None);
    }
}
