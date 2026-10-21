//! The SDK-facing probe lives in the separate `ssr-asio` companion, never here.

use std::path::Path;

pub fn parse_seconds(value: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .ok()
        .filter(|seconds| (1..=3600).contains(seconds))
        .ok_or_else(|| {
            ssr_i18n::live::asio_probe_invalid_seconds(ssr_i18n::cli_language()).to_owned()
        })
}

#[cfg(any(windows, test))]
fn command(
    current_exe: &Path,
    dll: &Path,
    seconds: u32,
    json: Option<&Path>,
) -> Result<std::process::Command, String> {
    if !current_exe.is_absolute() || current_exe.file_name().is_none() {
        return Err(
            ssr_i18n::live::asio_probe_executable_path(ssr_i18n::cli_language()).to_owned(),
        );
    }
    let executable = current_exe.with_file_name("asio_probe.exe");
    let mut command = std::process::Command::new(executable);
    // The companion's DLL argument is positional; the public ssr-live option
    // remains --dll. Its parser accepts the path verbatim, even with a dash.
    command.arg(dll);
    command.arg("--seconds").arg(seconds.to_string());
    if let Some(json) = json {
        command.arg("--json").arg(json);
    }
    Ok(command)
}

#[cfg(windows)]
pub fn run(dll: &Path, seconds: u32, json: Option<&Path>) -> Result<(), String> {
    let language = ssr_i18n::cli_language();
    let executable = std::env::current_exe()
        .map_err(|error| ssr_i18n::live::asio_probe_launch_failed(language, error))?;
    // No PATH lookup, shell, or output capture: preserve the companion's JSON
    // stdout and stderr, and pass paths as individual native arguments.
    let status = command(&executable, dll, seconds, json)?
        .status()
        .map_err(|error| ssr_i18n::live::asio_probe_launch_failed(language, error))?;
    if status.success() {
        Ok(())
    } else {
        Err(ssr_i18n::live::asio_probe_failed(language, status))
    }
}

#[cfg(not(windows))]
pub fn run(_dll: &Path, _seconds: u32, _json: Option<&Path>) -> Result<(), String> {
    Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_must_be_finite_positive_and_at_most_one_hour() {
        for value in ["0", "-1", "NaN", "inf", "-inf", "3600.01", "0.001", "bad"] {
            assert!(parse_seconds(value).is_err(), "{value}");
        }
        for value in ["1", "10", "3600"] {
            assert!(parse_seconds(value).is_ok(), "{value}");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn probe_on_other_platforms_reports_the_localized_platform_error() {
        assert_eq!(
            run(Path::new("driver.dll"), 1, None),
            Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned())
        );
    }

    #[test]
    fn companion_is_absolute_and_paths_are_single_arguments() {
        let current = std::env::current_exe().unwrap();
        let dll = Path::new("folder with spaces/driver;echo.exe.dll");
        let json = Path::new("results & other/report.json");
        let command = command(&current, dll, 2, Some(json)).unwrap();
        assert_eq!(
            command.get_program(),
            current.with_file_name("asio_probe.exe").as_os_str()
        );
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            [
                dll.as_os_str(),
                std::ffi::OsStr::new("--seconds"),
                std::ffi::OsStr::new("2"),
                std::ffi::OsStr::new("--json"),
                json.as_os_str(),
            ]
        );
        assert!(super::command(Path::new("ssr-live.exe"), dll, 1, None).is_err());
        assert_eq!(
            super::command(&current, dll, 1, None)
                .unwrap()
                .get_args()
                .count(),
            3
        );
    }
}
