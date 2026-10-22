//! Clean-room excitation and observation data, not a MAT PCM decoder (ADR 0005).
//! Coordinates are Windows metres: x right, y up, z back, NOT SceneBlock axes.

use serde::{Deserialize, Serialize};

pub const RATE: u32 = 48_000;
pub const PERIOD: usize = 32_767;
pub const MAX_OBJECTS: usize = 256;

/// x^15 + x^14 + 1, nonzero 15-bit state; output precedes the shift.
#[derive(Clone, Copy, Debug)]
pub struct Lfsr(u16);
impl Lfsr {
    pub fn new(seed: u16) -> Option<Self> {
        (seed > 0 && seed <= 0x7fff).then_some(Self(seed))
    }
    pub fn sample(&mut self) -> f32 {
        let value = if self.0 & 1 == 0 { -8192 } else { 8192 };
        self.0 = (self.0 >> 1) | (((self.0 ^ (self.0 >> 1)) & 1) << 14);
        value as f32 / 32768.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeObject {
    pub kind: String,
    pub audio_object_type: u32,
    pub seed: u16,
    pub static_position: Option<[f32; 3]>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ObjectState {
    pub first_sample: u64,
    pub position: [f32; 3],
    pub volume: f32,
    pub active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeEvent {
    pub qpc: u64,
    pub frames: u32,
    pub available_dynamic: u32,
    pub objects: Vec<ObjectState>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub endpoint_id: String,
    pub max_dynamic_objects: u32,
    pub native_static_mask: u32,
    pub float32_48000_hresult: i32,
    pub stream_available_hresult: i32,
    pub static_positions: Vec<(String, Option<[f32; 3]>, i32)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Truth {
    pub version: u32,
    pub endpoint: u16,
    pub sample_rate: u32,
    pub qpc_frequency: u64,
    pub start_qpc: u64,
    pub requested_seconds: f64,
    pub device: DeviceInfo,
    pub objects: Vec<ProbeObject>,
    pub events: Vec<ProbeEvent>,
    pub completed: bool,
    pub error: Option<String>,
}
impl Truth {
    pub fn validate(&self) -> bool {
        self.version == 1
            && self.sample_rate == RATE
            && self.qpc_frequency > 0
            && self.endpoint <= 2
            && !self.objects.is_empty()
            && self.objects.len() <= MAX_OBJECTS
            && self.requested_seconds.is_finite()
            && self.requested_seconds > 0.0
            && self.objects.iter().all(|o| Lfsr::new(o.seed).is_some())
            && self.events.windows(2).all(|e| e[0].qpc <= e[1].qpc)
            && self.events.iter().all(|e| {
                e.frames > 0
                    && e.frames <= RATE
                    && e.objects.len() == self.objects.len()
                    && e.objects
                        .iter()
                        .all(|s| s.volume.is_finite() && s.position.iter().all(|v| v.is_finite()))
            })
    }
}

pub fn solo_start(index: usize) -> u64 {
    u64::from(RATE) + index as u64 * u64::from(RATE / 2)
}
pub fn together_start(count: usize) -> u64 {
    solo_start(count)
}
pub fn audible(index: usize, count: usize, sample: u64) -> bool {
    sample >= together_start(count) || (solo_start(index)..solo_start(index + 1)).contains(&sample)
}

pub fn state(index: usize, count: usize, sample: u64, object: &ProbeObject) -> ObjectState {
    let moving = sample.saturating_sub(together_start(count)) as f32 / RATE as f32;
    let azimuth = (index as f32 * 30.0 + moving * (13.0 + index as f32 * 3.0)).to_radians();
    // Elevation also varies during motion, so y is identifiable rather than constant.
    let elevation = (if index.is_multiple_of(2) { 0.0 } else { 30.0 }
        + 15.0 * (moving * (0.7 + index as f32 * 0.03)).sin())
    .to_radians();
    ObjectState {
        first_sample: sample,
        position: object.static_position.unwrap_or([
            azimuth.sin() * elevation.cos(),
            elevation.sin(),
            -azimuth.cos() * elevation.cos(),
        ]),
        volume: if sample >= together_start(count)
            && (sample / u64::from(RATE / 2) + index as u64).is_multiple_of(3)
        {
            0.5
        } else {
            1.0
        },
        active: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lfsr_has_maximal_period_and_exact_integer_samples() {
        let mut l = Lfsr::new(1).unwrap();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..PERIOD {
            assert!(seen.insert(l.0));
            assert_eq!(l.sample().abs(), 0.25);
        }
        assert_eq!(l.0, 1);
        assert!(Lfsr::new(0).is_none());
    }
    #[test]
    fn silence_solos_motion_and_volume_are_deterministic() {
        let object = ProbeObject {
            kind: "dynamic_0".into(),
            audio_object_type: 1,
            seed: 7,
            static_position: None,
        };
        assert!(!audible(0, 2, 47999));
        assert!(audible(0, 2, 48000));
        assert!(!audible(1, 2, 48000));
        assert!(audible(1, 2, 72000));
        let a = state(0, 2, 0, &object);
        assert_eq!(a.position, [0.0, 0.0, -1.0]);
        let b = state(0, 2, 144000, &object);
        assert_ne!(a.position, b.position);
        assert_eq!(b.volume, 0.5);
    }
}
