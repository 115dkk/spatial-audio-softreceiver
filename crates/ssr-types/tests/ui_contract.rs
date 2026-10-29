#![cfg(feature = "ts")]
#![forbid(unsafe_code)]
//! The screen contract as files (ADR 0001): the TypeScript bindings specta
//! generates from `Intent` and `Snapshot`, and JSON fixtures of representative
//! snapshots and intents, serialized by serde exactly as the shell sends and
//! receives them, which the screen's tests render.
//!
//! The files live under `apps/spatial-softreceiver/ui`. `SSR_UI_BLESS=1`
//! rewrites them; without it the test fails when they differ, so a change to
//! the types comes with its regenerated bindings and fixtures in the same
//! commit (the same discipline as the driver's C header, `SSR_ABI_BLESS`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use specta::TypeCollection;
use specta_typescript::{BigIntExportBehavior, Typescript};
use ssr_types::app::{
    AppRule, AsioLayout, AsioSettings, CorrectionView, EndpointUpmixView, Entrance, HeadphonesView,
    HealthHistory, HealthIncident, HealthMinute, HealthPart, Intent, Language, MinuteState,
    NextAction, OutputChoice, Phase, PluginChoice, PluginFormat, PluginView, ProfileKind,
    ProfileView, RoutedApp, RoutingSettings, RoutingView, RuleView, SetupStep, Skin, Snapshot,
    SpeakerChannelView, SpeakerLayout, SpeakersView, UpmixApply, UpmixBypass, UpmixState,
    UpmixView, UpmixerChoice, UpmixerSetting, UserError,
};
use ssr_types::ports::PolicyState;
use ssr_types::status::{
    ChainErrorCounts, ChainErrorKind, ClockStatus, EndpointStatus, LatencyStats, UnderrunSnapshot,
    UnsupportedReason,
};
use ssr_types::{
    Arch, BitstreamCodec, BitstreamFormat, DeviceId, DirectionId, DirectionView, DriverState,
    EngineFault, EngineStatus, OutputError, OutputMode, PcmFormat, Platform, SampleType,
    SceneObject, SecureBoot, SourceFormat, Speaker,
};

const BLESS: &str = "SSR_UI_BLESS";

fn ui_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/spatial-softreceiver/ui")
}

/// Compares `content` with the file, or writes it when blessing.
fn check_or_bless(path: &Path, content: &str) {
    if std::env::var_os(BLESS).is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        return;
    }
    let existing = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{}: {error}; run with {BLESS}=1", path.display()));
    assert!(
        existing == content,
        "{} is out of date; run `{BLESS}=1 cargo test -p ssr-types --features ts --test ui_contract`",
        path.display()
    );
}

#[test]
fn typescript_bindings_match_the_types() {
    let mut types = TypeCollection::default();
    types
        .register::<Intent>()
        .register::<Snapshot>()
        .register::<RoutingSettings>()
        .register::<UserError>()
        .register::<EngineStatus>();
    let header = "// Generated from crates/ssr-types by `SSR_UI_BLESS=1 cargo test -p ssr-types \
                  --features ts --test ui_contract`. Do not edit.\n\
                  // The shape is serde's: enums with data are tagged `kind` with the data \
                  under `value`; 64-bit counters are JavaScript numbers.";
    let bindings = Typescript::default()
        .header(header)
        .bigint(BigIntExportBehavior::Number)
        .export(&types)
        .unwrap();
    let bindings = bindings
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    check_or_bless(&ui_dir().join("src/bindings.ts"), &bindings);
}

#[test]
fn asio_snapshot_contract_keeps_settings_separate_from_client_status() {
    for language in [Language::Ko, Language::En] {
        let mut snapshot = blank(language, Phase::Ready);
        assert_eq!(snapshot.asio, AsioSettings::default());
        let mut old = serde_json::to_value(&snapshot).unwrap();
        old.as_object_mut().unwrap().remove("asio");
        old.as_object_mut().unwrap().remove("health");
        assert_eq!(serde_json::from_value::<Snapshot>(old).unwrap(), snapshot);
        for layout in AsioLayout::ALL {
            snapshot.asio.layout = layout;
            snapshot.engine = Some(EngineStatus {
                asio: Some(ssr_types::AsioStatus {
                    client_pid: 42,
                    client_exe: "test-daw.exe".into(),
                    sample_rate: 48_000,
                    buffer_frames: 256,
                    channels: ssr_types::channel_layout_preset(layout.name())
                        .unwrap()
                        .len() as u32,
                    layout,
                    underruns: 0,
                    generation: 1,
                    clock_fault: false,
                    fault_reason: None,
                }),
                ..Default::default()
            });
            let json = serde_json::to_value(&snapshot).unwrap();
            assert_eq!(json["asio"]["layout"], layout.name());
            assert_eq!(json["engine"]["asio"]["layout"], layout.name());
            assert_eq!(serde_json::from_value::<Snapshot>(json).unwrap(), snapshot);
        }
    }
}

fn choice(id: &str, name: &str) -> OutputChoice {
    OutputChoice {
        id: DeviceId(id.to_owned()),
        name: name.to_owned(),
    }
}

fn outputs(language: Language) -> Vec<OutputChoice> {
    let (speakers, headphones) = match language {
        Language::Ko => ("스피커 (Realtek(R) Audio)", "헤드폰 (USB Audio DAC)"),
        Language::En => ("Speakers (Realtek(R) Audio)", "Headphones (USB Audio DAC)"),
    };
    vec![
        choice(
            "{0.0.0.00000000}.{3f2504e0-4f89-11d3-9a0c-0305e82c3301}",
            speakers,
        ),
        choice(
            "{0.0.0.00000000}.{7c1c3e55-3a8c-4b1f-9d33-5a2a7b0d7e12}",
            "DENON AVR (NVIDIA High Definition Audio)",
        ),
        choice(
            "{0.0.0.00000000}.{a1b2c3d4-e5f6-4a5b-8c9d-0e1f2a3b4c5d}",
            headphones,
        ),
    ]
}

fn platform(test_signing: bool, secure_boot: SecureBoot) -> Platform {
    Platform {
        computer_name: "STUDIO-PC".to_owned(),
        os_build: 26_100,
        arch: Arch::X64,
        test_signing,
        secure_boot,
    }
}

#[test]
fn old_platform_json_defaults_the_computer_name() {
    let mut old = serde_json::to_value(platform(true, SecureBoot::Disabled)).unwrap();
    old.as_object_mut().unwrap().remove("computer_name");
    let mut expected = platform(true, SecureBoot::Disabled);
    expected.computer_name.clear();
    assert_eq!(serde_json::from_value::<Platform>(old).unwrap(), expected);
}

fn installed(policy: PolicyState) -> DriverState {
    DriverState::Installed {
        problem_code: 0,
        service_running: true,
        exclusive_policy: policy,
    }
}

fn error(message: &str, next: NextAction) -> UserError {
    UserError {
        message: message.to_owned(),
        next,
    }
}

fn direction(id: u16, name: &str, azimuth_deg: f32, elevation_deg: f32) -> DirectionView {
    DirectionView {
        id: DirectionId(id),
        name: name.to_owned(),
        azimuth_deg,
        elevation_deg,
    }
}

/// A speaker layout as the supervisor shows it: every device channel. 22.2
/// and 30.2 have two subwoofers, `LFE` and `LFE2` (ADR 0042).
fn layout_channels(layout: SpeakerLayout) -> Vec<SpeakerChannelView> {
    layout
        .speakers()
        .iter()
        .enumerate()
        .map(|(channel, speaker)| SpeakerChannelView {
            name: speaker.name().to_owned(),
            channel: u16::try_from(channel).unwrap(),
            subwoofer: matches!(speaker, Speaker::Lfe | Speaker::Lfe2),
        })
        .collect()
}

fn speakers_view(layout: SpeakerLayout) -> SpeakersView {
    SpeakersView {
        layout: Some(layout),
        correction: None,
        channels: layout_channels(layout),
    }
}

/// The speakers of a layout as test-tone directions (the subwoofer is none).
fn speaker_directions(layout: SpeakerLayout) -> Vec<DirectionView> {
    layout
        .speakers()
        .iter()
        .filter_map(|speaker| speaker.angles().map(|angles| (speaker.name(), angles)))
        .enumerate()
        .map(|(id, (name, (azimuth, elevation)))| {
            direction(u16::try_from(id).unwrap(), name, azimuth, elevation)
        })
        .collect()
}

/// The first day's profile: speakers 2.0 without correction, the same sound
/// as the old default stereo.
fn stereo_profile(_language: Language) -> ProfileView {
    ProfileView {
        kind: ProfileKind::Speakers,
        name: "2.0".to_owned(),
        gain_db: 0.0,
        directions: speaker_directions(SpeakerLayout::Stereo),
        from_file: false,
        headphones: HeadphonesView { brir: None },
        speakers: speakers_view(SpeakerLayout::Stereo),
    }
}

/// Headphones in use; the speaker settings stay for when speakers return.
fn headphone_profile(language: Language) -> ProfileView {
    ProfileView {
        kind: ProfileKind::Headphones,
        name: match language {
            Language::Ko => "거실 7.1.4 헤드폰",
            Language::En => "Living room 7.1.4 headphones",
        }
        .to_owned(),
        gain_db: -3.5,
        directions: vec![
            // 7.1.4 has eleven render directions: LFE is routed separately and
            // therefore is not a BRIR direction or a test-tone target.
            direction(0, "FL", 30.0, 0.0),
            direction(1, "FR", -30.0, 0.0),
            direction(2, "FC", 0.0, 0.0),
            direction(3, "SL", 90.0, 0.0),
            direction(4, "SR", -90.0, 0.0),
            direction(5, "BL", 150.0, 0.0),
            direction(6, "BR", -150.0, 0.0),
            direction(7, "TFL", 30.0, 45.0),
            direction(8, "TFR", -30.0, 45.0),
            direction(9, "TBL", 150.0, 45.0),
            direction(10, "TBR", -150.0, 45.0),
        ],
        from_file: false,
        headphones: HeadphonesView {
            brir: Some(r"C:\Users\me\BRIR\living-room".to_owned()),
        },
        speakers: speakers_view(SpeakerLayout::Surround71),
    }
}

/// Speakers 7.1.4 by layout, without correction.
fn speaker_layout_profile(language: Language) -> ProfileView {
    ProfileView {
        kind: ProfileKind::Speakers,
        name: "7.1.4".to_owned(),
        gain_db: -6.0,
        directions: speaker_directions(SpeakerLayout::Surround714),
        from_file: false,
        speakers: speakers_view(SpeakerLayout::Surround714),
        ..headphone_profile(language)
    }
}

/// The inputs of a SECS 2R 7.1.4 export with two subwoofers, as render
/// directions: SECS's side surrounds `Ls/Rs` sit at the Windows side angles
/// because the layout has back surrounds `Lb/Rb` (ADR 0034).
const SECS_INPUTS: &[(&str, f32, f32)] = &[
    ("L", 30.0, 0.0),
    ("R", -30.0, 0.0),
    ("C", 0.0, 0.0),
    ("Ls", 90.0, 0.0),
    ("Rs", -90.0, 0.0),
    ("Lb", 150.0, 0.0),
    ("Rb", -150.0, 0.0),
    ("Tfl", 45.0, 45.0),
    ("Tfr", -45.0, 45.0),
    ("Tbl", 135.0, 45.0),
    ("Tbr", -135.0, 45.0),
];

/// Speakers in use through an imported SECS 2R correction; the stored layout
/// and the BRIR set stay for later.
fn speaker_profile(language: Language) -> ProfileView {
    let outputs = SECS_INPUTS
        .iter()
        .map(|(name, _, _)| (*name, false))
        .chain([("SW1", true), ("SW2", true)]);
    ProfileView {
        kind: ProfileKind::Speakers,
        name: "SECS 2 mixed (8.0 ms)".to_owned(),
        gain_db: -6.0,
        directions: SECS_INPUTS
            .iter()
            .enumerate()
            .map(|(id, (name, azimuth, elevation))| {
                direction(u16::try_from(id).unwrap(), name, *azimuth, *elevation)
            })
            .collect(),
        from_file: false,
        speakers: SpeakersView {
            layout: Some(SpeakerLayout::Surround714),
            correction: Some(CorrectionView {
                path: r"C:\Users\me\Documents\SECS_export\2026-10-16 living room\configs\secs_mixed.yml"
                    .to_owned(),
                title: "SECS 2 mixed (8.0 ms)".to_owned(),
            }),
            channels: outputs
                .enumerate()
                .map(|(channel, (name, subwoofer))| SpeakerChannelView {
                    name: name.to_owned(),
                    channel: u16::try_from(channel).unwrap(),
                    subwoofer,
                })
                .collect(),
        },
        ..headphone_profile(language)
    }
}

/// Speakers by `layout`, without correction, the name the layout's.
fn speaker_layout_named(language: Language, layout: SpeakerLayout) -> ProfileView {
    ProfileView {
        name: layout.name().to_owned(),
        directions: speaker_directions(layout),
        speakers: speakers_view(layout),
        ..speaker_layout_profile(language)
    }
}

/// Headphones through an Impulcifer 3.x layout file (ADR 0042): one
/// direction per channel but the LFEs, named and placed as the layout's
/// speakers, in channel order. The profile is named after the folder.
fn layout_file_profile(folder: &str, layout: SpeakerLayout) -> ProfileView {
    ProfileView {
        kind: ProfileKind::Headphones,
        name: folder.to_owned(),
        gain_db: -3.0,
        directions: speaker_directions(layout),
        from_file: false,
        headphones: HeadphonesView {
            brir: Some(format!(r"C:\Users\me\BRIR\{folder}\{}.wav", layout.name())),
        },
        speakers: speakers_view(SpeakerLayout::Surround714),
    }
}

/// A 22.2 source's objects in the main lane: the front, two of the top
/// layer and the three below the ears, at the speakers' nominal angles.
fn scene_22_2() -> Vec<SceneObject> {
    let object = |index: u8, azimuth: f32, elevation: f32, level_dbfs: f32| {
        let (azimuth, elevation) = (azimuth.to_radians(), elevation.to_radians());
        // Adding 0.0 writes an axis that is zero as 0.0 rather than -0.0.
        let round = |value: f32| (value * 1000.0).round() / 1000.0 + 0.0;
        SceneObject {
            lane: 0,
            index,
            x: round(-azimuth.sin() * elevation.cos()),
            y: round(azimuth.cos() * elevation.cos()),
            z: round(elevation.sin()),
            level_dbfs,
        }
    };
    vec![
        object(2, 0.0, 0.0, -10.0),
        object(6, 30.0, 0.0, -14.0),
        object(7, -30.0, 0.0, -16.0),
        object(14, 0.0, 45.0, -20.0),
        object(15, 0.0, 90.0, -28.0),
        object(21, 0.0, -30.0, -18.0),
        object(22, 45.0, -30.0, -24.0),
        object(23, -45.0, -30.0, -26.0),
    ]
}

fn profile_file(language: Language) -> ProfileView {
    ProfileView {
        kind: ProfileKind::Headphones,
        name: match language {
            Language::Ko => "작업실 7.1.4",
            Language::En => "Studio 7.1.4",
        }
        .to_owned(),
        gain_db: 0.0,
        directions: headphone_profile(language).directions,
        from_file: true,
        headphones: HeadphonesView {
            brir: Some(r"C:\Users\me\BRIR\studio\hesuvi.wav".to_owned()),
        },
        speakers: speakers_view(SpeakerLayout::Stereo),
    }
}

/// No upmixer chosen anywhere: both endpoints follow the default, which is
/// off until the first-party upmixer exists (ADR 0035).
fn default_upmix() -> UpmixView {
    UpmixView {
        endpoints: (0..3)
            .map(|endpoint| EndpointUpmixView {
                endpoint,
                choice: None,
                effective: UpmixerChoice::Off,
                plugin: None,
                state: UpmixState::Stopped,
                bypass: None,
                latency_ms: None,
                inputs: Vec::new(),
                outputs: Vec::new(),
            })
            .collect(),
        headphones: true,
        speakers: true,
        first_party_available: false,
    }
}

/// A made-up VST 3 upmixer that takes stereo and returns 5.1.
fn example_plugin() -> PluginView {
    PluginView {
        path: r"C:\Program Files\Common Files\VST3\Example Upmixer.vst3".to_owned(),
        format: PluginFormat::Vst3,
        name: "Example Upmixer".to_owned(),
        vendor: "Example Audio".to_owned(),
    }
}

/// The music endpoint goes through the example plugin; the others are
/// explicitly off.
fn music_upmix(state: UpmixState) -> UpmixView {
    plugin_upmix(1, state)
}

/// `endpoint` goes through the example plugin; the others are explicitly off.
fn plugin_upmix(endpoint: usize, state: UpmixState) -> UpmixView {
    let mut upmix = default_upmix();
    for view in &mut upmix.endpoints {
        view.choice = Some(UpmixerChoice::Off);
    }
    let running = matches!(state, UpmixState::Running | UpmixState::Fallback);
    let music = &mut upmix.endpoints[endpoint];
    music.choice = Some(UpmixerChoice::Plugin);
    music.effective = UpmixerChoice::Plugin;
    music.plugin = Some(example_plugin());
    music.state = state;
    music.latency_ms = running.then_some(23.3);
    music.inputs = ["L", "R"].map(str::to_owned).to_vec();
    music.outputs = ["L", "R", "C", "LFE", "Ls", "Rs"]
        .map(str::to_owned)
        .to_vec();
    upmix
}

fn blank(language: Language, phase: Phase) -> Snapshot {
    Snapshot {
        language,
        language_choice: None,
        skin: Skin::Standard,
        output_path: ssr_types::app::OutputPath::Wasapi,
        phase,
        profile: stereo_profile(language),
        upmix: default_upmix(),
        routing: RoutingView::default(),
        asio: AsioSettings::default(),
        engine: None,
        health: Default::default(),
        outputs: Vec::new(),
        chosen_output: None,
        driver: None,
        platform: None,
        roles_taken: false,
        error: None,
        notes: Vec::new(),
    }
}

fn with_phase(mut snapshot: Snapshot, phase: Phase) -> Snapshot {
    snapshot.phase = phase;
    snapshot
}

fn minute(state: MinuteState, plugins: MinuteState) -> HealthMinute {
    HealthMinute {
        driver: state,
        engine: state,
        output: state,
        plugins,
        underruns: 0,
        output_recoveries: 0,
    }
}

fn fixture_health(name: &str, phase: &Phase) -> HealthHistory {
    if name.starts_with("setup-") {
        return HealthHistory::default();
    }
    if *phase != Phase::Running && *phase != Phase::Faulted {
        let mut minutes = vec![minute(MinuteState::NoRecord, MinuteState::NoRecord); 30];
        minutes.extend((0..60).map(|_| HealthMinute {
            driver: MinuteState::Good,
            engine: MinuteState::Idle,
            output: MinuteState::Idle,
            plugins: MinuteState::Idle,
            underruns: 0,
            output_recoveries: 0,
        }));
        return HealthHistory {
            minutes,
            incidents: Vec::new(),
        };
    }

    let plugin_running = name.contains("upmix");
    let mut minutes = vec![minute(MinuteState::NoRecord, MinuteState::NoRecord); 25];
    minutes.extend((0..5).map(|_| HealthMinute {
        driver: MinuteState::Good,
        engine: MinuteState::Idle,
        output: MinuteState::Idle,
        plugins: MinuteState::Idle,
        underruns: 0,
        output_recoveries: 0,
    }));
    minutes.extend((0..60).map(|_| HealthMinute {
        driver: MinuteState::Good,
        engine: MinuteState::Good,
        output: MinuteState::Good,
        plugins: if plugin_running {
            MinuteState::Good
        } else {
            MinuteState::Idle
        },
        underruns: 0,
        output_recoveries: 0,
    }));
    minutes[49].engine = MinuteState::Degraded;
    minutes[49].output = MinuteState::Degraded;
    minutes[49].underruns = 1;
    let mut incidents = vec![
        HealthIncident {
            part: HealthPart::Engine,
            worst: MinuteState::Degraded,
            first: 49,
            last: 49,
            open: false,
            underruns: 1,
            output_recoveries: 0,
        },
        HealthIncident {
            part: HealthPart::Output,
            worst: MinuteState::Degraded,
            first: 49,
            last: 49,
            open: false,
            underruns: 0,
            output_recoveries: 0,
        },
    ];

    if name == "running-upmix-fallback" {
        for entry in &mut minutes[87..] {
            entry.plugins = MinuteState::Degraded;
        }
        incidents.push(HealthIncident {
            part: HealthPart::Plugins,
            worst: MinuteState::Degraded,
            first: 87,
            last: 89,
            open: true,
            underruns: 0,
            output_recoveries: 0,
        });
    }
    if name == "running-reconnecting" {
        for entry in &mut minutes[88..] {
            entry.output = MinuteState::Down;
        }
        incidents.push(HealthIncident {
            part: HealthPart::Output,
            worst: MinuteState::Down,
            first: 88,
            last: 89,
            open: true,
            underruns: 0,
            output_recoveries: 0,
        });
    }
    if *phase == Phase::Faulted {
        for entry in &mut minutes[87..] {
            entry.engine = MinuteState::Down;
            entry.output = MinuteState::Down;
        }
        incidents.extend([
            HealthIncident {
                part: HealthPart::Engine,
                worst: MinuteState::Down,
                first: 87,
                last: 89,
                open: true,
                underruns: 0,
                output_recoveries: 0,
            },
            HealthIncident {
                part: HealthPart::Output,
                worst: MinuteState::Down,
                first: 87,
                last: 89,
                open: true,
                underruns: 0,
                output_recoveries: 0,
            },
        ]);
    }

    HealthHistory { minutes, incidents }
}

/// The running engine with the music endpoint playing `format`.
fn engine_with_music(language: Language, format: SourceFormat) -> EngineStatus {
    let mut engine = engine_status(language);
    engine.endpoints[1].format = Some(format);
    engine
}

/// The running engine with the PASS endpoint playing `format`, not muted.
fn engine_with_pass(language: Language, format: SourceFormat) -> EngineStatus {
    let mut engine = engine_status(language);
    let pass = &mut engine.endpoints[2];
    pass.format = Some(format);
    pass.muted = false;
    pass.unsupported = None;
    pass.chain_errors = ChainErrorCounts::default();
    engine
}

/// Stereo PCM at 48 kHz: what the example upmixer takes.
fn stereo_pcm() -> SourceFormat {
    SourceFormat::Pcm(PcmFormat {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 32,
        valid_bits: 32,
        sample_type: SampleType::Float,
        channel_mask: 0x3,
    })
}

/// An E-AC-3 stream with `channels` encoded channels.
fn eac3(channels: u32) -> SourceFormat {
    SourceFormat::Bitstream(BitstreamFormat {
        codec: BitstreamCodec::Eac3,
        container_rate: 192_000,
        container_channels: 2,
        encoded_rate: 48_000,
        encoded_channels: channels,
    })
}

fn engine_status(language: Language) -> EngineStatus {
    let mut endpoints: [EndpointStatus; 4] = Default::default();
    endpoints[0] = EndpointStatus {
        format: Some(SourceFormat::Pcm(PcmFormat {
            channels: 8,
            sample_rate: 48_000,
            bits_per_sample: 32,
            valid_bits: 32,
            sample_type: SampleType::Float,
            channel_mask: 0x63f,
        })),
        epoch: 3,
        layout: None,
        volume_db: -6.0,
        muted: false,
        unsupported: None,
        chain_errors: ChainErrorCounts::default(),
    };
    endpoints[1] = EndpointStatus {
        format: Some(SourceFormat::Bitstream(BitstreamFormat {
            codec: BitstreamCodec::Eac3,
            container_rate: 192_000,
            container_channels: 2,
            encoded_rate: 48_000,
            encoded_channels: 6,
        })),
        epoch: 1,
        layout: None,
        volume_db: 0.0,
        muted: false,
        unsupported: None,
        chain_errors: ChainErrorCounts::from_fn(|kind| match kind {
            ChainErrorKind::DecoderBehind => 2,
            _ => 0,
        }),
    };
    endpoints[2] = EndpointStatus {
        format: Some(SourceFormat::Pcm(PcmFormat {
            channels: 12,
            sample_rate: 96_000,
            bits_per_sample: 24,
            valid_bits: 24,
            sample_type: SampleType::Int,
            channel_mask: 0,
        })),
        epoch: 2,
        layout: None,
        volume_db: 0.0,
        muted: true,
        unsupported: Some(UnsupportedReason::DirectLayout),
        chain_errors: ChainErrorCounts::from_fn(|kind| match kind {
            ChainErrorKind::MissingDirectLayout => 41,
            _ => 0,
        }),
    };
    EngineStatus {
        asio: None,
        directions: headphone_profile(language).directions,
        scene: vec![
            SceneObject {
                lane: 0,
                index: 0,
                x: -0.5,
                y: 0.866,
                z: 0.0,
                level_dbfs: -12.0,
            },
            SceneObject {
                lane: 0,
                index: 1,
                x: 0.5,
                y: 0.866,
                z: 0.0,
                level_dbfs: -18.0,
            },
            SceneObject {
                lane: 1,
                index: 0,
                x: 0.0,
                y: -1.0,
                z: 0.0,
                level_dbfs: -24.0,
            },
            SceneObject {
                lane: 2,
                index: 3,
                x: 0.25,
                y: 0.433,
                z: 0.866,
                level_dbfs: -30.0,
            },
        ],
        endpoints,
        unattributed_chain_errors: ChainErrorCounts::default(),
        skipped_records: 0,
        latency: LatencyStats {
            p50_ms: 12.4,
            p99_ms: 18.9,
            samples: 300,
        },
        handoff_latency: LatencyStats {
            p50_ms: 8.4,
            p99_ms: 14.9,
            samples: 300,
        },
        device_latency: LatencyStats {
            p50_ms: 4.0,
            p99_ms: 4.0,
            samples: 300,
        },
        ring_level_ms: 9.6,
        underruns: 1,
        last_underrun: Some(UnderrunSnapshot {
            ring_frames: 96,
            marker_frames: 96,
            layout_frames: 480,
            needed_frames: 480,
            produced_frames: 96,
            since_block_ms: 31.5,
        }),
        output_periods: 12_345,
        capture_reads: 6_789,
        output_restarts: 0,
        output_late_resyncs: 0,
        output_rate: 48_000,
        output_period_frames: 480,
        trim_available: true,
        output_channels: 2,
        output_channel_mask: 3,
        output_reopens: 1,
        output_reconnects: 0,
        output_reconnect_attempts: 0,
        output_mode: OutputMode::Exclusive,
        output_fell_back: false,
        output_ks_fallback: false,
        output_ks_fallback_reason: None,
        output_ks_fallback_code: 0,
        output_mix_channels: 0,
        output_cadence_ratio: 1.001,
        output_cadence_faults: 0,
        lost_bytes: 0,
        lane_underrun_frames: 1_920,
        dropped_frames: 0,
        decode_refused: 2,
        volume_transitions_coalesced: 0,
        output_overflow_frames: 0,
        idle_blocks: 1_204,
        dropped_events: 0,
        clock: ClockStatus::Asrc,
        clock_ppm: -37.5,
        fault: None,
    }
}

/// A DAW holding the ASIO input (ADR 0025) at the output's 48 kHz.
fn asio_client(layout: AsioLayout, buffer_frames: u32) -> ssr_types::AsioStatus {
    ssr_types::AsioStatus {
        client_pid: 9_120,
        client_exe: "reaper.exe".into(),
        sample_rate: 48_000,
        buffer_frames,
        channels: ssr_types::channel_layout_preset(layout.name())
            .unwrap()
            .len() as u32,
        layout,
        underruns: 0,
        generation: 1,
        clock_fault: false,
        fault_reason: None,
    }
}

/// The ASIO input whose start failed and has no DAW, as the engine reports it
/// (crates/engine/src/asio.rs `AsioShared::status`): the counters at rest,
/// the clock marked faulty and the transport's reason.
fn asio_fault(reason: &str) -> ssr_types::AsioStatus {
    ssr_types::AsioStatus {
        client_pid: 0,
        client_exe: String::new(),
        sample_rate: 0,
        buffer_frames: 0,
        channels: 0,
        layout: AsioLayout::Surround714,
        underruns: 0,
        generation: 0,
        clock_fault: true,
        fault_reason: Some(reason.to_owned()),
    }
}

/// The built-in rules with one turned off, one changed and one rule of the
/// user's own, as the supervisor lists them.
fn rules_with_overrides() -> Vec<RuleView> {
    let mut rules = RoutingView::default().rules;
    for row in &mut rules {
        match row.exe.as_str() {
            "kodi.exe" => row.entrance = None,
            "mpc-hc.exe" => row.entrance = Some(Entrance::Main),
            _ => {}
        }
    }
    rules.push(RuleView {
        exe: "player.exe".into(),
        entrance: Some(Entrance::Music),
        built_in: None,
    });
    rules.sort_by(|left, right| left.exe.cmp(&right.exe));
    rules
}

/// One snapshot per screen state the wizard, the status screen and the error
/// banner must draw. The messages are the supervisor's own sentences.
fn snapshots(language: Language) -> Vec<(&'static str, Snapshot)> {
    let output_choices = outputs(language);
    let selected_name = output_choices[1].name.clone();
    let ready = Snapshot {
        routing: RoutingView {
            apps: vec![
                RoutedApp {
                    exe: "applemusic.exe".into(),
                    display_name: "Apple Music".into(),
                    entrance_now: Some(Entrance::Main),
                    rule: Some(Entrance::Music),
                    moved: true,
                },
                RoutedApp {
                    exe: "browser.exe".into(),
                    display_name: "Browser".into(),
                    entrance_now: Some(Entrance::Main),
                    rule: None,
                    moved: false,
                },
            ],
            rules: rules_with_overrides(),
            ..RoutingView::default()
        },
        outputs: output_choices.clone(),
        chosen_output: Some(output_choices[1].clone()),
        driver: Some(installed(PolicyState::Applied)),
        platform: Some(platform(true, SecureBoot::Disabled)),
        ..blank(language, Phase::Ready)
    };
    let text = |ko: &'static str, en: &'static str| match language {
        Language::Ko => ko,
        Language::En => en,
    };
    let mut renamed_outputs = output_choices.clone();
    renamed_outputs[1].id =
        DeviceId("{0.0.0.00000000}.{5e8a2c44-74fb-4e6d-b257-82ea4b12f649}".to_owned());
    let renamed_output = renamed_outputs[1].clone();
    let mut outputs_without_saved = output_choices.clone();
    outputs_without_saved.remove(1);
    let output_id_changed = match language {
        Language::Ko => {
            format!("출력 장치의 식별자가 바뀌어 이름으로 다시 찾았습니다: {selected_name}")
        }
        Language::En => format!(
            "The output device identifier changed, so it was found again by name: {selected_name}"
        ),
    };
    let stored_output_missing = match language {
        Language::Ko => format!("저장된 출력 장치를 찾을 수 없습니다: {selected_name}"),
        Language::En => format!("The saved output device could not be found: {selected_name}"),
    };
    let brir_detail =
        r"C:\Users\me\BRIR\studio\hesuvi.wav has 2 tracks, which matches no HeSuVi layout";
    // What the engine reports, as it reports it: a removed output is the
    // fault's detail (engine publish_output_error), and a busy one fails the
    // start through StartError::Output (crates/engine/src/runtime/mod.rs).
    let lost = OutputError::Removed.to_string();
    let busy = format!(
        "the output device could not be opened: {}",
        OutputError::Busy
    );
    // What the engine's ASIO control thread keeps when the transport does
    // not open (engine AsioTransportError::Transport), and the note the
    // supervisor makes of it once.
    let asio_reason = "ASIO transport failed: owner_exists";
    let asio_unavailable = match language {
        Language::Ko => {
            format!("ASIO 입구를 시작하지 못했습니다. 시스템 소리는 계속 재생합니다: {asio_reason}")
        }
        Language::En => {
            format!("The ASIO input could not start. System audio continues: {asio_reason}")
        }
    };
    let mut fixtures = vec![
        (
            "setup-welcome",
            blank(language, Phase::Setup(SetupStep::Welcome)),
        ),
        (
            "setup-unsupported",
            Snapshot {
                platform: Some(Platform {
                    os_build: 18_363,
                    ..platform(false, SecureBoot::Enabled)
                }),
                error: Some(error(
                    text(
                        "Windows 10 2004(빌드 19041) 이상이 필요합니다.",
                        "Windows 10 version 2004 (build 19041) or newer is required.",
                    ),
                    NextAction::None,
                )),
                ..blank(language, Phase::Setup(SetupStep::Unsupported))
            },
        ),
        (
            "setup-not-test-signing",
            Snapshot {
                platform: Some(platform(false, SecureBoot::Enabled)),
                error: Some(error(
                    text(
                        "시험서명 모드가 꺼져 있습니다. Secure Boot를 끈 뒤 관리자 명령으로 bcdedit /set testsigning on을 실행하고 다시 시작하십시오. Secure Boot가 켜져 있어 먼저 꺼야 합니다.",
                        "Test-signing mode is off. Turn off Secure Boot, run bcdedit /set testsigning on as administrator, and restart Windows. Secure Boot is on and must be turned off first.",
                    ),
                    NextAction::Reboot,
                )),
                ..blank(language, Phase::Setup(SetupStep::NotTestSigning))
            },
        ),
        (
            "setup-driver-not-installed",
            Snapshot {
                platform: Some(platform(true, SecureBoot::Disabled)),
                driver: Some(DriverState::NotInstalled),
                ..blank(language, Phase::Setup(SetupStep::Driver))
            },
        ),
        (
            "setup-driver-problem",
            Snapshot {
                platform: Some(platform(true, SecureBoot::Disabled)),
                driver: Some(DriverState::Installed {
                    problem_code: 52,
                    service_running: false,
                    exclusive_policy: PolicyState::Unknown,
                }),
                error: Some(error(
                    text(
                        "Windows가 드라이버 서명을 확인하지 못했습니다(문제 코드 52). 시험서명 모드를 확인한 뒤 다시 설치하십시오.",
                        "Windows could not verify the driver signature (problem code 52). Check test-signing mode, then reinstall the driver.",
                    ),
                    NextAction::Retry,
                )),
                ..blank(language, Phase::Setup(SetupStep::Driver))
            },
        ),
        (
            "setup-driver-declined",
            Snapshot {
                platform: Some(platform(true, SecureBoot::Disabled)),
                driver: Some(DriverState::NotInstalled),
                error: Some(error(
                    text(
                        "관리자 승인을 취소했습니다.",
                        "Administrator approval was cancelled.",
                    ),
                    NextAction::Retry,
                )),
                ..blank(language, Phase::Setup(SetupStep::Driver))
            },
        ),
        (
            "setup-reboot-required",
            Snapshot {
                platform: Some(platform(true, SecureBoot::Disabled)),
                driver: Some(DriverState::RebootRequired),
                error: Some(error(
                    text(
                        "드라이버 설치를 마치려면 다시 시작해야 합니다.",
                        "Restart Windows to finish installing the driver.",
                    ),
                    NextAction::Reboot,
                )),
                ..blank(language, Phase::Setup(SetupStep::RebootRequired))
            },
        ),
        (
            "setup-choose-output",
            Snapshot {
                outputs: output_choices.clone(),
                driver: Some(installed(PolicyState::Applied)),
                platform: Some(platform(true, SecureBoot::Disabled)),
                ..blank(language, Phase::Setup(SetupStep::ChooseOutput))
            },
        ),
        (
            "setup-output-missing",
            Snapshot {
                outputs: outputs_without_saved,
                chosen_output: Some(output_choices[1].clone()),
                driver: Some(installed(PolicyState::Applied)),
                platform: Some(platform(true, SecureBoot::Disabled)),
                error: Some(error(&stored_output_missing, NextAction::OpenSettings)),
                ..blank(language, Phase::Setup(SetupStep::ChooseOutput))
            },
        ),
        (
            "setup-choose-output-empty",
            Snapshot {
                driver: Some(installed(PolicyState::Applied)),
                platform: Some(platform(true, SecureBoot::Disabled)),
                ..blank(language, Phase::Setup(SetupStep::ChooseOutput))
            },
        ),
        ("ready", ready.clone()),
        (
            "ready-output-renamed",
            Snapshot {
                outputs: renamed_outputs,
                chosen_output: Some(renamed_output),
                notes: vec![output_id_changed],
                ..ready.clone()
            },
        ),
        (
            "running",
            Snapshot {
                profile: headphone_profile(language),
                engine: Some(engine_status(language)),
                roles_taken: true,
                notes: vec![text("다시 연결했습니다.", "Reconnected.").to_owned()],
                ..with_phase(ready.clone(), Phase::Running)
            },
        ),
        (
            "running-reconnecting",
            Snapshot {
                profile: headphone_profile(language),
                engine: Some(EngineStatus {
                    fault: Some(EngineFault::OutputLost(lost.clone())),
                    ..engine_status(language)
                }),
                roles_taken: true,
                error: None,
                ..with_phase(ready.clone(), Phase::Running)
            },
        ),
        (
            "faulted",
            Snapshot {
                error: Some(error(
                    &match language {
                        Language::Ko => format!(
                            "엔진이 15초 동안 다시 연결하지 못했습니다. 출력 장치와의 연결이 끊어졌습니다: {lost}"
                        ),
                        Language::En => format!(
                            "The engine could not reconnect for 15 seconds. The output device disconnected: {lost}"
                        ),
                    },
                    NextAction::Retry,
                )),
                ..with_phase(ready.clone(), Phase::Faulted)
            },
        ),
        (
            "ready-headphones",
            Snapshot {
                profile: headphone_profile(language),
                ..ready.clone()
            },
        ),
        (
            "ready-speakers",
            Snapshot {
                profile: speaker_profile(language),
                ..ready.clone()
            },
        ),
        (
            "running-speakers",
            Snapshot {
                profile: speaker_profile(language),
                // The engine plays the correction's inputs and opens its 13
                // device channels direct (two subwoofers: no Windows mask).
                engine: Some(EngineStatus {
                    directions: speaker_profile(language).directions,
                    output_channels: 13,
                    output_channel_mask: 0,
                    ..engine_status(language)
                }),
                roles_taken: true,
                ..with_phase(ready.clone(), Phase::Running)
            },
        ),
        (
            "ready-layout-unsupported",
            Snapshot {
                profile: speaker_layout_profile(language),
                error: Some(error(
                    text(
                        "고른 출력 장치가 독점 모드에서 7.1.4 배치(12채널)를 지원하지 않습니다. 다른 배치나 출력 장치를 고르십시오.",
                        "The selected output device does not support the 7.1.4 layout (12 channels) in exclusive mode. Choose another layout or output device.",
                    ),
                    NextAction::None,
                )),
                ..ready.clone()
            },
        ),
        (
            // Speakers by the 30.2 layout: 32 device channels, two of them
            // subwoofers, and three directions below the ears (ADR 0042).
            "ready-speakers-30-2",
            Snapshot {
                profile: speaker_layout_named(language, SpeakerLayout::Surround302),
                ..ready.clone()
            },
        ),
        (
            // Headphones through a 24.1.10 layout file: 34 directions.
            "ready-headphones-24-1-10",
            Snapshot {
                profile: layout_file_profile("mix-stage", SpeakerLayout::Surround24110),
                ..ready.clone()
            },
        ),
        (
            // Headphones through a 22.2 layout file while a 22.2 source
            // plays direct (24 channels, no mask); three directions and three
            // objects are below the ears.
            "running-headphones-22-2",
            Snapshot {
                profile: layout_file_profile("hall", SpeakerLayout::Surround222),
                engine: Some({
                    let mut engine = engine_status(language);
                    engine.endpoints[0].format = Some(SourceFormat::Pcm(PcmFormat {
                        channels: 24,
                        sample_rate: 48_000,
                        bits_per_sample: 32,
                        valid_bits: 32,
                        sample_type: SampleType::Float,
                        channel_mask: 0,
                    }));
                    EngineStatus {
                        directions: layout_file_profile("hall", SpeakerLayout::Surround222)
                            .directions,
                        scene: scene_22_2(),
                        ..engine
                    }
                }),
                roles_taken: true,
                ..with_phase(ready.clone(), Phase::Running)
            },
        ),
        (
            "ready-upmix",
            Snapshot {
                upmix: music_upmix(UpmixState::Stopped),
                ..ready.clone()
            },
        ),
        (
            "running-upmix",
            Snapshot {
                profile: headphone_profile(language),
                upmix: music_upmix(UpmixState::Running),
                engine: Some(engine_with_music(language, stereo_pcm())),
                roles_taken: true,
                ..with_phase(ready.clone(), Phase::Running)
            },
        ),
        (
            "running-upmix-fallback",
            Snapshot {
                profile: headphone_profile(language),
                upmix: music_upmix(UpmixState::Fallback),
                engine: Some(engine_with_music(language, stereo_pcm())),
                roles_taken: true,
                ..with_phase(ready.clone(), Phase::Running)
            },
        ),
        (
            "running-upmix-pass",
            Snapshot {
                profile: headphone_profile(language),
                upmix: plugin_upmix(2, UpmixState::Running),
                engine: Some(engine_with_pass(language, eac3(2))),
                roles_taken: true,
                ..with_phase(ready.clone(), Phase::Running)
            },
        ),
        (
            "running-upmix-native-surround",
            Snapshot {
                profile: headphone_profile(language),
                upmix: {
                    let mut upmix = plugin_upmix(2, UpmixState::Running);
                    upmix.endpoints[2].bypass = Some(UpmixBypass::NativeSurround);
                    upmix
                },
                engine: Some(engine_with_pass(language, eac3(6))),
                roles_taken: true,
                ..with_phase(ready.clone(), Phase::Running)
            },
        ),
        (
            "ready-upmix-failed",
            Snapshot {
                upmix: music_upmix(UpmixState::Failed(
                    text(
                        "업믹스 플러그인이 10분 안에 세 번 멈춰 이번 실행에서는 껐습니다: Example Upmixer",
                        "The upmixer plugin stopped three times within 10 minutes, so it is off until the app restarts: Example Upmixer",
                    )
                    .to_owned(),
                )),
                ..ready.clone()
            },
        ),
        (
            "ready-speakers-upmix-off",
            Snapshot {
                profile: speaker_profile(language),
                upmix: UpmixView {
                    speakers: false,
                    ..music_upmix(UpmixState::Stopped)
                },
                ..ready.clone()
            },
        ),
        (
            "ready-profile-file",
            Snapshot {
                profile: profile_file(language),
                ..ready.clone()
            },
        ),
        (
            "ready-brir-unreadable",
            Snapshot {
                error: Some(error(
                    &match language {
                        Language::Ko => format!("BRIR 세트를 읽지 못했습니다: {brir_detail}"),
                        Language::En => {
                            format!("The BRIR set could not be read: {brir_detail}")
                        }
                    },
                    NextAction::None,
                )),
                ..ready.clone()
            },
        ),
        (
            "ready-start-failed",
            Snapshot {
                error: Some(error(
                    &match language {
                        Language::Ko => format!("엔진을 시작하지 못했습니다: {busy}"),
                        Language::En => format!("The engine could not start: {busy}"),
                    },
                    NextAction::Retry,
                )),
                ..ready.clone()
            },
        ),
        (
            // Kernel streaming chosen, and no app with sound at the moment.
            "ready-kernel-streaming",
            Snapshot {
                output_path: ssr_types::app::OutputPath::KernelStreaming,
                routing: RoutingView::default(),
                ..ready.clone()
            },
        ),
        (
            // Windows without the per-app output setting: the supervisor
            // lists no app (routing.rs clears them) but keeps the rules.
            "ready-routing-unavailable",
            Snapshot {
                routing: RoutingView {
                    available: false,
                    ..RoutingView::default()
                },
                ..ready.clone()
            },
        ),
        (
            // A DAW plays 7.1.4 into the ASIO input; system sound 12 dB down.
            "running-asio",
            Snapshot {
                profile: headphone_profile(language),
                asio: AsioSettings {
                    enabled: true,
                    layout: AsioLayout::Surround714,
                    system_attenuation_db: 12.0,
                },
                engine: Some(EngineStatus {
                    asio: Some(asio_client(AsioLayout::Surround714, 256)),
                    ..engine_status(language)
                }),
                roles_taken: true,
                ..with_phase(ready.clone(), Phase::Running)
            },
        ),
        (
            // The output clock stopped under a 5.1 DAW: the stand-in clock
            // drives it and the output found its queue empty meanwhile.
            "running-asio-clock-fault",
            Snapshot {
                profile: headphone_profile(language),
                asio: AsioSettings {
                    layout: AsioLayout::Surround51,
                    ..AsioSettings::default()
                },
                engine: Some(EngineStatus {
                    asio: Some(ssr_types::AsioStatus {
                        underruns: 37,
                        generation: 2,
                        clock_fault: true,
                        ..asio_client(AsioLayout::Surround51, 128)
                    }),
                    ..engine_status(language)
                }),
                roles_taken: true,
                ..with_phase(ready.clone(), Phase::Running)
            },
        ),
        (
            // The ASIO input did not start; system sound plays on.
            "running-asio-fault",
            Snapshot {
                profile: headphone_profile(language),
                engine: Some(EngineStatus {
                    asio: Some(asio_fault(asio_reason)),
                    ..engine_status(language)
                }),
                roles_taken: true,
                notes: vec![asio_unavailable],
                ..with_phase(ready.clone(), Phase::Running)
            },
        ),
        (
            // The ASIO input off; its layout and attenuation are kept.
            "ready-asio-off",
            Snapshot {
                asio: AsioSettings {
                    enabled: false,
                    layout: AsioLayout::Stereo,
                    system_attenuation_db: 20.0,
                },
                ..ready.clone()
            },
        ),
        (
            // A DAW may send 24.1.10, 35 channels, through the ASIO input.
            "ready-asio-24-1-10",
            Snapshot {
                asio: AsioSettings {
                    layout: AsioLayout::Surround24110,
                    ..AsioSettings::default()
                },
                ..ready.clone()
            },
        ),
        ("quit", with_phase(ready, Phase::Quit)),
    ];
    for (name, snapshot) in &mut fixtures {
        snapshot.health = fixture_health(name, &snapshot.phase);
    }
    fixtures
}

/// Every intent, as the screen must send it.
fn intents() -> Vec<Intent> {
    vec![
        Intent::SetLanguage(None),
        Intent::SetLanguage(Some(Language::Ko)),
        Intent::SetLanguage(Some(Language::En)),
        Intent::SetSkin(Skin::Standard),
        Intent::SetSkin(Skin::Panel),
        Intent::SetSkin(Skin::Console),
        Intent::SetOutputPath(ssr_types::app::OutputPath::KernelStreaming),
        Intent::SetOutputPath(ssr_types::app::OutputPath::Wasapi),
        Intent::SetRoutingEnabled(false),
        Intent::SetAppRule(AppRule {
            exe: "PLAYER.EXE".into(),
            entrance: Some(Entrance::Pass),
        }),
        Intent::SetAppRule(AppRule {
            exe: "kodi.exe".into(),
            entrance: None,
        }),
        Intent::RemoveAppRule("player.exe".into()),
        Intent::BeginSetup,
        Intent::InstallDriver,
        Intent::Recheck,
        Intent::ChooseOutput(outputs(Language::Ko)[1].id.clone()),
        Intent::SetProfileKind(ProfileKind::Headphones),
        Intent::SetProfileKind(ProfileKind::Speakers),
        Intent::ImportBrir(PathBuf::from(r"C:\Users\me\BRIR\living-room")),
        Intent::SetSpeakerLayout(SpeakerLayout::Surround714),
        Intent::ImportCorrection(PathBuf::from(
            r"C:\Users\me\Documents\SECS_export\2026-10-16 living room\configs\secs_mixed.yml",
        )),
        Intent::ClearCorrection,
        Intent::SetAsioEnabled(true),
        Intent::SetAsioEnabled(false),
        Intent::SetAsioLayout(AsioLayout::Stereo),
        Intent::SetAsioLayout(AsioLayout::Surround51),
        Intent::SetAsioLayout(AsioLayout::Surround71),
        Intent::SetAsioLayout(AsioLayout::Surround714),
        Intent::SetAsioLayout(AsioLayout::Surround91),
        Intent::SetAsioLayout(AsioLayout::Surround914),
        Intent::SetAsioLayout(AsioLayout::Surround916),
        Intent::SetAsioLayout(AsioLayout::Surround131),
        Intent::SetAsioLayout(AsioLayout::Surround222),
        Intent::SetAsioLayout(AsioLayout::Surround24110),
        Intent::SetAsioLayout(AsioLayout::Surround302),
        Intent::SetAsioSystemAttenuation(0.0),
        Intent::SetAsioSystemAttenuation(60.0),
        Intent::SetGain(-3.5),
        Intent::TestDirection(DirectionId(7)),
        Intent::SetUpmixer(UpmixerSetting {
            endpoint: 1,
            choice: UpmixerChoice::Plugin,
        }),
        Intent::SetUpmixer(UpmixerSetting {
            endpoint: 0,
            choice: UpmixerChoice::Off,
        }),
        Intent::ChoosePlugin(PluginChoice {
            endpoint: 1,
            path: PathBuf::from(r"C:\Program Files\Common Files\VST3\Example Upmixer.vst3"),
        }),
        Intent::OpenPluginEditor(1),
        Intent::SetUpmixApply(UpmixApply {
            kind: ProfileKind::Speakers,
            on: false,
        }),
        Intent::Enable,
        Intent::Disable,
        Intent::StartDiagnosticRecording,
        Intent::Quit,
    ]
}

fn pretty<T: serde::Serialize>(value: &T) -> String {
    let mut json = serde_json::to_string_pretty(value).unwrap();
    json.push('\n');
    json
}

fn json_files(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

fn reject_extra_json(dir: &Path, expected: &BTreeSet<String>) {
    if std::env::var_os(BLESS).is_none() {
        let actual = json_files(dir);
        assert_eq!(
            actual,
            *expected,
            "unexpected JSON fixtures in {}",
            dir.display()
        );
    }
}

#[test]
fn snapshot_fixtures_match_the_types() {
    let root = ui_dir().join("fixtures");
    let en = root.join("en");
    let fixtures = snapshots(Language::Ko);
    let names: Vec<_> = fixtures.iter().map(|(name, _)| *name).collect();

    for (language, dir) in [(Language::Ko, &root), (Language::En, &en)] {
        for (name, snapshot) in snapshots(language) {
            let content = pretty(&snapshot);
            check_or_bless(&dir.join(format!("{name}.json")), &content);
            let back: Snapshot = serde_json::from_str(&content).unwrap();
            assert_eq!(back, snapshot, "{name} does not round-trip in {language:?}");
            assert_eq!(snapshot.language, language);
            assert_eq!(snapshot.language_choice, None);
            assert_eq!(snapshot.skin, Skin::Standard);
            if language == Language::En {
                assert!(
                    !content.chars().any(|character| matches!(
                        character as u32,
                        0xAC00..=0xD7A3 | 0x1100..=0x11FF | 0x3130..=0x318F
                    )),
                    "{name} contains Hangul in the English fixture"
                );
            }
        }
    }

    check_or_bless(&root.join("intents.json"), &pretty(&intents()));
    let back: Vec<Intent> = serde_json::from_str(&pretty(&intents())).unwrap();
    assert_eq!(back, intents());
    check_or_bless(&root.join("index.json"), &pretty(&names));

    let fixture_files: BTreeSet<_> = names.iter().map(|name| format!("{name}.json")).collect();
    let mut root_files = fixture_files.clone();
    root_files.insert("index.json".to_owned());
    root_files.insert("intents.json".to_owned());
    reject_extra_json(&root, &root_files);
    reject_extra_json(&en, &fixture_files);
}
