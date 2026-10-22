//! Observational hypotheses only. No inferred layout is installed in a decoder.
use decode::mat_probe::{self as probe, Truth};
use serde::Serialize;
use ssr_types::{BitstreamCodec, SourceFormat};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Frame {
    pub qpc: u64,
    pub epoch: u32,
    pub content: Vec<u8>,
}
#[derive(Debug, Serialize)]
pub struct ByteRange {
    start: usize,
    end: usize,
    constant: bool,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Encoding {
    I16,
    I24,
    I32,
    F32,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Order {
    Le,
    Be,
    Swap16,
}
#[derive(Clone, Debug, Serialize)]
pub struct SampleCandidate {
    offset: usize,
    encoding: Encoding,
    order: Order,
    stride: usize,
    layout: &'static str,
    correlation: f64,
    lag_samples: i64,
    evidence_frames: usize,
    low_confidence: bool,
    aliases: Vec<(usize, Encoding, Order)>,
}
#[derive(Debug, Serialize)]
pub struct FieldCandidate {
    axis: &'static str,
    offset: usize,
    format: &'static str,
    correlation: f64,
    scale: f64,
    intercept: f64,
    sign: i8,
    low_confidence: bool,
}
#[derive(Debug, Serialize)]
pub struct ObjectResult {
    object: String,
    solo_observed: bool,
    samples: Option<SampleCandidate>,
    fields: Vec<FieldCandidate>,
}
#[derive(Debug, Serialize)]
pub struct Analysis {
    schema_version: u32,
    truehd: bool,
    frame_sizes: Vec<usize>,
    frames: usize,
    silence_frames: usize,
    byte_ranges: Vec<ByteRange>,
    objects: Vec<ObjectResult>,
    warnings: Vec<String>,
}

fn width(encoding: Encoding) -> usize {
    match encoding {
        Encoding::I16 => 2,
        Encoding::I24 => 3,
        _ => 4,
    }
}
fn value(bytes: &[u8], offset: usize, encoding: Encoding, order: Order) -> Option<f64> {
    let n = width(encoding);
    let mut b = [0u8; 4];
    for (i, dest) in b[..n].iter_mut().enumerate() {
        let index = match order {
            Order::Le => offset + i,
            Order::Be => offset + n - 1 - i,
            Order::Swap16 => (offset + i) ^ 1,
        };
        *dest = *bytes.get(index)?;
    }
    let v = match encoding {
        Encoding::I16 => i16::from_le_bytes([b[0], b[1]]) as f64 / 32768.0,
        Encoding::I24 => ((u32::from_le_bytes(b) << 8) as i32 >> 8) as f64 / 8388608.0,
        Encoding::I32 => i32::from_le_bytes(b) as f64 / 2147483648.0,
        Encoding::F32 => f32::from_le_bytes(b) as f64,
    };
    v.is_finite().then_some(v)
}
fn sample_time(truth: &Truth, qpc: u64) -> i64 {
    ((i128::from(qpc) - i128::from(truth.start_qpc)) * i128::from(probe::RATE)
        / i128::from(truth.qpc_frequency)) as i64
}
fn sequence(seed: u16) -> (Vec<f64>, Vec<usize>) {
    let mut l = probe::Lfsr::new(seed).expect("validated truth");
    let signal: Vec<_> = (0..probe::PERIOD).map(|_| f64::from(l.sample())).collect();
    let mut phases = vec![0; 1 << 15];
    for phase in 0..probe::PERIOD {
        let mut bits = 0;
        for k in 0..15 {
            bits |= usize::from(signal[(phase + k) % probe::PERIOD] > 0.0) << k;
        }
        phases[bits] = phase;
    }
    (signal, phases)
}
fn phase_at(
    frame: &Frame,
    offset: usize,
    step: usize,
    encoding: Encoding,
    order: Order,
    phases: &[usize],
) -> Option<usize> {
    let a = value(&frame.content, offset, encoding, order)?.abs();
    let b = value(&frame.content, offset + step, encoding, order)?.abs();
    if !(0.00001..=1.0).contains(&a) || (a - b).abs() > a * 0.001 {
        return None;
    }
    let mut bits = 0;
    for k in 0..15 {
        let v = value(&frame.content, offset + k * step, encoding, order)?;
        // The excitation is exactly +/-0.25. Accept gain changes, but not arbitrary metadata.
        if (v.abs() - a).abs() > a * 0.001 {
            return None;
        }
        bits |= usize::from(v > 0.0) << k;
    }
    (bits != 0).then_some(phases[bits])
}
fn correlation(frame: &Frame, c: &SampleCandidate, phase: usize, signal: &[f64]) -> Option<f64> {
    let step = c.stride * width(c.encoding);
    let mut pairs = Vec::with_capacity(128);
    for k in 0..128 {
        pairs.push((
            value(&frame.content, c.offset + k * step, c.encoding, c.order)?,
            signal[(phase + k) % probe::PERIOD],
        ));
    }
    regression(&pairs).map(|v| v.0)
}
fn regression(pairs: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    if pairs.len() < 3 {
        return None;
    }
    let n = pairs.len() as f64;
    let mx = pairs.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pairs.iter().map(|p| p.1).sum::<f64>() / n;
    let (mut xx, mut yy, mut xy) = (0.0, 0.0, 0.0);
    for &(x, y) in pairs {
        xx += (x - mx).powi(2);
        yy += (y - my).powi(2);
        xy += (x - mx) * (y - my);
    }
    if xx < 1e-20 || yy < 1e-20 {
        return None;
    }
    let r = xy / (xx * yy).sqrt();
    let scale = xy / xx;
    (r.is_finite() && scale.is_finite()).then_some((r.clamp(-1.0, 1.0), scale, my - scale * mx))
}
fn find_samples(frames: &[&Frame], truth: &Truth, index: usize) -> Option<SampleCandidate> {
    // Try the nominal midpoint first, then both edges of the +/-0.5 s lag window.
    // A silent frame must not prevent finding a delayed isolated interval.
    for fraction in [0.5, 0.2, 0.8, 0.0, 0.99] {
        if let Some(frame) = frames.get((frames.len() as f64 * fraction) as usize)
            && let Some(candidate) = search_frame(frame, frames, truth, index)
        {
            return Some(candidate);
        }
    }
    None
}
fn search_frame(
    frame: &Frame,
    frames: &[&Frame],
    truth: &Truth,
    index: usize,
) -> Option<SampleCandidate> {
    let (signal, phases) = sequence(truth.objects[index].seed);
    let start = probe::solo_start(index) as i64;
    let end = probe::solo_start(index + 1) as i64;
    let mut best: Option<SampleCandidate> = None;
    // All byte starts and strides 1..64. Stride 1 includes planar storage.
    // A 15-bit MLS signature maps straight to phase: no quadratic lag sweep.
    for encoding in [Encoding::I16, Encoding::I24, Encoding::I32, Encoding::F32] {
        let w = width(encoding);
        for order in [Order::Le, Order::Be, Order::Swap16] {
            for stride in 1..=64 {
                let step = stride * w;
                for offset in 0..frame.content.len().saturating_sub(127 * step + w - 1) {
                    let Some(phase) = phase_at(frame, offset, step, encoding, order, &phases)
                    else {
                        continue;
                    };
                    let expected = sample_time(truth, frame.qpc);
                    // Unwrap against the isolated interval, not the nearest phase:
                    // half a second exceeds half the MLS period.
                    let nearest = phase as i64
                        + ((start - phase as i64) as f64 / probe::PERIOD as f64).ceil() as i64
                            * probe::PERIOD as i64;
                    if nearest < start || nearest + 128 >= end || (expected - nearest).abs() > 24000
                    {
                        continue;
                    }
                    let mut candidate = SampleCandidate {
                        offset,
                        encoding,
                        order,
                        stride,
                        layout: if stride == 1 {
                            "planar_or_mono"
                        } else {
                            "interleaved"
                        },
                        correlation: 0.0,
                        lag_samples: expected - nearest,
                        evidence_frames: 0,
                        low_confidence: true,
                        aliases: Vec::new(),
                    };
                    let Some(r) = correlation(frame, &candidate, phase, &signal) else {
                        continue;
                    };
                    if r < 0.97 {
                        continue;
                    }
                    let mut sum = 0.0;
                    for other in frames.iter().step_by((frames.len() / 15).max(1)).take(15) {
                        let Some(p) = phase_at(other, offset, step, encoding, order, &phases)
                        else {
                            continue;
                        };
                        let expected = sample_time(truth, other.qpc) - candidate.lag_samples;
                        let actual = p as i64
                            + ((expected - p as i64) as f64 / probe::PERIOD as f64).round() as i64
                                * probe::PERIOD as i64;
                        if (actual - expected).abs() > 960 || actual < start || actual + 128 >= end
                        {
                            continue;
                        }
                        if let Some(r) = correlation(other, &candidate, p, &signal)
                            && r >= 0.97
                        {
                            candidate.evidence_frames += 1;
                            sum += r;
                        }
                    }
                    if candidate.evidence_frames == 0 {
                        continue;
                    }
                    candidate.correlation = sum / candidate.evidence_frames as f64;
                    candidate.low_confidence = candidate.evidence_frames < 3;
                    if best.as_ref().is_none_or(|b| {
                        candidate.evidence_frames > b.evidence_frames
                            || (candidate.evidence_frames == b.evidence_frames
                                && candidate.correlation > b.correlation + 1e-8)
                    }) {
                        best = Some(candidate);
                    } else if let Some(b) = best.as_mut()
                        && candidate.evidence_frames == b.evidence_frames
                        && (candidate.correlation - b.correlation).abs() < 1e-8
                        && candidate.lag_samples == b.lag_samples
                        && b.aliases.len() < 64
                    {
                        b.aliases
                            .push((candidate.offset, candidate.encoding, candidate.order));
                        b.low_confidence = true;
                    }
                }
            }
        }
    }
    best
}
fn fields(
    frames: &[Frame],
    truth: &Truth,
    index: usize,
    constant: &[bool],
    lag: i64,
) -> Vec<FieldCandidate> {
    let mut result = Vec::new();
    let samples: Vec<_> = frames
        .iter()
        .filter_map(|frame| {
            let qpc = i128::from(frame.qpc)
                - i128::from(lag) * i128::from(truth.qpc_frequency) / i128::from(probe::RATE);
            let event_index = truth
                .events
                .partition_point(|e| i128::from(e.qpc) <= qpc)
                .checked_sub(1)?;
            let event = &truth.events[event_index];
            if qpc - i128::from(event.qpc) > i128::from(truth.qpc_frequency) / 20 {
                return None;
            }
            Some((frame, &event.objects[index]))
        })
        .collect();
    for (axis, component) in [("x", 0), ("y", 1), ("z", 2), ("volume", 3)] {
        let mut best: Option<FieldCandidate> = None;
        for offset in 0..constant.len() {
            for (format, n) in [
                ("u8", 1),
                ("i8", 1),
                ("u16_le", 2),
                ("u16_be", 2),
                ("i16_le", 2),
                ("i16_be", 2),
                ("f32_le", 4),
                ("f32_be", 4),
            ] {
                if offset + n > constant.len() || !constant[offset..offset + n].iter().all(|v| *v) {
                    continue;
                }
                let pairs: Vec<_> = samples
                    .iter()
                    .filter_map(|(frame, state)| {
                        let b = frame.content.get(offset..offset + n)?;
                        let v = match format {
                            "u8" => b[0] as f64,
                            "i8" => (b[0] as i8) as f64,
                            "u16_le" => u16::from_le_bytes([b[0], b[1]]) as f64,
                            "u16_be" => u16::from_be_bytes([b[0], b[1]]) as f64,
                            "i16_le" => i16::from_le_bytes([b[0], b[1]]) as f64,
                            "i16_be" => i16::from_be_bytes([b[0], b[1]]) as f64,
                            "f32_le" => f32::from_le_bytes(b.try_into().ok()?) as f64,
                            _ => f32::from_be_bytes(b.try_into().ok()?) as f64,
                        };
                        let expected = if component == 3 {
                            state.volume
                        } else {
                            state.position[component]
                        };
                        v.is_finite().then_some((v, f64::from(expected)))
                    })
                    .collect();
                if let Some((r, scale, intercept)) = regression(&pairs)
                    && r.abs() > 0.8
                    && best
                        .as_ref()
                        .is_none_or(|b| r.abs() > b.correlation.abs() + 1e-8)
                {
                    best = Some(FieldCandidate {
                        axis,
                        offset,
                        format,
                        correlation: r,
                        scale,
                        intercept,
                        sign: if scale < 0.0 { -1 } else { 1 },
                        low_confidence: r.abs() < 0.98 || pairs.len() < 20,
                    });
                }
            }
        }
        if let Some(best) = best {
            result.push(best);
        }
    }
    result
}

pub fn analyze(frames: &[Frame], truth: &Truth, truehd: bool) -> Analysis {
    let mut sizes: Vec<_> = frames.iter().map(|f| f.content.len()).collect();
    sizes.sort_unstable();
    sizes.dedup();
    let mut result = Analysis {
        schema_version: 1,
        truehd,
        frame_sizes: sizes,
        frames: frames.len(),
        silence_frames: 0,
        byte_ranges: Vec::new(),
        objects: Vec::new(),
        warnings: Vec::new(),
    };
    if truehd {
        return result;
    }
    result.warnings.push("Hypotheses only; MLS seeds are cyclic shifts, so object identity requires isolated intervals. Offset/width aliases may be indistinguishable at this amplitude.".into());
    if !truth.completed {
        result.warnings.push("Probe did not complete.".into());
    }
    if frames.is_empty() {
        result.warnings.push("No valid MAT frames.".into());
        return result;
    }
    if result.frame_sizes.len() != 1 || frames.iter().any(|f| f.epoch != frames[0].epoch) {
        result.warnings.push(
            "Multiple sizes or epochs; analyze a single uninterrupted epoch separately.".into(),
        );
        return result;
    }
    let silent: Vec<_> = frames
        .iter()
        .filter(|f| (4800..38400).contains(&sample_time(truth, f.qpc)))
        .collect();
    result.silence_frames = silent.len();
    let mut constant = vec![false; frames[0].content.len()];
    if silent.len() >= 2 {
        for (i, c) in constant.iter_mut().enumerate() {
            *c = silent.iter().all(|f| f.content[i] == silent[0].content[i]);
        }
        let mut start = 0;
        while start < constant.len() {
            let mut end = start + 1;
            while end < constant.len() && constant[end] == constant[start] {
                end += 1;
            }
            result.byte_ranges.push(ByteRange {
                start,
                end,
                constant: constant[start],
            });
            start = end;
        }
    } else {
        result
            .warnings
            .push("Too few silent frames for byte stability.".into());
    }
    for (index, object) in truth.objects.iter().enumerate() {
        let solo: Vec<_> = frames
            .iter()
            .filter(|f| {
                let time = sample_time(truth, f.qpc);
                time >= probe::solo_start(index) as i64 - 24000
                    && time < probe::solo_start(index + 1) as i64 + 24000
            })
            .collect();
        let samples = find_samples(&solo, truth, index);
        let fields = fields(
            frames,
            truth,
            index,
            &constant,
            samples.as_ref().map_or(0, |s| s.lag_samples),
        );
        result.objects.push(ObjectResult {
            object: object.kind.clone(),
            solo_observed: truth.events.iter().any(|e| {
                e.objects[index].first_sample >= probe::solo_start(index)
                    && e.objects[index].first_sample < probe::solo_start(index + 1)
            }),
            samples,
            fields,
        });
    }
    result
}

pub fn run(dump: &Path, truth_path: &Path, json: Option<&Path>) -> Result<(), String> {
    let language = ssr_i18n::cli_language();
    let truth: Truth =
        serde_json::from_slice(&std::fs::read(truth_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if !truth.validate() {
        return Err(ssr_i18n::replay::mat_invalid_truth(language).into());
    }
    let bytes = std::fs::read(dump).map_err(|e| e.to_string())?;
    let (header, stream) = ssr_abi::split_dump(&bytes).map_err(|e| e.to_string())?;
    if header.qpc_frequency != truth.qpc_frequency {
        return Err(ssr_i18n::replay::mat_invalid_truth(language).into());
    }
    let mut frames = Vec::new();
    let mut truehd = false;
    let mut errors = Vec::new();
    let mut deframer = bitstream::Deframer::new();
    let mut format = None;
    let mut content = [0; decode::mat_frame::MAT_CONTENT_BYTES];
    let mut origin = None;
    let mut expected_pos = None;
    let mut segment = 0u32;
    // Recorder gaps have no typed Record variant; never stitch across a lost READ.
    let recorder_gap = ssr_abi::raw_records(stream)
        .any(|record| record.is_ok_and(|r| r.header.kind == ssr_abi::RECORD_KIND_RECORDER_GAP));
    for record in ssr_abi::records(stream) {
        let record = record.map_err(|e| e.to_string())?;
        if record.endpoint() != truth.endpoint {
            continue;
        }
        match record {
            ssr_abi::Record::FormatChange {
                epoch, format: new, ..
            } => {
                if format != Some((epoch, new)) {
                    deframer.reset();
                    origin = None;
                    expected_pos = None;
                    segment += 1;
                }
                format = Some((epoch, new));
            }
            ssr_abi::Record::Data {
                epoch,
                qpc,
                pos,
                bytes,
                flags,
                ..
            } => {
                let Some((current, SourceFormat::Bitstream(f))) = format else {
                    continue;
                };
                if epoch != current
                    || !matches!(f.codec, BitstreamCodec::Mat20 | BitstreamCodec::Mat21)
                {
                    continue;
                }
                if flags & ssr_abi::RECORD_FLAG_DISCONTINUITY != 0
                    || expected_pos.is_some_and(|p| p != pos)
                {
                    deframer.reset();
                    origin = None;
                    segment += 1;
                    errors.push("Discontinuity".into());
                }
                expected_pos = pos.checked_add(bytes.len() as u64);
                let byte_rate = SourceFormat::Bitstream(f).bytes_per_second() as u64;
                if byte_rate == 0 {
                    continue;
                }
                let base = *origin.get_or_insert(qpc);
                deframer.push(bytes, &mut |burst| {
                    if burst.data_type != bitstream::DataType::Mat {
                        return;
                    }
                    match decode::mat_frame::extract(&burst, &mut content) {
                        Ok(payload) => {
                            truehd |= matches!(payload, decode::mat_frame::MatPayload::TrueHd(_));
                            frames.push(Frame {
                                qpc: base.saturating_add(
                                    (u128::from(burst.start) * u128::from(header.qpc_frequency)
                                        / u128::from(byte_rate))
                                        as u64,
                                ),
                                epoch: segment,
                                content: payload.bytes().to_vec(),
                            });
                        }
                        Err(e) => errors.push(e.to_string()),
                    }
                });
            }
            ssr_abi::Record::Overrun { .. } | ssr_abi::Record::StreamStop { .. } => {
                deframer.reset();
                origin = None;
                expected_pos = None;
                segment += 1;
                errors.push("Stream interrupted".into());
            }
            _ => {}
        }
    }
    let mut result = analyze(if recorder_gap { &[] } else { &frames }, &truth, truehd);
    if recorder_gap {
        result
            .warnings
            .push("Recorder gap: refusing layout inference across lost READs.".into());
    }
    result.warnings.extend(errors);
    if let Some(path) = json {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    if result.truehd {
        println!("{}", ssr_i18n::replay::mat_truehd(language));
        return Ok(());
    }
    println!(
        "{}",
        ssr_i18n::replay::mat_summary(
            language,
            result.frames,
            format_args!("{:?}", result.frame_sizes),
            result.silence_frames
        )
    );
    println!("{}", ssr_i18n::replay::mat_ranges_heading(language));
    for range in &result.byte_ranges {
        println!(
            "{}..{}\t{}",
            range.start,
            range.end,
            if range.constant {
                ssr_i18n::replay::mat_constant(language)
            } else {
                ssr_i18n::replay::mat_varying(language)
            }
        );
    }
    println!("{}", ssr_i18n::replay::mat_samples_heading(language));
    for object in &result.objects {
        if let Some(s) = &object.samples {
            println!(
                "{}\t{}\t{:?}/{:?}\t{}\t{:.5}\t{}\t{}",
                object.object,
                s.offset,
                s.encoding,
                s.order,
                s.stride,
                s.correlation,
                s.lag_samples,
                if s.low_confidence {
                    ssr_i18n::replay::mat_low(language)
                } else {
                    ssr_i18n::replay::mat_candidate(language)
                }
            );
        } else {
            println!(
                "{}\t{}",
                object.object,
                ssr_i18n::replay::mat_missing(language)
            );
        }
        for f in &object.fields {
            println!(
                "{}\t{}\t{}\t{}\tr={:.5}\tscale={}\tbias={}\t{}",
                object.object,
                f.axis,
                f.offset,
                f.format,
                f.correlation,
                f.scale,
                f.intercept,
                if f.low_confidence {
                    ssr_i18n::replay::mat_low(language)
                } else {
                    ssr_i18n::replay::mat_candidate(language)
                }
            );
        }
    }
    println!("{}", ssr_i18n::replay::mat_caution(language));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use probe::{DeviceInfo, ProbeEvent, ProbeObject};
    // Invented containers only. Neither layout describes or derives from real MAT.
    fn synthetic(
        encoding: Encoding,
        order: Order,
        stride: usize,
        offset: usize,
    ) -> (Vec<Frame>, Truth) {
        let objects: Vec<_> = (0..2)
            .map(|i| ProbeObject {
                kind: format!("dynamic_{i}"),
                audio_object_type: 1,
                seed: 1 + i,
                static_position: None,
            })
            .collect();
        let signals: Vec<_> = objects.iter().map(|o| sequence(o.seed).0).collect();
        let mut truth = Truth {
            version: 1,
            endpoint: 0,
            sample_rate: probe::RATE,
            qpc_frequency: 48000,
            start_qpc: 100000,
            requested_seconds: 5.0,
            device: DeviceInfo::default(),
            objects,
            events: Vec::new(),
            completed: true,
            error: None,
        };
        let mut frames = Vec::new();
        for tick in 0..250u64 {
            let sample = tick * 960;
            let states: Vec<_> = truth
                .objects
                .iter()
                .enumerate()
                .map(|(i, o)| probe::state(i, 2, sample, o))
                .collect();
            let mut content = vec![0; offset + 2 * 960 * width(encoding) + 16];
            for (i, state) in states.iter().enumerate() {
                for axis in 0..3 {
                    let v = (state.position[axis] * 10000.0).round() as i16;
                    let p = 8 + i * 8 + axis * 2;
                    content[p..p + 2].copy_from_slice(&v.to_le_bytes());
                }
                content[8 + i * 8 + 6] = (state.volume * 100.0) as u8;
                for k in 0..960 {
                    let v = if probe::audible(i, 2, sample + k as u64) {
                        signals[i][(sample as usize + k) % probe::PERIOD]
                    } else {
                        0.0
                    };
                    let p = offset
                        + if stride == 1 {
                            (i * 960 + k) * width(encoding)
                        } else {
                            (k * stride + i) * width(encoding)
                        };
                    let raw = match encoding {
                        Encoding::I16 => (v * 32768.0).round() as i64,
                        Encoding::I24 => (v * 8388608.0).round() as i64,
                        Encoding::I32 => (v * 2147483648.0).round() as i64,
                        Encoding::F32 => (v as f32).to_bits() as i64,
                    };
                    let bytes = raw.to_le_bytes();
                    for (b, value) in bytes.iter().take(width(encoding)).enumerate() {
                        let target = match order {
                            Order::Le => p + b,
                            Order::Be => p + width(encoding) - 1 - b,
                            Order::Swap16 => (p + b) ^ 1,
                        };
                        content[target] = *value;
                    }
                }
            }
            truth.events.push(ProbeEvent {
                qpc: 100000 + sample,
                frames: 960,
                available_dynamic: 2,
                objects: states,
            });
            frames.push(Frame {
                qpc: 100000 + sample,
                epoch: 1,
                content,
            });
        }
        (frames, truth)
    }
    fn recover(encoding: Encoding, order: Order, stride: usize, offset: usize) {
        let (frames, truth) = synthetic(encoding, order, stride, offset);
        let result = analyze(&frames, &truth, false);
        for (i, object) in result.objects.iter().enumerate() {
            let s = object.samples.as_ref().expect("sample hypothesis");
            assert!(s.correlation > 0.99 && s.evidence_frames >= 3, "{s:?}");
            assert_eq!(s.stride, stride);
            // Fixed-amplitude samples can alias a narrower integer encoding.
            // The recovered byte address must be in the correct channel sample.
            let base = offset
                + if stride == 1 {
                    i * 960 * width(encoding)
                } else {
                    i * width(encoding)
                };
            assert!(
                (s.offset == base && s.encoding == encoding && s.order == order)
                    || s.aliases.contains(&(base, encoding, order)),
                "{s:?}, base={base}"
            );
            assert!(
                object
                    .fields
                    .iter()
                    .any(|f| f.axis == "x" && f.offset == 8 + i * 8 && f.correlation.abs() > 0.99)
            );
            assert!(
                object
                    .fields
                    .iter()
                    .any(|f| f.axis == "volume" && f.offset == 14 + i * 8)
            );
        }
    }
    #[test]
    fn synthetic_be24_planar_layout_and_fields() {
        recover(Encoding::I24, Order::Be, 1, 64);
    }
    #[test]
    fn synthetic_le16_interleaved_layout_and_fields() {
        recover(Encoding::I16, Order::Le, 2, 96);
    }
    #[test]
    fn synthetic_delay_over_half_mls_period_is_unwrapped() {
        let (mut frames, truth) = synthetic(Encoding::I16, Order::Le, 2, 96);
        for f in &mut frames {
            f.qpc += 20000;
        }
        let result = analyze(&frames, &truth, false);
        let candidate = result.objects[0].samples.as_ref().expect("delayed signal");
        assert_eq!(candidate.lag_samples, 20000);
        assert!(candidate.correlation > 0.99);
    }
    #[test]
    fn truehd_and_missing_intervals_are_not_layout_claims() {
        let (frames, mut truth) = synthetic(Encoding::I16, Order::Le, 2, 96);
        assert!(analyze(&frames, &truth, true).objects.is_empty());
        truth.events.truncate(20);
        let result = analyze(&frames[..20], &truth, false);
        assert!(
            result
                .objects
                .iter()
                .all(|o| !o.solo_observed && o.samples.is_none())
        );
    }
}
