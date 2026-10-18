//! Machine-readable per-app diagnostics; endpoint selection never uses names.
#![cfg_attr(not(any(windows, test)), allow(dead_code))]

use serde_json::{Value, json};
use ssr_types::{AudioApp, DeviceId, DeviceState, Host, HostError, OutputDevice};

fn error_message(error: HostError) -> String {
    let language = ssr_i18n::cli_language();
    match error {
        HostError::Unsupported => ssr_i18n::live::app_routing_unsupported(language).to_owned(),
        // The host's underlying detail is diagnostic data, not a translated sentence.
        HostError::Failed(detail) | HostError::Setup(detail) => {
            ssr_i18n::live::app_routing_failed(language, detail)
        }
        HostError::Conflict => ssr_i18n::live::app_routing_failed(language, HostError::Conflict),
        HostError::Declined => ssr_i18n::live::app_routing_failed(language, "declined"),
    }
}

fn app_json(app: &AudioApp, entrance: Result<Option<DeviceId>, HostError>) -> Value {
    let mut row = json!({
        "pid": app.pid,
        "exe": app.exe,
        "display_name": app.display_name,
        "device": app.device,
        "active": app.active,
    });
    match entrance {
        Ok(id) => {
            row["app_entrance"] = json!(id);
            row["app_entrance_error"] = Value::Null;
        }
        Err(error) => {
            row["app_entrance"] = Value::Null;
            row["app_entrance_error"] = json!({
                "kind": match &error {
                    HostError::Unsupported => "unsupported",
                    HostError::Conflict => "conflict",
                    _ => "failed",
                },
                "detail": error.to_string(),
            });
        }
    }
    row
}

/// app_entrance:null means no common console/multimedia override (including
/// a role mismatch); app_entrance_error distinguishes a failed/unsupported read.
pub fn apps_json(host: &mut impl Host) -> Value {
    let apps = host.audio_apps();
    let rows: Vec<_> = apps
        .iter()
        .map(|app| app_json(app, host.app_entrance(app.pid)))
        .collect();
    json!({"audio_apps": rows})
}

fn target(outputs: &[OutputDevice], endpoint: u16) -> Result<DeviceId, String> {
    let language = ssr_i18n::cli_language();
    let mut candidates = outputs
        .iter()
        .filter(|output| output.endpoint == Some(endpoint));
    let output = candidates
        .next()
        .ok_or_else(|| ssr_i18n::live::app_endpoint_missing(language, endpoint))?;
    if candidates.next().is_some() {
        return Err(ssr_i18n::live::app_endpoint_ambiguous(language, endpoint));
    }
    if output.state != DeviceState::Active {
        return Err(ssr_i18n::live::app_endpoint_inactive(language, endpoint));
    }
    Ok(output.id.clone())
}

pub fn set_app(host: &mut impl Host, pid: u32, endpoint: &str) -> Result<Value, String> {
    let device = if endpoint == "none" {
        None
    } else {
        // clap accepts only 0, 1, 2 and none before this function runs.
        let endpoint = endpoint
            .parse::<u16>()
            .expect("validated endpoint argument");
        Some(target(&host.outputs(), endpoint)?)
    };
    host.set_app_entrance(pid, device.as_ref())
        .map_err(error_message)?;
    let readback = host.app_entrance(pid).map_err(error_message)?;
    if readback != device {
        return Err(ssr_i18n::live::app_routing_mismatch(
            ssr_i18n::cli_language(),
            pid,
        ));
    }
    Ok(json!({"ok": true, "pid": pid, "app_entrance": readback}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssr_types::{
        DefaultRoles, DriverState, OutputLayout, Platform, Role, SetupOutcome, SetupTask,
    };

    struct FakeHost {
        outputs: Vec<OutputDevice>,
        apps: Vec<AudioApp>,
        readback: Result<Option<DeviceId>, HostError>,
        set_result: Result<(), HostError>,
        output_reads: usize,
        writes: Vec<(u32, Option<DeviceId>)>,
    }

    impl FakeHost {
        fn new(readback: Result<Option<DeviceId>, HostError>) -> Self {
            Self {
                outputs: Vec::new(),
                apps: Vec::new(),
                readback,
                set_result: Ok(()),
                output_reads: 0,
                writes: Vec::new(),
            }
        }
    }

    impl Host for FakeHost {
        fn platform(&mut self) -> Platform {
            panic!("unused")
        }

        fn driver(&mut self) -> DriverState {
            panic!("unused")
        }

        fn outputs(&mut self) -> Vec<OutputDevice> {
            self.output_reads += 1;
            self.outputs.clone()
        }

        fn default_roles(&mut self) -> DefaultRoles {
            panic!("unused")
        }

        fn set_default_role(&mut self, _role: Role, _device: &DeviceId) -> Result<(), HostError> {
            panic!("unused")
        }

        fn audio_apps(&mut self) -> Vec<AudioApp> {
            self.apps.clone()
        }

        fn app_entrance(&mut self, _pid: u32) -> Result<Option<DeviceId>, HostError> {
            self.readback.clone()
        }

        fn set_app_entrance(
            &mut self,
            pid: u32,
            device: Option<&DeviceId>,
        ) -> Result<(), HostError> {
            self.writes.push((pid, device.cloned()));
            self.set_result.clone()
        }

        fn supports_layout(
            &mut self,
            _output: &DeviceId,
            _layout: OutputLayout,
        ) -> Result<bool, HostError> {
            panic!("unused")
        }

        fn run_setup(&mut self, _task: &SetupTask) -> Result<SetupOutcome, HostError> {
            panic!("unused")
        }

        fn write_restore_point(&mut self, _json: Option<&str>) -> Result<(), HostError> {
            panic!("unused")
        }
    }

    fn output(id: &str, endpoint: Option<u16>, state: DeviceState) -> OutputDevice {
        OutputDevice {
            id: DeviceId(id.into()),
            name: "same display name".into(),
            endpoint,
            state,
        }
    }

    fn app() -> AudioApp {
        AudioApp {
            pid: 42,
            exe: "player.exe".into(),
            display_name: "Player".into(),
            device: DeviceId("observed-device".into()),
            active: true,
        }
    }

    #[test]
    fn apps_json_keeps_observed_device_distinct_from_persisted_entrance() {
        let mut host = FakeHost::new(Ok(Some(DeviceId("persisted-entrance".into()))));
        host.apps.push(app());

        let row = &apps_json(&mut host)["audio_apps"][0];
        assert_eq!(row["device"], "observed-device");
        assert_eq!(row["app_entrance"], "persisted-entrance");
        assert_eq!(row["app_entrance_error"], Value::Null);
    }

    #[test]
    fn apps_json_distinguishes_unset_unsupported_conflict_and_failed_reads() {
        for (readback, entrance, kind) in [
            (Ok(None), Value::Null, Value::Null),
            (
                Err(HostError::Unsupported),
                Value::Null,
                json!("unsupported"),
            ),
            (
                Err(HostError::Failed("read failed".into())),
                Value::Null,
                json!("failed"),
            ),
            (Err(HostError::Conflict), Value::Null, json!("conflict")),
        ] {
            let mut host = FakeHost::new(readback);
            host.apps.push(app());
            let row = &apps_json(&mut host)["audio_apps"][0];
            assert_eq!(row["app_entrance"], entrance);
            assert_eq!(row["app_entrance_error"]["kind"], kind);
        }
    }

    #[test]
    fn set_app_sets_and_reads_back_every_endpoint() {
        let outputs = vec![
            output("primary-id", Some(0), DeviceState::Active),
            output("music-id", Some(1), DeviceState::Active),
            output("pass-id", Some(2), DeviceState::Active),
        ];
        for (endpoint, id) in [("0", "primary-id"), ("1", "music-id"), ("2", "pass-id")] {
            let device = DeviceId(id.into());
            let mut host = FakeHost::new(Ok(Some(device.clone())));
            host.outputs = outputs.clone();

            let result = set_app(&mut host, 42, endpoint).unwrap();
            assert_eq!(result, json!({"ok": true, "pid": 42, "app_entrance": id}));
            assert_eq!(host.writes, vec![(42, Some(device))]);
            assert_eq!(host.output_reads, 1);
        }
    }

    #[test]
    fn set_app_clear_does_not_enumerate_outputs() {
        let mut host = FakeHost::new(Ok(None));

        let result = set_app(&mut host, 42, "none").unwrap();
        assert_eq!(result, json!({"ok": true, "pid": 42, "app_entrance": null}));
        assert_eq!(host.writes, vec![(42, None)]);
        assert_eq!(host.output_reads, 0);
    }

    #[test]
    fn set_app_rejects_readback_mismatch() {
        let mut host = FakeHost::new(Ok(Some(DeviceId("other-id".into()))));
        host.outputs = vec![output("primary-id", Some(0), DeviceState::Active)];

        assert!(set_app(&mut host, 42, "0").is_err());
        assert_eq!(host.writes, vec![(42, Some(DeviceId("primary-id".into())))]);
    }

    #[test]
    fn set_app_reports_unsupported_conflict_and_failed_host_calls() {
        for error in [
            HostError::Unsupported,
            HostError::Conflict,
            HostError::Failed("host failed".into()),
        ] {
            let mut write_failure = FakeHost::new(Ok(None));
            write_failure.set_result = Err(error.clone());
            assert!(
                !set_app(&mut write_failure, 42, "none")
                    .unwrap_err()
                    .is_empty()
            );

            let mut read_failure = FakeHost::new(Err(error));
            assert!(
                !set_app(&mut read_failure, 42, "none")
                    .unwrap_err()
                    .is_empty()
            );
            assert_eq!(read_failure.writes, vec![(42, None)]);
        }
    }

    #[test]
    fn set_app_does_not_write_for_invalid_targets() {
        let cases = [
            Vec::new(),
            vec![
                output("first", Some(1), DeviceState::Active),
                output("second", Some(1), DeviceState::Active),
            ],
            vec![output("music", Some(1), DeviceState::Disabled)],
        ];
        for outputs in cases {
            let mut host = FakeHost::new(Ok(None));
            host.outputs = outputs;
            assert!(set_app(&mut host, 42, "1").is_err());
            assert!(host.writes.is_empty());
        }
    }
}
