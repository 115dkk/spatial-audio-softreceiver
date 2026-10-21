// SPDX-License-Identifier: MIT
#![forbid(unsafe_code)]
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
struct Options {
    dll: PathBuf,
    seconds: u32,
    frames: u32,
    json: Option<PathBuf>,
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Options, ()> {
    let mut args = args.into_iter();
    let mut dll = None;
    let mut seconds = 5;
    let mut frames = 512;
    let mut json = None;
    while let Some(flag) = args.next() {
        match flag.to_str() {
            Some("--dll") if dll.is_none() => dll = Some(PathBuf::from(args.next().ok_or(())?)),
            Some("--seconds") => {
                seconds = args
                    .next()
                    .ok_or(())?
                    .to_str()
                    .ok_or(())?
                    .parse()
                    .map_err(|_| ())?;
            }
            Some("--frames") => {
                frames = args
                    .next()
                    .ok_or(())?
                    .to_str()
                    .ok_or(())?
                    .parse()
                    .map_err(|_| ())?;
            }
            Some("--json") => json = Some(PathBuf::from(args.next().ok_or(())?)),
            _ if dll.is_none() && !flag.to_string_lossy().starts_with('-') => {
                dll = Some(PathBuf::from(flag));
            }
            _ => return Err(()),
        }
    }
    if !(1..=3600).contains(&seconds) || !asio_proto::accepts_buffer(frames) {
        return Err(());
    }
    Ok(Options {
        dll: dll.ok_or(())?,
        seconds,
        frames,
        json,
    })
}

fn main() {
    let language = ssr_i18n::cli_language();
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{}", ssr_i18n::live::asio_probe_usage(language));
        return;
    }
    let options = match parse(args) {
        Ok(options) => options,
        Err(()) => {
            eprintln!("{}", ssr_i18n::live::asio_probe_usage(language));
            std::process::exit(2);
        }
    };
    #[cfg(windows)]
    if let Err(detail) = run(options) {
        eprintln!(
            "{}",
            ssr_i18n::live::asio_probe_launch_failed(language, detail)
        );
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    {
        let _ = options;
        eprintln!("{}", ssr_i18n::live::windows_only(language));
        std::process::exit(2);
    }
}

#[cfg(windows)]
fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let report = asio_sys::probe::run(&options.dll, options.seconds, options.frames)?;
    let json_text = report.json();
    if let Some(path) = options.json {
        std::fs::write(path, &json_text)?;
    }
    println!("{json_text}");
    if !report.accepted() {
        return Err("ASIO_PROBE_EXPECTATION".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(args: &[&str]) -> Result<Options, ()> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn explicit_and_legacy_dll_paths_match() {
        assert_eq!(options(&["--dll", "driver.dll"]), options(&["driver.dll"]));
        let parsed = options(&[
            "--seconds",
            "3600",
            "--dll",
            "driver.dll",
            "--frames",
            "2048",
        ])
        .expect("valid boundary values");
        assert_eq!(parsed.seconds, 3600);
        assert_eq!(parsed.frames, 2048);
        assert!(options(&["--dll", "driver.dll", "--seconds", "1", "--frames", "64"]).is_ok());
    }

    #[test]
    fn malformed_or_out_of_range_arguments_are_rejected() {
        for args in [
            vec![],
            vec!["--dll"],
            vec!["--dll", "driver.dll", "extra.dll"],
            vec!["driver.dll", "--unknown", "1"],
            vec!["driver.dll", "--seconds", "0"],
            vec!["driver.dll", "--seconds", "3601"],
            vec!["driver.dll", "--seconds", "NaN"],
            vec!["driver.dll", "--frames", "63"],
            vec!["driver.dll", "--frames", "2049"],
        ] {
            assert_eq!(options(&args), Err(()), "{args:?}");
        }
    }
}
