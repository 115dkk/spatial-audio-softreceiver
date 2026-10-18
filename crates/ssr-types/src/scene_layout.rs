//! Stable scene-layout metadata for status and JSONL diagnostics.

use crate::{EndpointStatus, MAX_OBJECTS, Position, SceneObject, Speaker};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum SceneSource {
    #[default]
    None,
    Pcm,
    Ddp,
    #[serde(rename = "ddp_joc")]
    DdpJoc,
    Upmix,
    TrueHd,
    #[serde(rename = "truehd_objects")]
    TrueHdObjects,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObjectRole {
    #[default]
    Unused,
    Bed(Speaker),
    Dynamic,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneLayoutTable {
    pub source: SceneSource,
    pub lfe: bool,
    pub objects: u8,
    pub roles: [ObjectRole; MAX_OBJECTS],
    /// Raw OAMD room coordinates in the ETSI TS 103 420 convention:
    /// x=0 left/1 right, y=0 front/1 back, z=-1 below/0 ear/1 ceiling,
    /// before this repository converts axes and maps the room cube to a direction.
    pub room: [Option<[f32; 3]>; MAX_OBJECTS],
}

impl Default for SceneLayoutTable {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl SceneLayoutTable {
    pub const EMPTY: Self = Self {
        source: SceneSource::None,
        lfe: false,
        objects: 0,
        roles: [ObjectRole::Unused; MAX_OBJECTS],
        room: [None; MAX_OBJECTS],
    };

    pub fn bed_count(&self) -> u8 {
        self.roles[..usize::from(self.objects)]
            .iter()
            .filter(|role| matches!(role, ObjectRole::Bed(_)))
            .count() as u8
    }

    pub fn dynamic_count(&self) -> u8 {
        self.roles[..usize::from(self.objects)]
            .iter()
            .filter(|role| matches!(role, ObjectRole::Dynamic))
            .count() as u8
    }

    /// Converts the fixed real-time table into allocation-owning control status.
    pub fn to_status(&self) -> SceneLayout {
        let mut layout = SceneLayout {
            source: self.source,
            lfe: self.lfe,
            ..SceneLayout::default()
        };
        for (index, role) in self.roles[..usize::from(self.objects)].iter().enumerate() {
            match role {
                ObjectRole::Bed(speaker) => layout.bed.push(SceneBedChannel {
                    index: index as u8,
                    speaker: speaker.name().to_owned(),
                }),
                ObjectRole::Dynamic => layout.dynamic.push(SceneDynamicObject {
                    index: index as u8,
                    room: self.room[index],
                }),
                ObjectRole::Unused => {}
            }
        }
        layout
    }

    /// Equality used by structural logs and reports. Room coordinates are live
    /// metadata and deliberately do not define a new structural layout.
    pub fn structurally_eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.lfe == other.lfe
            && self.objects == other.objects
            && self.roles == other.roles
    }
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(specta::Type))]
pub struct SceneLayout {
    pub source: SceneSource,
    pub lfe: bool,
    pub bed: Vec<SceneBedChannel>,
    pub dynamic: Vec<SceneDynamicObject>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(specta::Type))]
pub struct SceneBedChannel {
    pub index: u8,
    pub speaker: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(specta::Type))]
pub struct SceneDynamicObject {
    pub index: u8,
    pub room: Option<[f32; 3]>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SceneLogLine {
    pub t_ms: u64,
    pub endpoints: Vec<SceneLogEndpoint>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SceneLogEndpoint {
    pub endpoint: u8,
    pub epoch: u32,
    pub source: SceneSource,
    pub lfe: bool,
    pub bed_channels: u8,
    pub dynamic_objects: u8,
    pub active_objects: u8,
    pub objects: Vec<SceneLogObject>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SceneLogObject {
    pub index: u8,
    pub role: String,
    pub speaker: Option<String>,
    pub room: Option<[f32; 3]>,
    pub azimuth_deg: Option<f32>,
    pub elevation_deg: Option<f32>,
    pub level_dbfs: Option<f32>,
}

impl SceneLogLine {
    pub fn new(t_ms: u64, endpoints: &[EndpointStatus], scene: &[SceneObject]) -> Self {
        let endpoints = endpoints
            .iter()
            .enumerate()
            .filter_map(|(endpoint, status)| {
                status
                    .layout
                    .as_ref()
                    .map(|layout| log_endpoint(endpoint, status.epoch, layout, scene))
            })
            .collect();
        Self { t_ms, endpoints }
    }

    /// Builds a log line directly from fixed decoder layout tables. This is an
    /// allocation-allowed control-thread seam for replay tools that do not own
    /// complete endpoint status snapshots. Slice position is the endpoint/lane.
    pub fn from_layouts(
        t_ms: u64,
        layouts: &[(u32, SceneLayoutTable)],
        scene: &[SceneObject],
    ) -> Self {
        let endpoints = layouts
            .iter()
            .enumerate()
            .filter(|(_, (_, layout))| layout.source != SceneSource::None)
            .map(|(endpoint, (epoch, layout))| {
                log_endpoint(endpoint, *epoch, &layout.to_status(), scene)
            })
            .collect();
        Self { t_ms, endpoints }
    }
}

fn log_endpoint(
    endpoint: usize,
    epoch: u32,
    layout: &SceneLayout,
    scene: &[SceneObject],
) -> SceneLogEndpoint {
    let active: Vec<&SceneObject> = scene
        .iter()
        .filter(|object| usize::from(object.lane) == endpoint)
        .collect();
    let mut objects = Vec::with_capacity(layout.bed.len() + layout.dynamic.len() + active.len());
    for bed in &layout.bed {
        let live = active
            .iter()
            .find(|object| object.index == bed.index)
            .copied();
        objects.push(log_object(
            bed.index,
            "bed",
            Some(bed.speaker.clone()),
            None,
            live,
        ));
    }
    for dynamic in &layout.dynamic {
        let live = active
            .iter()
            .find(|object| object.index == dynamic.index)
            .copied();
        objects.push(log_object(
            dynamic.index,
            "dynamic",
            None,
            dynamic.room,
            live,
        ));
    }
    for object in &active {
        if !objects
            .iter()
            .any(|declared| declared.index == object.index)
        {
            objects.push(log_object(object.index, "other", None, None, Some(object)));
        }
    }
    SceneLogEndpoint {
        endpoint: endpoint as u8,
        epoch,
        source: layout.source,
        lfe: layout.lfe,
        bed_channels: layout.bed.len().min(u8::MAX as usize) as u8,
        dynamic_objects: layout.dynamic.len().min(u8::MAX as usize) as u8,
        active_objects: active.len().min(u8::MAX as usize) as u8,
        objects,
    }
}

fn log_object(
    index: u8,
    role: &str,
    speaker: Option<String>,
    room: Option<[f32; 3]>,
    active: Option<&SceneObject>,
) -> SceneLogObject {
    let angles = active.and_then(|object| Position::new(object.x, object.y, object.z).angles());
    SceneLogObject {
        index,
        role: role.to_owned(),
        speaker,
        room,
        azimuth_deg: angles.map(|angles| angles.0),
        elevation_deg: angles.map(|angles| angles.1),
        level_dbfs: active.map(|object| object.level_dbfs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structural_comparison_ignores_room_only_changes() {
        let mut a = SceneLayoutTable::EMPTY;
        a.source = SceneSource::DdpJoc;
        a.objects = 1;
        a.roles[0] = ObjectRole::Dynamic;
        let mut b = a;
        b.room[0] = Some([0.1, 0.2, 0.3]);
        assert!(a.structurally_eq(&b));
        b.lfe = true;
        assert!(!a.structurally_eq(&b));
    }

    #[test]
    fn status_preserves_scene_indices_and_speaker_names() {
        let mut table = SceneLayoutTable::EMPTY;
        table.source = SceneSource::Pcm;
        table.objects = 3;
        table.roles[0] = ObjectRole::Bed(Speaker::L);
        table.roles[2] = ObjectRole::Dynamic;
        table.room[2] = Some([0.25, 0.5, -0.5]);
        let status = table.to_status();
        assert_eq!(status.bed[0].index, 0);
        assert_eq!(status.bed[0].speaker, "L");
        assert_eq!(status.dynamic[0].index, 2);
        assert_eq!(status.dynamic[0].room, table.room[2]);
    }

    #[test]
    fn log_keeps_silent_declarations_and_adds_active_undeclared_objects() {
        let mut table = SceneLayoutTable::EMPTY;
        table.source = SceneSource::DdpJoc;
        table.objects = 2;
        table.roles[0] = ObjectRole::Bed(Speaker::L);
        table.roles[1] = ObjectRole::Dynamic;
        table.room[1] = Some([0.25, 0.0, 0.5]);
        let scene = [
            SceneObject {
                lane: 0,
                index: 1,
                x: -1.0,
                y: 0.0,
                z: 0.0,
                level_dbfs: -6.0,
            },
            SceneObject {
                lane: 0,
                index: 7,
                x: 0.0,
                y: 1.0,
                z: 0.0,
                level_dbfs: -12.0,
            },
            SceneObject {
                lane: 1,
                index: 9,
                x: 0.0,
                y: 1.0,
                z: 0.0,
                level_dbfs: -18.0,
            },
        ];
        let line = SceneLogLine::from_layouts(15, &[(4, table)], &scene);
        let endpoint = &line.endpoints[0];
        assert_eq!((endpoint.epoch, endpoint.active_objects), (4, 2));
        assert_eq!(endpoint.objects.len(), 3);
        assert_eq!(
            (endpoint.objects[0].index, endpoint.objects[0].role.as_str()),
            (0, "bed")
        );
        assert_eq!(endpoint.objects[0].level_dbfs, None);
        assert_eq!(
            (endpoint.objects[1].index, endpoint.objects[1].role.as_str()),
            (1, "dynamic")
        );
        assert_eq!(endpoint.objects[1].room, table.room[1]);
        assert_eq!(endpoint.objects[1].azimuth_deg, Some(90.0));
        assert_eq!(
            (endpoint.objects[2].index, endpoint.objects[2].role.as_str()),
            (7, "other")
        );
    }
}
