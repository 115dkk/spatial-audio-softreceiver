#![forbid(unsafe_code)]

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{f32::consts::TAU, io::Write, time::Instant};
    use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

    #[derive(Debug)]
    struct Options {
        output: String,
        seconds: f64,
        frequency: f64,
        level_db: f64,
        mute_session: bool,
        session_volume: Option<f32>,
    }

    fn options() -> Result<Options, Box<dyn std::error::Error>> {
        let mut output = None;
        let mut seconds = None;
        let mut frequency = 997.0;
        let mut level_db = -20.0;
        let mut mute_session = false;
        let mut session_volume = None;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--output" => output = Some(args.next().ok_or("--output needs a value")?),
                "--seconds" => {
                    seconds = Some(args.next().ok_or("--seconds needs a value")?.parse()?)
                }
                "--frequency" => {
                    frequency = args.next().ok_or("--frequency needs a value")?.parse()?
                }
                "--level-db" => {
                    level_db = args.next().ok_or("--level-db needs a value")?.parse()?
                }
                "--mute-session" => mute_session = true,
                "--session-volume" => {
                    session_volume = Some(
                        args.next()
                            .ok_or("--session-volume needs a value")?
                            .parse()?,
                    )
                }
                _ => return Err(format!("unknown argument {arg:?}").into()),
            }
        }
        let options = Options {
            output: output.ok_or("--output is required")?,
            seconds: seconds.ok_or("--seconds is required")?,
            frequency,
            level_db,
            mute_session,
            session_volume,
        };
        if !options.seconds.is_finite() || options.seconds <= 0.0 {
            return Err("--seconds must be finite and positive".into());
        }
        if !options.frequency.is_finite()
            || options.frequency <= 0.0
            || options.frequency >= 24_000.0
        {
            return Err("--frequency must be finite and in 0..24000".into());
        }
        if !options.level_db.is_finite() || options.level_db > 0.0 {
            return Err("--level-db must be finite and no greater than 0".into());
        }
        if options
            .session_volume
            .is_some_and(|volume| !(0.0..=1.0).contains(&volume))
        {
            return Err("--session-volume must be in 0..=1".into());
        }
        Ok(options)
    }

    let options = options()?;
    wasapi::initialize_mta().ok()?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let devices = DeviceEnumerator::new()?.get_device_collection(&Direction::Render)?;
        let needle = options.output.to_lowercase();
        let mut selected = None;
        for index in 0..devices.get_nbr_devices()? {
            let device = devices.get_device_at_index(index)?;
            if device.get_friendlyname()?.to_lowercase().contains(&needle) {
                selected = Some(device);
                break;
            }
        }
        let device = selected.ok_or("render endpoint missing")?;
        let mut client = device.get_iaudioclient()?;
        let wave = WaveFormat::new(32, 32, &SampleType::Float, 48_000, 2, Some(0x3));
        client.initialize_client(
            &wave,
            &Direction::Render,
            &StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: 200_000,
            },
        )?;
        let event = client.set_get_eventhandle()?;
        let render = client.get_audiorenderclient()?;
        let amplitude = 10.0_f32.powf(options.level_db as f32 / 20.0);
        let total_frames = (options.seconds * 48_000.0).round() as usize;
        let buffer_frames = client.get_buffer_size()? as usize;
        let mut samples = vec![0_u8; buffer_frames * 8];
        let mut frame = 0_usize;
        let fill = |bytes: &mut [u8], first: usize| {
            for (offset, stereo) in bytes.as_chunks_mut::<8>().0.iter_mut().enumerate() {
                let sample = amplitude
                    * (TAU * options.frequency as f32 * (first + offset) as f32 / 48_000.0).sin();
                stereo[..4].copy_from_slice(&sample.to_le_bytes());
                stereo[4..].copy_from_slice(&sample.to_le_bytes());
            }
        };
        let initial = buffer_frames.min(total_frames);
        fill(&mut samples[..initial * 8], frame);
        render.write_to_device(initial, &samples[..initial * 8], None)?;
        frame += initial;
        client.start_stream()?;
        ssr_sys::app_routing::set_process_session_volume(
            std::process::id(),
            options.mute_session,
            Some(options.session_volume.unwrap_or(1.0)),
        )?;
        println!("pid={}", std::process::id());
        std::io::stdout().flush()?;
        let started = Instant::now();
        while frame < total_frames {
            event.wait_for_event(2_000)?;
            let available = client.get_available_space_in_frames()? as usize;
            let count = available.min(total_frames - frame);
            fill(&mut samples[..count * 8], frame);
            render.write_to_device(count, &samples[..count * 8], None)?;
            frame += count;
        }
        while started.elapsed().as_secs_f64() < options.seconds {
            std::thread::yield_now();
        }
        client.stop_stream()?;
        Ok(())
    })();
    wasapi::deinitialize();
    result
}

#[cfg(not(windows))]
fn main() {
    eprintln!("tone_player requires Windows");
}
