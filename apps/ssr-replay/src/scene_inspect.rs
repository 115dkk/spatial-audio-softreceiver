//! Offline measurements through the engine's inline decoder. Layouts and raw
//! room coordinates are sampled after each record; direction ranges and
//! `oamd_updates` cover changes visible at those samples, not hidden intra-record updates.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use engine::{Chain, ChainConfig};
use serde::Serialize;
use ssr_abi::{RECORD_FLAG_DISCONTINUITY, Record, records};
use ssr_types::{
    BitstreamCodec, ObjectRole, SceneLayoutTable, SceneObject, SceneSource, SourceFormat,
};

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

#[derive(Clone, Debug, Default)]
pub struct MatDiagnostic {
    pub payload: Option<&'static str>,
    pub head: Option<String>,
}

#[derive(Debug, Default)]
pub struct Inspection {
    pub scenes: BTreeMap<(u16, u32), SceneSummary>,
    pub mat: BTreeMap<u16, MatDiagnostic>,
}

struct MatInspector {
    deframer: bitstream::Deframer,
    content: Box<[u8; decode::mat_frame::MAT_CONTENT_BYTES]>,
    format: Option<(u32, SourceFormat)>,
    running: bool,
}

impl Default for MatInspector {
    fn default() -> Self {
        Self {
            deframer: bitstream::Deframer::new(),
            content: Box::new([0; decode::mat_frame::MAT_CONTENT_BYTES]),
            format: None,
            running: false,
        }
    }
}

impl MatInspector {
    fn active(&self) -> bool {
        matches!(
            self.format,
            Some((_, SourceFormat::Bitstream(format)))
                if matches!(format.codec, BitstreamCodec::Mat20 | BitstreamCodec::Mat21)
        )
    }
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
                    if matches!(
                        self.source,
                        SceneSource::DdpJoc | SceneSource::TrueHdObjects
                    ) {
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

pub fn inspect(stream: &[u8], qpc_frequency: u64) -> Inspection {
    let mut chain = Chain::with_qpc_frequency(ChainConfig::default(), qpc_frequency);
    let mut epochs = [0; ssr_abi::MAX_ENDPOINTS];
    let mut mat =
        std::array::from_fn::<_, { ssr_abi::MAX_ENDPOINTS }, _>(|_| MatInspector::default());
    let mut result = Inspection::default();
    for record in records(stream).flatten() {
        match record {
            Record::FormatChange {
                endpoint,
                epoch,
                format,
                repeat,
                ..
            } => {
                if let Some(current) = epochs.get_mut(usize::from(endpoint)) {
                    *current = epoch;
                }
                if let Some(inspector) = mat.get_mut(usize::from(endpoint))
                    && (!repeat || inspector.format != Some((epoch, format)))
                {
                    inspector.deframer.reset();
                    inspector.format = Some((epoch, format));
                    result.mat.remove(&endpoint);
                }
            }
            Record::Data {
                endpoint,
                flags,
                bytes,
                ..
            } => {
                if let Some(inspector) = mat.get_mut(usize::from(endpoint))
                    && inspector.active()
                {
                    if flags & RECORD_FLAG_DISCONTINUITY != 0 {
                        inspector.deframer.reset();
                    }
                    inspector.deframer.push(bytes, &mut |burst| {
                        if let Ok(decode::mat_frame::MatPayload::Unknown(content)) =
                            decode::mat_frame::extract(&burst, &mut inspector.content)
                        {
                            let diagnostic = result.mat.entry(endpoint).or_default();
                            diagnostic.payload = Some("unknown");
                            let mut head = String::with_capacity(128);
                            for byte in content.iter().take(64) {
                                write!(&mut head, "{byte:02x}")
                                    .expect("writing to String cannot fail");
                            }
                            diagnostic.head = Some(head);
                        }
                    });
                }
            }
            Record::Overrun { endpoint, .. } => {
                if let Some(inspector) = mat.get_mut(usize::from(endpoint)) {
                    inspector.deframer.reset();
                }
            }
            Record::StreamStart {
                endpoint, repeat, ..
            } => {
                if let Some(inspector) = mat.get_mut(usize::from(endpoint))
                    && (!repeat || !inspector.running)
                {
                    inspector.deframer.reset();
                    inspector.running = true;
                }
            }
            Record::StreamStop {
                endpoint, repeat, ..
            } => {
                if let Some(inspector) = mat.get_mut(usize::from(endpoint))
                    && (!repeat || inspector.running)
                {
                    inspector.deframer.reset();
                    inspector.running = false;
                }
            }
            Record::Volume { .. } => {}
        }
        chain.push_record(record, &mut |_| {});
        observe(&mut result.scenes, &chain, &epochs);
    }
    chain.begin_finish();
    loop {
        let done = chain.finish_step(1, &mut |_| {});
        observe(&mut result.scenes, &chain, &epochs);
        if done {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssr_abi::{RecordWriter, wave_format_bytes};
    use ssr_types::BitstreamFormat;

    fn unknown_mat_payload() -> Vec<u8> {
        let mut payload = vec![0; decode::mat_frame::MAT_PAYLOAD_BYTES];
        payload[..decode::mat_frame::START_MARKER.len()]
            .copy_from_slice(&decode::mat_frame::START_MARKER);
        let middle = decode::mat_frame::MIDDLE_MARKER_OFFSET;
        payload[middle..middle + decode::mat_frame::MIDDLE_MARKER.len()]
            .copy_from_slice(&decode::mat_frame::MIDDLE_MARKER);
        payload[decode::mat_frame::END_MARKER_OFFSET..]
            .copy_from_slice(&decode::mat_frame::END_MARKER);
        payload
    }

    fn mat_format() -> SourceFormat {
        SourceFormat::Bitstream(BitstreamFormat {
            codec: BitstreamCodec::Mat21,
            container_rate: 192_000,
            container_channels: 8,
            encoded_rate: 48_000,
            encoded_channels: 6,
        })
    }

    #[test]
    fn repeated_mat_announcements_preserve_a_split_diagnostic_burst() {
        let payload = unknown_mat_payload();
        let mut burst = Vec::new();
        bitstream::frame_burst(bitstream::DataType::Mat, &payload, 61_440, &mut burst);
        let mut stream = vec![0; 100_000];
        let mut writer = RecordWriter::new(&mut stream);
        let format = wave_format_bytes(&mat_format());
        assert!(writer.format_change(7, 0, &format));
        assert!(writer.stream_start(7, 0));
        let split = burst.len() / 2;
        assert!(writer.data(7, 0, 100_000, &burst[..split]));
        assert!(writer.format_repeat(7, 100_001, &format));
        assert!(writer.stream_start_repeat(7, 100_001));
        assert!(writer.data(7, split as u64, 200_000, &burst[split..]));
        let length = writer.len();

        let inspection = inspect(&stream[..length], 10_000_000);
        assert_eq!(inspection.mat[&0].payload, Some("unknown"));
        assert_eq!(
            inspection.mat[&0].head.as_deref(),
            Some("00".repeat(64).as_str())
        );
    }

    #[test]
    fn a_new_format_clears_an_old_unknown_mat_diagnostic() {
        let payload = unknown_mat_payload();
        let mut burst = Vec::new();
        bitstream::frame_burst(bitstream::DataType::Mat, &payload, 61_440, &mut burst);
        let mut stream = vec![0; 100_000];
        let mut writer = RecordWriter::new(&mut stream);
        assert!(writer.format_change(7, 0, &wave_format_bytes(&mat_format())));
        assert!(writer.data(7, 0, 200_000, &burst));
        let pcm = SourceFormat::Pcm(ssr_types::PcmFormat {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            valid_bits: 32,
            sample_type: ssr_types::SampleType::Float,
            channel_mask: 3,
        });
        assert!(writer.format_change(8, 200_001, &wave_format_bytes(&pcm)));
        let length = writer.len();

        let inspection = inspect(&stream[..length], 10_000_000);
        assert!(!inspection.mat.contains_key(&0));
    }

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

    #[test]
    fn truehd_object_room_changes_count_as_oamd_updates() {
        let mut layout = SceneLayoutTable::EMPTY;
        layout.source = SceneSource::TrueHdObjects;
        layout.objects = 1;
        layout.roles[0] = ObjectRole::Dynamic;
        layout.room[0] = Some([0.1, 0.2, 0.3]);
        let mut summary = SceneSummary {
            source: SceneSource::TrueHdObjects,
            lfe: false,
            bed: Vec::new(),
            dynamic_objects: Range { min: 1, max: 1 },
            oamd_updates: 0,
            objects: BTreeMap::new(),
            previous_room: BTreeMap::new(),
            sampling: "record_boundary",
        };

        summary.observe(0, &layout, &[]);
        layout.room[0] = Some([0.4, 0.5, 0.6]);
        summary.observe(0, &layout, &[]);

        assert_eq!(summary.oamd_updates, 2);
    }
}
