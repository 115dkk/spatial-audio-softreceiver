//! Offline measurements through the engine's inline decoder. Layouts and raw
//! room coordinates are sampled after each record; direction ranges and
//! `oamd_updates` cover changes visible at those samples, not hidden intra-record updates.

use std::collections::BTreeMap;

use engine::{Chain, ChainConfig};
use serde::Serialize;
use ssr_abi::{Record, records};
use ssr_types::{ObjectRole, SceneLayoutTable, SceneObject, SceneSource};

#[derive(Clone, Debug, Serialize)]
pub struct Range<T> {
    min: T,
    max: T,
}

#[derive(Clone, Debug, Serialize)]
pub struct DynamicObject {
    index: u8,
    first_room: Option<[f32; 3]>,
    last_room: Option<[f32; 3]>,
    azimuth_deg: Option<Range<f32>>,
    elevation_deg: Option<Range<f32>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SceneSummary {
    source: SceneSource,
    lfe: bool,
    bed: Vec<String>,
    dynamic_objects: Range<usize>,
    oamd_updates: u64,
    objects: BTreeMap<u8, DynamicObject>,
    #[serde(skip)]
    previous_room: BTreeMap<u8, Option<[f32; 3]>>,
    sampling: &'static str,
}

fn extend(range: &mut Option<Range<f32>>, value: f32) {
    if !value.is_finite() {
        return;
    }
    match range {
        Some(range) => {
            range.min = range.min.min(value);
            range.max = range.max.max(value);
        }
        None => {
            *range = Some(Range {
                min: value,
                max: value,
            })
        }
    }
}

impl SceneSummary {
    fn observe(&mut self, endpoint: u16, layout: &SceneLayoutTable, scene: &[SceneObject]) {
        self.source = layout.source;
        self.lfe |= layout.lfe;
        let count = usize::from(layout.dynamic_count());
        self.dynamic_objects.min = self.dynamic_objects.min.min(count);
        self.dynamic_objects.max = self.dynamic_objects.max.max(count);
        for (index, role) in layout.roles[..usize::from(layout.objects)]
            .iter()
            .enumerate()
        {
            match role {
                ObjectRole::Bed(speaker) => {
                    if !self.bed.iter().any(|name| name == speaker.name()) {
                        self.bed.push(speaker.name().into());
                    }
                }
                ObjectRole::Dynamic => {
                    let object_index = index as u8;
                    if self.source == SceneSource::DdpJoc {
                        match self.previous_room.insert(object_index, layout.room[index]) {
                            None if layout.room[index].is_some() => {
                                self.oamd_updates = self.oamd_updates.saturating_add(1);
                            }
                            Some(previous) if previous != layout.room[index] => {
                                self.oamd_updates = self.oamd_updates.saturating_add(1);
                            }
                            _ => {}
                        }
                    }
                    let object =
                        self.objects
                            .entry(object_index)
                            .or_insert_with(|| DynamicObject {
                                index: object_index,
                                first_room: layout.room[index],
                                last_room: None,
                                azimuth_deg: None,
                                elevation_deg: None,
                            });
                    object.last_room = layout.room[index];
                    if let Some(live) = scene.iter().find(|live| {
                        u16::from(live.lane) == endpoint && usize::from(live.index) == index
                    }) && let Some((azimuth, elevation)) =
                        ssr_types::Position::new(live.x, live.y, live.z).angles()
                    {
                        extend(&mut object.azimuth_deg, azimuth);
                        extend(&mut object.elevation_deg, elevation);
                    }
                }
                ObjectRole::Unused => {}
            }
        }
    }
}

fn observe(result: &mut BTreeMap<(u16, u32), SceneSummary>, chain: &Chain, epochs: &[u32]) {
    for (endpoint, epoch) in epochs.iter().copied().enumerate() {
        let Some(layout) = chain
            .layout(endpoint as u16)
            .filter(|layout| layout.source != SceneSource::None)
        else {
            continue;
        };
        let count = usize::from(layout.dynamic_count());
        result
            .entry((endpoint as u16, epoch))
            .or_insert_with(|| SceneSummary {
                source: layout.source,
                lfe: false,
                bed: Vec::new(),
                dynamic_objects: Range {
                    min: count,
                    max: count,
                },
                oamd_updates: 0,
                objects: BTreeMap::new(),
                previous_room: BTreeMap::new(),
                sampling: "record_boundary",
            })
            .observe(endpoint as u16, layout, chain.scene());
    }
}

pub fn inspect(stream: &[u8], qpc_frequency: u64) -> BTreeMap<(u16, u32), SceneSummary> {
    let mut chain = Chain::with_qpc_frequency(ChainConfig::default(), qpc_frequency);
    let mut epochs = [0; ssr_abi::MAX_ENDPOINTS];
    let mut result = BTreeMap::new();
    for record in records(stream).flatten() {
        if let Record::FormatChange {
            endpoint, epoch, ..
        } = record
            && let Some(current) = epochs.get_mut(usize::from(endpoint))
        {
            *current = epoch;
        }
        chain.push_record(record, &mut |_| {});
        observe(&mut result, &chain, &epochs);
    }
    chain.begin_finish();
    loop {
        let done = chain.finish_step(1, &mut |_| {});
        observe(&mut result, &chain, &epochs);
        if done {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_measurements_preserve_rooms_and_rendered_angle_ranges() {
        let mut layout = SceneLayoutTable::EMPTY;
        layout.source = SceneSource::DdpJoc;
        layout.objects = 2;
        layout.roles[0] = ObjectRole::Bed(ssr_types::Speaker::L);
        layout.roles[1] = ObjectRole::Dynamic;
        layout.room[1] = Some([0.1, 0.2, 0.3]);
        let mut summary = SceneSummary {
            source: SceneSource::DdpJoc,
            lfe: false,
            bed: Vec::new(),
            dynamic_objects: Range { min: 1, max: 1 },
            oamd_updates: 0,
            objects: BTreeMap::new(),
            previous_room: BTreeMap::new(),
            sampling: "record_boundary",
        };
        for (azimuth, elevation) in [(-45.0, 10.0), (30.0, 40.0)] {
            let position = ssr_types::Position::from_angles(azimuth, elevation);
            summary.observe(
                2,
                &layout,
                &[SceneObject {
                    lane: 2,
                    index: 1,
                    x: position.x,
                    y: position.y,
                    z: position.z,
                    level_dbfs: -20.0,
                }],
            );
            layout.room[1] = Some([0.9, 0.8, 0.7]);
        }
        let object = &summary.objects[&1];
        assert_eq!(object.first_room, Some([0.1, 0.2, 0.3]));
        assert_eq!(object.last_room, Some([0.9, 0.8, 0.7]));
        assert_eq!(summary.oamd_updates, 2);
        let azimuth = object.azimuth_deg.as_ref().unwrap();
        let elevation = object.elevation_deg.as_ref().unwrap();
        assert!((azimuth.min + 45.0).abs() < 0.001 && (azimuth.max - 30.0).abs() < 0.001);
        assert!((elevation.min - 10.0).abs() < 0.001 && (elevation.max - 40.0).abs() < 0.001);
        assert_eq!(summary.bed, ["L"]);
    }
}
