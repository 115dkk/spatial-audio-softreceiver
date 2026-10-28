//! DAC-clocked ASIO entrance (ADR 0025).
//!
//! Platform code owns named mappings, events, process handles and MMCSS. This
//! module owns the platform-neutral streamring consumer, independent output
//! renderer, bounded rendered queue, DAC demand publication and status atomics.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use asio_proto::Lease;
pub use asio_proto::{ClientState, ControlMemory, Layout as AsioProtocolLayout};
use ssr_types::app::{AsioLayout, AsioSettings};
use ssr_types::{AsioStatus, Direction, Layout, OutputLayout, Speaker};

type WireLayout = AsioProtocolLayout;
pub use streamring::Signal;
use streamring::{Geometry, RegionMemory};

use crate::chain::{OutputProfile, PrepareError, Prepared};
use crate::limiter::Limiter;
use crate::mixer::BedBlock;
use crate::{MAX_OUTPUT_CHANNELS, MIX_FRAMES};

pub const MIN_ASIO_BUFFER_FRAMES: usize = 64;
pub const MAX_ASIO_BUFFER_FRAMES: usize = 2_048;
pub const ASIO_PARTITION_FRAMES: usize = 16;
const MAX_QUEUE_FRAMES: usize = 16_384;
const ASIO_POLL_WAIT: Duration = Duration::from_millis(20);

/// A client mapping opened by the platform adapter. `done` is used both for the
/// streamring ready notification and completion notification; both are hints,
/// while the atomic state and sequence words are authoritative.
pub struct AsioClient {
    pub memory: Arc<dyn RegionMemory>,
    pub geometry: Geometry,
    pub lease: Lease,
    pub executable: String,
    pub done: Arc<dyn Signal>,
}

impl std::fmt::Debug for AsioClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsioClient")
            .field("geometry", &self.geometry)
            .field("lease", &self.lease)
            .field("executable", &self.executable)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AsioTransportError {
    #[error("ASIO transport failed: {0}")]
    Transport(String),
}

/// Platform seam for the Windows host and Linux fake. Polling and opening are
/// non-real-time and every wait is bounded by the supplied timeout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputConfig {
    pub sample_rate: u32,
    pub dac_period_frames: u32,
    pub layout: AsioProtocolLayout,
    pub latency_frames: u32,
}

pub trait AsioTransport: Send + 'static {
    fn control_memory(&self) -> Arc<dyn ControlMemory>;
    fn demand_signal(&self) -> Arc<dyn Signal>;
    fn configure(&mut self, config: OutputConfig) -> Result<(), AsioTransportError>;
    fn set_ready(&mut self, ready: bool) -> Result<(), AsioTransportError>;
    fn poll_client(&mut self, timeout: Duration) -> Result<Option<AsioClient>, AsioTransportError>;
    fn wait_client(
        &mut self,
        timeout: Duration,
    ) -> Result<asio_proto::ClientState, AsioTransportError>;
}

/// Creates the thread-owned transport. The provider is cloneable inside
/// [`crate::EngineConfig`], while the transport itself need only be `Send`.
pub trait AsioTransportProvider: Send + Sync + 'static {
    fn open(&self) -> Result<Box<dyn AsioTransport>, AsioTransportError>;
}

impl<F> AsioTransportProvider for F
where
    F: Fn() -> Result<Box<dyn AsioTransport>, AsioTransportError> + Send + Sync + 'static,
{
    fn open(&self) -> Result<Box<dyn AsioTransport>, AsioTransportError> {
        self()
    }
}

#[derive(Clone)]
pub struct AsioRuntimeConfig {
    pub provider: Arc<dyn AsioTransportProvider>,
    pub profile: OutputProfile,
    /// Rendered queue lead before ASIO becomes audible. Its capacity is this
    /// lead plus two DAW buffers.
    pub target_lead_frames: u32,
}

impl std::fmt::Debug for AsioRuntimeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsioRuntimeConfig")
            .field("profile", &self.profile)
            .field("target_lead_frames", &self.target_lead_frames)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AsioPrepareError {
    #[error(transparent)]
    Output(#[from] PrepareError),
    #[error("ASIO sample rate must be at least 8 kHz")]
    SampleRate,
    #[error("ASIO layout contains an unpositioned non-LFE channel")]
    Position,
}

/// Prepares a second output back from the same profile at the DAC rate. Its DSP
/// state is entirely independent from the 48 kHz system chain.
pub fn prepare(
    profile: &OutputProfile,
    sample_rate: u32,
    layout: AsioLayout,
) -> Result<AsioRenderer, AsioPrepareError> {
    if sample_rate < 8_000 {
        return Err(AsioPrepareError::SampleRate);
    }
    let prepared =
        profile.prepare_at(sample_rate, ASIO_PARTITION_FRAMES, MAX_ASIO_BUFFER_FRAMES)?;
    AsioRenderer::new(Box::new(prepared), layout)
}

/// Maps the protocol-neutral channel enum to the engine's canonical speaker.
pub const fn speaker(channel: asio_proto::Channel) -> Speaker {
    use asio_proto::Channel as A;
    match channel {
        A::L => Speaker::L,
        A::R => Speaker::R,
        A::C => Speaker::C,
        A::Lfe => Speaker::Lfe,
        A::Ls => Speaker::Lrs,
        A::Rs => Speaker::Rrs,
        A::Ls110 => Speaker::Ls,
        A::Rs110 => Speaker::Rs,
        A::Lrs => Speaker::Lrs,
        A::Rrs => Speaker::Rrs,
        A::Lss => Speaker::Lss,
        A::Rss => Speaker::Rss,
        A::Lw => Speaker::Lw,
        A::Rw => Speaker::Rw,
        A::Tfl => Speaker::Tfl,
        A::Tfr => Speaker::Tfr,
        A::Tml | A::Tsl => Speaker::Tsl,
        A::Tmr | A::Tsr => Speaker::Tsr,
        A::Trl => Speaker::Trl,
        A::Trr => Speaker::Trr,
        A::Tc => Speaker::Tc,
        A::Tfc => Speaker::Tfc,
        A::Lbs => Speaker::Lbs,
        A::Rbs => Speaker::Rbs,
        A::Cs => Speaker::Cs,
        A::Lfe2 => Speaker::Lfe2,
        A::Trc => Speaker::Trc,
        A::Bfc => Speaker::Bfc,
        A::Bfl => Speaker::Bfl,
        A::Bfr => Speaker::Bfr,
        A::Lc => Speaker::Lc,
        A::Rc => Speaker::Rc,
        A::Lsc => Speaker::Lsc,
        A::Rsc => Speaker::Rsc,
        A::Ls1 => Speaker::Ls1,
        A::Rs1 => Speaker::Rs1,
        A::Lrs1 => Speaker::Lrs1,
        A::Rrs1 => Speaker::Rrs1,
        A::Lcs => Speaker::Lcs,
        A::Rcs => Speaker::Rcs,
        A::Hfl => Speaker::Hfl,
        A::Hfr => Speaker::Hfr,
        A::Hrl => Speaker::Hrl,
        A::Hrr => Speaker::Hrr,
    }
}

const fn is_lfe(channel: asio_proto::Channel) -> bool {
    matches!(
        channel,
        asio_proto::Channel::Lfe | asio_proto::Channel::Lfe2
    )
}

pub const fn wire_layout(layout: AsioLayout) -> WireLayout {
    match layout {
        AsioLayout::Stereo => WireLayout::Stereo,
        AsioLayout::Surround51 => WireLayout::Surround51,
        AsioLayout::Surround71 => WireLayout::Surround71,
        AsioLayout::Surround714 => WireLayout::Surround714,
        AsioLayout::Surround91 => WireLayout::Surround91,
        AsioLayout::Surround914 => WireLayout::Surround914,
        AsioLayout::Surround916 => WireLayout::Surround916,
        AsioLayout::Surround131 => WireLayout::Surround131,
        AsioLayout::Surround222 => WireLayout::Surround222,
        AsioLayout::Surround24110 => WireLayout::Surround24110,
        AsioLayout::Surround302 => WireLayout::Surround302,
    }
}

pub const fn app_layout(layout: WireLayout) -> AsioLayout {
    match layout {
        WireLayout::Stereo => AsioLayout::Stereo,
        WireLayout::Surround51 => AsioLayout::Surround51,
        WireLayout::Surround71 => AsioLayout::Surround71,
        WireLayout::Surround714 => AsioLayout::Surround714,
        WireLayout::Surround91 => AsioLayout::Surround91,
        WireLayout::Surround914 => AsioLayout::Surround914,
        WireLayout::Surround916 => AsioLayout::Surround916,
        WireLayout::Surround131 => AsioLayout::Surround131,
        WireLayout::Surround222 => AsioLayout::Surround222,
        WireLayout::Surround24110 => AsioLayout::Surround24110,
        WireLayout::Surround302 => AsioLayout::Surround302,
    }
}

/// Independent renderer with a fixed 16-frame hop. Arbitrary accepted DAW
/// buffers are reblocked by [`AsioReblocker`] without allocation.
#[derive(Debug)]
pub struct AsioRenderer {
    prepared: Box<Prepared>,
    layout: WireLayout,
    matrix: Vec<f32>,
    input_channels: usize,
    directions: usize,
    bed: Vec<f32>,
    lfe: [f32; ASIO_PARTITION_FRAMES],
    lfe_gain: f32,
    limiter: Limiter,
}

impl AsioRenderer {
    fn new(mut prepared: Box<Prepared>, layout: AsioLayout) -> Result<Self, AsioPrepareError> {
        let wire = wire_layout(layout);
        let input_channels = wire.channels() as usize;
        let directions = prepared.direction_layout().len();
        let mut matrix = vec![0.0; input_channels * directions];
        let mut gains = vec![0.0; directions];
        for (input, &channel) in wire.channel_order().iter().enumerate() {
            let source = speaker(channel);
            if is_lfe(channel) {
                continue;
            }
            let position = source.position().ok_or(AsioPrepareError::Position)?;
            prepared.geometry().gains(position, 0.0, &mut gains);
            matrix[input * directions..(input + 1) * directions].copy_from_slice(&gains);
        }
        let mut limiter = Limiter::new();
        let (gain, ceiling) = prepared.gain_and_ceiling();
        limiter.reset(prepared.sample_rate(), gain, ceiling);
        prepared.reset_state();
        Ok(Self {
            prepared,
            layout: wire,
            matrix,
            input_channels,
            directions,
            bed: vec![0.0; directions * ASIO_PARTITION_FRAMES],
            lfe: [0.0; ASIO_PARTITION_FRAMES],
            lfe_gain: 1.0,
            limiter,
        })
    }

    pub fn input_channels(&self) -> usize {
        self.input_channels
    }

    pub fn output_layout(&self) -> OutputLayout {
        self.prepared.layout()
    }

    pub fn sample_rate(&self) -> u32 {
        self.prepared.sample_rate()
    }

    pub fn algorithm_latency_frames(&self) -> u32 {
        self.prepared.algorithm_latency_frames() as u32
    }

    pub fn clear(&mut self) {
        self.prepared.reset_state();
        self.bed.fill(0.0);
        self.lfe.fill(0.0);
        let (gain, ceiling) = self.prepared.gain_and_ceiling();
        self.limiter.reset(self.sample_rate(), gain, ceiling);
    }

    /// Renders one full 16-frame planar input hop into planar output.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        debug_assert_eq!(input.len(), self.input_channels * ASIO_PARTITION_FRAMES);
        debug_assert_eq!(
            output.len(),
            self.prepared.channels() * ASIO_PARTITION_FRAMES
        );
        self.bed.fill(0.0);
        self.lfe.fill(0.0);
        for input_channel in 0..self.input_channels {
            let source = &input[input_channel * ASIO_PARTITION_FRAMES
                ..(input_channel + 1) * ASIO_PARTITION_FRAMES];
            if is_lfe(self.layout.channel_order()[input_channel]) {
                for (destination, sample) in self.lfe.iter_mut().zip(source) {
                    if sample.is_finite() {
                        *destination += *sample * self.lfe_gain;
                    }
                }
                continue;
            }
            for direction in 0..self.directions {
                let gain = self.matrix[input_channel * self.directions + direction];
                if gain == 0.0 {
                    continue;
                }
                let destination = &mut self.bed
                    [direction * ASIO_PARTITION_FRAMES..(direction + 1) * ASIO_PARTITION_FRAMES];
                for (destination, source) in destination.iter_mut().zip(source) {
                    if source.is_finite() {
                        *destination += *source * gain;
                    }
                }
            }
        }
        let bed = BedBlock {
            epoch: 0,
            rate: self.sample_rate(),
            frames: ASIO_PARTITION_FRAMES,
            source_qpc: 0,
            directions: self.directions,
            channels: &self.bed,
            lfe: Some(&self.lfe),
            silent: input.iter().all(|sample| *sample == 0.0),
            idle: false,
        };
        self.prepared.process_bed(&bed, output);
        self.limiter
            .process(output, self.prepared.channels(), ASIO_PARTITION_FRAMES);
    }
}

/// Preallocated arbitrary-buffer to fixed-hop adapter.
#[derive(Debug)]
pub struct AsioReblocker {
    input_channels: usize,
    pending: usize,
    input: Vec<f32>,
    output: Vec<f32>,
}

impl AsioReblocker {
    pub fn new(input_channels: usize, output_channels: usize) -> Self {
        Self {
            input_channels,
            pending: 0,
            input: vec![0.0; input_channels * ASIO_PARTITION_FRAMES],
            output: vec![0.0; output_channels * ASIO_PARTITION_FRAMES],
        }
    }

    pub fn clear(&mut self) {
        self.pending = 0;
        self.input.fill(0.0);
        self.output.fill(0.0);
    }

    /// Consumes planar input and emits interleaved complete output hops. A final
    /// partial hop remains buffered for the next DAW callback.
    pub fn process(
        &mut self,
        renderer: &mut AsioRenderer,
        planar: &[f32],
        frames: usize,
        mut emit: impl FnMut(&[f32], usize) -> bool,
    ) -> bool {
        debug_assert_eq!(planar.len(), self.input_channels * frames);
        let mut offset = 0;
        while offset < frames {
            let take = (ASIO_PARTITION_FRAMES - self.pending).min(frames - offset);
            for channel in 0..self.input_channels {
                let source = &planar[channel * frames + offset..channel * frames + offset + take];
                let start = channel * ASIO_PARTITION_FRAMES + self.pending;
                for (destination, sample) in self.input[start..start + take].iter_mut().zip(source)
                {
                    *destination = if sample.is_finite() { *sample } else { 0.0 };
                }
            }
            self.pending += take;
            offset += take;
            if self.pending == ASIO_PARTITION_FRAMES {
                renderer.process(&self.input, &mut self.output);
                if !emit(&self.output, ASIO_PARTITION_FRAMES) {
                    return false;
                }
                self.pending = 0;
            }
        }
        true
    }
}

/// Lock-free SPSC rendered queue. Samples are interleaved so the output thread
/// can add them directly to the sink period.
#[derive(Debug)]
pub struct AsioQueue {
    channels: AtomicU32,
    capacity: usize,
    samples: Box<[AtomicU32]>,
    logical_capacity: AtomicU32,
    written: AtomicU64,
    read: AtomicU64,
    generation: AtomicU64,
    frame_generations: Box<[AtomicU64]>,
}

impl AsioQueue {
    pub fn new(channels: usize, capacity_frames: usize) -> Self {
        assert!((1..=MAX_OUTPUT_CHANNELS).contains(&channels));
        let capacity = capacity_frames.clamp(1, MAX_QUEUE_FRAMES);
        Self {
            channels: AtomicU32::new(channels as u32),
            capacity,
            samples: (0..MAX_OUTPUT_CHANNELS * capacity)
                .map(|_| AtomicU32::new(0))
                .collect(),
            logical_capacity: AtomicU32::new(capacity as u32),
            written: AtomicU64::new(0),
            read: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            frame_generations: (0..capacity).map(|_| AtomicU64::new(0)).collect(),
        }
    }

    pub fn channels(&self) -> usize {
        self.channels.load(Ordering::Acquire) as usize
    }

    fn configure(&self, channels: usize, limit: usize) {
        self.channels.store(channels as u32, Ordering::Release);
        self.logical_capacity
            .store(limit.min(self.capacity) as u32, Ordering::Release);
    }

    pub fn available_frames(&self) -> usize {
        (self.logical_capacity.load(Ordering::Acquire) as usize)
            .saturating_sub(self.queued_frames())
    }

    pub fn queued_frames(&self) -> usize {
        self.written
            .load(Ordering::Acquire)
            .saturating_sub(self.read.load(Ordering::Acquire))
            .min(self.capacity as u64) as usize
    }

    pub fn push_planar(&self, samples: &[f32], frames: usize) -> bool {
        self.push_planar_epoch(samples, frames, self.generation())
    }

    /// The producer captures this epoch BEFORE DSP, not when publishing its result.
    pub fn push_planar_epoch(&self, samples: &[f32], frames: usize, generation: u64) -> bool {
        let channels = self.channels();
        if samples.len() != channels * frames {
            return false;
        }
        let written = self.written.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        if written.saturating_sub(read).saturating_add(frames as u64)
            > u64::from(self.logical_capacity.load(Ordering::Acquire))
        {
            return false;
        }
        for frame in 0..frames {
            let slot = (written as usize + frame) % self.capacity;
            for channel in 0..channels {
                self.samples[slot * MAX_OUTPUT_CHANNELS + channel].store(
                    samples[channel * frames + frame].to_bits(),
                    Ordering::Relaxed,
                );
            }
            self.frame_generations[slot].store(generation, Ordering::Release);
        }
        self.written
            .store(written + frames as u64, Ordering::Release);
        true
    }

    pub fn pop_add(&self, output: &mut [f32], frames: usize, gain: f32) -> usize {
        let channels = self.channels();
        debug_assert_eq!(output.len(), frames * channels);
        self.discard_stale();
        let read = self.read.load(Ordering::Relaxed);
        let written = self.written.load(Ordering::Acquire);
        let take = frames.min(written.saturating_sub(read) as usize);
        let generation = self.generation();
        let mut audible = 0;
        for frame in 0..take {
            let slot = (read as usize + frame) % self.capacity;
            if self.frame_generations[slot].load(Ordering::Acquire) != generation {
                break;
            }
            for channel in 0..channels {
                let sample = f32::from_bits(
                    self.samples[slot * MAX_OUTPUT_CHANNELS + channel].load(Ordering::Relaxed),
                );
                output[frame * channels + channel] += sample * gain;
            }
            audible += 1;
        }
        self.read.store(read + audible as u64, Ordering::Release);
        audible
    }

    /// Only the output consumer calls this. The worker never moves `read`.
    pub fn clear(&self, generation: u64) {
        self.generation.store(generation, Ordering::Release);
        let written = self.written.load(Ordering::Acquire);
        self.read.store(written, Ordering::Release);
    }

    /// Bounded by a single acquired writer snapshot, even with a late producer.
    fn discard_stale(&self) {
        let mut read = self.read.load(Ordering::Relaxed);
        let written = self.written.load(Ordering::Acquire);
        let generation = self.generation();
        for _ in 0..self.capacity {
            if read == written
                || self.frame_generations[read as usize % self.capacity].load(Ordering::Acquire)
                    == generation
            {
                break;
            }
            read += 1;
        }
        self.read.store(read, Ordering::Release);
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

/// Atomics shared by the ASIO worker, output thread and control-thread status.
#[derive(Debug)]
pub struct AsioShared {
    active: AtomicBool,
    client_pid: AtomicU32,
    sample_rate: AtomicU32,
    buffer_frames: AtomicU32,
    channels: AtomicU32,
    layout: AtomicU32,
    underruns: AtomicU64,
    generation: AtomicU64,
    clock_fault: AtomicBool,
    consumed_frames: AtomicU64,
    demand_origin: AtomicU64,
    target_lead_frames: AtomicU32,
    settings: AtomicU64,
    executable: std::sync::Mutex<String>,
    fault_reason: std::sync::Mutex<Option<String>>,
    reset_request: AtomicU64,
    reset_applied: AtomicU64,
    device_rate: AtomicU32,
    device_channels: AtomicU32,
    device_mask: AtomicU32,
    desired_layout: AtomicU64,
    device_period: AtomicU32,
    device_latency: AtomicU32,
    output_online: AtomicBool,
    profile: std::sync::Mutex<Option<OutputProfile>>,
    prepared: std::sync::Mutex<Option<AsioRenderer>>,
    profile_pending: AtomicU64,
    gain_bits: AtomicU32,
    lfe_bits: AtomicU32,
}

impl AsioShared {
    pub fn new(settings: AsioSettings) -> Self {
        Self {
            active: AtomicBool::new(false),
            client_pid: AtomicU32::new(0),
            sample_rate: AtomicU32::new(0),
            buffer_frames: AtomicU32::new(0),
            channels: AtomicU32::new(0),
            layout: AtomicU32::new(wire_layout(settings.layout) as u32),
            underruns: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            clock_fault: AtomicBool::new(false),
            consumed_frames: AtomicU64::new(0),
            demand_origin: AtomicU64::new(0),
            target_lead_frames: AtomicU32::new(0),
            settings: AtomicU64::new(pack_settings(settings)),
            executable: std::sync::Mutex::new(String::new()),
            fault_reason: std::sync::Mutex::new(None),
            reset_request: AtomicU64::new(0),
            reset_applied: AtomicU64::new(0),
            device_rate: AtomicU32::new(0),
            device_channels: AtomicU32::new(0),
            device_mask: AtomicU32::new(0),
            desired_layout: AtomicU64::new(0),
            device_period: AtomicU32::new(0),
            device_latency: AtomicU32::new(0),
            output_online: AtomicBool::new(false),
            profile: std::sync::Mutex::new(None),
            prepared: std::sync::Mutex::new(None),
            profile_pending: AtomicU64::new(0),
            gain_bits: AtomicU32::new(f32::NAN.to_bits()),
            lfe_bits: AtomicU32::new(f32::NAN.to_bits()),
        }
    }

    pub fn set_settings(&self, settings: AsioSettings) {
        let previous = self.settings();
        self.settings
            .store(pack_settings(settings), Ordering::Release);
        if previous.enabled != settings.enabled || previous.layout != settings.layout {
            self.request_reset();
        }
    }

    pub(crate) fn request_reset(&self) {
        self.reset_request.fetch_add(1, Ordering::AcqRel);
    }

    /// A queued profile is not installed yet or the output has not applied a
    /// requested reset. Either moves the DAC generation again.
    fn generation_pending(&self) -> bool {
        self.profile_pending.load(Ordering::Acquire) != 0
            || self.reset_applied.load(Ordering::Acquire)
                != self.reset_request.load(Ordering::Acquire)
    }

    pub(crate) fn current_profile(&self) -> Option<OutputProfile> {
        self.profile
            .lock()
            .expect("ASIO profile mutex poisoned")
            .clone()
    }

    pub(crate) fn set_profile(&self, profile: OutputProfile) {
        *self.profile.lock().expect("ASIO profile mutex poisoned") = Some(profile);
        self.request_reset();
    }

    pub(crate) fn queue_profile(
        &self,
        profile: OutputProfile,
        layout: OutputLayout,
        prepared: AsioRenderer,
    ) {
        *self.prepared.lock().expect("ASIO prepared mutex poisoned") = Some(prepared);
        self.profile_pending.fetch_add(1, Ordering::AcqRel);
        self.desired_layout.store(
            u64::from(layout.channels) | (u64::from(layout.channel_mask) << 32),
            Ordering::Release,
        );
        self.set_profile(profile);
    }

    pub(crate) fn desired_layout(&self) -> Option<OutputLayout> {
        let packed = self.desired_layout.load(Ordering::Acquire);
        (packed != 0 && self.profile_pending.load(Ordering::Acquire) == 0).then_some(OutputLayout {
            channels: packed as u16,
            channel_mask: (packed >> 32) as u32,
        })
    }

    pub(crate) fn profile_installed(&self) {
        if self.profile_pending.load(Ordering::Acquire) != 0 {
            self.profile_pending.fetch_sub(1, Ordering::AcqRel);
            self.request_reset();
        }
    }

    pub(crate) fn set_gain(&self, db: f32) {
        self.gain_bits.store(db.to_bits(), Ordering::Release);
    }

    pub(crate) fn set_lfe_gain(&self, db: f32) {
        self.lfe_bits.store(db.to_bits(), Ordering::Release);
    }

    pub fn settings(&self) -> AsioSettings {
        unpack_settings(self.settings.load(Ordering::Acquire))
    }

    pub fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub fn begin_client(
        &self,
        lease: Lease,
        executable: String,
        rate: u32,
        buffer_frames: u32,
        layout: WireLayout,
    ) {
        if let Ok(mut name) = self.executable.lock() {
            *name = executable;
        }
        self.client_pid.store(lease.pid, Ordering::Relaxed);
        self.sample_rate.store(rate, Ordering::Relaxed);
        self.buffer_frames.store(buffer_frames, Ordering::Relaxed);
        self.channels.store(layout.channels(), Ordering::Relaxed);
        self.layout.store(layout as u32, Ordering::Relaxed);
        self.active.store(true, Ordering::Release);
    }

    pub fn end_client(&self) {
        self.active.store(false, Ordering::Release);
        self.client_pid.store(0, Ordering::Release);
        if let Ok(mut name) = self.executable.lock() {
            name.clear();
        }
    }

    pub fn next_generation(&self) -> u64 {
        self.generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn publish_dac_period(&self, frames: usize) -> u64 {
        self.consumed_frames
            .fetch_add(frames as u64, Ordering::AcqRel)
            .wrapping_add(frames as u64)
    }

    pub fn demand(&self) -> u64 {
        self.consumed_frames
            .load(Ordering::Acquire)
            .saturating_sub(self.demand_origin.load(Ordering::Acquire))
            + u64::from(self.target_lead_frames.load(Ordering::Acquire))
    }

    pub fn set_target_lead(&self, frames: u32) {
        self.target_lead_frames.store(frames, Ordering::Release);
    }

    pub fn note_underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_clock_fault(&self, fault: bool) {
        self.clock_fault.store(fault, Ordering::Release);
    }

    pub(crate) fn set_fault_reason(&self, reason: Option<String>) {
        *self.fault_reason.lock().expect("ASIO fault mutex poisoned") = reason;
    }

    pub fn status(&self) -> Option<AsioStatus> {
        let fault_reason = self.fault_reason.lock().ok()?.clone();
        if !self.active() && fault_reason.is_none() {
            return None;
        }
        let executable = self.executable.lock().ok()?.clone();
        Some(AsioStatus {
            client_pid: self.client_pid.load(Ordering::Acquire),
            client_exe: executable,
            sample_rate: self.sample_rate.load(Ordering::Acquire),
            buffer_frames: self.buffer_frames.load(Ordering::Acquire),
            channels: self.channels.load(Ordering::Acquire),
            layout: app_layout(
                WireLayout::from_wire(self.layout.load(Ordering::Acquire))
                    .unwrap_or(WireLayout::Surround714),
            ),
            underruns: self.underruns.load(Ordering::Acquire),
            generation: self.generation(),
            clock_fault: self.clock_fault.load(Ordering::Acquire) || fault_reason.is_some(),
            fault_reason,
        })
    }
}

fn pack_settings(settings: AsioSettings) -> u64 {
    u64::from(settings.enabled)
        | ((wire_layout(settings.layout) as u64) << 1)
        | (u64::from(settings.system_attenuation_db.to_bits()) << 8)
}

fn unpack_settings(value: u64) -> AsioSettings {
    AsioSettings {
        enabled: value & 1 != 0,
        layout: app_layout(WireLayout::from_wire(((value >> 1) & 0x7f) as u32).unwrap_or_default()),
        system_attenuation_db: f32::from_bits((value >> 8) as u32),
    }
}

/// Output-thread side of the entrance. It advances demand on every DAC period,
/// even on underrun, primes to the configured lead, mixes the system and ASIO
/// paths, then applies one linked safety limiter.
pub struct AsioOutput {
    pub queue: Arc<AsioQueue>,
    pub shared: Arc<AsioShared>,
    control: Arc<dyn ControlMemory>,
    demand_signal: Arc<dyn Signal>,
    primed: bool,
    limiter: Limiter,
    rate: u32,
    last_lease: Lease,
    was_running: bool,
    run_serial: u64,
    had_clock_fault: bool,
}

impl std::fmt::Debug for AsioOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsioOutput")
            .field("queue", &self.queue)
            .field("shared", &self.shared)
            .field("primed", &self.primed)
            .field("limiter", &self.limiter)
            .finish_non_exhaustive()
    }
}

impl AsioOutput {
    pub fn new(
        queue: Arc<AsioQueue>,
        shared: Arc<AsioShared>,
        control: Arc<dyn ControlMemory>,
        demand_signal: Arc<dyn Signal>,
        rate: u32,
    ) -> Self {
        let mut limiter = Limiter::new();
        limiter.reset(rate, 1.0, 1.0);
        Self {
            queue,
            shared,
            control,
            demand_signal,
            primed: false,
            limiter,
            rate,
            last_lease: asio_proto::Lease::VACANT,
            was_running: false,
            run_serial: 0,
            had_clock_fault: false,
        }
    }

    pub fn reset(&mut self, rate: u32) {
        let request = self.shared.reset_request.load(Ordering::Acquire);
        let control = self.control.control();
        control.set_ready(false);
        self.rate = rate;
        self.shared.device_rate.store(rate, Ordering::Release);
        self.shared.demand_origin.store(
            self.shared.consumed_frames.load(Ordering::Acquire),
            Ordering::Release,
        );
        control.set_demand(u64::from(
            self.shared.target_lead_frames.load(Ordering::Acquire),
        ));
        let generation = self.shared.next_generation();
        self.queue.clear(generation);
        control.set_dac_generation(generation);
        self.shared.reset_applied.store(request, Ordering::Release);
        self.demand_signal.signal();
        self.primed = false;
        self.limiter.reset(rate, 1.0, 1.0);
    }

    pub(crate) fn reconfigure(&mut self, format: ssr_types::OutputFormat, latency: u32) {
        self.shared.output_online.store(false, Ordering::Release);
        self.shared
            .device_channels
            .store(u32::from(format.channels), Ordering::Release);
        self.shared
            .device_mask
            .store(format.channel_mask, Ordering::Release);
        self.shared
            .device_period
            .store(format.period_frames as u32, Ordering::Release);
        self.shared.device_latency.store(latency, Ordering::Release);
        self.reset(format.rate);
        self.shared.output_online.store(true, Ordering::Release);
    }

    pub(crate) fn latency(&mut self, frames: u32) {
        self.shared.device_latency.store(frames, Ordering::Release);
    }

    pub(crate) fn lost(&mut self) {
        self.shared.output_online.store(false, Ordering::Release);
        self.reset(self.rate);
    }

    pub fn mix_period(&mut self, output: &mut [f32], channels: usize) {
        let frames = output.len() / channels;
        let lease = self.control.control().lease();
        let running = self.control.control().client_running() && !lease.is_vacant();
        let request = self.shared.reset_request.load(Ordering::Acquire);
        let run_serial = self.control.control().client_run_serial();
        let clock_fault = self.control.control().clock_fault();
        let clock_stalled = clock_fault && !self.had_clock_fault;
        self.had_clock_fault = clock_fault;
        if lease != self.last_lease
            || running != self.was_running
            || run_serial != self.run_serial
            || clock_stalled
            || request != self.shared.reset_applied.load(Ordering::Acquire)
        {
            self.last_lease = lease;
            self.was_running = running;
            self.run_serial = run_serial;
            self.reset(self.rate);
        }
        self.queue.discard_stale();
        let control = self.control.control();
        self.shared.set_clock_fault(control.clock_fault());
        // No DAW uptime catch-up: before start the demand is exactly the lead.
        if running && control.ready() {
            self.shared.publish_dac_period(frames);
        }
        control.set_demand(self.shared.demand());
        self.demand_signal.signal();
        let settings = self.shared.settings();
        if !running || !settings.enabled {
            return;
        }
        // Attenuation follows Running, including priming/reset silence.
        let system_gain = if settings.system_attenuation_db >= 60.0 {
            0.0
        } else {
            10.0_f32.powf(-settings.system_attenuation_db.clamp(0.0, 60.0) / 20.0)
        };
        if system_gain != 1.0 {
            for sample in output.iter_mut() {
                *sample *= system_gain;
            }
        }
        if !self.shared.active()
            || !control.ready()
            || control.clock_fault()
            || self.shared.profile_pending.load(Ordering::Acquire) != 0
            || self.shared.reset_request.load(Ordering::Acquire)
                != self.shared.reset_applied.load(Ordering::Acquire)
            || control.client_generation_ack() != control.dac_generation()
            || self.queue.channels() != channels
        {
            return;
        }
        let lead = self.shared.target_lead_frames.load(Ordering::Acquire) as usize;
        if !self.primed {
            if self.queue.queued_frames() < lead {
                return;
            }
            self.primed = true;
        }
        let taken = self.queue.pop_add(output, frames, 1.0);
        if taken < frames {
            self.shared.note_underrun();
            // pop_add leaves only the missing ASIO contribution silent. Keep the
            // renderer and generation: a scheduling underrun is not a DAC restart.
        }
        self.limiter.process_interleaved(output, channels);
    }
}

pub struct AsioRuntime {
    pub queue: Arc<AsioQueue>,
    output: Option<AsioOutput>,
    thread: Option<std::thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

impl std::fmt::Debug for AsioRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsioRuntime")
            .field("queue", &self.queue)
            .field("output", &self.output)
            .field("running", &self.thread.is_some())
            .finish_non_exhaustive()
    }
}

impl AsioRuntime {
    pub fn start(
        config: AsioRuntimeConfig,
        settings: AsioSettings,
        shared: Arc<AsioShared>,
        device_rate: u32,
        dac_period_frames: usize,
        rt_scope: Option<crate::RtScope>,
        thread_hook: Option<crate::ThreadHook>,
    ) -> Result<Self, AsioTransportError> {
        Self::start_with_latency(
            config,
            settings,
            shared,
            device_rate,
            dac_period_frames,
            dac_period_frames as u32,
            rt_scope,
            thread_hook,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_with_latency(
        config: AsioRuntimeConfig,
        settings: AsioSettings,
        shared: Arc<AsioShared>,
        device_rate: u32,
        dac_period_frames: usize,
        dac_latency_frames: u32,
        rt_scope: Option<crate::RtScope>,
        thread_hook: Option<crate::ThreadHook>,
    ) -> Result<Self, AsioTransportError> {
        if config.target_lead_frames as usize > MAX_QUEUE_FRAMES - 2 * MAX_ASIO_BUFFER_FRAMES {
            return Err(AsioTransportError::Transport(
                "ASIO target lead exceeds queue capacity".into(),
            ));
        }
        let mut transport = config.provider.open()?;
        let control_memory = transport.control_memory();
        shared
            .generation
            .store(control_memory.control().dac_generation(), Ordering::Release);
        let demand_signal = transport.demand_signal();
        let wire = wire_layout(settings.layout);
        let capacity = usize::try_from(config.target_lead_frames)
            .unwrap_or(MAX_QUEUE_FRAMES)
            .saturating_add(2 * MAX_ASIO_BUFFER_FRAMES)
            .min(MAX_QUEUE_FRAMES);
        let prepared = prepare(&config.profile, device_rate, settings.layout)
            .map_err(|error| AsioTransportError::Transport(error.to_string()))?;
        if prepared.output_layout().channels == 0 {
            return Err(AsioTransportError::Transport(
                "ASIO output profile has no channels".into(),
            ));
        }
        let queue = Arc::new(AsioQueue::new(
            usize::from(prepared.output_layout().channels),
            capacity,
        ));
        shared.set_target_lead(config.target_lead_frames);
        transport.configure(OutputConfig {
            sample_rate: device_rate,
            dac_period_frames: dac_period_frames as u32,
            layout: wire,
            latency_frames: config
                .target_lead_frames
                .saturating_add((ASIO_PARTITION_FRAMES - 1) as u32)
                .saturating_add(dac_latency_frames),
        })?;
        shared.device_rate.store(device_rate, Ordering::Release);
        shared.device_channels.store(
            u32::from(prepared.output_layout().channels),
            Ordering::Release,
        );
        shared
            .device_mask
            .store(prepared.output_layout().channel_mask, Ordering::Release);
        shared
            .device_period
            .store(dac_period_frames as u32, Ordering::Release);
        shared
            .device_latency
            .store(dac_latency_frames, Ordering::Release);
        shared.output_online.store(true, Ordering::Release);
        let mut output = AsioOutput::new(
            Arc::clone(&queue),
            Arc::clone(&shared),
            Arc::clone(&control_memory),
            Arc::clone(&demand_signal),
            device_rate,
        );
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_queue = Arc::clone(&queue);
        let worker_shared = Arc::clone(&shared);
        shared.set_profile(config.profile);
        *shared
            .prepared
            .lock()
            .expect("ASIO prepared mutex poisoned") = Some(prepared);
        output.reset(device_rate);
        let thread = std::thread::Builder::new()
            .name("ssr-asio".into())
            .spawn(move || {
                if let Some(hook) = &thread_hook {
                    hook(ssr_types::EngineThread::Output);
                }
                run_worker(
                    transport.as_mut(),
                    &worker_queue,
                    &worker_shared,
                    &control_memory,
                    rt_scope.as_ref(),
                    &worker_stop,
                );
                let _ = transport.set_ready(false);
                worker_shared.end_client();
            })
            .map_err(|error| AsioTransportError::Transport(error.to_string()))?;
        Ok(Self {
            queue,
            output: Some(output),
            thread: Some(thread),
            stop,
        })
    }

    pub fn take_output(&mut self) -> Option<AsioOutput> {
        self.output.take()
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

impl Drop for AsioRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

fn configured_renderer(
    transport: &mut dyn AsioTransport,
    shared: &AsioShared,
    reuse: &mut Option<AsioRenderer>,
) -> Result<Option<AsioRenderer>, AsioTransportError> {
    if !shared.output_online.load(Ordering::Acquire) || shared.generation_pending() {
        return Ok(None);
    }
    let settings = shared.settings();
    if !settings.enabled {
        return Ok(None);
    }
    let profile = shared
        .profile
        .lock()
        .expect("ASIO profile mutex poisoned")
        .clone();
    let Some(profile) = profile else {
        return Ok(None);
    };
    let rate = shared.device_rate.load(Ordering::Acquire);
    let cached = shared
        .prepared
        .lock()
        .expect("ASIO prepared mutex poisoned")
        .take();
    // Only the returns above keep the caller's renderer. From here it is
    // spent, and a queued profile's renderer replaces it.
    let reused = reuse.take();
    let mut renderer = match cached.or(reused) {
        Some(renderer)
            if renderer.sample_rate() == rate
                && renderer.input_channels()
                    == wire_layout(settings.layout).channels() as usize =>
        {
            renderer
        }
        _ => prepare(&profile, rate, settings.layout)
            .map_err(|error| AsioTransportError::Transport(error.to_string()))?,
    };
    if u32::from(renderer.output_layout().channels)
        != shared.device_channels.load(Ordering::Acquire)
        || renderer.output_layout().channel_mask != shared.device_mask.load(Ordering::Acquire)
    {
        return Ok(None);
    }
    renderer.clear();
    transport.configure(OutputConfig {
        sample_rate: rate,
        dac_period_frames: shared.device_period.load(Ordering::Acquire),
        layout: wire_layout(settings.layout),
        // The reblocker can hold up to 15 frames; the output algorithms are
        // zero-latency partitioned (their FIR delay is part of the profile).
        latency_frames: shared
            .target_lead_frames
            .load(Ordering::Acquire)
            .saturating_add((ASIO_PARTITION_FRAMES - 1) as u32)
            .saturating_add(shared.device_latency.load(Ordering::Acquire)),
    })?;
    Ok(Some(renderer))
}

#[derive(Debug)]
struct PrepareRetry {
    due: std::time::Instant,
    delay: Duration,
}
impl PrepareRetry {
    fn new() -> Self {
        Self {
            due: std::time::Instant::now(),
            delay: Duration::from_secs(1),
        }
    }
    fn ready(&self) -> bool {
        std::time::Instant::now() >= self.due
    }
    fn failed(&mut self) {
        self.due = std::time::Instant::now() + self.delay;
        self.delay = (self.delay * 2).min(Duration::from_secs(10));
    }
    fn succeeded(&mut self) {
        *self = Self::new();
    }
}

fn run_worker(
    transport: &mut dyn AsioTransport,
    queue: &AsioQueue,
    shared: &AsioShared,
    memory: &Arc<dyn ControlMemory>,
    rt_scope: Option<&crate::RtScope>,
    stop: &AtomicBool,
) {
    let control = memory.control();
    let mut prepared_epoch = 0;
    let mut retry = PrepareRetry::new();
    while !stop.load(Ordering::Acquire) {
        let epoch = control.dac_generation();
        if epoch != prepared_epoch && retry.ready() {
            match configured_renderer(transport, shared, &mut None) {
                Ok(Some(renderer)) if epoch == control.dac_generation() => {
                    *shared
                        .prepared
                        .lock()
                        .expect("ASIO prepared mutex poisoned") = Some(renderer);
                    prepared_epoch = epoch;
                    retry.succeeded();
                    // With no lease there is no producer to quiesce. A newly
                    // announced client acknowledges this epoch before start.
                    if control.lease().is_vacant() {
                        control.set_ready_for_generation(epoch);
                    }
                }
                Ok(_) => retry.failed(),
                Err(_) => {
                    retry.failed();
                    shared.set_clock_fault(true);
                }
            }
        }
        match transport.poll_client(ASIO_POLL_WAIT) {
            Ok(Some(client)) => {
                if run_client(client, transport, queue, shared, memory, rt_scope, stop).is_err() {
                    shared.set_clock_fault(true);
                }
                shared.end_client();
                shared.request_reset();
                prepared_epoch = 0;
            }
            Ok(None) => {}
            Err(_) => shared.set_clock_fault(true),
        }
        std::thread::park_timeout(Duration::from_millis(1));
    }
}

#[allow(clippy::too_many_arguments)]
fn run_client(
    client: AsioClient,
    transport: &mut dyn AsioTransport,
    queue: &AsioQueue,
    shared: &AsioShared,
    memory: &Arc<dyn ControlMemory>,
    rt_scope: Option<&crate::RtScope>,
    stop: &AtomicBool,
) -> Result<(), AsioTransportError> {
    let geometry = client.geometry.validate().map_err(|error| {
        AsioTransportError::Transport(format!("invalid streamring geometry: {error:?}"))
    })?;
    if geometry.lanes != 1
        || geometry.output_channels != 2
        || !asio_proto::accepts_buffer(geometry.max_frames)
    {
        return Err(AsioTransportError::Transport(
            "invalid ASIO streamring geometry".into(),
        ));
    }
    let control = memory.control();
    let mut consumer = streamring::Consumer::new(
        client.memory.region(),
        client.done.as_ref(),
        [Some(client.done.as_ref()), None],
    );
    let actual = consumer
        .validate_and_ready(std::process::id())
        .map_err(|error| {
            AsioTransportError::Transport(format!("invalid streamring header: {error:?}"))
        })?;
    if actual != geometry {
        return Err(AsioTransportError::Transport("ASIO header changed".into()));
    }
    shared.begin_client(
        client.lease,
        client.executable.clone(),
        geometry.sample_rate,
        geometry.max_frames,
        wire_layout(shared.settings().layout),
    );
    let mut renderer = None;
    let mut reblocker = None;
    let mut block = vec![0.0; geometry.samples_per_slot()];
    let mut epoch = 0;
    let mut gain = f32::NAN.to_bits();
    let mut lfe = f32::NAN.to_bits();
    let mut retry = PrepareRetry::new();
    while !stop.load(Ordering::Acquire) && control.lease() == client.lease {
        let current = control.dac_generation();
        if epoch != current {
            let state = transport.wait_client(Duration::from_millis(2))?;
            if state.lease != client.lease {
                break;
            }
            reblocker = None;
            if control.client_generation_ack() != current {
                std::thread::park_timeout(Duration::from_millis(1));
                continue;
            }
            // ACK is published only after the last old callback returns. At
            // most the two ring slots can precede its barrier; no unbounded drain.
            let barrier = control.generation_barrier_sequence();
            for _ in 0..2 {
                let Some(sequence) = consumer.next_published(0) else {
                    break;
                };
                if !streamring::sequence_at_least(barrier, sequence) {
                    break;
                }
                consumer.complete(0, sequence);
            }
            if consumer
                .next_published(0)
                .is_some_and(|sequence| streamring::sequence_at_least(barrier, sequence))
            {
                return Err(AsioTransportError::Transport(
                    "ASIO generation barrier exceeds ring capacity".into(),
                ));
            }
            // The generation will move again: waiting for it is no prepare
            // failure, so it spends no retry and keeps the renderer.
            if shared.generation_pending() {
                std::thread::park_timeout(Duration::from_millis(1));
                continue;
            }
            if !retry.ready() {
                continue;
            }
            let next = match configured_renderer(transport, shared, &mut renderer) {
                Ok(Some(next)) => next,
                // A request that landed after the check above.
                Ok(None) if shared.generation_pending() => {
                    std::thread::park_timeout(Duration::from_millis(1));
                    continue;
                }
                Ok(None) | Err(_) => {
                    retry.failed();
                    std::thread::park_timeout(Duration::from_millis(1));
                    continue;
                }
            };
            if geometry.sample_rate != next.sample_rate()
                || geometry.input_channels as usize != next.input_channels()
            {
                // Immutable ring geometry requires a fresh DAW createBuffers.
                retry.failed();
                std::thread::park_timeout(Duration::from_millis(1));
                continue;
            }
            if current != control.dac_generation() || shared.generation_pending() {
                continue;
            }
            queue.configure(
                usize::from(next.output_layout().channels),
                shared.target_lead_frames.load(Ordering::Acquire) as usize
                    + 2 * geometry.max_frames as usize,
            );
            reblocker = Some(AsioReblocker::new(next.input_channels(), queue.channels()));
            renderer = Some(next);
            control.set_completed_frames(0);
            epoch = current;
            retry.succeeded();
            gain = f32::NAN.to_bits();
            lfe = f32::NAN.to_bits();
            control.set_ready_for_generation(epoch);
        }
        let mut tick = || {
            let pending = consumer.next_published(0).is_some()
                && queue.available_frames() >= geometry.max_frames as usize;
            let timeout = if pending {
                Duration::ZERO
            } else {
                Duration::from_millis(2)
            };
            let alive = match transport.wait_client(timeout) {
                Ok(state) => state,
                Err(_) => {
                    shared.set_clock_fault(true);
                    return;
                }
            };
            if alive.lease != client.lease || alive.generation != epoch {
                return;
            }
            shared.set_clock_fault(control.clock_fault());
            if !alive.peer_alive
                || !control.client_running()
                || !control.ready()
                || control.clock_fault()
                || !shared.settings().enabled
                || shared.reset_request.load(Ordering::Acquire)
                    != shared.reset_applied.load(Ordering::Acquire)
                || shared.profile_pending.load(Ordering::Acquire) != 0
            {
                return;
            }
            let Some(sequence) = consumer.next_published(0) else {
                return;
            };
            let renderer = renderer.as_mut().expect("epoch has renderer");
            let reblocker = reblocker.as_mut().expect("epoch has reblocker");
            let produced = (reblocker.pending + geometry.max_frames as usize)
                / ASIO_PARTITION_FRAMES
                * ASIO_PARTITION_FRAMES;
            if queue.available_frames() < produced {
                // Reserve before advancing DSP or acknowledging a slot. Backpressure
                // delays the callback without discarding/shifting its audio timeline.
                return;
            }
            {
                let new_gain = shared.gain_bits.load(Ordering::Acquire);
                if new_gain != gain {
                    let db = f32::from_bits(new_gain);
                    if db.is_finite() {
                        renderer.limiter.set_gain(
                            10.0_f32.powf(db / 20.0),
                            (renderer.sample_rate() / 100) as usize,
                        );
                    }
                    gain = new_gain;
                }
                let new_lfe = shared.lfe_bits.load(Ordering::Acquire);
                if new_lfe != lfe {
                    let db = f32::from_bits(new_lfe);
                    if db.is_finite() {
                        renderer.lfe_gain = 10.0_f32.powf(db / 20.0);
                    }
                    lfe = new_lfe;
                }
                if consumer.slot_mut(0, sequence, &mut block, |_| {}) {
                    let inputs = geometry.input_channels as usize * geometry.max_frames as usize;
                    let accepted = reblocker.process(
                        renderer,
                        &block[..inputs],
                        geometry.max_frames as usize,
                        |samples, frames| queue.push_planar_epoch(samples, frames, epoch),
                    );
                    debug_assert!(accepted, "reserved rendered queue space");
                    consumer.complete(0, sequence);
                    control.set_completed_frames(
                        control
                            .completed_frames()
                            .saturating_add(u64::from(geometry.max_frames)),
                    );
                }
            }
        };
        if let Some(scope) = rt_scope {
            scope(crate::RtSection::Asio, &mut tick);
        } else {
            tick();
        }
    }
    Ok(())
}

/// Converts an ASIO layout to a nominal direction layout, useful to adapters
/// and tests that need the exact channel-to-position contract.
pub fn input_direction_layout(layout: AsioLayout) -> Layout {
    let directions = wire_layout(layout)
        .channel_order()
        .iter()
        .filter_map(|&channel| {
            let speaker = speaker(channel);
            let (azimuth, elevation) = speaker.angles()?;
            Some(Direction::new(speaker.name(), azimuth, elevation))
        })
        .collect();
    Layout::new(directions).expect("every ASIO layout has positioned channels")
}

const _: () = assert!(MAX_ASIO_BUFFER_FRAMES.is_multiple_of(ASIO_PARTITION_FRAMES));
const _: () = assert!(MIX_FRAMES.is_multiple_of(ASIO_PARTITION_FRAMES));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layout_maps_to_the_shared_preset_in_protocol_order() {
        for layout in AsioLayout::ALL {
            let wire = wire_layout(layout);
            assert_eq!(wire.channels() as usize, wire.channel_order().len());
            let mapped = wire
                .channel_order()
                .iter()
                .copied()
                .map(speaker)
                .collect::<Vec<_>>();
            assert_eq!(
                mapped,
                ssr_types::channel_layout_preset(layout.name()).unwrap(),
                "{}",
                layout.name()
            );
            let positioned = mapped
                .iter()
                .filter(|&&speaker| !matches!(speaker, Speaker::Lfe | Speaker::Lfe2))
                .count();
            assert_eq!(input_direction_layout(layout).len(), positioned);
            for (&channel, mapped) in wire.channel_order().iter().zip(mapped) {
                assert_eq!(
                    matches!(mapped, Speaker::Lfe | Speaker::Lfe2),
                    is_lfe(channel)
                );
                assert_eq!(mapped.position().is_some(), !is_lfe(channel));
            }
        }
    }

    #[test]
    fn queue_is_bounded_cleared_by_generation_and_exact_once() {
        let queue = AsioQueue::new(2, 4);
        let planar = [1.0, 2.0, 3.0, 4.0, 10.0, 20.0, 30.0, 40.0];
        assert!(queue.push_planar(&planar, 4));
        assert!(!queue.push_planar(&planar, 1));
        let mut output = [0.0; 8];
        assert_eq!(queue.pop_add(&mut output, 4, 1.0), 4);
        assert_eq!(output, [1.0, 10.0, 2.0, 20.0, 3.0, 30.0, 4.0, 40.0]);
        assert_eq!(queue.pop_add(&mut output, 4, 1.0), 0);
        assert!(queue.push_planar(&planar, 4));
        queue.clear(9);
        assert_eq!(queue.generation(), 9);
        assert_eq!(queue.queued_frames(), 0);
    }

    #[test]
    fn asio_ring_fits_sixty_four_channels_largest_buffer_and_highest_rate() {
        let geometry = Geometry {
            lanes: 1,
            input_channels: 64,
            output_channels: 2,
            max_frames: MAX_ASIO_BUFFER_FRAMES as u32,
            sample_rate: 1_000_000,
            latency_bound_frames: 0,
        };
        assert_eq!(geometry.validate(), Ok(geometry));
        assert_eq!(geometry.total_bytes().unwrap(), 1_049_216);
        assert!(geometry.total_bytes().unwrap() < streamring::MAX_REGION_BYTES);
    }

    #[test]
    fn queue_supports_sixty_four_output_channels() {
        let queue = AsioQueue::new(64, 1);
        let planar = (0..64).map(|channel| channel as f32).collect::<Vec<_>>();
        assert!(queue.push_planar(&planar, 1));
        let mut output = [0.0; 64];
        assert_eq!(queue.pop_add(&mut output, 1, 1.0), 1);
        assert_eq!(output, planar.as_slice());
    }

    #[test]
    fn prepare_retry_backs_off_from_one_to_ten_seconds() {
        let mut retry = PrepareRetry::new();
        assert!(retry.ready());
        for seconds in [1, 2, 4, 8, 10, 10] {
            let before = std::time::Instant::now();
            retry.failed();
            assert!(retry.due >= before + Duration::from_secs(seconds));
            assert!(!retry.ready());
        }
        retry.succeeded();
        assert!(retry.ready());
        assert_eq!(retry.delay, Duration::from_secs(1));
    }
    #[test]
    fn a_concurrent_late_producer_keeps_its_original_epoch() {
        let queue = Arc::new(AsioQueue::new(2, 16));
        queue.clear(7);
        let (captured_tx, captured_rx) = std::sync::mpsc::sync_channel(0);
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(0);
        let producer = Arc::clone(&queue);
        let worker = std::thread::spawn(move || {
            let epoch = producer.generation();
            captured_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
            assert!(producer.push_planar_epoch(&[0.9; 8], 4, epoch));
            assert!(producer.push_planar_epoch(&[0.2; 8], 4, producer.generation()));
        });
        captured_rx.recv().unwrap();
        queue.clear(8);
        resume_tx.send(()).unwrap();
        worker.join().unwrap();
        let mut system = [0.1; 8];
        assert_eq!(queue.pop_add(&mut system, 4, 1.0), 4);
        assert!(system.iter().all(|sample| (*sample - 0.3).abs() < 1e-6));
        assert_eq!(queue.queued_frames(), 0);
    }

    #[test]
    fn two_lfe_channels_sum_into_the_single_scene_lfe() {
        let ir = [1.0_f32];
        let bank =
            convolver::FilterBank::new(&[[ir.as_slice(), ir.as_slice()]], ASIO_PARTITION_FRAMES)
                .unwrap();
        let prepared = Prepared::new(
            Arc::new(brir::PreparedBrir {
                layout: Layout::new(vec![Direction::new("front", 0.0, 0.0)]).unwrap(),
                bank: Arc::new(bank),
                rate: 48_000,
                partition: ASIO_PARTITION_FRAMES,
                lfe: None,
                gain: 1.0,
                limiter_ceiling: 1.0,
                ir_frames: 1,
                trimmed_leading_frames: 0,
            }),
            MAX_ASIO_BUFFER_FRAMES,
        )
        .unwrap();
        let mut renderer = AsioRenderer::new(Box::new(prepared), AsioLayout::Surround222).unwrap();
        let mut input = vec![0.0; 24 * ASIO_PARTITION_FRAMES];
        input[3 * ASIO_PARTITION_FRAMES..4 * ASIO_PARTITION_FRAMES].fill(0.25);
        input[9 * ASIO_PARTITION_FRAMES..10 * ASIO_PARTITION_FRAMES].fill(0.5);
        let mut output = vec![0.0; 2 * ASIO_PARTITION_FRAMES];
        renderer.process(&input, &mut output);
        assert!(
            renderer
                .lfe
                .iter()
                .all(|sample| (*sample - 0.75).abs() < 1e-6)
        );
    }

    #[test]
    fn headphone_renderer_and_arbitrary_callback_residual_are_real_dsp() {
        let ir = [0.5, 0.25, -0.125];
        let bank =
            convolver::FilterBank::new(&[[ir.as_slice(), ir.as_slice()]], ASIO_PARTITION_FRAMES)
                .unwrap();
        let prepared = Prepared::new(
            Arc::new(brir::PreparedBrir {
                layout: Layout::new(vec![Direction::new("front", 0.0, 0.0)]).unwrap(),
                bank: Arc::new(bank),
                rate: 48_000,
                partition: ASIO_PARTITION_FRAMES,
                lfe: None,
                gain: 0.5,
                limiter_ceiling: 1.0,
                ir_frames: ir.len(),
                trimmed_leading_frames: 0,
            }),
            MAX_ASIO_BUFFER_FRAMES,
        )
        .unwrap();
        let mut renderer = AsioRenderer::new(Box::new(prepared), AsioLayout::Stereo).unwrap();
        let mut adapter = AsioReblocker::new(2, 2);
        let mut emitted = Vec::new();
        let mut samples = vec![0.0; 130];
        samples[0] = 1.0;
        samples[65] = f32::NAN;
        for callback in 0..16 {
            assert!(
                adapter.process(&mut renderer, &samples, 65, |block, frames| {
                    emitted.extend_from_slice(&block[..frames]);
                    true
                })
            );
            if callback == 0 {
                samples.fill(0.0);
            }
        }
        assert_eq!(emitted.len(), 16 * 65);
        assert_eq!(adapter.pending, 0);
        assert!(emitted.iter().all(|sample| sample.is_finite()));
        assert!((emitted[0] - 0.25).abs() < 1e-5);
        assert!((emitted[1] - 0.125).abs() < 1e-5);
        assert!((emitted[2] + 0.0625).abs() < 1e-5);
        assert!(emitted[3..].iter().all(|sample| sample.abs() < 1e-5));
    }
}
