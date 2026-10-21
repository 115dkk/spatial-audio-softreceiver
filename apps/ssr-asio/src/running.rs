// SPDX-License-Identifier: MIT
// Pending buffers and bounded callback retirement ported from EqualizerAPO-XT
// asio/AsioWrapper.cpp and asio/CallbackTrampolines.cpp, commit
// 3a6a213cf83a0606f00b8cce03b6781ee48d8c03 (115dkk, authorized MIT port).
use asio_proto::{ClockEvent, ClockTracker, ControlMemory, Layout, Lease};
use asio_sys::{
    ControlMapping, Error, Event, Host, HostBuffers, Peer, Position, Result, RingMapping,
};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use streamring::{Geometry, Producer, Publish, Publisher, RegionMemory, Signal, Wait, Waited};

static SERIAL: AtomicU32 = AtomicU32::new(1);
#[derive(Debug)]
enum SessionControl {
    Shared(ControlMapping),
    Silent(asio_proto::ControlBuf),
}
impl ControlMemory for SessionControl {
    fn control(&self) -> asio_proto::Control<'_> {
        match self {
            Self::Shared(mapping) => mapping.control(),
            Self::Silent(memory) => memory.control(),
        }
    }
}

#[derive(Debug)]
struct Session {
    control: SessionControl,
    lease: Lease,
    ring: RingMapping,
    publisher: Publisher,
    work: Event,
    demand: Event,
    done: Event,
    peer: Peer,
    peer_pid: u32,
    host: Host,
    buffers: HostBuffers,
    layout: Layout,
    rate: u32,
    frames: u32,
    last_published: AtomicU32,
    position: Arc<AtomicU64>,
    nanos: Arc<AtomicU64>,
}
impl Drop for Session {
    fn drop(&mut self) {
        let c = self.control.control();
        if c.lease() == self.lease {
            c.set_client_running(false);
            c.set_buffer_frames(0);
            c.release_lease(self.lease);
        }
        let producer = Producer::new(self.ring.region(), [Some(&self.work), None], &self.done);
        producer.close();
    }
}
#[derive(Debug)]
struct LeaseRollback<'a> {
    control: &'a SessionControl,
    lease: Lease,
    armed: bool,
}
impl Drop for LeaseRollback<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.control.control().release_lease(self.lease);
        }
    }
}
#[derive(Debug)]
struct StopState {
    stop: AtomicBool,
    armed: AtomicBool,
    finished: AtomicBool,
    thread: OnceLock<std::thread::ThreadId>,
    demand: Event,
}
#[derive(Clone, Debug)]
pub(super) struct StopToken(Arc<StopState>);
impl StopToken {
    pub(super) fn arm(&self) {
        self.0.armed.store(true, Ordering::Release);
        self.0.demand.signal();
    }
    pub(super) fn stop(&self) -> bool {
        self.0.stop.store(true, Ordering::Release);
        self.0.demand.signal();
        if self.0.thread.get() == Some(&std::thread::current().id()) {
            return true;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while !self.0.finished.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.0.finished.load(Ordering::Acquire)
    }
}
#[derive(Debug)]
pub(super) struct Prepared {
    session: Arc<Session>,
    stop: StopToken,
}
impl Prepared {
    pub(super) fn create(
        _channels: &[u32],
        frames: u32,
        host: Host,
        buffers: HostBuffers,
        position: Arc<AtomicU64>,
        nanos: Arc<AtomicU64>,
    ) -> Result<Self> {
        let deadline = Instant::now() + Duration::from_secs(5);
        // The DLL never starts a per-user application from a machine registration.
        let control = if let Ok(mapping) = ControlMapping::open() {
            SessionControl::Shared(mapping)
        } else {
            let memory = asio_proto::ControlBuf::new();
            let c = memory.control();
            c.initialize(std::process::id())
                .map_err(|_| Error::Hardware)?;
            c.set_sample_rate(48000);
            c.set_layout(Layout::Surround714);
            c.set_dac_period_frames(480);
            c.set_dac_generation(1);
            SessionControl::Silent(memory)
        };
        let silent = matches!(control, SessionControl::Silent(_));
        let c = control.control();
        let layout = c.layout().ok_or(Error::Hardware)?;
        if _channels
            .iter()
            .any(|&channel| channel >= layout.channels())
        {
            return Err(Error::InvalidParameter);
        }
        let rate = c.sample_rate();
        let lease = Lease {
            pid: std::process::id(),
            serial: SERIAL.fetch_add(1, Ordering::Relaxed),
        };
        c.acquire_lease(lease).map_err(|_| Error::InvalidMode)?;
        let mut rollback = LeaseRollback {
            control: &control,
            lease,
            armed: true,
        };
        let peer_pid = c.app_pid();
        let peer = Peer::open(peer_pid).map_err(|_| Error::Hardware)?;
        let geometry = Geometry {
            lanes: 1,
            input_channels: layout.channels(),
            output_channels: 2,
            max_frames: frames,
            sample_rate: rate,
            latency_bound_frames: c.latency_frames(),
        };
        let ring =
            RingMapping::create(lease.pid, lease.serial, geometry).map_err(|_| Error::Hardware)?;
        let work = Event::named(&asio_proto::work_event_name(lease.pid, lease.serial))
            .map_err(|_| Error::Hardware)?;
        let demand = Event::named(&asio_proto::demand_event_name(lease.pid, lease.serial))
            .map_err(|_| Error::Hardware)?;
        let done = Event::named(&asio_proto::done_event_name(lease.pid, lease.serial))
            .map_err(|_| Error::Hardware)?;
        let mut producer = Producer::new(ring.region(), [Some(&work), None], &done);
        producer
            .announce(geometry, lease.pid)
            .map_err(|_| Error::Hardware)?;
        let publisher = producer.publisher().ok_or(Error::Hardware)?;
        c.set_loaded_frames(0);
        c.set_generation_barrier_sequence(0);
        c.set_client_generation_ack(c.dac_generation());
        c.set_clock_fault(false);
        c.set_client_running(false);
        // Publish the buffer geometry only after all named objects/header are ready.
        c.set_buffer_frames(frames);
        work.signal();
        if !silent {
            loop {
                // No callback exists before start: preparation itself is quiescent.
                // Also acknowledge an epoch announced while createBuffers is waiting.
                c.set_generation_barrier_sequence(0);
                c.set_client_generation_ack(c.dac_generation());
                if producer.wait_ready(Duration::from_millis(10)) == Waited::Signalled {
                    break;
                }
                if Instant::now() >= deadline || c.lease() != lease || !peer.alive() {
                    c.set_buffer_frames(0);
                    return Err(Error::Hardware);
                }
            }
        }
        rollback.armed = false;
        drop(rollback);
        let stop = StopToken(Arc::new(StopState {
            stop: AtomicBool::new(true),
            armed: AtomicBool::new(false),
            finished: AtomicBool::new(true),
            thread: OnceLock::new(),
            demand: demand.clone(),
        }));
        Ok(Self {
            session: Arc::new(Session {
                control,
                lease,
                ring,
                publisher,
                work,
                demand,
                done,
                peer,
                peer_pid,
                host,
                buffers,
                layout,
                rate,
                frames,
                last_published: AtomicU32::new(0),
                position,
                nanos,
            }),
            stop,
        })
    }
    pub(super) fn stop_token(&self) -> StopToken {
        self.stop.clone()
    }
    pub(super) fn start(&mut self) -> Result {
        if !self.stop.0.finished.load(Ordering::Acquire) {
            return Err(Error::InvalidMode);
        }
        self.stop = StopToken(Arc::new(StopState {
            stop: AtomicBool::new(false),
            armed: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            thread: OnceLock::new(),
            demand: self.session.demand.clone(),
        }));
        let state = self.stop.0.clone();
        let session = self.session.clone();
        // Allocations and module retention precede the realtime worker.
        let scratch = vec![0.0; session.layout.channels() as usize * session.frames as usize];
        self.session.position.store(0, Ordering::Release);
        self.session
            .nanos
            .store(asio_sys::qpc_nanos(), Ordering::Release);
        match asio_sys::spawn_worker(move || {
            let _ = state.thread.set(std::thread::current().id());
            struct Finished(Arc<StopState>);
            impl Drop for Finished {
                fn drop(&mut self) {
                    self.0.finished.store(true, Ordering::Release);
                }
            }
            let _finished = Finished(state.clone());
            while !state.armed.load(Ordering::Acquire) && !state.stop.load(Ordering::Acquire) {
                state.demand.wait(Duration::from_millis(1));
            }
            // A panic never crosses into the DAW; owning Arcs retain callback storage
            // through unwind, and the next control operation sees a completed worker.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(&session, &state, scratch)
            }));
            session.control.control().set_client_running(false);
            session.host.worker_finished();
            drop(session);
        }) {
            Ok(_thread) => {
                // Spawn succeeded but the worker is not armed until start returns.
                // Persistent serial catches stop/start hidden between DAC polls.
                let c = self.session.control.control();
                c.set_client_run_serial(c.client_run_serial().wrapping_add(1));
                c.set_client_running(true);
                Ok(())
            }
            Err(_) => {
                self.stop.0.finished.store(true, Ordering::Release);
                self.session.control.control().set_client_running(false);
                Err(Error::Hardware)
            }
        }
    }
}
impl Drop for Prepared {
    fn drop(&mut self) {
        let _ = self.stop.stop();
    }
}
fn run(session: &Session, stop: &StopState, mut scratch: Vec<f32>) {
    let _mmcss = asio_sys::Mmcss::pro_audio().ok();
    let control = session.control.control();
    let mut tracker = ClockTracker::new(control.dac_generation(), control.demand());
    // A fresh start gets a new observation window. The engine must advance its
    // epoch on client_running's rising edge; ready/ACK still gate publication.
    control.set_clock_fault(false);
    let mut last_advance = asio_sys::qpc_nanos();
    let mut fallback_due = last_advance;
    let mut demand_seen = control.demand();
    let mut supplied = control.loaded_frames();
    let mut sequence = asio_proto::next_sequence(session.last_published.load(Ordering::Acquire));
    let mut acknowledged = control.client_generation_ack();
    let mut replacement_peer: Option<Peer> = None;
    let mut peer_pid = session.peer_pid;
    let mut half = 0;
    let mut frames = 0_u64;
    let period_nanos = u64::from(session.frames) * 1_000_000_000 / u64::from(session.rate);
    let mut reset_seen = control.reset_serial();
    let mut latency_seen = control.latency_frames();
    let mut fault_notified = false;
    let mut layout_notified = false;
    let mut sequence_notified = false;
    loop {
        if stop.stop.load(Ordering::Acquire) || control.lease() != session.lease {
            break;
        }
        let now = asio_sys::qpc_nanos();
        let generation = control.dac_generation();
        if generation != acknowledged {
            // This boundary is AFTER the preceding Host::switch returns, including
            // outputReady's publication. A late old-epoch write is in this barrier.
            control.set_generation_barrier_sequence(session.last_published.load(Ordering::Acquire));
            supplied = 0;
            control.set_loaded_frames(0);
            control.set_client_generation_ack(generation);
            acknowledged = generation;
            last_advance = now;
        }
        // Non-RT epoch transition, outside Host::switch/callbacks: retain the old
        // process identity until a new owner's ready acknowledgement is visible.
        // Failed opens stay in fallback and are retried, never treated as alive.
        let new_pid = control.app_pid();
        if new_pid != 0
            && new_pid != peer_pid
            && control.ready_generation() == generation
            && generation == acknowledged
            && let Ok(peer) = Peer::open(new_pid)
            && peer.alive()
            && control.app_pid() == new_pid
            && control.dac_generation() == generation
        {
            replacement_peer = Some(peer);
            peer_pid = new_pid;
        }
        let demand = control.demand();
        if demand != demand_seen {
            last_advance = now;
            demand_seen = demand;
        }
        let timeout = asio_proto::clock_fault_timeout(
            session.rate,
            session.frames,
            control.dac_period_frames(),
        );
        let epoch_ready = generation != 0
            && control.ready_generation() == generation
            && control.client_generation_ack() == generation;
        if !epoch_ready {
            last_advance = now;
        }
        let event = tracker.observe(
            generation,
            demand,
            replacement_peer.as_ref().unwrap_or(&session.peer).alive(),
            // Reset/ACK time is not a running DAC stall. A slow consumer must
            // not fault the new epoch before it can acknowledge readiness.
            if epoch_ready {
                Duration::from_nanos(now.saturating_sub(last_advance))
            } else {
                Duration::ZERO
            },
            timeout,
        );
        match event {
            ClockEvent::GenerationChanged | ClockEvent::Recovered => {
                last_advance = now;
                // Counters reset only at the quiescent ACK above, never after
                // the consumer has acknowledged ready for this generation.
                if event == ClockEvent::Recovered {
                    fault_notified = false;
                    control.set_clock_fault(false);
                }
            }
            ClockEvent::Faulted(_) => {
                control.set_clock_fault(true);
                if !fault_notified {
                    fault_notified = true;
                    fallback_due = now;
                    session.host.reset_request();
                }
            }
            _ => {}
        }
        let changed =
            control.layout() != Some(session.layout) || control.sample_rate() != session.rate;
        let reset = control.reset_serial();
        if (changed && !layout_notified) || reset != reset_seen {
            layout_notified |= changed;
            reset_seen = reset;
            session.host.reset_request();
        }
        let latency = control.latency_frames();
        if latency != latency_seen {
            latency_seen = latency;
            session.host.latencies_changed();
        }
        // The ring cannot wrap MAX -> 1: that would reuse the same odd slot.
        // Require a fresh session instead of publishing a wrapped sequence.
        let sequence_exhausted = sequence.is_none();
        if sequence_exhausted && !sequence_notified {
            sequence_notified = true;
            fallback_due = now;
            control.set_clock_fault(true);
            session.host.reset_request();
        }
        // reset_request may synchronously reenter stop/dispose on this worker.
        if stop.stop.load(Ordering::Acquire) {
            break;
        }
        let discard = tracker.fault().is_some() || changed || sequence_exhausted || !epoch_ready;
        let due = if discard {
            now >= fallback_due
        } else {
            asio_proto::deficit(
                demand,
                supplied,
                control.loaded_frames(),
                control.completed_frames(),
            ) > 0
        };
        let available = discard
            || sequence.is_some_and(|next| {
                asio_proto::can_publish(session.publisher.completed(session.ring.region(), 0), next)
            });
        if due && available {
            let position = Position { frames, nanos: now };
            session.position.store(frames, Ordering::Release);
            session.nanos.store(now, Ordering::Release);
            let invoked = session.host.switch(
                &session.buffers,
                half,
                position,
                session.rate,
                &mut scratch,
                |samples| {
                    // outputReady is reentrant but runs only on this callback thread. Recheck
                    // generation and cancellation before publishing its just-completed audio.
                    if !discard
                        && !stop.stop.load(Ordering::Acquire)
                        && control.dac_generation() == generation
                        && control.ready_generation() == generation
                        && let Some(next) = sequence
                    {
                        let published = session.publisher.try_publish(
                            session.ring.region(),
                            0,
                            next,
                            Some(&session.work),
                            |slot| {
                                slot.copy_from(samples);
                            },
                        );
                        if published == Publish::Published {
                            session.last_published.store(next, Ordering::Release);
                            sequence = asio_proto::next_sequence(next);
                            supplied = supplied.saturating_add(u64::from(session.frames));
                            control.set_loaded_frames(supplied);
                        }
                    }
                },
            );
            if !invoked {
                break;
            }
            frames = frames.saturating_add(u64::from(session.frames));
            half ^= 1;
            if discard {
                fallback_due = asio_proto::advance_deadline(fallback_due, now, period_nanos);
            }
            continue;
        }
        let wait = if discard {
            Duration::from_nanos(fallback_due.saturating_sub(now)).min(Duration::from_millis(2))
        } else {
            Duration::from_millis(2)
        };
        session.demand.wait_either(&session.done, wait);
    }
}
