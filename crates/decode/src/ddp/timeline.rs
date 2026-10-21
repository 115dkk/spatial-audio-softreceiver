//! DD+ metadata mapping into the shared bounded OAMD timeline.

use eac3::{OamdElementKind, OamdObjectBlock, OamdPayload, ObjectAnchor, ObjectPcmFrame};
use ssr_types::{MAX_OBJECTS, ObjectRole, Position, SceneLayoutTable, SceneSource};

use super::{FRAMES, bed_position, bed_speaker, corrupt, unsupported};
use crate::timeline::{EMPTY_UPDATE, Event, Room, Update};
use crate::{DecodeError, SceneBuffers};

#[derive(Debug)]
pub(super) struct Timeline(crate::timeline::Timeline);

impl Timeline {
    pub(super) fn new() -> Self {
        Self(crate::timeline::Timeline::new("E-AC-3"))
    }

    pub(super) fn reset(&mut self) {
        self.0.reset();
    }

    pub(super) fn discontinuity(&mut self, offset: usize) -> Result<(), DecodeError> {
        self.0.discontinuity(offset)
    }

    pub(super) fn schedule(
        &mut self,
        pcm: &ObjectPcmFrame,
        offset: usize,
    ) -> Result<(), DecodeError> {
        if pcm.object_channels.len() > MAX_OBJECTS {
            return Err(corrupt());
        }
        for (payload, payload_offset) in &pcm.oamd_payloads {
            let ObjectMapping { mapping, speakers } =
                object_mapping(payload, pcm.object_channels.len())?;
            let base = offset as u64 + u64::from(payload_offset.unwrap_or(0));
            let mut has_element = false;
            for element in &payload.elements {
                let OamdElementKind::Object(element) = &element.kind else {
                    continue;
                };
                has_element = true;
                if element.block_updates.is_empty()
                    || element.block_updates.len() > 8
                    || element.object_blocks.len() != payload.object_count
                    || element
                        .object_blocks
                        .iter()
                        .any(|b| b.len() != element.block_updates.len())
                {
                    return Err(corrupt());
                }
                let mut previous = None;
                for (block_index, timing) in element.block_updates.iter().enumerate() {
                    // Pinned eac3 stores factor + sample_offset, not sample count.
                    let factor = i32::from(timing.offset) - i32::from(element.sample_offset);
                    if !(0..=63).contains(&factor)
                        || element.sample_offset > 31
                        || !(0..=2048).contains(&timing.ramp_duration)
                    {
                        return Err(corrupt());
                    }
                    let sample = base + u64::from(element.sample_offset) + 32 * factor as u64;
                    if previous.is_some_and(|p| p >= sample) {
                        return Err(corrupt());
                    }
                    previous = Some(sample);
                    let mut event = Event {
                        sample,
                        ramp: timing.ramp_duration as u64,
                        reset: false,
                        objects: pcm.object_channels.len(),
                        updates: [EMPTY_UPDATE; MAX_OBJECTS],
                    };
                    for (oamd_index, &output) in
                        mapping.iter().enumerate().take(payload.object_count)
                    {
                        let Some(output) = output else { continue };
                        event.updates[output] = update_from_block(
                            &element.object_blocks[oamd_index][block_index],
                            speakers[output],
                        )?;
                    }
                    self.0.insert(event)?;
                }
            }
            if !has_element {
                let mut event = Event {
                    sample: base,
                    ramp: 0,
                    reset: false,
                    objects: pcm.object_channels.len(),
                    updates: [EMPTY_UPDATE; MAX_OBJECTS],
                };
                for (update, speaker) in event.updates.iter_mut().zip(speakers) {
                    update.speaker = speaker;
                }
                self.0.insert(event)?;
            }
        }
        Ok(())
    }

    pub(super) fn layout(&self, pcm: &ObjectPcmFrame) -> Result<SceneLayoutTable, DecodeError> {
        let mut layout = SceneLayoutTable::EMPTY;
        layout.source = SceneSource::DdpJoc;
        layout.lfe = pcm.core.lfe_channel.is_some();
        layout.objects = pcm.object_channels.len() as u8;
        let Some((payload, _)) = pcm.oamd_payloads.last() else {
            layout.roles[..pcm.object_channels.len()].fill(ObjectRole::Dynamic);
            return Ok(layout);
        };
        let mapping = object_mapping(payload, pcm.object_channels.len())?;
        let mut output = 0;
        for &bed in payload.bed_assignment.iter().flatten() {
            if let Some(speaker) = bed_speaker(bed).filter(|speaker| speaker.position().is_some()) {
                layout.roles[output] = ObjectRole::Bed(speaker);
                output += 1;
            }
        }
        for role in &mut layout.roles[output..pcm.object_channels.len()] {
            *role = ObjectRole::Dynamic;
        }
        for index in output..pcm.object_channels.len() {
            layout.room[index] = self.0.latest_room(index);
        }
        debug_assert_eq!(
            mapping
                .speakers
                .iter()
                .filter(|speaker| speaker.is_some())
                .count(),
            output
        );
        Ok(layout)
    }

    pub(super) fn emit(
        &mut self,
        buffers: &mut SceneBuffers,
        epoch: u32,
        rate: u32,
        qpc: u64,
        out: &mut dyn FnMut(&ssr_types::SceneBlock<'_>),
    ) {
        self.0.emit(buffers, FRAMES, epoch, rate, qpc, out);
    }
}

fn update_from_block(
    block: &OamdObjectBlock,
    speaker: Option<Position>,
) -> Result<Update, DecodeError> {
    // Cached reuse blocks must not reapply deltas or restart in-flight ramps.
    let position_updated = block.render_info_status != 2
        && (block.render_info_status != 3 || block.render_info_blocks.is_some_and(|b| b & 1 != 0));
    let room = if speaker.is_none() && block.valid_position && position_updated {
        if block.anchor == ObjectAnchor::Speaker {
            return Err(corrupt());
        }
        let p = block.position.ok_or_else(corrupt)?;
        if ![p.x, p.y, p.z].into_iter().all(f32::is_finite) {
            return Err(corrupt());
        }
        Some(Room {
            x: p.x,
            y: p.y,
            z: p.z,
        })
    } else {
        None
    };
    if block.gain.is_some_and(|g| !g.is_finite() || g < 0.0)
        || block.size.is_some_and(|s| s.iter().any(|v| !v.is_finite()))
    {
        return Err(corrupt());
    }
    Ok(Update {
        room,
        differential: block.differential_position,
        speaker,
        gain: if block.basic_info_status == 2
            || (block.basic_info_status == 3 && block.basic_info_blocks.is_none_or(|b| b & 2 == 0))
        {
            None
        } else {
            block.gain
        },
        spread: if block.render_info_status == 2
            || (block.render_info_status == 3
                && block.render_info_blocks.is_none_or(|b| b & 4 == 0))
        {
            None
        } else {
            block
                .size
                .map(|s| s.into_iter().fold(0.0_f32, f32::max).clamp(0.0, 1.0))
        },
    })
}

struct ObjectMapping {
    mapping: [Option<usize>; MAX_OBJECTS],
    speakers: [Option<Position>; MAX_OBJECTS],
}

fn object_mapping(payload: &OamdPayload, objects: usize) -> Result<ObjectMapping, DecodeError> {
    if payload.isf_in_use || payload.alternate_object_present {
        return Err(unsupported("ISF or alternate OAMD object mapping"));
    }
    if payload.object_count > MAX_OBJECTS || objects > MAX_OBJECTS {
        return Err(corrupt());
    }
    let mut mapping = [None; MAX_OBJECTS];
    let mut speakers = [None; MAX_OBJECTS];
    let mut oamd = 0;
    let mut output = 0;
    for &bed in payload.bed_assignment.iter().flatten() {
        if oamd >= payload.object_count {
            return Err(corrupt());
        }
        if let Some(position) = bed_position(bed) {
            mapping[oamd] = Some(output);
            speakers[output] = Some(position);
            output += 1;
        }
        oamd += 1;
    }
    if oamd != payload.bed_or_isf_objects || oamd + payload.dynamic_objects != payload.object_count
    {
        return Err(corrupt());
    }
    for slot in &mut mapping[oamd..payload.object_count] {
        *slot = Some(output);
        output += 1;
    }
    if output != objects {
        return Err(corrupt());
    }
    Ok(ObjectMapping { mapping, speakers })
}
