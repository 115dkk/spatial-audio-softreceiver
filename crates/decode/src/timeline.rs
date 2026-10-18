//! Codec-neutral, bounded OAMD events and room-coordinate ramps.
//! Codec adapters validate and translate metadata before scheduling it here.

use ssr_types::{MAX_OBJECTS, Position, SceneBlock, room_to_direction};

use crate::{DecodeError, SceneBuffers};

const MAX_EVENTS: usize = 128;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Room {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Room {
    fn clamped(self) -> Self {
        Self {
            x: self.x.clamp(0.0, 1.0),
            y: self.y.clamp(0.0, 1.0),
            z: self.z.clamp(-1.0, 1.0),
        }
    }

    fn plus(self, delta: Self) -> Self {
        Self {
            x: self.x + delta.x,
            y: self.y + delta.y,
            z: self.z + delta.z,
        }
        .clamped()
    }

    fn direction(self) -> Position {
        let p = self.clamped();
        room_to_direction(2.0 * p.x - 1.0, 1.0 - 2.0 * p.y, p.z)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Update {
    pub room: Option<Room>,
    pub differential: bool,
    pub speaker: Option<Position>,
    pub gain: Option<f32>,
    pub spread: Option<f32>,
}

pub(crate) const EMPTY_UPDATE: Update = Update {
    room: None,
    differential: false,
    speaker: None,
    gain: None,
    spread: None,
};

/// `sample` is relative to the start of the next audio submitted to the timeline.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Event {
    pub sample: u64,
    pub ramp: u64,
    pub reset: bool,
    pub objects: usize,
    pub updates: [Update; MAX_OBJECTS],
}

#[derive(Clone, Copy, Debug)]
struct Meta {
    position: Position,
    room: Room,
    is_room: bool,
    gain: f32,
    spread: f32,
    known: bool,
}

const CENTRE: Room = Room {
    x: 0.5,
    y: 0.5,
    z: 0.0,
};
const EMPTY_META: Meta = Meta {
    position: Position::new(0.0, 0.0, 0.0),
    room: CENTRE,
    is_room: true,
    gain: 1.0,
    spread: 0.0,
    known: false,
};

#[derive(Clone, Copy, Debug)]
struct Ramp {
    from: Room,
    start: u64,
    end: u64,
}
const NO_RAMP: Ramp = Ramp {
    from: CENTRE,
    start: 0,
    end: 0,
};

#[derive(Debug)]
pub(crate) struct Timeline {
    events: Vec<Event>,
    metadata: [Meta; MAX_OBJECTS],
    ramps: [Ramp; MAX_OBJECTS],
    clock: u64,
    codec: &'static str,
}

impl Timeline {
    pub(crate) fn new(codec: &'static str) -> Self {
        Self {
            events: Vec::with_capacity(MAX_EVENTS),
            metadata: [EMPTY_META; MAX_OBJECTS],
            ramps: [NO_RAMP; MAX_OBJECTS],
            clock: 0,
            codec,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.events.clear();
        self.metadata.fill(EMPTY_META);
        self.ramps.fill(NO_RAMP);
        self.clock = 0;
    }

    pub(crate) fn insert(&mut self, mut event: Event) -> Result<(), DecodeError> {
        if self.events.len() == self.events.capacity() {
            return Err(DecodeError::Unsupported {
                codec: self.codec,
                detail: "too many pending OAMD updates",
            });
        }
        event.sample += self.clock;
        let index = self.events.partition_point(|e| e.sample <= event.sample);
        self.events.insert(index, event);
        Ok(())
    }

    pub(crate) fn discontinuity(&mut self, offset: usize) -> Result<(), DecodeError> {
        let sample = self.clock + offset as u64;
        self.events.retain(|event| event.sample < sample);
        self.insert(Event {
            sample: offset as u64,
            ramp: 0,
            reset: true,
            objects: 0,
            updates: [EMPTY_UPDATE; MAX_OBJECTS],
        })
    }

    fn room_at(&self, index: usize, sample: u64) -> Room {
        let target = self.metadata[index].room;
        let ramp = self.ramps[index];
        if ramp.end <= sample || ramp.end <= ramp.start {
            return target;
        }
        let t = sample.saturating_sub(ramp.start) as f32 / (ramp.end - ramp.start) as f32;
        Room {
            x: ramp.from.x + (target.x - ramp.from.x) * t,
            y: ramp.from.y + (target.y - ramp.from.y) * t,
            z: ramp.from.z + (target.z - ramp.from.z) * t,
        }
    }

    fn apply(&mut self, event: Event) {
        if event.reset {
            self.metadata.fill(EMPTY_META);
            self.ramps.fill(NO_RAMP);
            return;
        }
        for (i, update) in event.updates.iter().enumerate().take(event.objects) {
            let from = self.room_at(i, event.sample);
            let slot = &mut self.metadata[i];
            if let Some(position) = update.speaker {
                slot.position = position;
                slot.known = true;
                slot.is_room = false;
                self.ramps[i] = NO_RAMP;
            } else if let Some(room) = update.room
                && (!update.differential || (slot.known && slot.is_room))
            {
                let was_known = slot.known && slot.is_room;
                // Deltas refer to the previous target, not the interpolated position.
                slot.room = if update.differential {
                    slot.room.plus(room)
                } else {
                    room.clamped()
                };
                slot.known = true;
                slot.is_room = true;
                self.ramps[i] = if was_known && event.ramp != 0 {
                    Ramp {
                        from,
                        start: event.sample,
                        end: event.sample + event.ramp,
                    }
                } else {
                    NO_RAMP
                };
            }
            if let Some(gain) = update.gain {
                slot.gain = gain;
            }
            if let Some(spread) = update.spread {
                slot.spread = spread;
            }
        }
    }

    /// Latest raw target, including queued updates, without consuming the queue.
    pub(crate) fn latest_room(&self, index: usize) -> Option<[f32; 3]> {
        let meta = self.metadata[index];
        let mut room = (meta.known && meta.is_room).then_some(meta.room);
        for event in &self.events {
            let update = event.updates[index];
            if event.reset || update.speaker.is_some() {
                room = None;
            } else if let Some(next) = update.room {
                room = if update.differential {
                    room.map(|old| old.plus(next))
                } else {
                    Some(next.clamped())
                };
            }
        }
        room.map(|r| [r.x, r.y, r.z])
    }

    pub(crate) fn emit(
        &mut self,
        buffers: &mut SceneBuffers,
        frames: usize,
        epoch: u32,
        rate: u32,
        qpc: u64,
        out: &mut dyn FnMut(&SceneBlock<'_>),
    ) {
        let end = self.clock + frames as u64;
        let mut sample = self.clock;
        let mut consumed = 0;
        while sample < end {
            while consumed < self.events.len() && self.events[consumed].sample <= sample {
                self.apply(self.events[consumed]);
                consumed += 1;
            }
            let mut next = self.events.get(consumed).map_or(end, |e| e.sample.min(end));
            for i in 0..buffers.objects() {
                let meta = self.metadata[i];
                let (position, spread) = if !meta.known {
                    (Position::default(), 1.0)
                } else if meta.is_room {
                    (self.room_at(i, sample).direction(), meta.spread)
                } else {
                    (meta.position, meta.spread)
                };
                buffers.set_object(i, position, meta.gain, spread);
                if self.ramps[i].end > sample {
                    next = next.min(self.ramps[i].end).min(sample + 256);
                }
            }
            buffers.emit_range(
                (sample - self.clock) as usize..(next - self.clock) as usize,
                epoch,
                rate,
                qpc,
                out,
            );
            sample = next;
        }
        self.events.drain(..consumed);
        self.clock = end;
    }
}
