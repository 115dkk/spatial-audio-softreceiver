#![forbid(unsafe_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use engine::asio::{
    AsioClient, AsioOutput, AsioQueue, AsioRuntime, AsioRuntimeConfig, AsioShared, AsioTransport,
    AsioTransportError, ControlMemory, OutputConfig,
};
use engine::{OutputProfile, RtSection};
use ssr_types::AsioSettings;
use streamring::{Geometry, InProcessEvent, Producer, Publish, RegionBuf, Signal};

#[global_allocator]
static ALLOCATOR: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

#[derive(Default)]
struct CountSignal(AtomicU64);
impl Signal for CountSignal {
    fn signal(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "worker failed to reach deterministic checkpoint"
        );
        std::thread::yield_now();
    }
}

fn profile() -> OutputProfile {
    OutputProfile::Speakers(speakers::layout_profile(
        ssr_types::app::SpeakerLayout::Stereo,
        0.0,
    ))
}

struct FakeTransport {
    control: Arc<asio_proto::ControlBuf>,
    signal: Arc<CountSignal>,
    ring: Arc<RegionBuf>,
    done: Arc<InProcessEvent>,
    geometry: Geometry,
    alive: Arc<AtomicBool>,
    waits: Arc<AtomicU64>,
}
impl AsioTransport for FakeTransport {
    fn control_memory(&self) -> Arc<dyn ControlMemory> {
        self.control.clone()
    }
    fn demand_signal(&self) -> Arc<dyn Signal> {
        self.signal.clone()
    }
    fn configure(&mut self, config: OutputConfig) -> Result<(), AsioTransportError> {
        let c = self.control.control();
        c.set_ready(false);
        c.set_sample_rate(config.sample_rate);
        c.set_layout(config.layout);
        c.set_dac_period_frames(config.dac_period_frames);
        c.set_latency_frames(config.latency_frames);
        Ok(())
    }
    fn set_ready(&mut self, ready: bool) -> Result<(), AsioTransportError> {
        self.control.control().set_ready(ready);
        Ok(())
    }
    fn wait_client(
        &mut self,
        _timeout: Duration,
    ) -> Result<asio_proto::ClientState, AsioTransportError> {
        self.waits.fetch_add(1, Ordering::AcqRel);
        let c = self.control.control();
        let alive = self.alive.load(Ordering::Acquire);
        if !alive {
            c.release_lease(c.lease());
        }
        Ok(asio_proto::ClientState {
            lease: c.lease(),
            generation: c.dac_generation(),
            peer_alive: alive,
        })
    }
    fn poll_client(
        &mut self,
        _timeout: Duration,
    ) -> Result<Option<AsioClient>, AsioTransportError> {
        let c = self.control.control();
        if !self.alive.load(Ordering::Acquire) {
            c.release_lease(c.lease());
        }
        let lease = c.lease();
        Ok((!lease.is_vacant()).then(|| AsioClient {
            memory: self.ring.clone(),
            geometry: self.geometry,
            lease,
            executable: "fake-daw".into(),
            done: self.done.clone(),
        }))
    }
}

struct Harness {
    control: Arc<asio_proto::ControlBuf>,
    ring: Arc<RegionBuf>,
    done: Arc<InProcessEvent>,
    signal: Arc<CountSignal>,
    alive: Arc<AtomicBool>,
    waits: Arc<AtomicU64>,
    shared: Arc<AsioShared>,
    queue: Arc<AsioQueue>,
    output: AsioOutput,
    runtime: Option<AsioRuntime>,
    geometry: Geometry,
    asio_steps: Arc<AtomicU64>,
    sequence: u32,
    supplied: u64,
    callbacks: u64,
    publisher: streamring::Publisher,
}
impl Harness {
    fn new(frames: u32) -> Self {
        Self::new_at(frames, 0)
    }

    fn new_at(frames: u32, origin: u32) -> Self {
        let geometry = Geometry {
            lanes: 1,
            input_channels: 2,
            output_channels: 2,
            max_frames: frames,
            sample_rate: 48_000,
            latency_bound_frames: 4096,
        };
        let control = Arc::new(asio_proto::ControlBuf::new());
        control.control().initialize(1).unwrap();
        let ring = Arc::new(RegionBuf::new(geometry).unwrap());
        let done = Arc::new(InProcessEvent::new());
        let signal = Arc::new(CountSignal::default());
        let alive = Arc::new(AtomicBool::new(true));
        let waits = Arc::new(AtomicU64::new(0));
        let settings = AsioSettings {
            layout: ssr_types::AsioLayout::Stereo,
            ..AsioSettings::default()
        };
        let shared = Arc::new(AsioShared::new(settings));
        let asio_steps = Arc::new(AtomicU64::new(0));
        let provider = {
            let (control, ring, done, signal, alive, waits) = (
                control.clone(),
                ring.clone(),
                done.clone(),
                signal.clone(),
                alive.clone(),
                waits.clone(),
            );
            Arc::new(move || {
                Ok(Box::new(FakeTransport {
                    control: control.clone(),
                    ring: ring.clone(),
                    done: done.clone(),
                    signal: signal.clone(),
                    alive: alive.clone(),
                    waits: waits.clone(),
                    geometry,
                }) as Box<dyn AsioTransport>)
            })
        };
        let counts = asio_steps.clone();
        let mut runtime = AsioRuntime::start(
            AsioRuntimeConfig {
                provider,
                profile: profile(),
                target_lead_frames: 2048,
            },
            settings,
            shared.clone(),
            48_000,
            480,
            Some(Arc::new(move |section, work| match section {
                RtSection::Asio => {
                    assert_no_alloc::assert_no_alloc(work);
                    counts.fetch_add(1, Ordering::Release);
                }
                RtSection::Capture | RtSection::Output => panic!("standalone ASIO worker scope"),
            })),
            None,
        )
        .unwrap();
        let output = runtime.take_output().unwrap();
        let queue = runtime.queue.clone();
        wait(|| control.control().ready());
        let mut producer = Producer::new(ring.region(), [Some(done.as_ref()), None], done.as_ref());
        producer.announce_at(geometry, 77, origin).unwrap();
        let publisher = producer.publisher().unwrap();
        let lease = asio_proto::Lease::new(77, 1).unwrap();
        control.control().acquire_lease(lease).unwrap();
        control.control().set_buffer_frames(frames);
        control.control().set_client_run_serial(1);
        control.control().set_client_running(true);
        let mut result = Self {
            control,
            ring,
            done,
            signal,
            alive,
            waits,
            shared,
            queue,
            output,
            runtime: Some(runtime),
            geometry,
            asio_steps,
            sequence: origin,
            supplied: 0,
            callbacks: 0,
            publisher,
        };
        result.period(0.0);
        result.ack();
        wait(|| result.control.control().ready());
        result
    }
    fn ack(&mut self) {
        let c = self.control.control();
        c.set_generation_barrier_sequence(self.sequence);
        self.supplied = 0;
        c.set_loaded_frames(0);
        c.set_client_generation_ack(c.dac_generation());
    }
    fn period(&mut self, system: f32) -> Vec<f32> {
        let mut samples = vec![system; 480 * 2];
        assert_no_alloc::assert_no_alloc(|| self.output.mix_period(&mut samples, 2));
        samples
    }
    fn supply(&mut self) -> u64 {
        let before = self.callbacks;
        let c = self.control.control();
        while asio_proto::deficit(
            c.demand(),
            self.supplied,
            c.loaded_frames(),
            c.completed_frames(),
        ) != 0
        {
            let next = self.sequence.wrapping_add(1);
            wait(|| asio_proto::can_publish(self.publisher.completed(self.ring.region(), 0), next));
            let published = self.publisher.try_publish(
                self.ring.region(),
                0,
                next,
                Some(self.done.as_ref()),
                |slot| {
                    for frame in 0..self.geometry.max_frames as usize {
                        slot.write(0, frame, 0.125);
                        slot.write(1, frame, -0.125);
                    }
                },
            );
            assert_eq!(published, Publish::Published);
            self.sequence = next;
            self.supplied += u64::from(self.geometry.max_frames);
            c.set_loaded_frames(self.supplied);
            self.callbacks += 1;
            // Counter order is the DLL's: publish, supplied/loaded, consumer complete.
            wait(|| c.completed_frames() >= self.supplied);
        }
        self.callbacks - before
    }
}

struct IdleCapture;
impl ssr_types::CaptureSource for IdleCapture {
    fn read(&mut self, _: &mut [u8], _: Duration) -> Result<usize, ssr_types::CaptureError> {
        std::thread::sleep(Duration::from_millis(1));
        Ok(0)
    }
    fn set_rate_trim(&mut self, _: f64) -> Result<(), ssr_types::TrimUnsupported> {
        Err(ssr_types::TrimUnsupported)
    }
}

#[test]
fn disabled_asio_never_opens_and_failed_enable_keeps_system_output_running() {
    let state = Arc::new(ClockedSinkState {
        requested: AtomicU64::new(0),
        completed: AtomicU64::new(0),
        stop: AtomicBool::new(false),
        restarts: AtomicU64::new(0),
        left: std::sync::atomic::AtomicU32::new(0),
        channels: std::sync::atomic::AtomicU32::new(0),
        rate: std::sync::atomic::AtomicU32::new(48000),
        opened: AtomicU64::new(0),
    });
    let opens = Arc::new(AtomicU64::new(0));
    let counter = opens.clone();
    let config = engine::EngineConfig {
        asio: AsioSettings {
            enabled: false,
            ..Default::default()
        },
        asio_runtime: Some(AsioRuntimeConfig {
            provider: Arc::new(move || {
                counter.fetch_add(1, Ordering::Release);
                Err(AsioTransportError::Transport("owner_exists".into()))
            }),
            profile: profile(),
            target_lead_frames: 2048,
        }),
        ..Default::default()
    };
    let engine = engine::start_with(
        config,
        IdleCapture,
        ClockedOpener(state.clone()),
        engine::StartOptions {
            qpc_frequency: 10_000_000,
            prepared: Some(Box::new(profile().prepare().unwrap())),
        },
    )
    .unwrap();
    let _stop = StopClock(state.clone());
    let tick = || {
        let n = state.requested.fetch_add(1, Ordering::AcqRel) + 1;
        wait(|| state.completed.load(Ordering::Acquire) >= n);
    };
    tick();
    assert_eq!(opens.load(Ordering::Acquire), 0);
    engine
        .apply(engine::LiveChange::AsioSettings(AsioSettings {
            enabled: true,
            ..Default::default()
        }))
        .unwrap();
    wait(|| {
        tick();
        opens.load(Ordering::Acquire) == 1
    });
    wait(|| {
        engine.status().asio.is_some_and(|s| {
            s.fault_reason.as_deref() == Some("ASIO transport failed: owner_exists")
        })
    });
    let before = state.completed.load(Ordering::Acquire);
    tick();
    assert!(state.completed.load(Ordering::Acquire) > before);
    assert_eq!(opens.load(Ordering::Acquire), 1);
    engine
        .apply(engine::LiveChange::AsioSettings(AsioSettings {
            enabled: false,
            ..Default::default()
        }))
        .unwrap();
    wait(|| {
        tick();
        engine.status().asio.is_none()
    });
    state.stop.store(true, Ordering::Release);
    engine.stop();
}

struct EofCapture;
impl ssr_types::CaptureSource for EofCapture {
    fn read(&mut self, _: &mut [u8], _: Duration) -> Result<usize, ssr_types::CaptureError> {
        Err(ssr_types::CaptureError::Ended)
    }
    fn set_rate_trim(&mut self, _: f64) -> Result<(), ssr_types::TrimUnsupported> {
        Err(ssr_types::TrimUnsupported)
    }
}

struct ClockedSinkState {
    requested: AtomicU64,
    completed: AtomicU64,
    stop: AtomicBool,
    restarts: AtomicU64,
    left: std::sync::atomic::AtomicU32,
    channels: std::sync::atomic::AtomicU32,
    rate: std::sync::atomic::AtomicU32,
    opened: AtomicU64,
}
struct StopClock(Arc<ClockedSinkState>);
impl Drop for StopClock {
    fn drop(&mut self) {
        self.0.stop.store(true, Ordering::Release);
    }
}
struct ClockedOpener(Arc<ClockedSinkState>);
struct ClockedSink {
    state: Arc<ClockedSinkState>,
    layout: ssr_types::OutputLayout,
    samples: Vec<f32>,
    rate: u32,
}
impl ssr_types::OutputOpener for ClockedOpener {
    type Sink = ClockedSink;
    fn open(
        &mut self,
        layout: ssr_types::OutputLayout,
    ) -> Result<Self::Sink, ssr_types::OutputError> {
        self.0
            .channels
            .store(u32::from(layout.channels), Ordering::Release);
        self.0.opened.fetch_add(1, Ordering::Release);
        Ok(ClockedSink {
            state: self.0.clone(),
            layout,
            samples: vec![0.0; 480 * usize::from(layout.channels)],
            rate: self.0.rate.load(Ordering::Acquire),
        })
    }
}
impl ssr_types::OutputSink for ClockedSink {
    fn format(&self) -> ssr_types::OutputFormat {
        ssr_types::OutputFormat::exclusive(
            self.rate,
            self.layout.channels,
            self.layout.channel_mask,
            480,
        )
    }
    fn next_period(
        &mut self,
        fill: &mut dyn FnMut(&mut [f32]),
    ) -> Result<u64, ssr_types::OutputError> {
        self.next_period_with_restarts(&mut |samples, _| fill(samples))
    }
    fn next_period_with_restarts(
        &mut self,
        fill: &mut dyn FnMut(&mut [f32], u64),
    ) -> Result<u64, ssr_types::OutputError> {
        while self.state.requested.load(Ordering::Acquire)
            <= self.state.completed.load(Ordering::Acquire)
            && !self.state.stop.load(Ordering::Acquire)
        {
            std::thread::yield_now();
        }
        if self.state.stop.load(Ordering::Acquire) {
            return Err(ssr_types::OutputError::Removed);
        }
        fill(
            &mut self.samples,
            self.state.restarts.load(Ordering::Acquire),
        );
        self.state
            .left
            .store(self.samples[0].to_bits(), Ordering::Release);
        let period = self.state.completed.fetch_add(1, Ordering::AcqRel) + 1;
        Ok(period * 100_000)
    }
    fn restarts(&self) -> u64 {
        self.state.restarts.load(Ordering::Acquire)
    }
}

#[test]
fn real_engine_survives_system_eof_profiles_mute_and_prefill_restart_without_allocating() {
    // Reuse the fake transport's exact counter-order DAW, but let Engine own the
    // actual output thread rather than calling its mixer from the test.
    let mut h = Harness::new(64);
    h.control.control().set_client_running(false);
    h.control
        .control()
        .release_lease(h.control.control().lease());
    h.runtime.take().unwrap().stop();
    let state = Arc::new(ClockedSinkState {
        requested: AtomicU64::new(0),
        completed: AtomicU64::new(0),
        stop: AtomicBool::new(false),
        restarts: AtomicU64::new(0),
        left: std::sync::atomic::AtomicU32::new(0),
        channels: std::sync::atomic::AtomicU32::new(0),
        rate: std::sync::atomic::AtomicU32::new(48_000),
        opened: AtomicU64::new(0),
    });
    let provider = {
        let (control, ring, done, signal, alive, waits, geometry) = (
            h.control.clone(),
            h.ring.clone(),
            h.done.clone(),
            h.signal.clone(),
            h.alive.clone(),
            h.waits.clone(),
            h.geometry,
        );
        Arc::new(move || {
            Ok(Box::new(FakeTransport {
                control: control.clone(),
                ring: ring.clone(),
                done: done.clone(),
                signal: signal.clone(),
                alive: alive.clone(),
                waits: waits.clone(),
                geometry,
            }) as Box<dyn AsioTransport>)
        })
    };
    let counts = Arc::new([AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)]);
    let counted = counts.clone();
    let config = engine::EngineConfig {
        asio: AsioSettings {
            layout: ssr_types::AsioLayout::Stereo,
            ..Default::default()
        },
        asio_runtime: Some(AsioRuntimeConfig {
            provider,
            profile: profile(),
            target_lead_frames: 2048,
        }),
        rt_scope: Some(Arc::new(move |section, work| {
            let index = match section {
                RtSection::Capture => 0,
                RtSection::Output => 1,
                RtSection::Asio => 2,
            };
            assert_no_alloc::assert_no_alloc(work);
            counted[index].fetch_add(1, Ordering::Release);
        })),
        ..Default::default()
    };
    let mut engine = engine::start_with(
        config,
        EofCapture,
        ClockedOpener(state.clone()),
        engine::StartOptions {
            qpc_frequency: 10_000_000,
            prepared: Some(Box::new(profile().prepare().unwrap())),
        },
    )
    .unwrap();
    let _stop_on_panic = StopClock(state.clone());
    wait(|| h.control.control().ready());
    let mut producer = Producer::new(
        h.ring.region(),
        [Some(h.done.as_ref()), None],
        h.done.as_ref(),
    );
    producer.announce(h.geometry, 77).unwrap();
    h.publisher = producer.publisher().unwrap();
    h.sequence = 0;
    h.control
        .control()
        .acquire_lease(asio_proto::Lease::new(77, 2).unwrap())
        .unwrap();
    h.control.control().set_client_run_serial(2);
    h.control.control().set_client_running(true);
    let tick = || {
        let n = state.requested.fetch_add(1, Ordering::AcqRel) + 1;
        wait(|| state.completed.load(Ordering::Acquire) >= n);
        f32::from_bits(state.left.load(Ordering::Acquire))
    };
    let before_attach = h.control.control().dac_generation();
    wait(|| {
        tick();
        h.control.control().dac_generation() > before_attach
    });
    h.ack();
    wait(|| h.control.control().ready());
    h.supply();
    assert!((tick() - 0.125).abs() < 1e-5);
    h.supply();
    engine.apply(engine::LiveChange::Mute(true)).unwrap();
    // Count from after the push: the capture period running now may have
    // drained before it, so only the next one is sure to install the change.
    let capture_before = counts[0].load(Ordering::Acquire);
    wait(|| counts[0].load(Ordering::Acquire) >= capture_before + 2);
    tick();
    h.supply();
    assert_eq!(tick(), 0.0);
    engine.apply(engine::LiveChange::Mute(false)).unwrap();
    let mut quiet_profile = profile();
    let fir = std::env::temp_dir().join(format!("ssr-asio-prepared-{}.wav", std::process::id()));
    // A one-tap PCM16 room filter; remove it after control-plane validation.
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&38_u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&48000_u32.to_le_bytes());
    wav.extend_from_slice(&96000_u32.to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&2_u32.to_le_bytes());
    wav.extend_from_slice(&32767_i16.to_le_bytes());
    std::fs::write(&fir, wav).unwrap();
    if let OutputProfile::Speakers(p) = &mut quiet_profile {
        p.gain_db = -6.0;
        p.speakers[0].fir = Some(fir.clone());
    }
    engine
        .apply(engine::LiveChange::Prepared(Box::new(
            quiet_profile.prepare().unwrap(),
        )))
        .unwrap();
    // Both prepares read the filter, which can outlast several capture periods.
    let capture_before = counts[0].load(Ordering::Acquire);
    std::fs::remove_file(&fir).unwrap();
    wait(|| counts[0].load(Ordering::Acquire) >= capture_before + 2);
    tick();
    h.ack();
    wait(|| h.control.control().ready());
    h.supply();
    tick();
    h.supply();
    assert!((tick() - 0.125 * 10.0_f32.powf(-6.0 / 20.0)).abs() < 1e-5);
    h.supply();
    let old = h.control.control().dac_generation();
    state.restarts.fetch_add(1, Ordering::Release);
    assert_eq!(
        tick(),
        0.0,
        "old queue must not reach the first restarted fill"
    );
    assert!(h.control.control().dac_generation() > old);
    h.ack();
    wait(|| h.control.control().ready());
    h.supply();
    assert!(tick() > 0.0);
    engine.poll();
    assert!(counts[0].load(Ordering::Acquire) > 0);
    assert!(counts[1].load(Ordering::Acquire) > 0);
    assert!(counts[2].load(Ordering::Acquire) > 0);
    assert!(engine.status().output_periods >= 6);
    state.stop.store(true, Ordering::Release);
    engine.stop();
}

#[test]
fn fake_daw_64_128_512_follow_dac_demand_and_process_without_allocations() {
    for frames in [64, 128, 512] {
        let mut h = Harness::new(frames);
        h.supply();
        assert!(h.queue.queued_frames() >= 2048);
        let before = h.callbacks;
        for _ in 0..20 {
            let samples = h.period(0.25);
            assert!(samples.as_chunks::<2>().0.iter().all(|frame| (frame[0] - 0.375).abs() < 1e-5 && (frame[1] - 0.125).abs() < 1e-5));
            h.supply();
            assert!(h.queue.queued_frames() <= 2048 + 2 * frames as usize);
        }
        assert_eq!(
            h.callbacks - before,
            (20_u64 * 480).div_ceil(u64::from(frames))
        );
        if frames == 64 {
            assert_eq!(h.callbacks - before, 150);
        }
        assert!(h.asio_steps.load(Ordering::Acquire) >= h.callbacks);
        assert_eq!(h.shared.status().unwrap().underruns, 0);
        assert!(h.signal.0.load(Ordering::Acquire) >= 21);
        h.runtime.take().unwrap().stop();
    }
}

#[test]
fn modular_ring_sequences_cross_zero_without_interrupting_output() {
    let mut h = Harness::new_at(64, u32::MAX - 2);
    h.supply();
    assert!(h.sequence < 32, "the initial prefill must cross zero");
    for _ in 0..20 {
        let samples = h.period(0.25);
        assert!(
            samples.as_chunks::<2>().0.iter().all(|frame| {
                (frame[0] - 0.375).abs() < 1e-5 && (frame[1] - 0.125).abs() < 1e-5
            })
        );
        h.supply();
    }
    assert!(h.sequence < 256);
    h.runtime.take().unwrap().stop();
}

#[test]
fn generation_barrier_discards_old_blocks_across_zero() {
    let mut h = Harness::new_at(64, u32::MAX - 1);
    let old_generation = h.control.control().dac_generation();
    h.control.control().set_client_run_serial(2);
    h.period(0.25);
    assert!(h.control.control().dac_generation() > old_generation);

    for sequence in [u32::MAX, 0] {
        assert_eq!(
            h.publisher.try_publish(
                h.ring.region(),
                0,
                sequence,
                Some(h.done.as_ref()),
                |slot| {
                    for frame in 0..h.geometry.max_frames as usize {
                        slot.write(0, frame, 0.75);
                        slot.write(1, frame, -0.75);
                    }
                },
            ),
            Publish::Published
        );
    }
    h.sequence = 0;
    h.supplied = 128;
    h.control.control().set_loaded_frames(h.supplied);
    h.ack();
    wait(|| h.control.control().ready());
    assert_eq!(h.publisher.completed(h.ring.region(), 0), 0);

    h.supply();
    let resumed = h.period(0.25);
    assert!(
        resumed
            .as_chunks::<2>()
            .0
            .iter()
            .all(|frame| { (frame[0] - 0.375).abs() < 1e-5 && (frame[1] - 0.125).abs() < 1e-5 })
    );
    h.runtime.take().unwrap().stop();
}

#[test]
fn a_reset_requested_before_the_ack_does_not_spend_a_prepare_retry() {
    let mut h = Harness::new(64);
    let settings = |enabled| AsioSettings {
        enabled,
        layout: ssr_types::AsioLayout::Stereo,
        ..Default::default()
    };
    // A queued profile works the same way: the output applies the first reset,
    // the capture thread's install requests a second before the DAW acks.
    h.control.control().set_client_run_serial(2);
    h.period(0.0);
    h.shared.set_settings(settings(false));
    h.ack();
    // Two more client waits: a whole worker pass saw the ack with the reset
    // still pending.
    let waits = h.waits.load(Ordering::Acquire);
    wait(|| h.waits.load(Ordering::Acquire) >= waits + 2);
    h.shared.set_settings(settings(true));
    h.period(0.0);
    h.ack();
    let acked = Instant::now();
    wait(|| h.control.control().ready());
    assert!(
        acked.elapsed() < Duration::from_millis(750),
        "a pending reset must not back the worker off for a second"
    );
    h.runtime.take().unwrap().stop();
}

#[test]
fn same_lease_epochs_purge_old_audio_and_live_attenuation_applies_while_priming() {
    let mut h = Harness::new(64);
    h.supply();
    let old = h.control.control().dac_generation();
    h.shared.set_settings(AsioSettings {
        layout: ssr_types::AsioLayout::Stereo,
        system_attenuation_db: 60.0,
        enabled: true,
    });
    // A rapid stop/start between output periods cannot hide behind a boolean.
    h.control.control().set_client_run_serial(2);
    let reset_period = h.period(0.25);
    assert_eq!(reset_period, vec![0.0; 960]);
    assert!(h.control.control().dac_generation() > old);
    assert_eq!(h.queue.queued_frames(), 0);
    h.ack();
    wait(|| h.control.control().ready());
    h.supply();
    let resumed = h.period(0.25);
    assert!(
        resumed
            .as_chunks::<2>()
            .0
            .iter()
            .all(|frame| (frame[0] - 0.125).abs() < 1e-5 && (frame[1] + 0.125).abs() < 1e-5)
    );
    h.shared.set_settings(AsioSettings {
        enabled: false,
        layout: ssr_types::AsioLayout::Stereo,
        system_attenuation_db: 60.0,
    });
    assert_eq!(h.period(0.25), vec![0.25; 960]);
    assert!(!h.control.control().ready());
    h.runtime.take().unwrap().stop();
}

#[test]
fn underrun_keeps_generation_and_dead_lease_removes_only_its_audio() {
    let mut h = Harness::new(64);
    h.supply();
    let generation = h.control.control().dac_generation();
    for _ in 0..8 {
        h.period(0.25);
    }
    assert!(h.shared.status().unwrap().underruns >= 1);
    assert!(h.control.control().ready());
    assert_eq!(h.control.control().dac_generation(), generation);
    assert_eq!(h.period(0.25), vec![0.25; 960]);
    // Supply only up to queue capacity; no generation rebuild is necessary.
    h.control.control().set_demand(h.supplied + 1024);
    h.supply();
    wait(|| h.queue.queued_frames() >= 480);
    let resumed = h.period(0.25);
    assert!(resumed[0] > 0.25);
    h.alive.store(false, Ordering::Release);
    wait(|| h.control.control().lease().is_vacant());
    let system = h.period(0.25);
    assert_eq!(system, vec![0.25; 960]);
    assert_eq!(h.queue.queued_frames(), 0);
    h.runtime.take().unwrap().stop();
}
