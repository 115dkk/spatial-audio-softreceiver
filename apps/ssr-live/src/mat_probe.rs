use std::path::Path;

#[cfg(not(windows))]
pub fn run(_endpoint: u16, _seconds: f64, _out: &Path, _truth: &Path) -> Result<(), String> {
    Err(ssr_i18n::live::windows_only(ssr_i18n::cli_language()).into())
}

#[cfg(windows)]
pub fn run(endpoint: u16, seconds: f64, out: &Path, truth_path: &Path) -> Result<(), String> {
    use decode::mat_probe::{self as probe, DeviceInfo, ProbeEvent, ProbeObject, Truth};
    use ssr_sys::{
        driver_ring::DriverRing,
        qpc,
        spatial::{Client, OpenError},
    };
    use ssr_types::{BitstreamCodec, CaptureSource, SourceFormat};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    let language = ssr_i18n::cli_language();
    if out == truth_path || !seconds.is_finite() || !(0.01..=3600.0).contains(&seconds) {
        return Err(ssr_i18n::live::mat_invalid(language).into());
    }
    if out.exists() || truth_path.exists() {
        return Err(ssr_i18n::live::mat_invalid(language).into());
    }
    let client = Client::open(endpoint).map_err(|e| match e {
        OpenError::SpatialUnavailable(detail) => {
            ssr_i18n::live::mat_enable_spatial(language, detail)
        }
        OpenError::Endpoint(detail) => detail,
    })?;
    let info = client.info.clone();
    let count = info.static_mask.count_ones() as usize + info.max_dynamic as usize;
    if count == 0 || count > probe::MAX_OBJECTS {
        return Err(ssr_i18n::live::mat_invalid(language).into());
    }
    let mut objects = Vec::with_capacity(count);
    for p in &info.positions {
        if info.static_mask & p.object_type != 0 {
            objects.push(ProbeObject {
                kind: format!("AudioObjectType_{}", p.kind),
                audio_object_type: p.object_type,
                seed: (objects.len() + 1) as u16,
                static_position: p.position,
            });
        }
    }
    for i in 0..info.max_dynamic {
        objects.push(ProbeObject {
            kind: format!("dynamic_{i}"),
            audio_object_type: 1,
            seed: (objects.len() + 1) as u16,
            static_position: None,
        });
    }
    let mut truth = Truth {
        version: 1,
        endpoint,
        sample_rate: probe::RATE,
        qpc_frequency: qpc::frequency(),
        start_qpc: 0,
        requested_seconds: seconds,
        device: DeviceInfo {
            endpoint_id: info.endpoint_id,
            max_dynamic_objects: info.max_dynamic,
            native_static_mask: info.static_mask,
            float32_48000_hresult: info.format_hresult,
            stream_available_hresult: info.available_hresult,
            static_positions: info
                .positions
                .iter()
                .map(|p| (format!("AudioObjectType_{}", p.kind), p.position, p.hresult))
                .collect(),
        },
        objects,
        events: Vec::new(),
        completed: false,
        error: None,
    };
    // Refuse an already existing truth file rather than truncate an observation.
    let truth_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(truth_path)
        .map_err(|e| e.to_string())?;
    let ring = DriverRing::open().map_err(|e| e.to_string())?;
    let frequency = ring.info().qpc_frequency;
    if frequency != truth.qpc_frequency {
        return Err(ssr_i18n::live::mat_invalid(language).into());
    }
    let tee = supervisor::TeeCapture::new(ring, qpc::now);
    let (mut ring, recorder) = supervisor::Recorder::start(
        supervisor::RecorderConfig {
            path: out.to_owned(),
            now: qpc::now,
        },
        frequency,
        tee,
    )?;
    let stop = Arc::new(AtomicBool::new(false));
    let capture_stop = Arc::clone(&stop);
    let capture = std::thread::spawn(move || {
        let mut buffer = vec![0; 1 << 20];
        let mut deframer = bitstream::Deframer::new();
        let mut bursts = 0u64;
        let mut formats = std::collections::BTreeSet::new();
        let mut active = None;
        let result = (|| -> Result<(), String> {
            while !capture_stop.load(Ordering::Acquire) {
                let n = ring
                    .read(&mut buffer, Duration::from_millis(50))
                    .map_err(|e| e.to_string())?;
                for record in ssr_abi::records(&buffer[..n]) {
                    let record = record.map_err(|e| e.to_string())?;
                    if record.endpoint() != endpoint {
                        continue;
                    }
                    match record {
                        ssr_abi::Record::FormatChange { epoch, format, .. } => {
                            if active != Some((epoch, format)) {
                                deframer.reset();
                            }
                            active = Some((epoch, format));
                            if let SourceFormat::Bitstream(f) = format {
                                match f.codec {
                                    BitstreamCodec::Mat20 => {
                                        formats.insert("MAT 2.0");
                                    }
                                    BitstreamCodec::Mat21 => {
                                        formats.insert("MAT 2.1");
                                    }
                                    _ => {}
                                }
                            }
                        }
                        ssr_abi::Record::Data {
                            epoch,
                            bytes,
                            flags,
                            ..
                        } => {
                            if flags & ssr_abi::RECORD_FLAG_DISCONTINUITY != 0 {
                                deframer.reset();
                            }
                            if matches!(active,Some((e,SourceFormat::Bitstream(f))) if e==epoch && matches!(f.codec,BitstreamCodec::Mat20|BitstreamCodec::Mat21))
                            {
                                deframer.push(bytes, &mut |b| {
                                    if b.data_type == bitstream::DataType::Mat {
                                        bursts += 1;
                                    }
                                });
                            }
                        }
                        ssr_abi::Record::Overrun { .. } | ssr_abi::Record::StreamStop { .. } => {
                            deframer.reset()
                        }
                        _ => {}
                    }
                }
            }
            Ok(())
        })();
        drop(ring);
        let lost = recorder.dropped_bytes();
        let closed = recorder.stop();
        result.and(closed).map(|()| (bursts, formats, lost))
    });
    // All event/metadata storage is prepared before entering the rendering loop.
    // The API's minimum update can be shorter than 10 ms; provision 1 ms slots.
    let capacity = ((seconds * 1000.0).ceil() as usize + 100).min(1_000_000 / count);
    let mut events: Vec<_> = (0..capacity)
        .map(|_| ProbeEvent {
            qpc: 0,
            frames: 0,
            available_dynamic: 0,
            objects: vec![probe::ObjectState::default(); count],
        })
        .collect();
    let mut generators: Vec<_> = truth
        .objects
        .iter()
        .map(|o| probe::Lfsr::new(o.seed).expect("generated seed"))
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    let render = (|| -> Result<usize, String> {
        let mut stream = client.activate().map_err(|e| e.to_string())?;
        truth.start_qpc = qpc::now();
        stream.start().map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + Duration::from_secs_f64(seconds + 5.0);
        let mut sample = 0u64;
        let mut used = 0;
        let mut activated = false;
        while sample < (seconds * f64::from(probe::RATE)) as u64 {
            if std::time::Instant::now() >= deadline {
                return Err(ssr_i18n::live::mat_capture_failed(language).into());
            }
            let Some(event) = events.get_mut(used) else {
                return Err(ssr_i18n::live::mat_capacity(language).into());
            };
            let (available, frames) = stream.begin().map_err(|e| e.to_string())?;
            if frames == 0 {
                stream.end().map_err(|e| e.to_string())?;
                continue;
            }
            event.qpc = qpc::now();
            event.available_dynamic = available;
            if !activated {
                for object in &truth.objects {
                    stream
                        .activate_object(object.audio_object_type)
                        .map_err(|e| e.to_string())?;
                }
                activated = true;
            }
            for (i, object) in truth.objects.iter().enumerate() {
                let state = probe::state(i, count, sample, object);
                event.objects[i] = state;
                stream
                    .fill(
                        i,
                        (object.audio_object_type == 1).then_some(state.position),
                        state.volume,
                        |buffer| {
                            for (k, value) in buffer.iter_mut().enumerate() {
                                let v = generators[i].sample();
                                *value = if probe::audible(i, count, sample + k as u64) {
                                    v
                                } else {
                                    0.0
                                };
                            }
                        },
                    )
                    .map_err(|e| e.to_string())?;
            }
            stream.end().map_err(|e| e.to_string())?;
            event.frames = frames;
            sample += u64::from(frames);
            used += 1;
        }
        stream.stop().map_err(|e| e.to_string())?;
        Ok(used)
    })();
    // Keep the tail to cover render/encoder latency, then always join and flush.
    std::thread::sleep(Duration::from_millis(500));
    stop.store(true, Ordering::Release);
    let captured = capture
        .join()
        .map_err(|_| ssr_i18n::live::mat_capture_failed(language).to_owned())
        .and_then(|r| r);
    let used = events.iter().take_while(|e| e.frames != 0).count();
    events.truncate(used);
    truth.events = events;
    truth.completed = render.is_ok() && captured.is_ok();
    truth.error = render.as_ref().err().or(captured.as_ref().err()).cloned();
    serde_json::to_writer_pretty(truth_file, &truth).map_err(|e| e.to_string())?;
    render?;
    let (bursts, formats, lost) = captured?;
    println!(
        "{}",
        ssr_i18n::live::mat_finished(
            language,
            out.display(),
            truth_path.display(),
            bursts,
            formats.into_iter().collect::<Vec<_>>().join(", "),
            lost
        )
    );
    if seconds
        < f64::from(probe::RATE + count as u32 * probe::RATE / 2) / f64::from(probe::RATE) + 2.0
    {
        eprintln!(
            "{}",
            ssr_i18n::live::mat_short(language, 1.0 + count as f64 * 0.5 + 2.0)
        );
    }
    Ok(())
}
