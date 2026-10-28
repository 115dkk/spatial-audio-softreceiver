#![forbid(unsafe_code)]

use ssr_types::{AsioLayout, AsioSettings, AsioStatus, EngineStatus};

#[test]
fn asio_layouts_serialize_with_explicit_names() {
    for (layout, name) in AsioLayout::ALL.into_iter().zip([
        "2.0", "5.1", "7.1", "7.1.4", "9.1", "9.1.4", "9.1.6", "13.1", "22.2", "24.1.10", "30.2",
    ]) {
        let json = serde_json::to_string(&layout).unwrap();
        assert_eq!(json, format!("\"{name}\""));
        assert_eq!(layout.name(), name);
        assert_eq!(serde_json::from_str::<AsioLayout>(&json).unwrap(), layout);
    }
    assert_eq!(AsioLayout::default(), AsioLayout::Surround714);
    assert!(serde_json::from_str::<AsioLayout>("\"surround714\"").is_err());
    assert!(serde_json::from_str::<AsioLayout>("\"5.1.4\"").is_err());
}

#[test]
fn asio_settings_default_missing_fields_and_round_trip() {
    let defaults = AsioSettings::default();
    assert!(defaults.enabled);
    assert_eq!(defaults.layout, AsioLayout::Surround714);
    assert_eq!(defaults.system_attenuation_db, 0.0);
    assert_eq!(
        serde_json::from_str::<AsioSettings>("{}").unwrap(),
        defaults
    );
    let partial: AsioSettings = serde_json::from_str(r#"{"enabled":false}"#).unwrap();
    assert_eq!(
        partial,
        AsioSettings {
            enabled: false,
            ..defaults
        }
    );
    let settings = AsioSettings {
        enabled: false,
        layout: AsioLayout::Surround916,
        system_attenuation_db: 60.0,
    };
    let json = serde_json::to_string(&settings).unwrap();
    assert_eq!(
        serde_json::from_str::<AsioSettings>(&json).unwrap(),
        settings
    );
}

#[test]
fn asio_attenuation_accepts_only_finite_zero_through_sixty() {
    for db in [0.0, 0.5, 12.0, 60.0] {
        assert!(
            AsioSettings {
                system_attenuation_db: db,
                ..Default::default()
            }
            .is_valid()
        );
    }
    for db in [-0.1, 60.1, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(
            !AsioSettings {
                system_attenuation_db: db,
                ..Default::default()
            }
            .is_valid()
        );
    }
}

#[test]
fn asio_status_is_optional_and_old_engine_status_still_loads() {
    let mut json = serde_json::to_value(EngineStatus::default()).unwrap();
    assert!(json["asio"].is_null());
    json.as_object_mut().unwrap().remove("asio");
    assert!(
        serde_json::from_value::<EngineStatus>(json)
            .unwrap()
            .asio
            .is_none()
    );

    let status = EngineStatus {
        asio: Some(AsioStatus {
            client_pid: 42,
            client_exe: "test-daw.exe".into(),
            sample_rate: 96_000,
            buffer_frames: 256,
            channels: 16,
            layout: AsioLayout::Surround916,
            underruns: 3,
            generation: 7,
            clock_fault: true,
            fault_reason: Some("owner_exists".into()),
        }),
        ..Default::default()
    };
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(json["asio"]["layout"], "9.1.6");
    assert_eq!(
        serde_json::from_value::<EngineStatus>(json).unwrap(),
        status
    );
}
