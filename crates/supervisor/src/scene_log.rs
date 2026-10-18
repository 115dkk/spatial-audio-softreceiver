//! Control-thread scene diagnostics. No file I/O belongs on the audio threads.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use ssr_types::{EndpointStatus, SceneLayout, SceneLogLine, SceneObject};

/// Remembers structural layouts, not moving room coordinates or epoch numbers.
#[derive(Debug, Default)]
pub struct SceneChanges {
    previous: Vec<Option<SceneLayout>>,
}

impl SceneChanges {
    /// First observed layouts and removals count as changes; initial absence does not.
    pub fn observe(&mut self, endpoints: &[EndpointStatus]) -> Vec<usize> {
        self.previous.resize(endpoints.len(), None);
        let mut changed = Vec::new();
        for (index, endpoint) in endpoints.iter().enumerate() {
            let same = match (&self.previous[index], &endpoint.layout) {
                (Some(old), Some(new)) => structurally_eq(old, new),
                (None, None) => true,
                _ => false,
            };
            if !same {
                changed.push(index);
                self.previous[index] = endpoint.layout.clone();
            }
        }
        changed
    }
}

pub fn structurally_eq(a: &SceneLayout, b: &SceneLayout) -> bool {
    a.source == b.source
        && a.lfe == b.lfe
        && a.bed == b.bed
        && a.dynamic
            .iter()
            .map(|object| object.index)
            .eq(b.dynamic.iter().map(|object| object.index))
}

#[derive(Debug)]
pub struct SceneLogWriter {
    writer: BufWriter<File>,
    interval_ms: u64,
    last_interval_ms: Option<u64>,
    changes: SceneChanges,
}

impl SceneLogWriter {
    pub fn create(path: &Path, interval_ms: u64) -> io::Result<Self> {
        if interval_ms == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "zero scene-log interval",
            ));
        }
        Ok(Self {
            writer: BufWriter::new(File::create(path)?),
            interval_ms,
            last_interval_ms: None,
            changes: SceneChanges::default(),
        })
    }

    /// Sample at the caller's monotonic timeline. An interval and a structural
    /// change due at the same sample produce one line. Changes between intervals
    /// do not postpone the interval; missed intervals are not backfilled with
    /// invented historical scenes. Each written line is flushed immediately.
    pub fn sample(
        &mut self,
        t_ms: u64,
        endpoints: &[EndpointStatus],
        scene: &[SceneObject],
    ) -> io::Result<bool> {
        let changed = !self.changes.observe(endpoints).is_empty();
        let interval = self
            .last_interval_ms
            .is_none_or(|last| t_ms.saturating_sub(last) >= self.interval_ms);
        if !changed && !interval {
            return Ok(false);
        }
        let line = SceneLogLine::new(t_ms, endpoints, scene);
        serde_json::to_writer(&mut self.writer, &line)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        if interval {
            self.last_interval_ms = Some(t_ms);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssr_types::{SceneDynamicObject, SceneSource};

    #[test]
    fn scene_changes_ignore_room_and_epoch_but_include_removal() {
        let mut endpoints = [EndpointStatus::default()];
        let mut changes = SceneChanges::default();
        assert!(changes.observe(&endpoints).is_empty());
        endpoints[0].layout = Some(SceneLayout {
            source: SceneSource::DdpJoc,
            dynamic: vec![SceneDynamicObject {
                index: 0,
                room: None,
            }],
            ..Default::default()
        });
        assert_eq!(changes.observe(&endpoints), [0]);
        endpoints[0].epoch = 2;
        endpoints[0].layout.as_mut().unwrap().dynamic[0].room = Some([0.2, 0.3, 0.4]);
        assert!(changes.observe(&endpoints).is_empty());
        endpoints[0].layout.as_mut().unwrap().lfe = true;
        assert_eq!(changes.observe(&endpoints), [0]);
        endpoints[0].layout = None;
        assert_eq!(changes.observe(&endpoints), [0]);
        assert!(changes.observe(&endpoints).is_empty());
    }

    #[test]
    fn scene_log_intervals_and_changes_share_one_line() {
        let path = std::env::temp_dir().join(format!("ssr-scene-log-{}.jsonl", std::process::id()));
        let mut writer = SceneLogWriter::create(&path, 200).unwrap();
        let mut endpoints = [EndpointStatus::default()];
        assert!(writer.sample(0, &endpoints, &[]).unwrap());
        assert!(!writer.sample(100, &endpoints, &[]).unwrap());
        endpoints[0].layout = Some(SceneLayout {
            source: SceneSource::Pcm,
            ..Default::default()
        });
        assert!(writer.sample(150, &endpoints, &[]).unwrap());
        assert!(writer.sample(200, &endpoints, &[]).unwrap());
        endpoints[0].layout.as_mut().unwrap().lfe = true;
        assert!(writer.sample(400, &endpoints, &[]).unwrap());
        assert!(!writer.sample(400, &endpoints, &[]).unwrap());
        assert!(writer.sample(600, &endpoints, &[]).unwrap());
        let lines = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<SceneLogLine>(line).unwrap().t_ms)
            .collect::<Vec<_>>();
        assert_eq!(lines, [0, 150, 200, 400, 600]);
        drop(writer);
        std::fs::remove_file(path).unwrap();
    }
}
