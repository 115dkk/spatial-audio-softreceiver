use clap::Subcommand;
use serde_json::json;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum BitstreamCommand {
    /// Probe exclusive IEC 61937 codec formats on our endpoints.
    Probe {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Play raw E-AC-3 syncframes to the passthrough endpoint.
    Play {
        /// Our passthrough endpoint number (2).
        #[arg(long, value_parser = clap::value_parser!(u16).range(2..=2))]
        endpoint: u16,
        /// Encoded input codec (eac3).
        #[arg(long, value_parser = ["eac3"])]
        codec: String,
        /// Raw 48 kHz, 5.1, six-block E-AC-3 syncframes.
        #[arg(long)]
        input: PathBuf,
        /// Seconds to play (greater than zero, at most 3600).
        #[arg(long, default_value_t = 8.0, value_parser = seconds)]
        seconds: f64,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}

fn seconds(value: &str) -> Result<f64, String> {
    crate::ks_lifecycle::positive_seconds(value).and_then(|seconds| {
        if seconds <= 3600.0 {
            Ok(seconds)
        } else {
            Err(ssr_i18n::live::invalid_seconds(ssr_i18n::cli_language()).to_owned())
        }
    })
}

#[cfg_attr(not(windows), allow(dead_code))]
fn prepare(bytes: &[u8], seconds: f64) -> Result<(Vec<u8>, u64), String> {
    if !seconds.is_finite() || seconds <= 0.0 || seconds > 3600.0 {
        return Err("duration_out_of_range".into());
    }
    let frames =
        bitstream::eac3::frames_48k_51(bytes).map_err(|error| format!("eac3:{error:?}"))?;
    let period = bitstream::repetition_period(bitstream::DataType::Eac3).expect("EAC3 period");
    // An input file can otherwise expand 3,072-fold with tiny malicious frames.
    let capacity = frames
        .len()
        .checked_mul(period)
        .filter(|size| *size <= 256 * 1024 * 1024)
        .ok_or("prepared_stream_too_large")?;
    let mut bursts = Vec::with_capacity(capacity);
    for frame in frames {
        bitstream::frame_burst(bitstream::DataType::Eac3, frame, period, &mut bursts);
    }
    Ok((bursts, (seconds / 0.032).ceil() as u64))
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn hresult(hr: i32) -> String {
    format!("0x{:08X}", hr as u32)
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn probe_json(endpoints: Vec<ssr_sys::bitstream::EndpointProbe>) -> serde_json::Value {
    json!({"endpoints": endpoints.into_iter().map(|endpoint| json!({
        "endpoint": endpoint.endpoint, "id": endpoint.id, "codec_mask": endpoint.codec_mask,
        "codecs": endpoint.codecs.into_iter().map(|codec| json!({
            "bit": codec.bit, "codec": codec.codec, "supported_hr": hresult(codec.supported_hr),
            "initialize_hr": codec.initialize_hr.map(hresult)
        })).collect::<Vec<_>>()
    })).collect::<Vec<_>>()})
}

pub fn run(command: BitstreamCommand) -> Result<(), String> {
    #[cfg(windows)]
    let result = match command {
        BitstreamCommand::Probe { .. } => ssr_sys::bitstream::probe()
            .map(probe_json)
            .map(|value| (value, true)),
        BitstreamCommand::Play { input, seconds, .. } => (|| {
            use std::io::Read;
            // Bound the input before expansion; all reading and framing precedes WASAPI.
            let file = std::fs::File::open(&input).map_err(|e| format!("input:{e}"))?;
            let mut bytes = Vec::new();
            file.take(16 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| format!("input:{e}"))?;
            if bytes.len() > 16 * 1024 * 1024 {
                return Err("input_too_large".to_owned());
            }
            let (bursts, count) = prepare(&bytes, seconds)?;
            let result = ssr_sys::bitstream::play_eac3(&bursts, count)?;
            Ok((
                json!({"endpoint":2, "codec":"eac3", "sent_bursts":result.sent_bursts,
                "sent_frames":result.sent_frames, "error_hr":result.error_hr.map(hresult)}),
                result.error_hr.is_none(),
            ))
        })(),
    };
    #[cfg(not(windows))]
    let result: Result<(serde_json::Value, bool), String> = {
        let _ = command;
        Err("windows_only".to_owned())
    };
    match result {
        Ok((value, success)) => {
            println!("{value}");
            if success {
                Ok(())
            } else {
                Err(value.to_string())
            }
        }
        Err(error) => {
            println!("{}", json!({"error":error}));
            Err(json!({"error":error}).to_string())
        }
    }
}

pub fn host_formats() -> Result<(), String> {
    #[cfg(windows)]
    {
        let endpoints = ssr_sys::host::endpoint_formats()?;
        let endpoints: Vec<_> = endpoints.into_iter().map(|row| json!({
            "endpoint":row.endpoint, "id":row.id, "state":row.state,
            "device_desc":row.device_desc, "friendly_name":row.friendly_name,
            "device_format":row.device_format.map(|f| json!({"tag":f.tag, "subformat":f.subformat,
                "rate":f.rate, "channels":f.channels, "bits":f.bits, "valid_bits":f.valid_bits, "channel_mask":f.channel_mask})),
            "device_format_error":row.device_format_error
        })).collect();
        println!("{}", json!({"endpoints":endpoints}));
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_frames_round_trip_through_iec_bursts() {
        let raw = include_bytes!("../../../crates/decode/tests/fixtures/ddp/tones-5.1.eac3");
        let frames = bitstream::eac3::frames_48k_51(raw).unwrap();
        assert_eq!(frames.len(), 32);
        assert!(frames.iter().all(|frame| frame.len() == 2560));
        let (bytes, count) = prepare(raw, 8.0).unwrap();
        assert_eq!(count, 250);
        let mut deframer = bitstream::Deframer::new();
        let mut index = 0;
        for chunk in bytes.chunks(997) {
            deframer.push(chunk, &mut |burst| {
                assert_eq!(burst.data_type, bitstream::DataType::Eac3);
                assert_eq!(burst.payload, frames[index]);
                index += 1;
            });
        }
        assert_eq!(index, 32);
        assert_eq!(deframer.stats().resyncs, 0);
    }

    #[test]
    fn documented_commands_and_invalid_arguments() {
        use clap::Parser;
        assert!(crate::Cli::try_parse_from(["ssr-live", "host", "formats"]).is_ok());
        assert!(crate::Cli::try_parse_from(["ssr-live", "bitstream", "probe", "--json"]).is_ok());
        let args = [
            "ssr-live",
            "bitstream",
            "play",
            "--endpoint",
            "2",
            "--codec",
            "eac3",
            "--input",
            "file",
            "--seconds",
            "8",
            "--json",
        ];
        assert!(crate::Cli::try_parse_from(args).is_ok());
        for (index, value) in [
            (4, "0"),
            (4, "3"),
            (6, "ac3"),
            (10, "0"),
            (10, "NaN"),
            (10, "3601"),
        ] {
            let mut invalid = args;
            invalid[index] = value;
            assert!(crate::Cli::try_parse_from(invalid).is_err());
        }
    }

    #[test]
    fn duration_bounds_and_burst_count() {
        for value in ["0", "-1", "NaN", "inf", "3600.01"] {
            assert!(seconds(value).is_err());
        }
        assert_eq!(seconds("3600"), Ok(3600.0));
        let frame = [0x0b, 0x77, 0, 7, 0x3f, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let (bytes, count) = prepare(&frame, 8.0).unwrap();
        assert_eq!(count, 250);
        assert_eq!(bytes.len(), 24_576);
        assert_eq!(&bytes[..8], &[0x72, 0xf8, 0x1f, 0x4e, 21, 0, 16, 0]);
        assert_eq!(prepare(&frame, 0.033).unwrap().1, 2);
    }
    #[test]
    fn json_uses_bit_indices_and_exact_hresult_strings() {
        let value = probe_json(vec![ssr_sys::bitstream::EndpointProbe {
            endpoint: 0,
            id: "id".into(),
            codec_mask: 108,
            codecs: vec![ssr_sys::bitstream::CodecProbe {
                bit: 1,
                codec: "ac3",
                supported_hr: 0x88890008u32 as i32,
                initialize_hr: None,
            }],
        }]);
        assert_eq!(
            value["endpoints"][0]["codecs"][0],
            json!({"bit":1,"codec":"ac3","supported_hr":"0x88890008","initialize_hr":null})
        );
        assert_eq!(hresult(0), "0x00000000");
        assert_eq!(hresult(0xabcdef01u32 as i32), "0xABCDEF01");
    }
}
