//! App-side, SDK-independent ASIO transport (ADR 0025).
//!
//! `Local\\` names isolate ordinary logon sessions, not hostile processes in
//! the same session. The owner object is separate from the DAW's CAS lease.
//! Create/poll/configure on the control thread. The output thread may retain
//! `control_memory()` and `demand_signal()` independently of polling; their
//! atomic access and signal paths neither allocate nor acquire a lock.
//!
//! The app must publish ready only after preparing the engine and opening the
//! DAC. It owns generation/demand and must stop the old output writer before
//! replacing it. Polling never resets those counters. Same-geometry generations
//! retain the ring, even while ready=false: the consumer needs it to drain the
//! DLL's callback-quiescent barrier before acknowledging ready. Rate/layout
//! changes require a new lease/ring. Consumers wait for client_generation_ack,
//! discard through generation_barrier_sequence, reset DSP/queue/completed_frames,
//! then publish ready (after rechecking generation). `AsioClient::is_current`
//! gates rendering, NOT attachment/drain, and must be checked around DSP.
//! MMCSS is optional: call `crate::mmcss::promote("Pro Audio")` on the worker
//! and keep its thread-local guard there.

mod platform;
mod validation;

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Weak};
use std::time::Duration;

use asio_proto::{ControlMemory, Lease};
use streamring::{Geometry, RegionMemory, Signal, Wait, Waited};
pub use validation::OutputConfig;

use platform::{ControlMapping, DemandSignal, Event, PeerHandle, RingMapping};

/// Errors are diagnostic data, not localized user-facing messages.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Windows(#[from] windows::core::Error),
    #[error(transparent)]
    Control(#[from] asio_proto::ControlError),
    #[error("ASIO app owner already exists")]
    OwnerExists,
    #[error("ASIO control owner is not confirmed dead")]
    OwnerAlive,
    #[error("ASIO app no longer owns control")]
    NotOwner,
    #[error("invalid ASIO output configuration")]
    Configuration,
    #[error("ASIO client control or mapping geometry mismatch")]
    ClientGeometry,
}

/// A lease-bound ring, not proof that playback is running.
#[derive(Clone)]
pub struct AsioClient {
    pub lease: Lease,
    pub generation: u64,
    pub geometry: Geometry,
    pub memory: Arc<dyn RegionMemory>,
    pub done: Arc<dyn Signal>,
    pub executable: String,
}

impl fmt::Debug for AsioClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsioClient")
            .field("lease", &self.lease)
            .field("generation", &self.generation)
            .field("geometry", &self.geometry)
            .field("executable", &self.executable)
            .finish_non_exhaustive()
    }
}

impl AsioClient {
    /// Allocation-free admission check. Check again before committing processed
    /// audio: a peer can stop or change its lease during DSP.
    pub fn is_current(&self, memory: &dyn ControlMemory) -> bool {
        let control = memory.control();
        control.validate().is_ok()
            && control.ready()
            && control.client_running()
            && !control.clock_fault()
            && control.lease() == self.lease
            && control.dac_generation() == self.generation
            && control.client_generation_ack() == self.generation
            && control.sample_rate() == self.geometry.sample_rate
            && control
                .layout()
                .is_some_and(|layout| layout.channels() == self.geometry.input_channels)
            && control.buffer_frames() == self.geometry.max_frames
            && control.completed_frames() <= control.loaded_frames()
    }
}

#[derive(Debug)]
struct Attached {
    client: AsioClient,
    work: Arc<Event>,
    demand: Arc<Event>,
    mapping: Arc<RingMapping>,
}

#[derive(Debug)]
pub struct AsioHost {
    control: Arc<ControlMapping>,
    demand: Arc<DemandSignal>,
    config: OutputConfig,
    previous_generation: u64,
    attached: Option<Attached>,
    // Kept across detach/configure; only a different lease retires the handle.
    peer: validation::LeaseMonitor<PeerHandle>,
    // Preserve creation geometry (including header latency) across epochs.
    mappings: HashMap<u64, Weak<RingMapping>>,
}

impl AsioHost {
    pub fn create() -> Result<Self, Error> {
        let control = Arc::new(ControlMapping::create()?);
        let previous = control.control();
        let lease = previous.lease();
        let mut peer = validation::LeaseMonitor::new();
        let _ = peer.observe(lease, PeerHandle::open);
        let previous_generation = previous.dac_generation();
        let mut host = Self {
            control,
            previous_generation,
            demand: Arc::new(DemandSignal::default()),
            config: OutputConfig::default(),
            attached: None,
            peer,
            mappings: HashMap::new(),
        };
        host.configure(OutputConfig::default())?;
        Ok(host)
    }

    pub fn control_memory(&self) -> Arc<dyn ControlMemory> {
        self.control.clone()
    }

    pub fn demand_signal(&self) -> Arc<dyn Signal> {
        self.demand.clone()
    }

    /// Non-RT configuration publication. Leaves ready false; the caller must
    /// reopen/prepare the output and publish a new DAC generation before ready.
    /// Does not touch lease, client-owned fields, demand or output counters.
    pub fn configure(&mut self, config: OutputConfig) -> Result<(), Error> {
        if !(1_000..=1_000_000).contains(&config.sample_rate)
            || !(1..=streamring::MAX_FRAMES).contains(&config.dac_period_frames)
        {
            return Err(Error::Configuration);
        }
        self.check_owner()?;
        let control = self.control.control();
        control.set_ready(false);
        self.previous_generation = control.dac_generation();
        control.set_sample_rate(config.sample_rate);
        control.set_dac_period_frames(config.dac_period_frames);
        control.set_layout(config.layout);
        control.set_latency_frames(config.latency_frames);
        if self.config.requires_reset(config) {
            control.set_reset_serial(control.reset_serial().wrapping_add(1));
        }
        self.config = config;
        self.demand.signal();
        Ok(())
    }

    /// Allocation/lock-free. `true` is the app's assertion that engine and DAC
    /// are prepared and generation differs from the last configure/create.
    /// Shutdown clears this before notifying the peer. A caller publishing via
    /// control_memory must enforce the same order: ready=false, demand reset,
    /// new generation, then ready=true; generation and demand are separate
    /// atomic protocol words, not an atomic pair.
    pub fn set_ready(&self, ready: bool) -> Result<(), Error> {
        self.check_owner()?;
        if ready && self.control.control().dac_generation() == self.previous_generation {
            return Err(Error::Configuration);
        }
        self.control.control().set_ready(ready);
        self.demand.signal();
        Ok(())
    }

    /// Non-RT: checks actual process death, CAS-reclaims only the observed lease,
    /// validates client fields and opens its named objects. An unpublished ring
    /// or a changed lease returns None and is retried on the next poll.
    pub fn poll_client(&mut self) -> Result<Option<AsioClient>, Error> {
        self.check_owner()?;
        let control = self.control.control();
        let lease = control.lease();
        let dead = self
            .peer
            .observe(lease, PeerHandle::open)
            .is_some_and(PeerHandle::is_dead);
        self.mappings
            .retain(|_, mapping| mapping.strong_count() != 0);
        if lease.is_vacant() {
            self.detach();
            return Ok(None);
        }
        if dead {
            // Death belongs to the retained process and this exact token, never
            // a new process found by reopening the PID. CAS protects replacements.
            control.release_lease(lease);
            self.detach();
            return Ok(None);
        }
        if control.lease() != lease {
            return Ok(None);
        }
        let generation = control.dac_generation();
        let Some(geometry) = validation::client_geometry(control, self.config) else {
            // createBuffers acquires its lease before publishing buffer geometry.
            if control.buffer_frames() == 0 {
                return Ok(None);
            }
            return Err(Error::ClientGeometry);
        };
        if let Some(attached) = &mut self.attached
            && attached.client.lease == lease
        {
            if !validation::same_ring_geometry(attached.client.geometry, geometry) {
                // Keep the old immutable view alive until the DAW opens a new lease.
                return Ok(None);
            }
            if !attached
                .mapping
                .validate_header(attached.client.geometry, lease.pid)?
            {
                return Ok(None);
            }
            attached.client.generation = generation;
            return Ok(Some(attached.client.clone()));
        }
        self.detach();
        let mapping =
            if let Some(mapping) = self.mappings.get(&lease.to_wire()).and_then(Weak::upgrade) {
                if !validation::same_ring_geometry(mapping.geometry(), geometry) {
                    return Ok(None);
                }
                mapping
            } else {
                let Some(mapping) = RingMapping::open(lease, geometry)? else {
                    return Ok(None);
                };
                self.mappings
                    .insert(lease.to_wire(), Arc::downgrade(&mapping));
                mapping
            };
        let geometry = mapping.geometry();
        if !mapping.validate_header(geometry, lease.pid)? {
            return Ok(None);
        }
        let work = Arc::new(Event::named(&asio_proto::work_event_name(
            lease.pid,
            lease.serial,
        ))?);
        let done = Arc::new(Event::named(&asio_proto::done_event_name(
            lease.pid,
            lease.serial,
        ))?);
        let demand = Arc::new(Event::named(&asio_proto::demand_event_name(
            lease.pid,
            lease.serial,
        ))?);
        let executable = platform::executable_basename(lease.pid);
        let control = self.control.control();
        if control.lease() != lease || control.dac_generation() != generation {
            return Ok(None);
        }
        let client = AsioClient {
            lease,
            generation,
            geometry,
            memory: mapping.clone(),
            done,
            executable,
        };
        self.demand.replace(Some(demand.clone()));
        self.attached = Some(Attached {
            client: client.clone(),
            work,
            demand,
            mapping,
        });
        self.demand.signal();
        Ok(Some(client))
    }

    pub fn wait_client(&mut self, timeout: Duration) -> Result<asio_proto::ClientState, Error> {
        let _ = self.wait_work(timeout);
        self.check_owner()?;
        let control = self.control.control();
        let lease = control.lease();
        let alive = self
            .peer
            .observe(lease, PeerHandle::open)
            .is_some_and(|peer| !peer.is_dead());
        if !alive && !lease.is_vacant() {
            // Unknown process opens are not proof of death.
            if self
                .peer
                .observe(lease, PeerHandle::open)
                .is_some_and(PeerHandle::is_dead)
            {
                control.release_lease(lease);
            }
        }
        Ok(asio_proto::ClientState {
            lease: control.lease(),
            generation: control.dac_generation(),
            peer_alive: alive,
        })
    }

    /// Bounded worker wait; events are hints, never frame counts. No client
    /// means TimedOut immediately (the caller should use its normal poll cadence).
    pub fn wait_work(&self, timeout: Duration) -> Waited {
        self.attached
            .as_ref()
            .map_or(Waited::TimedOut, |attached| attached.work.wait(timeout))
    }

    fn check_owner(&self) -> Result<(), Error> {
        if self.control.is_owner() {
            Ok(())
        } else {
            Err(Error::NotOwner)
        }
    }

    fn detach(&mut self) {
        self.demand.replace(None);
        if let Some(attached) = self.attached.take() {
            attached.demand.signal();
            attached.client.done.signal();
        }
        self.mappings
            .retain(|_, mapping| mapping.strong_count() != 0);
    }
}

impl Drop for AsioHost {
    fn drop(&mut self) {
        if self.control.is_owner() {
            self.control.control().set_ready(false);
            self.demand.signal();
        }
        self.detach();
    }
}
