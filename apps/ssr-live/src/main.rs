#![forbid(unsafe_code)]

//! ssr-live: development tool for the SpatialAudioLab test PC (ADR 0018).
//!
//! * `record` writes the SAL ring's record stream to a dump file, the input
//!   the MAT decoder and the black-box goldens are built from (ADR 0005).
//! * `play` runs SAL ring → engine → WASAPI with a BRIR profile, printing
//!   latency, crossing-ring level, underruns and clock correction every
//!   second; `--record` keeps a dump of the same stream. `--from-dump` plays
//!   a dump instead of the ring (`--loop` repeats it) and `--report` writes
//!   the run's figures as JSON, which is how CI soaks the live path.
//! * `devices` lists render endpoints.
//! * `host` drives the supervisor's Windows host adapter
//!   (`ssr_sys::WindowsHost`) and prints one JSON document per call (shapes
//!   in `host_json`): `show` the platform, driver, endpoints and default
//!   roles, `set-default` one role, `setup` one elevated `ssr-setup` task.
//!   CI checks the adapter with it (`tools/ci/host_check.py`,
//!   `tools/ci/host_role_check.py`).
//!
//! The SAL device opens only from an administrator prompt.

mod app_routing;
mod asio_probe;
#[cfg(windows)]
mod asio_transport;
mod bitstream_cli;
#[cfg_attr(not(windows), allow(dead_code))]
mod host_json;
#[cfg_attr(not(windows), allow(dead_code))]
mod ks_lifecycle;
mod mat_probe;
#[cfg(any(windows, test))]
mod silence;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use ssr_types::app::Language;
use ssr_types::{Role, SetupTask};

#[derive(Parser)]
#[command(
    version,
    about = "Spatial SoftReceiver live tool for the SAL development PC"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Clock {
    Auto,
    Asrc,
    Trim,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CaptureKind {
    Driver,
    Sal,
}

/// What only the command line knows for `--report` (source kind, requested
/// clock and output mode, direct layouts).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn request_fields(
    source: CaptureKind,
    from_dump: bool,
    clock: Clock,
    shared: bool,
    kernel_streaming: bool,
    direct_layouts: Vec<supervisor::DirectLayoutField>,
) -> supervisor::RequestFields {
    supervisor::RequestFields {
        source: if from_dump {
            "dump"
        } else {
            match source {
                CaptureKind::Driver => "driver",
                CaptureKind::Sal => "sal",
            }
        }
        .to_owned(),
        clock_requested: match clock {
            Clock::Auto => "auto",
            Clock::Asrc => "asrc",
            Clock::Trim => "trim",
        }
        .to_owned(),
        output_mode_requested: if kernel_streaming {
            "kernel_streaming"
        } else if shared {
            "shared"
        } else {
            "exclusive"
        }
        .to_owned(),
        direct_layouts,
    }
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn parse_endpoints(value: Option<&str>) -> Result<engine::EndpointFilter, String> {
    let Some(value) = value else {
        return Ok(engine::EndpointFilter::All);
    };
    let mut mask = 0_u32;
    let mut count = 0;
    for part in value.split(',') {
        let part = part.trim();
        let endpoint = part.parse::<u16>().map_err(|_| {
            ssr_i18n::live::invalid_endpoint(ssr_i18n::cli_language(), format_args!("{part:?}"))
        })?;
        if endpoint as usize >= ssr_abi::MAX_ENDPOINTS {
            return Err(ssr_i18n::live::endpoint_out_of_range(
                ssr_i18n::cli_language(),
                endpoint,
                ssr_abi::MAX_ENDPOINTS - 1,
            ));
        }
        let bit = 1_u32 << endpoint;
        if mask & bit != 0 {
            return Err(ssr_i18n::live::endpoint_duplicate(
                ssr_i18n::cli_language(),
                endpoint,
            ));
        }
        mask |= bit;
        count += 1;
    }
    if count == 0 {
        return Err(ssr_i18n::live::endpoints_empty(ssr_i18n::cli_language()).to_owned());
    }
    Ok(engine::EndpointFilter::Mask(mask))
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn select_endpoint(endpoints: &[(&str, &str, bool)], name: Option<&str>) -> Result<usize, String> {
    if let Some(name) = name {
        if let Some(index) = endpoints.iter().position(|e| e.0 == name) {
            return Ok(index);
        }
        let matches = endpoints
            .iter()
            .enumerate()
            .filter(|(_, e)| e.1.to_lowercase().contains(&name.to_lowercase()))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [index] => Ok(*index),
            [] => Err(
                ssr_i18n::supervisor::chosen_output_missing(ssr_i18n::cli_language()).to_owned(),
            ),
            _ => Err(ssr_i18n::live::output_not_unique(
                ssr_i18n::cli_language(),
                name,
            )),
        }
    } else {
        endpoints.iter().position(|e| e.2).ok_or_else(|| {
            ssr_i18n::supervisor::chosen_output_missing(ssr_i18n::cli_language()).to_owned()
        })
    }
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;

    #[test]
    fn ks_selection_is_exact_or_unambiguous() {
        let endpoints = [("a", "DAC speakers", false), ("b", "DAC headphones", true)];
        assert_eq!(select_endpoint(&endpoints, Some("a")), Ok(0));
        assert_eq!(select_endpoint(&endpoints, Some("HEADPHONES")), Ok(1));
        assert_eq!(select_endpoint(&endpoints, None), Ok(1));
        assert!(select_endpoint(&endpoints, Some("DAC")).is_err());
        assert!(select_endpoint(&endpoints, Some("missing")).is_err());
    }

    #[test]
    fn ks_cli_parsing_and_conflicts() {
        let cli = Cli::try_parse_from(["ssr-live", "ks-probe", "--output", "DAC"]).unwrap();
        assert!(matches!(cli.command, Command::KsProbe { seconds: 2.0, .. }));
        for seconds in ["0", "-1", "NaN", "inf"] {
            assert!(
                Cli::try_parse_from([
                    "ssr-live",
                    "ks-probe",
                    "--output",
                    "DAC",
                    "--seconds",
                    seconds
                ])
                .is_err()
            );
        }
        for conflict in ["--shared", "--allow-shared-fallback"] {
            assert!(
                Cli::try_parse_from([
                    "ssr-live",
                    "play",
                    "--profile",
                    "p.toml",
                    "--kernel-streaming",
                    conflict
                ])
                .is_err()
            );
        }
        assert!(Cli::try_parse_from(["ssr-live", "devices", "--kernel", "--json"]).is_ok());
    }

    #[test]
    fn english_help_is_the_derived_help_and_korean_help_is_complete() {
        ssr_i18n::clap_help::verify(Cli::command(), localized_command);
    }

    #[test]
    fn asio_play_accepts_driver_dump_and_explicit_no_driver_source() {
        for args in [
            vec!["--asio"],
            vec!["--asio", "--from-dump", "capture.ssrdump"],
            vec!["--asio", "--silence"],
        ] {
            let mut line = vec!["ssr-live", "play", "--profile", "profile.toml"];
            line.extend(args);
            let cli = Cli::try_parse_from(line).unwrap();
            let Command::Play { asio, .. } = cli.command else {
                panic!("expected play");
            };
            assert!(asio);
        }
        for args in [
            vec!["--silence"],
            vec!["--asio", "--silence", "--source", "driver"],
            vec!["--asio", "--silence", "--source", "sal"],
            vec!["--asio", "--silence", "--from-dump", "capture.ssrdump"],
        ] {
            let mut line = vec!["ssr-live", "play", "--profile", "profile.toml"];
            line.extend(args);
            assert!(Cli::try_parse_from(line).is_err());
        }
        let cli = Cli::try_parse_from(["ssr-live", "play", "--profile", "p"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Play {
                source: CaptureKind::Driver,
                asio: false,
                silence: false,
                ..
            }
        ));
    }

    #[test]
    fn asio_probe_requires_dll_and_validates_duration() {
        assert!(Cli::try_parse_from(["ssr-live", "asio-probe"]).is_err());
        for (seconds, valid) in [
            ("1", true),
            ("3600", true),
            ("0", false),
            ("NaN", false),
            ("inf", false),
            ("3601", false),
        ] {
            assert_eq!(
                Cli::try_parse_from([
                    "ssr-live",
                    "asio-probe",
                    "--dll",
                    "driver.dll",
                    "--seconds",
                    seconds,
                    "--json",
                    "probe.json"
                ])
                .is_ok(),
                valid
            );
        }
        let cli = Cli::try_parse_from(["ssr-live", "asio-probe", "--dll", "driver.dll"]).unwrap();
        let Command::AsioProbe { seconds, json, .. } = cli.command else {
            panic!("expected asio-probe");
        };
        assert_eq!(seconds, 5);
        assert_eq!(json, None);
    }

    #[test]
    fn scene_layout_lines_localize_sources_and_lfe() {
        use ssr_types::{SceneBedChannel, SceneDynamicObject, SceneLayout, SceneSource};
        for (source, english_label, korean_label) in [
            (SceneSource::Pcm, "PCM", "PCM"),
            (SceneSource::Ddp, "E-AC-3", "E-AC-3"),
            (SceneSource::DdpJoc, "E-AC-3 JOC", "E-AC-3 JOC"),
            (SceneSource::Upmix, "upmix", "업믹스"),
            (SceneSource::TrueHd, "TrueHD", "TrueHD"),
            (
                SceneSource::TrueHdObjects,
                "TrueHD objects",
                "TrueHD (위치 포함)",
            ),
        ] {
            let mut layout = SceneLayout {
                source,
                lfe: true,
                bed: (0..7)
                    .map(|index| SceneBedChannel {
                        index,
                        speaker: "L".into(),
                    })
                    .collect(),
                dynamic: (7..14)
                    .map(|index| SceneDynamicObject { index, room: None })
                    .collect(),
            };
            for (lfe, suffix) in [(true, " + LFE"), (false, "")] {
                layout.lfe = lfe;
                assert_eq!(
                    scene_layout_line(Language::En, 1, Some(&layout)),
                    format!(
                        "endpoint 1: {english_label}, bed 7 channels{suffix}, dynamic objects 7"
                    )
                );
                assert_eq!(
                    scene_layout_line(Language::Ko, 1, Some(&layout)),
                    format!(
                        "엔드포인트 1: {korean_label}, 고정 채널 7개{suffix}, 움직이는 소리 7개"
                    )
                );
            }
        }
        for (language, expected) in [
            (Language::En, "endpoint 1: scene layout unavailable"),
            (Language::Ko, "엔드포인트 1: 소리 배치 정보 없음"),
        ] {
            assert_eq!(scene_layout_line(language, 1, None), expected);
            let layout = SceneLayout {
                source: SceneSource::None,
                ..Default::default()
            };
            assert_eq!(scene_layout_line(language, 1, Some(&layout)), expected);
        }
    }

    #[test]
    fn scene_log_options_enforce_interval_bounds() {
        for (value, valid) in [
            ("9", false),
            ("10", true),
            ("10000", true),
            ("10001", false),
        ] {
            assert_eq!(
                Cli::try_parse_from([
                    "ssr-live",
                    "play",
                    "--profile",
                    "p",
                    "--scene-log",
                    "scene.jsonl",
                    "--scene-log-ms",
                    value
                ])
                .is_ok(),
                valid
            );
        }
    }

    #[test]
    fn report_request_fields_cover_sources_clocks_and_output_modes() {
        let report = supervisor::SessionReport {
            seconds: 1.0,
            engine: engine::StopReport {
                final_status: ssr_types::EngineStatus {
                    output_rate: 44_100,
                    output_period_frames: 441,
                    trim_available: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            figures: supervisor::RunFigures::default(),
            output_kind: engine::OutputKind::Speakers,
            output_channels: 2,
            output_channel_mask: 3,
            recorder_lost_bytes: 0,
            driver: None,
            driver_stats_error: None,
        };
        for (source, from_dump, expected) in [
            (CaptureKind::Driver, false, "driver"),
            (CaptureKind::Sal, false, "sal"),
            (CaptureKind::Driver, true, "dump"),
        ] {
            for (clock, clock_name) in [
                (Clock::Auto, "auto"),
                (Clock::Asrc, "asrc"),
                (Clock::Trim, "trim"),
            ] {
                for (shared, kernel_streaming, mode) in [
                    (false, false, "exclusive"),
                    (true, false, "shared"),
                    (false, true, "kernel_streaming"),
                ] {
                    let fields = report.to_json(&request_fields(
                        source,
                        from_dump,
                        clock,
                        shared,
                        kernel_streaming,
                        Vec::new(),
                    ));
                    assert_eq!(fields["source"], expected);
                    assert_eq!(fields["clock_requested"], clock_name);
                    assert_eq!(fields["output_mode_requested"], mode);
                    assert_eq!(fields["output_sample_rate"], 44_100);
                    assert_eq!(fields["output_period_frames"], 441);
                    assert_eq!(fields["trim_available"], true);
                    assert_eq!(fields["direct_layouts"], serde_json::json!([]));
                }
            }
        }
    }

    #[test]
    fn report_distinguishes_ks_success_and_exclusive_fallback() {
        for (mode, fallback, expected) in [
            (
                ssr_types::OutputMode::KernelStreaming,
                false,
                "kernel_streaming",
            ),
            (ssr_types::OutputMode::Exclusive, true, "exclusive"),
        ] {
            let report = supervisor::SessionReport {
                seconds: 1.0,
                engine: engine::StopReport {
                    final_status: ssr_types::EngineStatus {
                        output_mode: mode,
                        output_ks_fallback: fallback,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                figures: supervisor::RunFigures::default(),
                output_kind: engine::OutputKind::Speakers,
                output_channels: 2,
                output_channel_mask: 3,
                recorder_lost_bytes: 0,
                driver: None,
                driver_stats_error: None,
            };
            let json = report.to_json(&request_fields(
                CaptureKind::Driver,
                false,
                Clock::Auto,
                false,
                true,
                Vec::new(),
            ));
            assert_eq!(json["output_mode"], expected);
            assert_eq!(json["ks_fallback"], fallback);
            assert_eq!(json["output_mode_requested"], "kernel_streaming");
        }
    }

    #[test]
    fn endpoint_parser_accepts_arbitrary_subsets_and_rejects_bad_masks() {
        let subset = parse_endpoints(Some("0,2")).unwrap();
        assert!(subset.includes(0));
        assert!(!subset.includes(1));
        assert!(subset.includes(2));
        assert!(!subset.includes(3));
        assert!(parse_endpoints(Some("4")).is_err());
        assert!(parse_endpoints(Some("2,2")).is_err());
        assert!(parse_endpoints(Some("")).is_err());
    }

    #[test]
    fn play_direct_layouts_are_repeatable_and_record_rejects_them() {
        let cli = Cli::try_parse_from([
            "ssr-live",
            "play",
            "--profile",
            "profile.toml",
            "--direct-layout",
            "2=L,R",
            "--direct-layout",
            "12=7.1.4",
        ])
        .unwrap();
        let Command::Play { direct_layouts, .. } = cli.command else {
            panic!("expected play command");
        };
        assert_eq!(direct_layouts.len(), 2);
        assert_eq!(direct_layouts[0].channels, 2);
        assert_eq!(direct_layouts[1].channels, 12);

        assert!(
            Cli::try_parse_from([
                "ssr-live",
                "record",
                "--out",
                "capture.ssrdump",
                "--direct-layout",
                "2=stereo",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "ssr-live",
                "play",
                "--profile",
                "profile.toml",
                "--direct-layout",
                "2=L,L",
            ])
            .is_err()
        );
    }
}

/// `ssr-setup.exe`, beside `ssr-live.exe` or in the package directory.
const SETUP_EXE: &str = "ssr-setup.exe";

/// The state directory under the temporary directory when `--state-dir` is
/// not given.
const HOST_STATE_DIR: &str = "ssr-live-host";

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum RoleArg {
    Console,
    Multimedia,
    Communications,
}

impl From<RoleArg> for Role {
    fn from(role: RoleArg) -> Self {
        match role {
            RoleArg::Console => Role::Console,
            RoleArg::Multimedia => Role::Multimedia,
            RoleArg::Communications => Role::Communications,
        }
    }
}

/// The `ssr-setup` tasks, in the order `host setup --help` lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum SetupKind {
    /// Trust the package's test certificate, create the root device, install
    /// the driver and apply the endpoints' exclusive-mode policy.
    Install,
    /// Remove the current and legacy devices, driver packages, orphan services
    /// and test certificates.
    Uninstall,
    /// Reinstall the driver on the device and apply the exclusive-mode policy,
    /// or install when there is no device.
    Repair,
    /// Disable and enable the device.
    RestartDevice,
    /// Set the driver's endpoint mask (--endpoint-mask), restart the device
    /// and apply the exclusive-mode policy to the endpoints it enables.
    SetFormats,
}

#[derive(Subcommand, Debug)]
enum HostCommand {
    /// Print our endpoints' device descriptions and shared device formats as JSON.
    Formats,
    /// Print audio sessions and each app's persisted render endpoint.
    Apps,
    /// Set or clear an app's persisted render endpoint without changing communications.
    SetApp {
        /// The process id of the app to route.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        pid: u32,
        /// Our endpoint number (0, 1 or 2), or none to clear this app's override.
        #[arg(long, value_parser = ["0", "1", "2", "none"])]
        endpoint: String,
    },
    /// Print the platform, the driver state, every render endpoint (ours with
    /// its endpoint number) and the default device of each role.
    Show {
        /// Directory for ssr-setup's result file and the restore point
        /// [default: %TEMP%\ssr-live-host].
        #[arg(long)]
        state_dir: Option<PathBuf>,
    },
    /// Make a render endpoint the default device of one role.
    SetDefault {
        /// The role whose default device changes.
        #[arg(long, value_enum)]
        role: RoleArg,
        /// The endpoint id, as `host show` prints it.
        #[arg(long)]
        id: String,
        /// Directory for ssr-setup's result file and the restore point
        /// [default: %TEMP%\ssr-live-host].
        #[arg(long)]
        state_dir: Option<PathBuf>,
    },
    /// Run one administrator task through an elevated ssr-setup (one UAC
    /// prompt).
    ///
    /// ssr-setup.exe runs from --package-dir when that directory holds one, and
    /// otherwise from beside ssr-live.exe; install and repair take
    /// spatialsoftreceiver.inf and spatialsoftreceiver-test.cer from the
    /// directory of the ssr-setup.exe that runs. ssr-live copies nothing: put
    /// ssr-setup.exe into the package directory first (the CI step copies the
    /// binaries into the kit directory before it calls this).
    Setup {
        /// Administrator task to run.
        #[arg(value_enum)]
        task: SetupKind,
        /// With set-formats: the driver's EndpointMask, bit n enables endpoint n
        /// (1-15).
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..16))]
        endpoint_mask: Option<u8>,
        /// The driver package directory (spatialsoftreceiver.inf, its catalog
        /// and driver, the test certificate, and ssr-setup.exe).
        #[arg(long)]
        package_dir: PathBuf,
        /// Directory for ssr-setup's result file and the restore point
        /// [default: %TEMP%\ssr-live-host].
        #[arg(long)]
        state_dir: Option<PathBuf>,
    },
}

/// The task `host setup` asks for; `--endpoint-mask` belongs to set-formats
/// and set-formats needs it.
fn setup_task(kind: SetupKind, endpoint_mask: Option<u8>) -> Result<SetupTask, String> {
    match (kind, endpoint_mask) {
        (SetupKind::SetFormats, Some(endpoint_mask)) => Ok(SetupTask::SetFormats { endpoint_mask }),
        (SetupKind::SetFormats, None) => {
            Err(ssr_i18n::live::endpoint_mask_required(ssr_i18n::cli_language()).to_owned())
        }
        (_, Some(_)) => {
            Err(ssr_i18n::live::endpoint_mask_only(ssr_i18n::cli_language()).to_owned())
        }
        (SetupKind::Install, None) => Ok(SetupTask::Install),
        (SetupKind::Uninstall, None) => Ok(SetupTask::Uninstall),
        (SetupKind::Repair, None) => Ok(SetupTask::Repair),
        (SetupKind::RestartDevice, None) => Ok(SetupTask::RestartDevice),
    }
}

/// `--state-dir`, or `ssr-live-host` under the temporary directory.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn host_state_dir(state_dir: Option<PathBuf>, temp_dir: &Path) -> PathBuf {
    state_dir.unwrap_or_else(|| temp_dir.join(HOST_STATE_DIR))
}

/// The `ssr-setup.exe` that `host setup` runs: the package directory's when it
/// holds one, else `sibling` (the one beside ssr-live.exe). The adapter takes
/// the package files from the chosen executable's directory.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn package_setup_exe(
    package_dir: &Path,
    sibling: PathBuf,
    is_file: impl Fn(&Path) -> bool,
) -> PathBuf {
    let packaged = package_dir.join(SETUP_EXE);
    if is_file(&packaged) {
        packaged
    } else {
        sibling
    }
}

#[cfg(test)]
mod host_tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<HostCommand, clap::Error> {
        let mut line = vec!["ssr-live", "host"];
        line.extend_from_slice(args);
        let cli = Cli::try_parse_from(line)?;
        let Command::Host { command } = cli.command else {
            panic!("expected the host command");
        };
        Ok(command)
    }

    #[test]
    fn host_subcommands_parse_their_documented_lines() {
        let HostCommand::Show { state_dir } = parse(&["show"]).unwrap() else {
            panic!("expected show");
        };
        assert_eq!(state_dir, None);
        let HostCommand::Show { state_dir } = parse(&["show", "--state-dir", "s"]).unwrap() else {
            panic!("expected show");
        };
        assert_eq!(state_dir, Some(PathBuf::from("s")));

        for (name, role) in [
            ("console", Role::Console),
            ("multimedia", Role::Multimedia),
            ("communications", Role::Communications),
        ] {
            let HostCommand::SetDefault {
                role: parsed, id, ..
            } = parse(&[
                "set-default",
                "--role",
                name,
                "--id",
                "{0.0.0.00000000}.{a}",
            ])
            .unwrap()
            else {
                panic!("expected set-default");
            };
            assert_eq!(Role::from(parsed), role);
            assert_eq!(id, "{0.0.0.00000000}.{a}");
        }
        assert!(parse(&["set-default", "--role", "default", "--id", "x"]).is_err());
        assert!(parse(&["set-default", "--role", "console"]).is_err());

        for (name, kind) in [
            ("install", SetupKind::Install),
            ("uninstall", SetupKind::Uninstall),
            ("repair", SetupKind::Repair),
            ("restart-device", SetupKind::RestartDevice),
            ("set-formats", SetupKind::SetFormats),
        ] {
            let HostCommand::Setup {
                task,
                endpoint_mask,
                package_dir,
                state_dir,
            } = parse(&["setup", name, "--package-dir", "kit"]).unwrap()
            else {
                panic!("expected setup");
            };
            assert_eq!((task, endpoint_mask), (kind, None));
            assert_eq!(package_dir, PathBuf::from("kit"));
            assert_eq!(state_dir, None);
        }
        assert!(parse(&["setup", "install"]).is_err());
        assert!(parse(&["setup", "reinstall", "--package-dir", "kit"]).is_err());
        for mask in ["0", "16", "x"] {
            assert!(
                parse(&[
                    "setup",
                    "set-formats",
                    "--endpoint-mask",
                    mask,
                    "--package-dir",
                    "kit"
                ])
                .is_err()
            );
        }
        let HostCommand::Setup { endpoint_mask, .. } = parse(&[
            "setup",
            "set-formats",
            "--endpoint-mask",
            "7",
            "--package-dir",
            "kit",
        ])
        .unwrap() else {
            panic!("expected setup");
        };
        assert_eq!(endpoint_mask, Some(7));
    }

    #[test]
    fn app_routing_commands_validate_pid_and_endpoint() {
        assert!(matches!(parse(&["apps"]).unwrap(), HostCommand::Apps));
        for endpoint in ["0", "1", "2", "none"] {
            let HostCommand::SetApp {
                pid,
                endpoint: parsed,
            } = parse(&["set-app", "--pid", "42", "--endpoint", endpoint]).unwrap()
            else {
                panic!("expected set-app")
            };
            assert_eq!(pid, 42);
            assert_eq!(parsed, endpoint);
        }
        for args in [
            vec!["set-app", "--pid", "0", "--endpoint", "1"],
            vec!["set-app", "--pid", "42", "--endpoint", "3"],
            vec!["set-app", "--pid", "42"],
            vec!["set-app", "--endpoint", "none"],
        ] {
            assert!(parse(&args).is_err());
        }
    }

    #[test]
    fn setup_task_needs_the_mask_for_set_formats_only() {
        assert_eq!(
            setup_task(SetupKind::SetFormats, Some(5)),
            Ok(SetupTask::SetFormats { endpoint_mask: 5 })
        );
        assert!(setup_task(SetupKind::SetFormats, None).is_err());
        for (kind, task) in [
            (SetupKind::Install, SetupTask::Install),
            (SetupKind::Uninstall, SetupTask::Uninstall),
            (SetupKind::Repair, SetupTask::Repair),
            (SetupKind::RestartDevice, SetupTask::RestartDevice),
        ] {
            assert_eq!(setup_task(kind, None), Ok(task));
            assert!(setup_task(kind, Some(7)).is_err());
        }
    }

    #[test]
    fn state_dir_defaults_under_the_temporary_directory() {
        let temp = Path::new("tmp");
        assert_eq!(host_state_dir(None, temp), temp.join("ssr-live-host"));
        assert_eq!(
            host_state_dir(Some(PathBuf::from("mine")), temp),
            PathBuf::from("mine")
        );
    }

    #[test]
    fn setup_exe_comes_from_the_package_dir_when_it_holds_one() {
        let kit = Path::new("kit");
        let sibling = PathBuf::from("bin").join(SETUP_EXE);
        assert_eq!(
            package_setup_exe(kit, sibling.clone(), |path| path == kit.join(SETUP_EXE)),
            kit.join(SETUP_EXE)
        );
        assert_eq!(package_setup_exe(kit, sibling.clone(), |_| false), sibling);
    }
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Probe or play IEC 61937 bitstreams on our endpoints.
    Bitstream {
        #[command(subcommand)]
        command: bitstream_cli::BitstreamCommand,
    },
    /// Observe spatial audio input and record MAT bursts with signal truth.
    MatProbe {
        /// Driver endpoint number (resolved by KS filter identity).
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u16).range(0..=2))]
        endpoint: u16,
        /// Seconds of spatial excitation (maximum 3600).
        #[arg(long, default_value_t = 12.0, value_parser = ks_lifecycle::positive_seconds)]
        seconds: f64,
        /// Output file (`.ssrdump`).
        #[arg(long)]
        out: PathBuf,
        /// Output signal truth JSON file.
        #[arg(long)]
        truth: PathBuf,
    },
    /// Record a capture ring (the Spatial SoftReceiver driver or SAL) to a dump file (run as administrator).
    Record {
        /// Capture source; the Spatial SoftReceiver driver is the normal source.
        #[arg(long, value_enum, default_value_t = CaptureKind::Driver)]
        source: CaptureKind,
        /// Output file (`.ssrdump`).
        #[arg(long)]
        out: PathBuf,
        /// Seconds to record.
        #[arg(long, default_value_t = 60.0)]
        seconds: f64,
    },
    /// Play a capture ring through a BRIR profile (run as administrator).
    Play {
        /// Capture source; ignored when --from-dump supplies the source.
        #[arg(long, value_enum, default_value_t = CaptureKind::Driver)]
        source: CaptureKind,
        /// Enable the ASIO input alongside the capture source.
        #[arg(long)]
        asio: bool,
        /// Use a paced silence source without the kernel driver (requires --asio).
        #[arg(long, requires = "asio", conflicts_with_all = ["source", "from_dump"])]
        silence: bool,
        /// Endpoints to consume: all exposed endpoints, or a comma-separated subset.
        #[arg(long)]
        endpoints: Option<String>,
        /// Profile TOML (see crates/brir).
        #[arg(long)]
        profile: PathBuf,
        /// Part of the output device's name; the default device when omitted.
        #[arg(long)]
        output: Option<String>,
        /// Use Kernel Streaming with the endpoint disabled; fall back to WASAPI exclusive.
        #[arg(long, conflicts_with_all = ["shared", "allow_shared_fallback"])]
        kernel_streaming: bool,
        /// Use WASAPI shared mode instead of exclusive.
        #[arg(long)]
        shared: bool,
        /// Permit an exclusive request to fall back to WASAPI shared mode.
        #[arg(long, conflicts_with = "shared")]
        allow_shared_fallback: bool,
        /// Device sample rate.
        #[arg(long, default_value_t = 48_000)]
        rate: u32,
        /// Device period in milliseconds; the device's minimum when omitted.
        #[arg(long)]
        period_ms: Option<f64>,
        /// Crossing-ring level to hold, in milliseconds (10 for games, 40 for music).
        #[arg(long, default_value_t = 10.0)]
        target_ms: f32,
        /// Clock-control mode.
        #[arg(long, value_enum, default_value_t = Clock::Auto)]
        clock: Clock,
        /// Also write the captured record stream to this dump file.
        #[arg(long)]
        record: Option<PathBuf>,
        /// Stop after this many seconds (otherwise press Enter to stop).
        #[arg(long)]
        seconds: Option<f64>,
        /// Play a dump file at its recorded pace instead of the SAL ring, to run
        /// the live path on a PC without SAL. Stops when the dump ends.
        #[arg(long)]
        from_dump: Option<PathBuf>,
        /// With --from-dump: repeat the dump's data records, with continuous
        /// positions and times, until --seconds or Enter stops the run.
        #[arg(long = "loop", requires = "from_dump")]
        repeat: bool,
        /// Run everything but send silence to the device.
        #[arg(long)]
        mute: bool,
        /// Write the run's figures (underruns, restarts, faults, latency,
        /// ring trough, clock) as JSON to this file when it stops.
        #[arg(long)]
        report: Option<PathBuf>,
        /// Write the entrances' scene layouts and active objects to this JSON Lines file
        #[arg(long, value_name = "PATH")]
        scene_log: Option<PathBuf>,
        /// Milliseconds between scene-log lines
        #[arg(long, value_name = "MS", default_value_t = 200, value_parser = clap::value_parser!(u64).range(10..=10000))]
        scene_log_ms: u64,
        /// Override a zero-mask PCM order, as CHANNELS=PRESET or CHANNELS=SPEAKER,SPEAKER,...
        #[arg(long = "direct-layout")]
        direct_layouts: Vec<engine::DirectLayoutOverride>,
    },
    /// Test direct Kernel Streaming with the endpoint disabled, then enable it again.
    KsProbe {
        /// Part of the output device's name or its exact endpoint id.
        #[arg(long)]
        output: String,
        /// Seconds to play a 1 kHz sine at -12 dBFS (48 kHz stereo).
        #[arg(long, default_value_t = 2.0, value_parser = ks_lifecycle::positive_seconds)]
        seconds: f64,
        /// Write the result as JSON to this file, including failures.
        #[arg(long)]
        json: Option<PathBuf>,
    },
    /// Test an ASIO DLL using asio_probe.exe beside ssr-live.exe.
    AsioProbe {
        /// Path to the ASIO driver DLL to test.
        #[arg(long, value_name = "PATH")]
        dll: PathBuf,
        /// Whole seconds to run the probe (1-3600).
        #[arg(long, default_value_t = 5, value_parser = asio_probe::parse_seconds)]
        seconds: u32,
        /// Write the probe's JSON report to this file.
        #[arg(long, value_name = "PATH")]
        json: Option<PathBuf>,
    },
    /// List render endpoints (name, default flag, id).
    Devices {
        /// Include the Kernel Streaming filter and pin, or the discovery failure.
        #[arg(long)]
        kernel: bool,
        /// Include shared mix format and standard-layout exclusive support.
        #[arg(long)]
        formats: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Query and drive the Windows host adapter; prints one JSON document.
    Host {
        #[command(subcommand)]
        command: HostCommand,
    },
}

fn localized_command(language: Language) -> clap::Command {
    use ssr_i18n::live;

    let mut command = Cli::command().about(live::about(language));
    for (name, about) in [
        ("mat-probe", live::mat_probe_about(language)),
        ("record", live::record_about(language)),
        ("play", live::play_about(language)),
        ("asio-probe", live::asio_probe_about(language)),
        ("devices", live::devices_about(language)),
        ("ks-probe", live::ks_probe_about(language)),
        ("host", live::host_about(language)),
        ("bitstream", live::bitstream_about(language)),
    ] {
        command = command.mut_subcommand(name, |subcommand| subcommand.about(about));
    }
    command = command
        .mut_subcommand("bitstream", |subcommand| {
            subcommand
                .mut_subcommand("probe", |nested| {
                    nested
                        .about(live::bitstream_probe_about(language))
                        .mut_arg("json", |arg| arg.help(live::devices_json_help(language)))
                })
                .mut_subcommand("play", |nested| {
                    nested
                        .about(live::bitstream_play_about(language))
                        .mut_arg("endpoint", |arg| {
                            arg.help(live::bitstream_endpoint_help(language))
                        })
                        .mut_arg("codec", |arg| {
                            arg.help(live::bitstream_codec_help(language))
                        })
                        .mut_arg("input", |arg| {
                            arg.help(live::bitstream_input_help(language))
                        })
                        .mut_arg("seconds", |arg| {
                            arg.help(live::bitstream_seconds_help(language))
                        })
                        .mut_arg("period_ms", |arg| {
                            arg.help(live::play_period_ms_help(language))
                        })
                        .mut_arg("json", |arg| arg.help(live::devices_json_help(language)))
                })
        })
        .mut_subcommand("mat-probe", |subcommand| {
            subcommand
                .mut_arg("endpoint", |arg| {
                    arg.help(live::mat_endpoint_help(language))
                })
                .mut_arg("seconds", |arg| arg.help(live::mat_seconds_help(language)))
                .mut_arg("out", |arg| arg.help(live::record_out_help(language)))
                .mut_arg("truth", |arg| arg.help(live::mat_truth_help(language)))
        })
        .mut_subcommand("record", |subcommand| {
            subcommand
                .mut_arg("source", |arg| arg.help(live::record_source_help(language)))
                .mut_arg("out", |arg| arg.help(live::record_out_help(language)))
                .mut_arg("seconds", |arg| {
                    arg.help(live::record_seconds_help(language))
                })
        })
        .mut_subcommand("play", |subcommand| {
            subcommand
                .mut_arg("source", |arg| arg.help(live::play_source_help(language)))
                .mut_arg("asio", |arg| arg.help(live::play_asio_help(language)))
                .mut_arg("silence", |arg| arg.help(live::play_silence_help(language)))
                .mut_arg("endpoints", |arg| {
                    arg.help(live::play_endpoints_help(language))
                })
                .mut_arg("profile", |arg| arg.help(live::play_profile_help(language)))
                .mut_arg("output", |arg| arg.help(live::play_output_help(language)))
                .mut_arg("kernel_streaming", |arg| {
                    arg.help(live::play_kernel_streaming_help(language))
                })
                .mut_arg("shared", |arg| arg.help(live::play_shared_help(language)))
                .mut_arg("allow_shared_fallback", |arg| {
                    arg.help(live::play_allow_shared_fallback_help(language))
                })
                .mut_arg("rate", |arg| arg.help(live::play_rate_help(language)))
                .mut_arg("period_ms", |arg| {
                    arg.help(live::play_period_ms_help(language))
                })
                .mut_arg("target_ms", |arg| {
                    arg.help(live::play_target_ms_help(language))
                })
                .mut_arg("clock", |arg| arg.help(live::play_clock_help(language)))
                .mut_arg("record", |arg| arg.help(live::play_record_help(language)))
                .mut_arg("seconds", |arg| arg.help(live::play_seconds_help(language)))
                .mut_arg("from_dump", |arg| {
                    arg.help(live::play_from_dump_help(language))
                })
                .mut_arg("repeat", |arg| arg.help(live::play_loop_help(language)))
                .mut_arg("mute", |arg| arg.help(live::play_mute_help(language)))
                .mut_arg("report", |arg| arg.help(live::play_report_help(language)))
                .mut_arg("scene_log", |arg| {
                    arg.help(ssr_i18n::cli::scene_log_help(language))
                })
                .mut_arg("scene_log_ms", |arg| {
                    arg.help(ssr_i18n::cli::scene_log_ms_help(language))
                })
                .mut_arg("direct_layouts", |arg| {
                    arg.help(live::play_direct_layout_help(language))
                })
        })
        .mut_subcommand("ks-probe", |subcommand| {
            subcommand
                .mut_arg("output", |arg| {
                    arg.help(live::ks_probe_output_help(language))
                })
                .mut_arg("seconds", |arg| {
                    arg.help(live::ks_probe_seconds_help(language))
                })
                .mut_arg("json", |arg| arg.help(live::ks_probe_json_help(language)))
        })
        .mut_subcommand("asio-probe", |subcommand| {
            subcommand
                .mut_arg("dll", |arg| arg.help(live::asio_probe_dll_help(language)))
                .mut_arg("seconds", |arg| {
                    arg.help(live::asio_probe_seconds_help(language))
                })
                .mut_arg("json", |arg| arg.help(live::asio_probe_json_help(language)))
        })
        .mut_subcommand("devices", |subcommand| {
            subcommand
                .mut_arg("kernel", |arg| {
                    arg.help(live::devices_kernel_help(language))
                })
                .mut_arg("formats", |arg| {
                    arg.help(live::devices_formats_help(language))
                })
                .mut_arg("json", |arg| arg.help(live::devices_json_help(language)))
        })
        .mut_subcommand("host", |mut subcommand| {
            for (name, about) in [
                ("formats", live::host_formats_about(language)),
                ("apps", live::host_apps_about(language)),
                ("set-app", live::host_set_app_about(language)),
                ("show", live::host_show_about(language)),
                ("set-default", live::host_set_default_about(language)),
                ("setup", live::host_setup_about(language)),
            ] {
                subcommand = subcommand.mut_subcommand(name, |nested| nested.about(about));
            }
            subcommand
                .mut_subcommand("set-app", |nested| {
                    nested
                        .mut_arg("pid", |arg| arg.help(live::host_app_pid_help(language)))
                        .mut_arg("endpoint", |arg| {
                            arg.help(live::host_app_endpoint_help(language))
                        })
                })
                .mut_subcommand("show", |nested| {
                    nested.mut_arg("state_dir", |arg| {
                        arg.help(live::host_state_dir_help(language))
                    })
                })
                .mut_subcommand("set-default", |nested| {
                    nested
                        .mut_arg("role", |arg| arg.help(live::host_role_help(language)))
                        .mut_arg("id", |arg| arg.help(live::host_id_help(language)))
                        .mut_arg("state_dir", |arg| {
                            arg.help(live::host_state_dir_help(language))
                        })
                })
                .mut_subcommand("setup", |nested| {
                    nested
                        .long_about(live::host_setup_long_about(language))
                        .mut_arg("task", |arg| {
                            arg.help(live::host_setup_task_help(language)).value_parser(
                                ssr_i18n::clap_help::Values::new(|task: &SetupKind| match task {
                                    SetupKind::Install => live::host_setup_task_install(language),
                                    SetupKind::Uninstall => {
                                        live::host_setup_task_uninstall(language)
                                    }
                                    SetupKind::Repair => ssr_i18n::setup::repair_about(language),
                                    SetupKind::RestartDevice => {
                                        ssr_i18n::setup::restart_device_about(language)
                                    }
                                    SetupKind::SetFormats => {
                                        live::host_setup_task_set_formats(language)
                                    }
                                }),
                            )
                        })
                        .mut_arg("endpoint_mask", |arg| {
                            arg.help(live::host_endpoint_mask_help(language))
                        })
                        .mut_arg("package_dir", |arg| {
                            arg.help(live::host_package_dir_help(language))
                        })
                        .mut_arg("state_dir", |arg| {
                            arg.help(live::host_state_dir_help(language))
                        })
                })
        });
    ssr_i18n::clap_help::localize(command, language)
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn scene_layout_line(
    language: Language,
    endpoint: usize,
    layout: Option<&ssr_types::SceneLayout>,
) -> String {
    use ssr_i18n::live;
    use ssr_types::SceneSource;
    let Some(layout) = layout else {
        return live::scene_unavailable(language, endpoint);
    };
    let source = match layout.source {
        SceneSource::None => return live::scene_unavailable(language, endpoint),
        SceneSource::Pcm => live::scene_source_pcm(language),
        SceneSource::Ddp => live::scene_source_ddp(language),
        SceneSource::DdpJoc => live::scene_source_ddp_joc(language),
        SceneSource::Upmix => live::scene_source_upmix(language),
        SceneSource::TrueHd => live::scene_source_truehd(language),
        SceneSource::TrueHdObjects => live::scene_source_truehd_objects(language),
    };
    live::scene_layout(
        language,
        endpoint,
        source,
        layout.bed.len(),
        if layout.lfe {
            live::scene_lfe(language)
        } else {
            ""
        },
        layout.dynamic.len(),
    )
}

fn parse_cli(language: Language) -> Result<Cli, clap::Error> {
    let matches = localized_command(language).try_get_matches()?;
    Cli::from_arg_matches(&matches)
}

fn parse_error_exit(error: clap::Error, language: Language) -> ExitCode {
    if matches!(
        error.kind(),
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
    ) {
        let _ = error.print();
        ExitCode::SUCCESS
    } else {
        eprintln!("{}", ssr_i18n::cli::parse_error(language));
        let _ = error.print();
        ExitCode::from(2)
    }
}

fn main() -> ExitCode {
    let language = ssr_i18n::cli_language();
    let cli = match parse_cli(language) {
        Ok(cli) => cli,
        Err(error) => return parse_error_exit(error, language),
    };
    let result = match cli.command {
        Command::Bitstream { command } => bitstream_cli::run(command),
        Command::MatProbe {
            endpoint,
            seconds,
            out,
            truth,
        } => mat_probe::run(endpoint, seconds, &out, &truth),
        Command::Record {
            source,
            out,
            seconds,
        } => platform::record(source, &out, seconds),
        Command::Play {
            source,
            asio,
            silence,
            endpoints,
            profile,
            output,
            kernel_streaming,
            shared,
            allow_shared_fallback,
            rate,
            period_ms,
            target_ms,
            clock,
            record,
            seconds,
            from_dump,
            repeat,
            mute,
            report,
            scene_log,
            scene_log_ms,
            direct_layouts,
        } => platform::play(platform::PlayOptions {
            source,
            asio,
            silence,
            endpoints,
            profile,
            output,
            kernel_streaming,
            shared,
            allow_shared_fallback,
            rate,
            period_ms,
            target_ms,
            clock,
            record,
            seconds,
            from_dump,
            repeat,
            mute,
            report,
            scene_log,
            scene_log_ms,
            direct_layouts,
        }),
        Command::Devices {
            formats,
            json,
            kernel,
        } => platform::devices(formats, json, kernel),
        Command::KsProbe {
            output,
            seconds,
            json,
        } => platform::ks_probe(&output, seconds, json.as_deref()),
        Command::AsioProbe { dll, seconds, json } => {
            asio_probe::run(&dll, seconds, json.as_deref())
        }
        Command::Host { command } => match command {
            HostCommand::Formats => bitstream_cli::host_formats(),
            HostCommand::Apps => platform::host_apps(),
            HostCommand::SetApp { pid, endpoint } => platform::host_set_app(pid, &endpoint),
            HostCommand::Show { state_dir } => platform::host_show(state_dir),
            HostCommand::SetDefault {
                role,
                id,
                state_dir,
            } => platform::host_set_default(role.into(), id, state_dir),
            HostCommand::Setup {
                task,
                endpoint_mask,
                package_dir,
                state_dir,
            } => setup_task(task, endpoint_mask)
                .and_then(|task| platform::host_setup(&task, &package_dir, state_dir)),
        },
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{}", ssr_i18n::live::error_prefix(language, message));
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::collections::BTreeMap;
    use std::io::BufRead;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    use engine::adapters::DumpCapture;
    use engine::{ClockPreference, EngineConfig};
    #[cfg(test)]
    use ssr_abi::dump_header_bytes;
    use ssr_abi::{Record, records};
    use ssr_sys::driver_ring::DriverRing;
    use ssr_sys::mmcss;
    use ssr_sys::output::WasapiOpener;
    use ssr_sys::{WindowsHost, devices, qpc, sal::SalRing};
    use ssr_types::{
        CaptureError, CaptureSource, DeviceId, Host, Role, SetupTask, TrimUnsupported,
    };

    use super::{
        CaptureKind, Clock, SETUP_EXE, host_json, host_state_dir, package_setup_exe,
        parse_endpoints,
    };

    pub struct PlayOptions {
        pub source: CaptureKind,
        pub asio: bool,
        pub silence: bool,
        pub endpoints: Option<String>,
        pub profile: PathBuf,
        pub output: Option<String>,
        pub kernel_streaming: bool,
        pub shared: bool,
        pub allow_shared_fallback: bool,
        pub rate: u32,
        pub period_ms: Option<f64>,
        pub target_ms: f32,
        pub clock: Clock,
        pub record: Option<PathBuf>,
        pub seconds: Option<f64>,
        pub from_dump: Option<PathBuf>,
        pub repeat: bool,
        pub mute: bool,
        pub report: Option<PathBuf>,
        pub scene_log: Option<PathBuf>,
        pub scene_log_ms: u64,
        pub direct_layouts: Vec<engine::DirectLayoutOverride>,
    }

    fn open_sal() -> Result<SalRing, String> {
        SalRing::open().map_err(|e| capture_message("SAL", e))
    }

    fn open_driver() -> Result<DriverRing, String> {
        DriverRing::open().map_err(|e| {
            capture_message(ssr_i18n::live::source_driver(ssr_i18n::cli_language()), e)
        })
    }

    fn capture_message(name: &str, error: CaptureError) -> String {
        let language = ssr_i18n::cli_language();
        match error {
            CaptureError::NotPresent => ssr_i18n::live::capture_not_present(language, name),
            CaptureError::AccessDenied => ssr_i18n::live::capture_access_denied(language, name),
            other => other.to_string(),
        }
    }

    pub fn record(source: CaptureKind, out: &Path, seconds: f64) -> Result<(), String> {
        let (ring, qpc_frequency, source_name) = match source {
            CaptureKind::Driver => {
                let ring = open_driver()?;
                let frequency = ring.info().qpc_frequency;
                (
                    Source::Driver(ring),
                    frequency,
                    ssr_i18n::live::source_driver(ssr_i18n::cli_language()),
                )
            }
            CaptureKind::Sal => (Source::Sal(open_sal()?), qpc::frequency(), "SAL"),
        };
        let tee = supervisor::TeeCapture::new(ring, qpc::now);
        let (mut ring, recorder) = supervisor::Recorder::start(
            supervisor::RecorderConfig {
                path: out.to_owned(),
                now: qpc::now,
            },
            qpc_frequency,
            tee,
        )?;
        let mut buf = vec![0_u8; 1 << 20];
        let mut bytes = 0_u64;
        let mut endpoints = BTreeMap::<u16, (u64, u64, u64)>::new();
        let started = Instant::now();
        let mut next_report = Duration::from_secs(1);
        let language = ssr_i18n::cli_language();
        eprintln!(
            "{}",
            ssr_i18n::live::recording(language, source_name, out.display(), seconds)
        );
        while started.elapsed().as_secs_f64() < seconds {
            let n = ring
                .read(&mut buf, Duration::from_millis(100))
                .map_err(|e| e.to_string())?;
            if n > 0 {
                bytes += n as u64;
                for record in records(&buf[..n]).flatten() {
                    let endpoint = record.endpoint();
                    let counts = endpoints.entry(endpoint).or_default();
                    match record {
                        Record::Data { bytes: payload, .. } => counts.0 += payload.len() as u64,
                        Record::FormatChange { epoch, format, .. } => {
                            counts.1 += 1;
                            eprintln!(
                                "{}",
                                ssr_i18n::live::format_changed(
                                    language,
                                    endpoint,
                                    epoch,
                                    format_args!("{format:?}"),
                                )
                            );
                        }
                        Record::Overrun { lost_bytes, .. } => counts.2 += lost_bytes,
                        _ => {}
                    }
                }
            }
            if started.elapsed() >= next_report {
                for (endpoint, (data, formats, lost)) in &endpoints {
                    eprintln!(
                        "{}",
                        ssr_i18n::live::record_progress(
                            language,
                            format_args!("{:>4}", next_report.as_secs()),
                            endpoint,
                            data,
                            formats,
                            lost,
                        )
                    );
                }
                next_report += Duration::from_secs(1);
            }
        }
        drop(ring);
        let recorder_lost = recorder.dropped_bytes();
        recorder.stop()?;
        let data = endpoints.values().map(|counts| counts.0).sum::<u64>();
        eprintln!(
            "{}",
            ssr_i18n::live::record_finished(language, bytes, data, recorder_lost, out.display(),)
        );
        if data == 0 {
            return Err(ssr_i18n::live::no_data(language).to_owned());
        }
        Ok(())
    }

    enum Source {
        Driver(DriverRing),
        Sal(SalRing),
        Dump(DumpCapture),
        Silence(super::silence::SilenceCapture),
    }

    impl CaptureSource for Source {
        fn read(&mut self, buf: &mut [u8], wait: Duration) -> Result<usize, CaptureError> {
            match self {
                Self::Driver(ring) => ring.read(buf, wait),
                Self::Sal(ring) => ring.read(buf, wait),
                Self::Dump(dump) => dump.read(buf, wait),
                Self::Silence(silence) => silence.read(buf, wait),
            }
        }

        fn now_qpc(&self) -> Option<u64> {
            match self {
                Self::Driver(ring) => ring.now_qpc(),
                Self::Sal(ring) => ring.now_qpc(),
                Self::Dump(dump) => dump.now_qpc(),
                Self::Silence(silence) => silence.now_qpc(),
            }
        }

        fn set_rate_trim(&mut self, ppm: f64) -> Result<(), TrimUnsupported> {
            match self {
                Self::Driver(ring) => ring.set_rate_trim(ppm),
                Self::Sal(ring) => ring.set_rate_trim(ppm),
                Self::Dump(dump) => dump.set_rate_trim(ppm),
                Self::Silence(silence) => silence.set_rate_trim(ppm),
            }
        }

        fn reconnect(&mut self) -> Result<(), CaptureError> {
            match self {
                Self::Driver(ring) => ring.reconnect(),
                Self::Sal(ring) => ring.reconnect(),
                Self::Dump(dump) => dump.reconnect(),
                Self::Silence(silence) => silence.reconnect(),
            }
        }
    }

    type OpenedSource = (Source, u64, Option<Box<dyn supervisor::DriverStatsSource>>);

    fn open_source(options: &PlayOptions, ended: Arc<AtomicBool>) -> Result<OpenedSource, String> {
        if options.silence {
            return Ok((
                Source::Silence(super::silence::SilenceCapture::new(qpc::now)),
                qpc::frequency(),
                None,
            ));
        }
        if let Some(path) = &options.from_dump {
            let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            return Ok((
                Source::Dump(DumpCapture::from_dump(
                    &bytes,
                    qpc::frequency(),
                    qpc::now,
                    Some(ended),
                    options.repeat,
                )?),
                qpc::frequency(),
                None,
            ));
        }
        match options.source {
            CaptureKind::Driver => {
                let ring = open_driver()?;
                let frequency = ring.info().qpc_frequency;
                let observer = ring.observer();
                Ok((Source::Driver(ring), frequency, Some(Box::new(observer))))
            }
            CaptureKind::Sal => Ok((Source::Sal(open_sal()?), qpc::frequency(), None)),
        }
    }

    fn engine_config(options: &PlayOptions) -> Result<EngineConfig, String> {
        let endpoints = parse_endpoints(options.endpoints.as_deref())?;
        let mut direct = engine::DirectLayouts::default();
        for value in &options.direct_layouts {
            direct
                .set(value.channels, value.layout)
                .map_err(str::to_owned)?;
        }
        Ok(EngineConfig {
            chain: engine::ChainConfig {
                direct,
                endpoints,
                ..engine::ChainConfig::default()
            },
            asio: ssr_types::app::AsioSettings {
                enabled: options.asio,
                ..Default::default()
            },
            target_ms: options.target_ms,
            clock: match options.clock {
                Clock::Auto => ClockPreference::Auto,
                Clock::Asrc => ClockPreference::Asrc,
                Clock::Trim => ClockPreference::Trim,
            },
            thread_hook: Some(Arc::new(mmcss::promote_engine_thread)),
            ..EngineConfig::default()
        })
    }

    fn report_request(options: &PlayOptions) -> supervisor::RequestFields {
        let mut fields = super::request_fields(
            options.source,
            options.from_dump.is_some(),
            options.clock,
            options.shared,
            options.kernel_streaming,
            options
                .direct_layouts
                .iter()
                .map(|value| supervisor::DirectLayoutField {
                    channels: value.channels,
                    speakers: value
                        .layout
                        .speakers()
                        .iter()
                        .copied()
                        .map(engine::speaker_name)
                        .map(str::to_owned)
                        .collect(),
                })
                .collect(),
        );
        if options.silence {
            fields.source = "silence".to_owned();
        }
        fields
    }

    pub fn play(options: PlayOptions) -> Result<(), String> {
        let profile = engine::load_output_profile(&options.profile).map_err(|e| e.to_string())?;
        let endpoint = if options.kernel_streaming {
            selected_endpoint(options.output.as_deref())?
        } else {
            devices::find(options.output.as_deref()).map_err(|e| e.to_string())?
        };
        let cancel = ssr_sys::ks::ConsoleCancel::install().map_err(|e| e.to_string())?;
        let target = if options.kernel_streaming {
            match windows_host(&sibling_setup_exe()?, None)?
                .kernel_target(&DeviceId(endpoint.id.clone()))
            {
                Ok(target) => Some(target),
                Err(error) => {
                    eprintln!(
                        "{}",
                        ssr_i18n::supervisor::ks_unavailable(ssr_i18n::cli_language(), error)
                    );
                    eprintln!(
                        "{}",
                        ssr_i18n::live::ks_fallback_detail(
                            ssr_i18n::cli_language(),
                            "no_target",
                            0
                        )
                    );
                    None
                }
            }
        } else {
            None
        };
        // Declared before Session: Engine::drop joins both threads before this
        // guard can restore the endpoint, including during unwinding.
        let mut restore = target
            .as_ref()
            .map(|_| super::ks_lifecycle::RestoreOnDrop::new(|| restore_endpoint(&endpoint.id)));
        let language = ssr_i18n::cli_language();
        eprintln!("{}", ssr_i18n::live::output(language, &endpoint.name));
        let ended = Arc::new(AtomicBool::new(false));
        let (source, source_qpc, driver_stats) = open_source(&options, ended.clone())?;
        let config = engine_config(&options)?.with_asio_runtime(engine::asio::AsioRuntimeConfig {
            provider: super::asio_transport::provider(),
            profile: profile.clone(),
            target_lead_frames: engine::asio::MAX_ASIO_BUFFER_FRAMES as u32,
        });
        let output = WasapiOpener {
            endpoint: Some(endpoint.id.clone()),
            rate: options.rate,
            exclusive: options.kernel_streaming || !options.shared,
            shared_fallback: !options.kernel_streaming && options.allow_shared_fallback,
            period: options
                .period_ms
                .filter(|ms| ms.is_finite() && *ms > 0.0)
                .map(|ms| Duration::from_secs_f64(ms / 1000.0)),
        };
        let recorder = options
            .record
            .as_ref()
            .map(|path| supervisor::RecorderConfig {
                path: path.clone(),
                now: qpc::now,
            });
        let session_config = supervisor::SessionConfig {
            engine: config,
            profile,
            qpc_frequency: source_qpc,
            mute: options.mute,
            recorder,
        };
        let mut session = if let Some(target) = target {
            let output = ssr_sys::ks::KsOrWasapiOpener {
                ks: ssr_sys::ks::KsOpener {
                    target,
                    rate: options.rate,
                    period: output.period,
                },
                wasapi: output,
                endpoint: endpoint.id.clone(),
            };
            supervisor::Session::start(session_config, source, output, driver_stats)
        } else {
            supervisor::Session::start(session_config, source, output, driver_stats)
        }
        .map_err(|error| error.to_string())?;
        let discovery_fallback = options.kernel_streaming && restore.is_none();
        let mut fallback_reported = discovery_fallback;

        let stop = Arc::new(AtomicBool::new(false));
        if options.seconds.is_none() {
            let stop = stop.clone();
            thread::spawn(move || {
                let mut line = String::new();
                if std::io::stdin()
                    .lock()
                    .read_line(&mut line)
                    .is_ok_and(|n| n > 0)
                {
                    stop.store(true, Ordering::Release);
                }
            });
            eprintln!("{}", ssr_i18n::live::playing(language));
        }

        let started = Instant::now();
        let mut scene_log = options
            .scene_log
            .as_ref()
            .map(|path| {
                supervisor::scene_log::SceneLogWriter::create(path, options.scene_log_ms)
                    .map_err(|error| format!("{}: {error}", path.display()))
            })
            .transpose()?;
        let mut scene_changes = supervisor::scene_log::SceneChanges::default();
        let mut next_report = Duration::from_secs(1);
        let mut ended_at: Option<Instant> = None;
        let mut reported_underruns = 0;
        let mut reported_lane_underrun_frames = 0;
        while !stop.load(Ordering::Acquire)
            && !cancel.cancelled()
            && options
                .seconds
                .is_none_or(|s| started.elapsed().as_secs_f64() < s)
            && ended_at.is_none_or(|t| t.elapsed() < Duration::from_millis(500))
        {
            if ended_at.is_none() && ended.load(Ordering::Acquire) {
                eprintln!("{}", ssr_i18n::live::dump_ended(language));
                ended_at = Some(Instant::now());
                session.source_ended();
            }
            if session.status().output_ks_fallback && !fallback_reported {
                eprintln!("{}", ssr_i18n::supervisor::ks_fallback(language));
                let status = session.status();
                eprintln!(
                    "{}",
                    ssr_i18n::live::ks_fallback_detail(
                        language,
                        serde_json::to_value(status.output_ks_fallback_reason).unwrap_or_default(),
                        status.output_ks_fallback_code
                    )
                );
                fallback_reported = true;
            }
            for notice in session.poll().map_err(|error| error.to_string())? {
                print_notice(notice, language);
            }
            let status = session.status();
            for endpoint in scene_changes.observe(&status.endpoints) {
                eprintln!(
                    "{}",
                    super::scene_layout_line(
                        language,
                        endpoint,
                        status.endpoints[endpoint].layout.as_ref()
                    )
                );
            }
            if let Some(writer) = &mut scene_log {
                writer
                    .sample(
                        started.elapsed().as_millis() as u64,
                        &status.endpoints,
                        &status.scene,
                    )
                    .map_err(|error| error.to_string())?;
            }
            if started.elapsed() >= next_report {
                session.observe(next_report.as_secs());
                print_status(
                    next_report.as_secs(),
                    &status,
                    session.recorder_dropped_bytes(),
                    &mut reported_underruns,
                    &mut reported_lane_underrun_frames,
                    language,
                );
                next_report += Duration::from_secs(1);
            }
            thread::sleep(Duration::from_millis(if scene_log.is_some() {
                options.scene_log_ms.min(33)
            } else {
                33
            }));
        }

        let mut report = session.stop().map_err(|error| error.to_string())?;
        if discovery_fallback {
            report.engine.final_status.output_ks_fallback = true;
            report.engine.final_status.output_ks_fallback_reason =
                Some(ssr_types::ports::KsFallbackReason::NoTarget);
            report.engine.final_status.output_ks_fallback_code = 0;
        }
        let restoration = restore.as_mut().map_or(Ok(()), |guard| guard.restore());
        eprintln!(
            "{}",
            ssr_i18n::live::stopped(
                language,
                report.engine.blocks,
                report.engine.underruns,
                report.engine.final_status.output_restarts,
                report.engine.lost_bytes,
                report.recorder_lost_bytes,
                format_args!("{:.1}", report.engine.final_status.latency.p50_ms),
                format_args!("{:.1}", report.engine.final_status.latency.p99_ms),
            )
        );
        if let Some(path) = &options.report {
            let text = serde_json::to_string_pretty(&report.to_json(&report_request(&options)))
                .map_err(|error| error.to_string())?;
            std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))?;
        }
        restoration
    }

    fn print_notice(notice: supervisor::SessionNotice, language: ssr_types::app::Language) {
        use ssr_i18n::live;

        match notice {
            supervisor::SessionNotice::FormatChanged {
                endpoint,
                epoch,
                format,
                unsupported,
            } => match unsupported {
                Some(reason) => eprintln!(
                    "{}",
                    live::format_unsupported(
                        language,
                        endpoint,
                        epoch,
                        format_args!("{format:?}"),
                        reason
                    )
                ),
                None => eprintln!(
                    "{}",
                    live::format_changed(language, endpoint, epoch, format_args!("{format:?}"))
                ),
            },
            supervisor::SessionNotice::NeedsPrepared => {}
            supervisor::SessionNotice::Prepared {
                kind,
                channels,
                millis,
            } => eprintln!(
                "{}",
                live::prepared(
                    language,
                    kind.as_str(),
                    channels,
                    engine::MIX_RATE,
                    format_args!("{millis:.0}"),
                )
            ),
            supervisor::SessionNotice::PrepareRetry => {
                eprintln!("{}", live::prepare_retry(language))
            }
            supervisor::SessionNotice::Overrun { lost_bytes } => {
                eprintln!("{}", live::overrun(language, lost_bytes))
            }
            supervisor::SessionNotice::OutputOverflow { frames } => {
                eprintln!("{}", live::output_overflow(language, frames))
            }
            supervisor::SessionNotice::OutputLayoutChanged {
                channels,
                channel_mask,
            } => eprintln!(
                "{}",
                live::output_reopened(language, channels, format_args!("{channel_mask:#x}"))
            ),
            supervisor::SessionNotice::OutputReconnected => {
                eprintln!("{}", live::output_reconnected(language))
            }
            supervisor::SessionNotice::FaultCleared => {
                eprintln!("{}", live::fault_cleared(language))
            }
            supervisor::SessionNotice::OutputCadenceFault { ratio } => eprintln!(
                "{}",
                live::output_cadence_fault(language, format_args!("{ratio:.3}"))
            ),
            supervisor::SessionNotice::OutputCadenceReopen { period_ms } => eprintln!(
                "{}",
                live::output_cadence_reopen(language, format_args!("{period_ms:.1}"))
            ),
            supervisor::SessionNotice::Fault(fault) => {
                eprintln!("{}", live::fault(language, format_args!("{fault:?}")))
            }
        }
    }

    fn print_status(
        second: u64,
        status: &ssr_types::EngineStatus,
        recorder_dropped: u64,
        reported_underruns: &mut u64,
        reported_lane_underrun_frames: &mut u64,
        language: ssr_types::app::Language,
    ) {
        let chain_errors = status.unattributed_chain_errors.total()
            + status
                .endpoints
                .iter()
                .map(|endpoint| endpoint.chain_errors.total())
                .sum::<u64>();
        let suffix = if chain_errors != 0 {
            ssr_i18n::live::chain_errors(language, chain_errors)
        } else {
            String::new()
        };
        let fault = status
            .fault
            .as_ref()
            .map(|fault| ssr_i18n::live::fault_suffix(language, format_args!("{fault:?}")))
            .unwrap_or_default();
        let gap_ms = status
            .lane_underrun_frames
            .saturating_sub(*reported_lane_underrun_frames)
            .saturating_add(24)
            / 48;
        eprintln!(
            "{}",
            ssr_i18n::live::status(
                language,
                format_args!("{second:>4}"),
                format_args!("{:.1}", status.latency.p50_ms),
                format_args!("{:.1}", status.latency.p99_ms),
                format_args!("{:.1}", status.ring_level_ms),
                status.underruns,
                gap_ms,
                status.output_restarts,
                format_args!("{:+.0}", status.clock_ppm),
                status.lost_bytes,
                recorder_dropped,
                suffix,
                fault,
            )
        );
        if status.underruns > *reported_underruns
            && let Some(value) = status.last_underrun
        {
            eprintln!(
                "{}",
                ssr_i18n::live::underrun(
                    language,
                    value.ring_frames,
                    value.marker_frames,
                    value.layout_frames,
                    value.needed_frames,
                    value.produced_frames,
                    format_args!("{:.1}", value.since_block_ms),
                )
            );
        }
        *reported_underruns = status.underruns;
        *reported_lane_underrun_frames = status.lane_underrun_frames;
    }

    pub fn devices(formats: bool, json: bool, kernel: bool) -> Result<(), String> {
        if kernel {
            return kernel_devices(formats, json);
        }
        if json {
            let endpoints = if formats {
                devices::render_endpoint_formats()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(|formats| {
                        serde_json::json!({
                            "id": formats.endpoint.id,
                            "name": formats.endpoint.name,
                            "default": formats.endpoint.is_default,
                            "mix": {
                                "rate": formats.mix.rate,
                                "channels": formats.mix.channels,
                                "channel_mask": formats.mix.channel_mask,
                            },
                            "layouts": formats.layouts.into_iter().map(|layout| serde_json::json!({
                                "channels": layout.channels,
                                "channel_mask": layout.channel_mask,
                                "exclusive": layout.exclusive,
                            })).collect::<Vec<_>>(),
                        })
                    })
                    .collect::<Vec<_>>()
            } else {
                devices::render_endpoints()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(|endpoint| {
                        serde_json::json!({
                            "id": endpoint.id,
                            "name": endpoint.name,
                            "default": endpoint.is_default,
                        })
                    })
                    .collect::<Vec<_>>()
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&endpoints).map_err(|e| e.to_string())?
            );
            return Ok(());
        }
        match DriverRing::open() {
            Ok(ring) => {
                let info = ring.info();
                let language = ssr_i18n::cli_language();
                println!(
                    "{}",
                    ssr_i18n::live::driver_info(
                        language,
                        format_args!("{:X}", info.endpoint_mask),
                        info.qpc_frequency,
                    )
                );
                for endpoint in 0..ssr_abi::MAX_ENDPOINTS {
                    if info.endpoint_mask & (1_u32 << endpoint) != 0 {
                        let formats = info.endpoints[endpoint];
                        let codecs = [
                            (ssr_abi::FORMAT_AC3, "AC-3"),
                            (ssr_abi::FORMAT_EAC3, "E-AC-3"),
                            (ssr_abi::FORMAT_EAC3_JOC, "E-AC-3 JOC"),
                            (ssr_abi::FORMAT_TRUEHD, "TrueHD"),
                            (ssr_abi::FORMAT_MAT20, "MAT 2.0"),
                            (ssr_abi::FORMAT_MAT21, "MAT 2.1"),
                            (ssr_abi::FORMAT_DTS, "DTS"),
                            (ssr_abi::FORMAT_DTS_HD, "DTS-HD"),
                        ]
                        .into_iter()
                        .filter_map(|(bit, name)| (formats.codecs & bit != 0).then_some(name))
                        .collect::<Vec<_>>();
                        let rates = ssr_abi::PCM_RATES
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| formats.pcm.rates & (1 << i) != 0)
                            .map(|(_, rate)| rate.to_string())
                            .collect::<Vec<_>>();
                        let shapes = ["int16", "int24 packed", "int24-in-32", "int32", "float32"]
                            .into_iter()
                            .enumerate()
                            .filter_map(|(i, shape)| {
                                (formats.pcm.shapes & (1 << i) != 0).then_some(shape)
                            })
                            .collect::<Vec<_>>();
                        let mut direct = Vec::new();
                        let mut channel = 1_u32;
                        while channel <= 64 {
                            if formats.pcm.direct_channels & (1_u64 << (channel - 1)) == 0 {
                                channel += 1;
                                continue;
                            }
                            let first = channel;
                            while channel < 64
                                && formats.pcm.direct_channels & (1_u64 << channel) != 0
                            {
                                channel += 1;
                            }
                            direct.push(if first == channel {
                                first.to_string()
                            } else {
                                format!("{first}-{channel}")
                            });
                            channel += 1;
                        }
                        println!(
                            "{}",
                            ssr_i18n::live::driver_endpoint(language, endpoint, codecs.join(", "),)
                        );
                        println!(
                            "{}",
                            ssr_i18n::live::driver_pcm(
                                language,
                                rates.join(", "),
                                shapes.join(", "),
                                formats.pcm.max_masked_channels,
                                direct.join(", "),
                            )
                        );
                    }
                }
            }
            Err(CaptureError::NotPresent) => println!(
                "{}",
                ssr_i18n::live::driver_missing(ssr_i18n::cli_language())
            ),
            Err(error) => println!(
                "{}",
                ssr_i18n::live::driver_error(ssr_i18n::cli_language(), error)
            ),
        }
        if formats {
            for formats in devices::render_endpoint_formats().map_err(|e| e.to_string())? {
                let mark = if formats.endpoint.is_default {
                    "*"
                } else {
                    " "
                };
                println!(
                    "{mark} {}  [{}]",
                    formats.endpoint.name, formats.endpoint.id
                );
                let language = ssr_i18n::cli_language();
                println!(
                    "{}",
                    ssr_i18n::live::device_mix(
                        language,
                        formats.mix.rate,
                        formats.mix.channels,
                        format_args!("{:#x}", formats.mix.channel_mask),
                    )
                );
                for layout in formats.layouts {
                    println!(
                        "{}",
                        ssr_i18n::live::device_layout(
                            language,
                            format_args!("{:>2}", layout.channels),
                            format_args!("{:#07x}", layout.channel_mask),
                            if layout.exclusive {
                                ssr_i18n::live::yes(language)
                            } else {
                                ssr_i18n::live::no(language)
                            },
                        )
                    );
                }
            }
        } else {
            for endpoint in devices::render_endpoints().map_err(|e| e.to_string())? {
                let mark = if endpoint.is_default { "*" } else { " " };
                println!("{mark} {}  [{}]", endpoint.name, endpoint.id);
            }
        }
        Ok(())
    }

    fn selected_endpoint(name: Option<&str>) -> Result<devices::Endpoint, String> {
        let endpoints = devices::render_endpoints().map_err(|e| e.to_string())?;
        let choices = endpoints
            .iter()
            .map(|e| (e.id.as_str(), e.name.as_str(), e.is_default))
            .collect::<Vec<_>>();
        let index = super::select_endpoint(&choices, name)?;
        Ok(endpoints[index].clone())
    }

    fn restore_endpoint(endpoint: &str) -> Result<(), String> {
        let mut host = windows_host(&sibling_setup_exe()?, None)?;
        let id = DeviceId(endpoint.to_owned());
        let mut last_error = None;
        for _ in 0..20 {
            if let Err(error) = host.set_endpoint_visible(&id, true) {
                last_error = Some(error.to_string());
            }
            if host
                .outputs()
                .iter()
                .any(|output| output.id == id && output.state == ssr_types::DeviceState::Active)
            {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err(last_error.unwrap_or_else(|| {
            ssr_i18n::live::endpoint_not_active(ssr_i18n::cli_language(), endpoint)
        }))
    }

    pub fn ks_probe(output: &str, seconds: f64, json: Option<&Path>) -> Result<(), String> {
        use ssr_types::{OutputLayout, OutputOpener, OutputSink};
        let mut report = super::ks_lifecycle::probe_report();
        let mut chosen = None;
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                let endpoint = selected_endpoint(Some(output))?;
                chosen = Some(endpoint.id.clone());
                let cancel = ssr_sys::ks::ConsoleCancel::install().map_err(|e| e.to_string())?;
                let mut host = windows_host(&sibling_setup_exe()?, None)?;
                let id = DeviceId(endpoint.id.clone());
                let target = host.kernel_target(&id).map_err(|e| e.to_string())?;
                report["filter"] = target.filter.clone().into();
                report["pin"] = target.pin.into();
                let mut restore =
                    super::ks_lifecycle::RestoreOnDrop::new(|| restore_endpoint(&endpoint.id));
                host.set_endpoint_visible(&id, false)
                    .map_err(|e| e.to_string())?;
                let mut opener = ssr_sys::ks::KsOpener {
                    target,
                    rate: 48_000,
                    period: None,
                };
                let mut sink = opener
                    .open(OutputLayout::STEREO)
                    .map_err(|e| e.to_string())?;
                let format = sink.format();
                report["opened"] = true.into();
                report["interface"] = sink.interface().into();
                report["sample_format"] = sink.sample_format().into();
                report["rate"] = format.rate.into();
                report["channels"] = format.channels.into();
                report["channel_mask"] = format.channel_mask.into();
                let started = Instant::now();
                let mut frame = 0_u64;
                let amplitude = 10.0_f32.powf(-12.0 / 20.0);
                while started.elapsed().as_secs_f64() < seconds && !cancel.cancelled() {
                    sink.next_period(&mut |samples| {
                        for channels in samples.chunks_mut(usize::from(format.channels)) {
                            let sample = (std::f64::consts::TAU * 1000.0 * frame as f64
                                / f64::from(format.rate))
                            .sin() as f32
                                * amplitude;
                            channels.fill(sample);
                            frame += 1;
                        }
                    })
                    .map_err(|e| e.to_string())?;
                }
                report["late_resyncs"] = sink.late_resyncs().into();
                drop(sink);
                restore.restore()
            }));
        let mut result = result.unwrap_or_else(|_| {
            Err(ssr_i18n::live::ks_probe_panicked(ssr_i18n::cli_language()).to_owned())
        });
        // Read the actual state after every unwind/error path has dropped its sink
        // and restoration guard. A successful visibility call alone is not proof.
        if let Some(id) = chosen {
            let inspection = sibling_setup_exe()
                .and_then(|exe| windows_host(&exe, None))
                .map(|mut host| host.outputs());
            let restored = inspection.as_ref().is_ok_and(|outputs| {
                outputs.iter().any(|output| {
                    output.id.0 == id && output.state == ssr_types::DeviceState::Active
                })
            });
            report["endpoint_restored"] = restored.into();
            if !restored && result.is_ok() {
                result = Err(inspection.err().unwrap_or_else(|| {
                    ssr_i18n::live::endpoint_not_active(ssr_i18n::cli_language(), id)
                }));
            }
        }
        if let Err(error) = &result {
            report["error"] = error.clone().into();
        }
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        println!("{text}");
        if let Some(path) = json {
            std::fs::write(path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        eprintln!(
            "{}",
            ssr_i18n::live::ks_probe_result(
                ssr_i18n::cli_language(),
                &report["opened"],
                &report["endpoint_restored"]
            )
        );
        result
    }

    fn kernel_devices(formats: bool, json: bool) -> Result<(), String> {
        let mut host = windows_host(&sibling_setup_exe()?, None)?;
        let outputs = host.outputs();
        let defaults = host.default_roles();
        let formats = if formats {
            devices::render_endpoint_formats().map_err(|e| e.to_string())?
        } else {
            Vec::new()
        };
        let mut rows = Vec::new();
        for output in outputs {
            let target = host.kernel_target(&output.id);
            let mut row = serde_json::json!({
                "id": output.id.0, "name": output.name,
                "default": defaults.console.as_ref() == Some(&output.id),
                "state": output.state,
                "kernel": match &target {
                    Ok(target) => serde_json::json!({"filter": target.filter, "pin": target.pin, "reason": null}),
                    Err(error) => serde_json::json!({"filter": null, "pin": null, "reason": error.to_string()}),
                },
            });
            if let Some(format) = formats
                .iter()
                .find(|format| format.endpoint.id == output.id.0)
            {
                row["mix"] = serde_json::json!({"rate": format.mix.rate, "channels": format.mix.channels, "channel_mask": format.mix.channel_mask});
                row["layouts"] = format.layouts.iter().map(|layout| serde_json::json!({"channels": layout.channels, "channel_mask": layout.channel_mask, "exclusive": layout.exclusive})).collect::<Vec<_>>().into();
            }
            if !json {
                println!("{}  [{}]", output.name, output.id.0);
                match target {
                    Ok(target) => println!(
                        "{}",
                        ssr_i18n::live::ks_target(
                            ssr_i18n::cli_language(),
                            target.filter,
                            target.pin
                        )
                    ),
                    Err(error) => println!(
                        "{}",
                        ssr_i18n::live::ks_target_failed(ssr_i18n::cli_language(), error)
                    ),
                }
            }
            rows.push(row);
        }
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?
            );
        }
        Ok(())
    }

    /// `ssr-setup.exe` beside the running ssr-live.exe.
    fn sibling_setup_exe() -> Result<PathBuf, String> {
        let exe = std::env::current_exe()
            .map_err(|error| ssr_i18n::live::locate_exe_failed(ssr_i18n::cli_language(), error))?;
        Ok(exe.with_file_name(SETUP_EXE))
    }

    /// The adapter `host` drives. Both paths are made absolute: the elevated
    /// `ssr-setup` does not start in this process's directory, and it is given
    /// the package files and its result file by path.
    fn windows_host(setup_exe: &Path, state_dir: Option<PathBuf>) -> Result<WindowsHost, String> {
        let absolute = |path: &Path| {
            std::path::absolute(path).map_err(|error| format!("{}: {error}", path.display()))
        };
        let state_dir = host_state_dir(state_dir, &std::env::temp_dir());
        Ok(WindowsHost::new(
            absolute(setup_exe)?,
            absolute(&state_dir)?,
        ))
    }

    /// Prints the answer's document, if any, then fails with its error.
    fn print_answer(answer: host_json::Answer) -> Result<(), String> {
        if let Some(document) = &answer.stdout {
            println!("{}", host_json::to_ascii_json(document));
        }
        answer.error.map_or(Ok(()), Err)
    }

    pub fn host_apps() -> Result<(), String> {
        let mut host = windows_host(&sibling_setup_exe()?, None)?;
        let document = super::app_routing::apps_json(&mut host);
        println!("{}", host_json::to_ascii_json(&document));
        Ok(())
    }

    pub fn host_set_app(pid: u32, endpoint: &str) -> Result<(), String> {
        let mut host = windows_host(&sibling_setup_exe()?, None)?;
        let document = super::app_routing::set_app(&mut host, pid, endpoint)?;
        println!("{}", host_json::to_ascii_json(&document));
        Ok(())
    }

    pub fn host_show(state_dir: Option<PathBuf>) -> Result<(), String> {
        let mut host = windows_host(&sibling_setup_exe()?, state_dir)?;
        let platform = host.platform();
        let driver = host.driver();
        let outputs = host.outputs();
        let roles = host.default_roles();
        let document = host_json::show_json(&platform, &driver, &outputs, &roles);
        println!("{}", host_json::to_ascii_json(&document));
        Ok(())
    }

    pub fn host_set_default(
        role: Role,
        id: String,
        state_dir: Option<PathBuf>,
    ) -> Result<(), String> {
        let mut host = windows_host(&sibling_setup_exe()?, state_dir)?;
        let id = DeviceId(id);
        let result = host.set_default_role(role, &id);
        print_answer(host_json::set_default_answer(role, &id, result))
    }

    pub fn host_setup(
        task: &SetupTask,
        package_dir: &Path,
        state_dir: Option<PathBuf>,
    ) -> Result<(), String> {
        let package_dir = std::path::absolute(package_dir)
            .map_err(|error| format!("{}: {error}", package_dir.display()))?;
        let setup_exe = package_setup_exe(&package_dir, sibling_setup_exe()?, Path::is_file);
        if setup_exe != package_dir.join(SETUP_EXE) {
            eprintln!(
                "{}",
                ssr_i18n::live::package_setup_fallback(
                    ssr_i18n::cli_language(),
                    package_dir.display(),
                    setup_exe.display(),
                )
            );
        }
        let mut host = windows_host(&setup_exe, state_dir)?;
        print_answer(host_json::setup_answer(host.run_setup(task)))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use ssr_abi::RecordWriter;

        fn record_qpc(bytes: &[u8]) -> u64 {
            ssr_abi::raw_records(bytes)
                .next()
                .unwrap()
                .unwrap()
                .header
                .qpc
        }

        #[test]
        fn paced_dump_releases_records_at_their_pace_and_restamps_them() {
            // 10 MHz dump clock: a stream start at 0, data at 10 and 20 ms.
            let mut dump = dump_header_bytes(10_000_000);
            let mut buf = vec![0_u8; 4096];
            let mut writer = RecordWriter::new(&mut buf);
            assert!(writer.stream_start(1, 0));
            assert!(writer.data(1, 0, 100_000, &[1; 16]));
            assert!(writer.data(1, 16, 200_000, &[2; 16]));
            let len = writer.len();
            dump.extend_from_slice(&buf[..len]);
            let ended = Arc::new(AtomicBool::new(false));
            let mut paced = DumpCapture::from_dump(
                &dump,
                qpc::frequency(),
                qpc::now,
                Some(ended.clone()),
                false,
            )
            .unwrap();
            let frequency = qpc::frequency();

            let mut out = vec![0_u8; 4096];
            let before = qpc::now();
            let n = paced.read(&mut out, Duration::ZERO).unwrap();
            assert_eq!(n, ssr_abi::record_size(0), "only the stream start is due");
            assert!(record_qpc(&out) >= before);

            // A record is stamped when it is released, never before it is due
            // (counted from the first read, which is after `before`). A late
            // release delays only its own stamp, so each one is checked
            // against its due time rather than against the one before it.
            let released = released_data(&mut paced, 2);
            for (&(payload, _, stamp), (expected, due_ms)) in
                released.iter().zip([(1, 10), (2, 20)])
            {
                assert_eq!(payload, expected);
                assert!(
                    stamp - before >= due(frequency, due_ms),
                    "released before {due_ms} ms"
                );
            }

            assert!(!ended.load(Ordering::Acquire));
            assert_eq!(
                paced.read(&mut out, Duration::ZERO),
                Err(CaptureError::Ended)
            );
            assert!(ended.load(Ordering::Acquire));
        }

        /// QPC ticks from the origin to a record due `ms` later, rounded down
        /// as the pacing does.
        fn due(frequency: u64, ms: u64) -> u64 {
            frequency * ms / 1000
        }

        /// Reads until `count` data records have been released and returns
        /// each one's first payload byte, byte position and stamp. A read
        /// that comes late hands out every record that fell due meanwhile,
        /// so the records are checked one by one rather than read by read.
        fn released_data(paced: &mut DumpCapture, count: usize) -> Vec<(u8, u64, u64)> {
            let mut out = vec![0_u8; 4096];
            let mut released = Vec::new();
            while released.len() < count {
                let n = paced.read(&mut out, Duration::from_millis(500)).unwrap();
                assert!(n > 0, "a record falls due within 500 ms");
                for raw in ssr_abi::raw_records(&out[..n]) {
                    let raw = raw.unwrap();
                    assert_eq!(raw.size, ssr_abi::record_size(16));
                    released.push((raw.payload[0], raw.header.byte_pos, raw.header.qpc));
                }
            }
            released
        }

        #[test]
        fn paced_dump_loops_with_continuous_positions_and_times() {
            // Data at 10 and 20 ms, 16 bytes each: one pass spans 20 ms and
            // 32 bytes, so the repeat is due at 30 and 40 ms at 32 and 48.
            let mut dump = dump_header_bytes(10_000_000);
            let mut buf = vec![0_u8; 4096];
            let mut writer = RecordWriter::new(&mut buf);
            assert!(writer.stream_start(1, 0));
            assert!(writer.data(1, 0, 100_000, &[1; 16]));
            assert!(writer.data(1, 16, 200_000, &[2; 16]));
            assert!(writer.stream_stop(1, 250_000));
            let len = writer.len();
            dump.extend_from_slice(&buf[..len]);
            let ended = Arc::new(AtomicBool::new(false));
            let mut paced = DumpCapture::from_dump(
                &dump,
                qpc::frequency(),
                qpc::now,
                Some(ended.clone()),
                true,
            )
            .unwrap();
            let frequency = qpc::frequency();

            let mut out = vec![0_u8; 4096];
            let before = qpc::now();
            assert_eq!(
                paced.read(&mut out, Duration::ZERO).unwrap(),
                ssr_abi::record_size(0)
            );
            let released = released_data(&mut paced, 5);
            for (&(payload, pos, stamp), (pass, expected, expected_pos, due_ms)) in
                released.iter().zip([
                    (0, 1, 0, 10),
                    (0, 2, 16, 20),
                    (1, 1, 32, 30),
                    (1, 2, 48, 40),
                    (2, 1, 64, 50),
                ])
            {
                assert_eq!(payload, expected, "pass {pass}");
                assert_eq!(pos, expected_pos, "pass {pass}");
                assert!(
                    stamp - before >= due(frequency, due_ms),
                    "released before {due_ms} ms in pass {pass}"
                );
            }
            // The stop record after the span never plays while looping.
            assert!(!ended.load(Ordering::Acquire));
        }

        #[test]
        fn format_two_loop_span_uses_whole_recorded_reads() {
            let mut dump = dump_header_bytes(1);
            let mut buf = vec![0_u8; 512];
            let mut writer = RecordWriter::new(&mut buf);
            assert!(writer.stream_start(1, 0));
            assert!(writer.read_end(0));
            assert!(writer.format_change(1, 0, &[]));
            assert!(writer.data(1, 0, 0, &[1]));
            assert!(writer.read_end(0));
            assert!(writer.data(1, 1, 0, &[2]));
            assert!(writer.stream_stop(1, 0));
            assert!(writer.read_end(0));
            assert!(writer.stream_start(2, 0));
            assert!(writer.read_end(0));
            let len = writer.len();
            dump.extend_from_slice(&buf[..len]);

            let mut capture = DumpCapture::from_dump(&dump, 1, || 0, None, true).unwrap();
            let mut out = vec![0_u8; 512];
            let mut next_kinds = || {
                let len = capture.read(&mut out, Duration::ZERO).unwrap();
                ssr_abi::raw_records(&out[..len])
                    .map(|record| record.unwrap().header.kind)
                    .collect::<Vec<_>>()
            };
            assert_eq!(next_kinds(), [ssr_abi::RECORD_KIND_STREAM_START]);
            assert_eq!(
                next_kinds(),
                [
                    ssr_abi::RECORD_KIND_FORMAT_CHANGE,
                    ssr_abi::RECORD_KIND_DATA
                ]
            );
            assert_eq!(
                next_kinds(),
                [ssr_abi::RECORD_KIND_DATA, ssr_abi::RECORD_KIND_STREAM_STOP]
            );
            assert_eq!(
                next_kinds(),
                [
                    ssr_abi::RECORD_KIND_FORMAT_CHANGE,
                    ssr_abi::RECORD_KIND_DATA
                ]
            );
        }

        #[test]
        fn a_dump_that_ends_on_data_keeps_looping() {
            // The fixtures end on their last data record, with no stop.
            let mut dump = dump_header_bytes(10_000_000);
            let mut buf = vec![0_u8; 4096];
            let mut writer = RecordWriter::new(&mut buf);
            assert!(writer.data(1, 0, 100_000, &[1; 16]));
            assert!(writer.data(1, 16, 200_000, &[2; 16]));
            let len = writer.len();
            dump.extend_from_slice(&buf[..len]);
            let ended = Arc::new(AtomicBool::new(false));
            let mut paced = DumpCapture::from_dump(
                &dump,
                qpc::frequency(),
                qpc::now,
                Some(ended.clone()),
                true,
            )
            .unwrap();
            let positions = released_data(&mut paced, 5)
                .iter()
                .map(|&(_, pos, _)| pos)
                .take(5)
                .collect::<Vec<_>>();
            assert_eq!(positions, [0, 16, 32, 48, 64]);
            assert!(!ended.load(Ordering::Acquire));
        }

        #[test]
        fn looping_needs_two_data_records() {
            let mut dump = dump_header_bytes(10_000_000);
            let mut buf = vec![0_u8; 256];
            let mut writer = RecordWriter::new(&mut buf);
            assert!(writer.data(1, 0, 100_000, &[1; 16]));
            let len = writer.len();
            dump.extend_from_slice(&buf[..len]);
            let ended = Arc::new(AtomicBool::new(false));
            assert!(
                DumpCapture::from_dump(&dump, qpc::frequency(), qpc::now, Some(ended), true)
                    .is_err()
            );
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::{Path, PathBuf};

    use super::{CaptureKind, Clock};

    #[allow(dead_code)]
    pub struct PlayOptions {
        pub source: CaptureKind,
        pub asio: bool,
        pub silence: bool,
        pub endpoints: Option<String>,
        pub profile: PathBuf,
        pub output: Option<String>,
        pub kernel_streaming: bool,
        pub shared: bool,
        pub allow_shared_fallback: bool,
        pub rate: u32,
        pub period_ms: Option<f64>,
        pub target_ms: f32,
        pub clock: Clock,
        pub record: Option<PathBuf>,
        pub seconds: Option<f64>,
        pub from_dump: Option<PathBuf>,
        pub repeat: bool,
        pub mute: bool,
        pub report: Option<PathBuf>,
        pub scene_log: Option<PathBuf>,
        pub scene_log_ms: u64,
        pub direct_layouts: Vec<engine::DirectLayoutOverride>,
    }

    pub fn record(_source: CaptureKind, _out: &Path, _seconds: f64) -> Result<(), String> {
        Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned())
    }

    pub fn play(_options: PlayOptions) -> Result<(), String> {
        Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned())
    }

    pub fn ks_probe(_output: &str, _seconds: f64, json: Option<&Path>) -> Result<(), String> {
        let error = ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned();
        let mut report = super::ks_lifecycle::probe_report();
        report["error"] = error.clone().into();
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        println!("{text}");
        if let Some(path) = json {
            std::fs::write(path, &text).map_err(|e| e.to_string())?;
        }
        Err(error)
    }

    pub fn devices(_formats: bool, _json: bool, _kernel: bool) -> Result<(), String> {
        Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned())
    }

    pub fn host_apps() -> Result<(), String> {
        Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned())
    }

    pub fn host_set_app(_pid: u32, _endpoint: &str) -> Result<(), String> {
        Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned())
    }

    pub fn host_show(_state_dir: Option<PathBuf>) -> Result<(), String> {
        Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned())
    }

    pub fn host_set_default(
        _role: ssr_types::Role,
        _id: String,
        _state_dir: Option<PathBuf>,
    ) -> Result<(), String> {
        Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned())
    }

    pub fn host_setup(
        _task: &ssr_types::SetupTask,
        _package_dir: &Path,
        _state_dir: Option<PathBuf>,
    ) -> Result<(), String> {
        Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned())
    }
}
