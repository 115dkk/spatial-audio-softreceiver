use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use ssr_types::{SceneLayoutTable, SceneLogLine, SceneObject};

/// Replay-local writer keeps the offline executable independent of supervisor.
/// A change and interval at the same sample emit one line; changes do not move
/// the interval deadline. Missed intervals are never populated with future data.
pub struct SceneLogWriter {
    writer: BufWriter<File>,
    interval_ms: u64,
    last_interval_ms: Option<u64>,
    previous: Vec<SceneLayoutTable>,
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
            previous: Vec::new(),
        })
    }

    pub fn sample(
        &mut self,
        t_ms: u64,
        endpoints: &[(u32, SceneLayoutTable)],
        scene: &[SceneObject],
    ) -> io::Result<bool> {
        self.previous
            .resize(endpoints.len(), SceneLayoutTable::EMPTY);
        let changed = endpoints
            .iter()
            .zip(&self.previous)
            .any(|((_, layout), previous)| !layout.structurally_eq(previous));
        let interval = self
            .last_interval_ms
            .is_none_or(|last| t_ms.saturating_sub(last) >= self.interval_ms);
        if !changed && !interval {
            return Ok(false);
        }
        serde_json::to_writer(
            &mut self.writer,
            &SceneLogLine::from_layouts(t_ms, endpoints, scene),
        )?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        for (previous, (_, layout)) in self.previous.iter_mut().zip(endpoints) {
            *previous = *layout;
        }
        if interval {
            self.last_interval_ms = Some(t_ms);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssr_types::{ObjectRole, SceneSource};

    #[test]
    fn replay_scene_log_keeps_intervals_ignores_motion_and_coalesces_change() {
        let path =
            std::env::temp_dir().join(format!("ssr-replay-scene-log-{}.jsonl", std::process::id()));
        let mut writer = SceneLogWriter::create(&path, 200).unwrap();
        let mut layouts = [(1, SceneLayoutTable::EMPTY)];
        layouts[0].1.source = SceneSource::DdpJoc;
        layouts[0].1.objects = 1;
        layouts[0].1.roles[0] = ObjectRole::Dynamic;
        assert!(writer.sample(0, &layouts, &[]).unwrap());
        layouts[0].1.room[0] = Some([0.2, 0.3, 0.4]);
        assert!(!writer.sample(100, &layouts, &[]).unwrap());
        assert!(writer.sample(200, &layouts, &[]).unwrap());
        layouts[0].1.lfe = true;
        assert!(writer.sample(400, &layouts, &[]).unwrap());
        assert!(!writer.sample(400, &layouts, &[]).unwrap());
        let lines = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<SceneLogLine>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            lines.iter().map(|line| line.t_ms).collect::<Vec<_>>(),
            [0, 200, 400]
        );
        assert_eq!(lines[1].endpoints[0].objects[0].room, Some([0.2, 0.3, 0.4]));
        drop(writer);
        std::fs::remove_file(path).unwrap();
    }
}
