//! TrueHD extraction/decode on the decode thread, allocation-free scene placement.
//! The pinned Apache-2.0 decoder returns signed 24-bit, sample-major PCM.
//! Raw payloads and unwrapped MAT content use the same streaming front.

use ssr_types::{MAX_OBJECTS, ObjectRole, SceneBlock, SceneLayoutTable, SceneSource, Speaker};
use truehd::process::decode::{DecodedAccessUnit, Decoder};
use truehd::process::extract::Extractor;
use truehd::process::parse::Parser;
use truehd::structs::access_unit::AccessUnit;
use truehd::structs::channel::ChannelLabel;
use truehd::structs::oamd::{GAIN_MINUS_INFINITY, ObjectAudioMetadataPayload};
use truehd::utils::errors::ExtractError;

use crate::timeline::{EMPTY_UPDATE, Event, Room, Timeline};
use crate::{DecodeConfig, DecodeError, SceneBuffers};

const CHANNELS: usize = 16;
const PCM_SCALE: f32 = 1.0 / 8_388_608.0;

/// Allocating extraction, parsing and PCM decoding. Never call on a realtime thread.
pub struct TrueHdFront {
    extractor: Extractor,
    parser: Parser,
    decoder: Decoder,
    access_unit: AccessUnit,
    pcm: DecodedAccessUnit,
    input: Vec<u8>,
    synchronized: bool,
}

impl std::fmt::Debug for TrueHdFront {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrueHdFront").finish_non_exhaustive()
    }
}

impl Default for TrueHdFront {
    fn default() -> Self {
        Self::new()
    }
}

impl TrueHdFront {
    pub fn new() -> Self {
        let mut parser = Parser::default();
        // Presentation 3 resolves to the highest available independent presentation.
        parser.set_required_presentations(&[false, false, false, true]);
        Self {
            extractor: Extractor::default(),
            parser,
            decoder: Decoder::default(),
            access_unit: AccessUnit::default(),
            pcm: DecodedAccessUnit::default(),
            input: Vec::new(),
            synchronized: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Appends arbitrary contiguous bytes; incomplete frames stay in the front.
    /// Frames preceding an error are retained in the delivery. On error all
    /// upstream state resets together; the next call searches for a major sync.
    pub fn decode(&mut self, payload: &[u8], into: &mut TrueHdDecoded) -> Result<(), DecodeError> {
        into.clear();
        let result = self.decode_frames(payload, into);
        if let Err(error) = result {
            self.reset();
            into.error = Some(error);
        }
        result
    }

    fn decode_frames(
        &mut self,
        payload: &[u8],
        into: &mut TrueHdDecoded,
    ) -> Result<(), DecodeError> {
        self.input.extend_from_slice(payload);
        let mut cursor = 0;
        loop {
            let bytes = &self.input[cursor..];
            if !self.synchronized {
                // Keep the AU header and any partial major-sync word across calls.
                let sync = bytes.get(4..).and_then(|b| {
                    b.windows(4).position(|w| {
                        w == [0xf8, 0x72, 0x6f, 0xba] || w == [0xf8, 0x72, 0x6f, 0xbb]
                    })
                });
                let Some(offset) = sync else {
                    cursor += bytes.len().saturating_sub(7);
                    break;
                };
                cursor += offset;
                self.synchronized = true;
            }
            let bytes = &self.input[cursor..];
            if bytes.len() < 2 {
                break;
            }
            // MAT inserts zero words BETWEEN AUs. Never inspect or remove bytes
            // inside a length-delimited AU, even when its PCM residual is zero.
            if bytes[..2] == [0, 0] {
                cursor += 2;
                continue;
            }
            let length = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]) & 0x0fff) * 2;
            if length < 8 {
                return Err(corrupt());
            }
            if bytes.len() < length {
                break;
            }
            let errors = self.extractor.error_count();
            self.extractor.push_bytes(&bytes[..length]);
            cursor += length;
            let frame = match self.extractor.next() {
                Some(Ok(frame)) if self.extractor.error_count() == errors => frame,
                Some(Err(ExtractError::InsufficientData)) | None => return Err(corrupt()),
                _ => return Err(corrupt()),
            };
            self.parser
                .parse_into(&frame, &mut self.access_unit)
                .map_err(|_| corrupt())?;
            // Long-lived playback must not accumulate a history of branch points.
            drop(self.parser.take_branches());
            self.decoder
                .decode_presentation_into(&self.access_unit, 3, &mut self.pcm)
                .map_err(|_| corrupt())?;
            // A duplicate output timing/checksum is not additional source audio.
            if self.pcm.is_duplicate {
                continue;
            }
            let shape = Shape::from_pcm(&self.pcm)?;
            let mut events = Vec::new();
            for payload in &self.pcm.oamd {
                translate_oamd(payload, &shape, &mut events)?;
            }
            into.groups.push(DecodedGroup {
                pcm: self.pcm.clone(),
                shape,
                events,
            });
        }
        self.input.drain(..cursor);
        Ok(())
    }
}

/// Decode-thread-owned delivery. Capture only borrows it; clear/drop on decode.
#[derive(Debug, Default)]
pub struct TrueHdDecoded {
    groups: Vec<DecodedGroup>,
    error: Option<DecodeError>,
}

impl TrueHdDecoded {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn clear(&mut self) {
        self.groups.clear();
        self.error = None;
    }
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.error.is_none()
    }
    pub fn error(&self) -> Option<DecodeError> {
        self.error
    }
    pub fn frames(&self) -> usize {
        self.groups.iter().map(|g| g.pcm.sample_length).sum()
    }

    /// Sum same-rate runs before rounding, so 44.1/88.2 kHz AUs do not each
    /// acquire a fractional-frame rounding error. Rate changes start a new run.
    pub fn frames_at_mix_rate(&self) -> u64 {
        let mut total = 0;
        let mut rate = 0;
        let mut samples = 0;
        for group in &self.groups {
            if rate != group.pcm.sampling_frequency {
                if rate != 0 {
                    total += (samples * 48_000_u64).div_ceil(u64::from(rate));
                }
                rate = group.pcm.sampling_frequency;
                samples = 0;
            }
            samples += group.pcm.sample_length as u64;
        }
        if rate != 0 {
            total += (samples * 48_000_u64).div_ceil(u64::from(rate));
        }
        total
    }

    /// Exact integer PCM and original metadata for lossless fixture comparisons.
    /// Only the first `sample_length` rows and `channel_count` columns are valid.
    pub fn access_units(&self) -> impl ExactSizeIterator<Item = &DecodedAccessUnit> {
        self.groups.iter().map(|g| &g.pcm)
    }
}

#[derive(Debug)]
struct DecodedGroup {
    pcm: DecodedAccessUnit,
    shape: Shape,
    events: Vec<Event>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shape {
    channels: usize,
    beds: usize,
    speakers: [Option<Speaker>; CHANNELS],
    output: [Option<usize>; CHANNELS],
    objects: usize,
    has_lfe: bool,
    dynamic: bool,
}

impl Shape {
    fn from_pcm(pcm: &DecodedAccessUnit) -> Result<Self, DecodeError> {
        if pcm.channel_count == 0
            || pcm.channel_count > CHANNELS
            || pcm.sample_length == 0
            || pcm.sample_length > pcm.pcm_data.len()
            || pcm.channel_labels.len() > pcm.channel_count
            || !matches!(
                pcm.sampling_frequency,
                44_100 | 48_000 | 88_200 | 96_000 | 176_400 | 192_000
            )
        {
            return Err(corrupt());
        }
        let mut shape = Self {
            channels: pcm.channel_count,
            beds: pcm.channel_labels.len(),
            speakers: [None; CHANNELS],
            output: [None; CHANNELS],
            objects: 0,
            has_lfe: false,
            dynamic: pcm.channel_labels.len() < pcm.channel_count,
        };
        for channel in 0..pcm.channel_count {
            if let Some(&label) = pcm.channel_labels.get(channel) {
                let speaker = channel_speaker(label)?;
                shape.speakers[channel] = Some(speaker);
                if speaker.position().is_none() {
                    shape.has_lfe = true;
                    continue;
                }
            }
            shape.output[channel] = Some(shape.objects);
            shape.objects += 1;
        }
        Ok(shape)
    }

    fn layout(self) -> SceneLayoutTable {
        let mut layout = SceneLayoutTable::EMPTY;
        layout.source = if self.dynamic {
            SceneSource::TrueHdObjects
        } else {
            SceneSource::TrueHd
        };
        layout.lfe = self.has_lfe;
        layout.objects = self.objects as u8;
        for channel in 0..self.channels {
            if let Some(output) = self.output[channel] {
                layout.roles[output] =
                    self.speakers[channel].map_or(ObjectRole::Dynamic, ObjectRole::Bed);
            }
        }
        layout
    }
}

/// Capture-side buffers and bounded timeline. `place` and `reset` do not allocate.
#[derive(Debug)]
pub struct TrueHdBack {
    buffers: SceneBuffers,
    timeline: Timeline,
    epoch: Option<u32>,
    shape: Option<(Shape, u32)>,
    layout: SceneLayoutTable,
}

impl TrueHdBack {
    pub fn new(config: DecodeConfig) -> Self {
        Self {
            buffers: SceneBuffers::new(config.max_frames),
            timeline: Timeline::new("TrueHD"),
            epoch: None,
            shape: None,
            layout: SceneLayoutTable::EMPTY,
        }
    }

    pub fn reset(&mut self) {
        self.timeline.reset();
        self.buffers.set_layout(0, false);
        self.epoch = None;
        self.shape = None;
        self.layout = SceneLayoutTable::EMPTY;
    }

    pub fn layout(&self) -> &SceneLayoutTable {
        &self.layout
    }

    /// Empty deliveries are normal while extraction waits for more bytes.
    /// Groups before an upstream error are placed before that error is returned.
    pub fn place(
        &mut self,
        decoded: &TrueHdDecoded,
        qpc: u64,
        epoch: u32,
        out: &mut dyn FnMut(&SceneBlock<'_>),
    ) -> Result<(), DecodeError> {
        if self.epoch != Some(epoch) {
            self.reset();
            self.epoch = Some(epoch);
        }
        let result = self.write(decoded, qpc, epoch, out);
        if result.is_err() {
            self.reset();
        }
        result
    }

    fn write(
        &mut self,
        decoded: &TrueHdDecoded,
        qpc: u64,
        epoch: u32,
        out: &mut dyn FnMut(&SceneBlock<'_>),
    ) -> Result<(), DecodeError> {
        if self.buffers.max_frames() == 0 {
            return Err(unsupported("max_frames is zero"));
        }
        for group in &decoded.groups {
            let pcm = &group.pcm;
            let shape = group.shape;
            if self.shape != Some((shape, pcm.sampling_frequency)) {
                self.timeline.reset();
                self.shape = Some((shape, pcm.sampling_frequency));
            }
            self.buffers.set_layout(shape.objects, shape.has_lfe);
            self.layout = shape.layout();
            let mut bed_event = Event {
                sample: 0,
                ramp: 0,
                reset: false,
                objects: shape.objects,
                updates: [EMPTY_UPDATE; MAX_OBJECTS],
            };
            for channel in 0..shape.beds {
                if let Some(output) = shape.output[channel] {
                    bed_event.updates[output].speaker =
                        shape.speakers[channel].and_then(Speaker::position);
                }
            }
            self.timeline.insert(bed_event)?;
            for &event in &group.events {
                self.timeline.insert(event)?;
            }
            for i in 0..shape.objects {
                if self.layout.roles[i] == ObjectRole::Dynamic {
                    self.layout.room[i] = self.timeline.latest_room(i);
                }
            }
            let mut start = 0;
            while start < pcm.sample_length {
                let frames = (pcm.sample_length - start).min(self.buffers.max_frames());
                if shape.has_lfe {
                    self.buffers.lfe_mut(frames).fill(0.0);
                }
                for channel in 0..shape.channels {
                    if let Some(output) = shape.output[channel] {
                        for (target, row) in self
                            .buffers
                            .signal_mut(output, frames)
                            .iter_mut()
                            .zip(&pcm.pcm_data[start..start + frames])
                        {
                            *target = row[channel] as f32 * PCM_SCALE;
                        }
                    } else {
                        // SceneBlock has one LFE bus; preserve both LFE signals by summing.
                        for (target, row) in self
                            .buffers
                            .lfe_mut(frames)
                            .iter_mut()
                            .zip(&pcm.pcm_data[start..start + frames])
                        {
                            *target += row[channel] as f32 * PCM_SCALE;
                        }
                    }
                }
                self.timeline.emit(
                    &mut self.buffers,
                    frames,
                    epoch,
                    pcm.sampling_frequency,
                    qpc,
                    out,
                );
                start += frames;
            }
        }
        if let Some(error) = decoded.error {
            return Err(error);
        }
        Ok(())
    }
}

/// Same front and back, run synchronously for offline callers.
#[derive(Debug)]
pub struct TrueHdDecoder {
    front: TrueHdFront,
    back: TrueHdBack,
    decoded: TrueHdDecoded,
}

impl TrueHdDecoder {
    pub fn new(config: DecodeConfig) -> Self {
        Self {
            front: TrueHdFront::new(),
            back: TrueHdBack::new(config),
            decoded: TrueHdDecoded::new(),
        }
    }

    pub fn push(
        &mut self,
        payload: &[u8],
        qpc: u64,
        epoch: u32,
        out: &mut dyn FnMut(&SceneBlock<'_>),
    ) -> Result<(), DecodeError> {
        if self.back.epoch != Some(epoch) {
            self.reset();
        }
        let _upstream = self.front.decode(payload, &mut self.decoded);
        let result = self.back.place(&self.decoded, qpc, epoch, out);
        if result.is_err() {
            self.reset();
        }
        result
    }

    pub fn layout(&self) -> &SceneLayoutTable {
        self.back.layout()
    }
    pub fn reset(&mut self) {
        self.front.reset();
        self.back.reset();
        self.decoded.clear();
    }
}

fn translate_oamd(
    payload: &ObjectAudioMetadataPayload,
    shape: &Shape,
    events: &mut Vec<Event>,
) -> Result<(), DecodeError> {
    let assignment = &payload.program_assignment;
    if payload.b_alternate_object_data_present || assignment.num_isf_objects != 0 {
        return Err(unsupported("ISF or alternate OAMD object mapping"));
    }
    if payload.object_count != shape.channels
        || assignment.num_bed_objects != shape.beds
        || assignment.num_bed_objects + assignment.num_dynamic_objects != payload.object_count
    {
        return Err(corrupt());
    }
    // Bed assignments include LFE, unlike DD+ JOC's reconstructed channel array.
    let mut channel = 0;
    for bed in &assignment.bed_assignment {
        for (label, &present) in bed.0.iter().enumerate() {
            if !present {
                continue;
            }
            let expected = oamd_bed_speaker(label).ok_or_else(corrupt)?;
            if channel >= shape.beds || shape.speakers[channel] != Some(expected) {
                return Err(unsupported(
                    "OAMD bed assignment differs from decoded channel order",
                ));
            }
            channel += 1;
        }
    }
    if channel != shape.beds {
        return Err(corrupt());
    }
    let Some(element) = &payload.object_element else {
        return Ok(());
    };
    let timing = &element.md_update_info;
    if timing.sample_offset > 31
        || !(1..=8).contains(&timing.num_obj_info_blocks)
        || timing.block_update_info.len() != timing.num_obj_info_blocks
        || element.object_data.len() != payload.object_count
        || element
            .object_data
            .iter()
            .any(|blocks| blocks.len() != timing.num_obj_info_blocks)
    {
        return Err(corrupt());
    }
    // Limit far-future metadata rather than permitting overflow or unbounded queues.
    if payload.evo_sample_offset > 65_535 {
        return Err(unsupported(
            "OAMD sample offset exceeds the bounded timeline",
        ));
    }
    let mut previous = None;
    for (block_index, timing_block) in timing.block_update_info.iter().enumerate() {
        if timing_block.block_offset_factor_bits > 63 || timing_block.ramp_duration > 2048 {
            return Err(corrupt());
        }
        let sample = payload.evo_sample_offset
            + timing.sample_offset as u64
            + 32 * u64::from(timing_block.block_offset_factor_bits);
        if previous.is_some_and(|old| sample <= old) {
            return Err(corrupt());
        }
        previous = Some(sample);
        let mut event = Event {
            sample,
            ramp: u64::from(timing_block.ramp_duration),
            reset: false,
            objects: shape.objects,
            updates: [EMPTY_UPDATE; MAX_OBJECTS],
        };
        for channel in 0..shape.channels {
            let Some(output) = shape.output[channel] else {
                continue;
            };
            let block = &element.object_data[channel][block_index];
            let update = &mut event.updates[output];
            let db = block.object_basic_info.object_gain;
            update.gain = Some(if block.b_object_not_active || db == GAIN_MINUS_INFINITY {
                0.0
            } else {
                10.0_f32.powf(f32::from(db) / 20.0)
            });
            if channel < shape.beds || block.b_object_not_active {
                continue;
            }
            let render = &block.object_render_info;
            let mut position = render.pos3d;
            if let Some(extended) = &payload.extended_object_element
                && extended.b_ext_prec_pos_block
            {
                let precision = extended
                    .ext_prec_pos_block
                    .get(channel)
                    .and_then(|blocks| blocks.get(block_index))
                    .ok_or_else(corrupt)?;
                position[0] += precision.ext_prec_pos3d_x;
                position[1] += precision.ext_prec_pos3d_y;
                position[2] += precision.ext_prec_pos3d_z;
            }
            if !position
                .into_iter()
                .chain(render.object_size)
                .all(f64::is_finite)
            {
                return Err(corrupt());
            }
            // Upstream has already resolved differential coordinates. Never add twice.
            update.room = Some(Room {
                x: position[0].clamp(0.0, 1.0) as f32,
                y: position[1].clamp(0.0, 1.0) as f32,
                z: position[2].clamp(-1.0, 1.0) as f32,
            });
            update.spread = Some(
                render
                    .object_size
                    .into_iter()
                    .fold(0.0_f64, f64::max)
                    .clamp(0.0, 1.0) as f32,
            );
        }
        events.push(event);
    }
    Ok(())
}

fn oamd_bed_speaker(index: usize) -> Option<Speaker> {
    // OAMD's side/back labels correspond to TrueHD's Ls/Lb bed channels.
    const BEDS: [Speaker; 17] = [
        Speaker::L,
        Speaker::R,
        Speaker::C,
        Speaker::Lfe,
        Speaker::Ls,
        Speaker::Rs,
        Speaker::Lrs,
        Speaker::Rrs,
        Speaker::Tfl,
        Speaker::Tfr,
        Speaker::Tsl,
        Speaker::Tsr,
        Speaker::Trl,
        Speaker::Trr,
        Speaker::Lw,
        Speaker::Rw,
        Speaker::Lfe2,
    ];
    BEDS.get(index).copied()
}

fn channel_speaker(label: ChannelLabel) -> Result<Speaker, DecodeError> {
    use ChannelLabel::*;
    Ok(match label {
        L => Speaker::L,
        R => Speaker::R,
        C => Speaker::C,
        LFE => Speaker::Lfe,
        LFE2 => Speaker::Lfe2,
        Ls => Speaker::Ls,
        Rs => Speaker::Rs,
        Lb => Speaker::Lrs,
        Rb => Speaker::Rrs,
        Cb => Speaker::Cs,
        Tfl => Speaker::Tfl,
        Tfr => Speaker::Tfr,
        Tsl => Speaker::Tsl,
        Tsr => Speaker::Tsr,
        Tbl => Speaker::Trl,
        Tbr => Speaker::Trr,
        Lsc => Speaker::Lc,
        Rsc => Speaker::Rc,
        Lw => Speaker::Lw,
        Rw => Speaker::Rw,
        Tc => Speaker::Tc,
        Tfc => Speaker::Tfc,
        // Surround-direct is a different rendering role, not a side-speaker alias.
        Lsd | Rsd => {
            return Err(unsupported(
                "surround-direct channels have no Speaker representation",
            ));
        }
    })
}

#[cfg(test)]
#[path = "../tests/truehd/unit.rs"]
mod tests;

fn corrupt() -> DecodeError {
    DecodeError::Corrupt {
        codec: "TrueHD",
        detail: "an access unit or object payload failed to extract, parse or decode",
    }
}
fn unsupported(detail: &'static str) -> DecodeError {
    DecodeError::Unsupported {
        codec: "TrueHD",
        detail,
    }
}
