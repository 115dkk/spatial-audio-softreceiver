#![allow(unsafe_code)]
//! Fixed-width atomic mappings and owned Windows handles. No SDK types.

use std::collections::HashMap;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use asio_proto::{CONTROL_BYTES, CONTROL_WORDS, Control, ControlMemory, Lease};
use streamring::{Geometry, Region, RegionMemory, Signal, Wait, Waited};
use windows::Win32::{
    Foundation::{
        ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    System::{
        Memory::{
            CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
            OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
        },
        Threading::{
            CreateEventW, CreateMutexW, GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW,
            SetEvent, WaitForSingleObject,
        },
    },
};
use windows::core::{PCWSTR, PWSTR};

use super::{Error, validation};

const OWNER_NAME: &str = r"Local\SpatialSoftReceiver.Asio.Owner";

fn wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(Some(0)).collect()
}

#[derive(Debug)]
struct Mapping {
    _handle: OwnedHandle,
    address: usize,
    bytes: usize,
}

impl Mapping {
    fn map(handle: OwnedHandle, bytes: usize) -> Result<Self, Error> {
        // SAFETY: owned section handle; Windows checks this exact nonzero view
        // length against the section before any Rust references are constructed.
        let view = unsafe {
            MapViewOfFile(
                HANDLE(handle.as_raw_handle()),
                FILE_MAP_ALL_ACCESS,
                0,
                0,
                bytes,
            )
        };
        if view.Value.is_null() {
            return Err(windows::core::Error::from_thread().into());
        }
        Ok(Self {
            _handle: handle,
            address: view.Value as usize,
            bytes,
        })
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: our unique view stays mapped until all borrowing slices and
        // Arc owners are gone. The owned section handle closes after this call.
        let _ = unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.address as *mut _,
            })
        };
    }
}

#[derive(Debug)]
pub(super) struct ControlMapping {
    mapping: Mapping,
    pid: u32,
    // An existence mutex, not a thread-owned lock. No ReleaseMutex or thread
    // affinity is needed. Retain it with every independently held control Arc.
    _owner: OwnedHandle,
}

impl ControlMapping {
    pub(super) fn create() -> Result<Self, Error> {
        let owner_name = wide(OWNER_NAME);
        // SAFETY: NUL-terminated session-local name, default noninheritable
        // security, and no initial thread ownership. Existence is exclusive.
        let owner = unsafe { CreateMutexW(None, false, PCWSTR(owner_name.as_ptr())) }?;
        // SAFETY: read immediately after CreateMutexW, before another Win32 call.
        let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        // SAFETY: successful CreateMutexW returned a unique owned handle.
        let owner = unsafe { OwnedHandle::from_raw_handle(owner.0) };
        if existed {
            return Err(Error::OwnerExists);
        }
        let name = wide(asio_proto::CONTROL_NAME);
        // SAFETY: paging-file section with fixed nonzero protocol length;
        // default security and no inherited handle; name is NUL-terminated.
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                CONTROL_BYTES as u32,
                PCWSTR(name.as_ptr()),
            )
        }?;
        // SAFETY: must observe CreateFileMappingW's last error before other calls.
        let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        // SAFETY: successful CreateFileMappingW transfers this handle to us.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        let mapping = Mapping::map(handle, CONTROL_BYTES)?;
        // SAFETY: GetCurrentProcessId has no preconditions.
        let pid = unsafe { GetCurrentProcessId() };
        let result = Self {
            mapping,
            pid,
            _owner: owner,
        };
        let control = result.control();
        if existed {
            control.validate()?;
            let previous_pid = control.app_pid();
            if !validation::owner_takeover_allowed(
                previous_pid,
                pid,
                previous_pid != pid && process_dead(previous_pid),
            ) {
                return Err(Error::OwnerAlive);
            }
            // Preserve the lease and counters in a surviving DAW's view. The
            // new output writer must advance generation before setting ready.
            control.set_ready(false);
            control.set_app_pid(pid);
        } else {
            control.initialize(pid)?;
        }
        Ok(result)
    }

    pub(super) fn is_owner(&self) -> bool {
        let control = self.control();
        control.validate().is_ok() && control.app_pid() == self.pid
    }
}

impl ControlMemory for ControlMapping {
    fn control(&self) -> Control<'_> {
        // SAFETY: page-aligned fixed CONTROL_BYTES view, exclusively AtomicU64
        // for its entire life; slices are borrowed and cannot outlive the view.
        let words = unsafe {
            std::slice::from_raw_parts(self.mapping.address as *const AtomicU64, CONTROL_WORDS)
        };
        Control::from_slice(words).expect("fixed control mapping length")
    }
}

#[derive(Debug)]
pub(super) struct RingMapping {
    mapping: Mapping,
    geometry: Geometry,
    prefix_bytes: usize,
    // The mapped suffix is reserved but unused by ASIO. Keep streamring's u64
    // heartbeat/error counters local so NO peer geometry can overlap widths.
    local_counters: [AtomicU64; 16],
}

impl RingMapping {
    pub(super) fn open(lease: Lease, geometry: Geometry) -> Result<Option<Arc<Self>>, Error> {
        // Preserve immutable geometry across hosts borrowing the same ring.
        // Soundness does not depend on this cache: every shared access is u32.
        static VIEWS: OnceLock<Mutex<HashMap<u64, Weak<RingMapping>>>> = OnceLock::new();
        let mut views = VIEWS
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        views.retain(|_, view| view.strong_count() != 0);
        if let Some(view) = views.get(&lease.to_wire()).and_then(Weak::upgrade) {
            if !validation::same_ring_geometry(view.geometry, geometry) {
                return Err(Error::ClientGeometry);
            }
            return Ok(Some(view));
        }
        let bytes = geometry.total_bytes().map_err(|_| Error::ClientGeometry)?;
        let name = wide(&asio_proto::ring_name(lease.pid, lease.serial));
        // SAFETY: NUL-terminated protocol name, requested access matches the
        // atomic consumer writes; this does not create or resize a peer section.
        let handle = match unsafe {
            OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, PCWSTR(name.as_ptr()))
        } {
            Ok(handle) => handle,
            Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        // SAFETY: successful OpenFileMappingW returned a unique owned handle.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        // Map the expected LOCAL length, not header.total or any other peer size.
        let mapping = Mapping::map(handle, bytes)?;
        // SAFETY: every valid local geometry maps at least the fixed u32 header.
        // Read only AtomicU32, including for an adversarial smaller peer geometry.
        let header = unsafe {
            std::slice::from_raw_parts(
                mapping.address as *const AtomicU32,
                streamring::HEADER_BYTES / 4,
            )
        };
        if u32::from_le(header[9].load(Ordering::Acquire)) == streamring::State::Empty as u32 {
            return Ok(None);
        }
        // A restarted app has no local creation cache. Adopt only this bounded
        // metadata value, never lengths/rate/layout; subsequent checks freeze it.
        let geometry = Geometry {
            latency_bound_frames: u32::from_le(header[8].load(Ordering::Acquire)),
            ..geometry
        };
        let view = Arc::new(Self {
            mapping,
            geometry,
            prefix_bytes: bytes - 128,
            local_counters: std::array::from_fn(|_| AtomicU64::new(0)),
        });
        views.insert(lease.to_wire(), Arc::downgrade(&view));
        Ok(Some(view))
    }

    pub(super) fn geometry(&self) -> Geometry {
        self.geometry
    }

    pub(super) fn validate_header(&self, expected: Geometry, pid: u32) -> Result<bool, Error> {
        // SAFETY: every accepted geometry has a >=512-byte AtomicU32 prefix.
        // Header access uses that same width even before full region admission.
        let words = unsafe {
            std::slice::from_raw_parts(
                self.mapping.address as *const AtomicU32,
                streamring::HEADER_BYTES / 4,
            )
        };
        let mut header = [0; streamring::HEADER_BYTES / 4];
        // Acquire the announcement before reading relaxed immutable geometry.
        header[9] = u32::from_le(words[9].load(Ordering::Acquire));
        if header[9] == streamring::State::Empty as u32 {
            return Ok(false);
        }
        for (index, value) in header.iter_mut().enumerate() {
            if index != 9 {
                *value = u32::from_le(words[index].load(Ordering::Acquire));
            }
        }
        if !validation::validate_header(&header, expected, pid, self.mapping.bytes) {
            return Err(Error::ClientGeometry);
        }
        Ok(true)
    }
}

impl RegionMemory for RingMapping {
    fn region(&self) -> Region<'_> {
        // SAFETY: immutable local split, bounded by the successfully mapped
        // exact view. The page-aligned prefix is only accessed as AtomicU32.
        let words32 = unsafe {
            std::slice::from_raw_parts(
                self.mapping.address as *const AtomicU32,
                self.prefix_bytes / 4,
            )
        };
        // Protocol-specific heartbeat/errors live in Control, not this local
        // streamring compatibility suffix. No generic atomic split is exposed.
        Region::new(words32, &self.local_counters)
    }
}

#[derive(Debug)]
pub(super) struct Event(OwnedHandle);

impl Event {
    pub(super) fn named(name: &str) -> Result<Self, Error> {
        let name = wide(name);
        // SAFETY: valid NUL-terminated session-local name; default security,
        // noninheritable auto-reset event initially unsignalled when created.
        let handle = unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) }?;
        // SAFETY: successful CreateEventW returned a unique handle to own.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle.0) }))
    }
}

impl Signal for Event {
    fn signal(&self) {
        // SAFETY: this event owns the handle throughout the nonblocking call.
        let _ = unsafe { SetEvent(HANDLE(self.0.as_raw_handle())) };
    }
}

impl Wait for Event {
    fn wait(&self, timeout: Duration) -> Waited {
        let milliseconds = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        // SAFETY: owned event stays alive during this finite, non-alertable wait.
        let status = unsafe { WaitForSingleObject(HANDLE(self.0.as_raw_handle()), milliseconds) };
        if status == WAIT_OBJECT_0 {
            Waited::Signalled
        } else if status == WAIT_TIMEOUT {
            Waited::TimedOut
        } else {
            Waited::PeerGone
        }
    }
}

/// Readers enter before loading the handle. Replacement keeps retired handles
/// until it observes no readers; sequential consistency orders that observation
/// against admission and publication. Only non-RT replace locks.
#[derive(Debug, Default)]
pub(super) struct DemandSignal {
    active: AtomicUsize,
    readers: AtomicUsize,
    retained: Mutex<Vec<Arc<Event>>>,
}

impl DemandSignal {
    pub(super) fn replace(&self, event: Option<Arc<Event>>) {
        let handle = event
            .as_ref()
            .map_or(0, |event| event.0.as_raw_handle() as usize);
        let mut retained = self
            .retained
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(event) = event {
            retained.push(event);
        }
        self.active.store(handle, Ordering::SeqCst);
        if self.readers.load(Ordering::SeqCst) == 0 {
            retained.retain(|event| event.0.as_raw_handle() as usize == handle);
        }
    }
}

impl Signal for DemandSignal {
    fn signal(&self) {
        self.readers.fetch_add(1, Ordering::SeqCst);
        let handle = self.active.load(Ordering::SeqCst);
        if handle != 0 {
            // SAFETY: the reader reservation precedes this load. A replacement
            // cannot retire its handle until all such readers have left; self
            // remains borrowed throughout the call and no lock is acquired.
            let _ = unsafe { SetEvent(HANDLE(handle as *mut _)) };
        }
        self.readers.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Retains the process object, not just its reusable PID. None from `open` is
/// unknown (including access denied); a missing PID is a confirmed dead peer.
/// Before the first successful open, PID reuse cannot be distinguished without
/// a process-instance identifier in the protocol. Open as soon as a lease is
/// observed, not only after its ring is ready, to narrow that unavoidable gap.
#[derive(Debug)]
pub(super) enum PeerHandle {
    Process(OwnedHandle),
    Gone,
}

impl PeerHandle {
    pub(super) fn open(pid: u32) -> Option<Self> {
        if pid == 0 {
            return None;
        }
        // SAFETY: OpenProcess validates the PID; only synchronization is requested.
        match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) } {
            Ok(handle) => {
                // SAFETY: successful OpenProcess transfers this live handle to us.
                Some(Self::Process(unsafe {
                    OwnedHandle::from_raw_handle(handle.0)
                }))
            }
            Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => Some(Self::Gone),
            Err(_) => None,
        }
    }

    pub(super) fn is_dead(&self) -> bool {
        let Self::Process(handle) = self else {
            return true;
        };
        // SAFETY: retained process handle with SYNCHRONIZE, zero bounded timeout.
        // WAIT_FAILED is unknown, never evidence of death.
        (unsafe { WaitForSingleObject(HANDLE(handle.as_raw_handle()), 0) }) == WAIT_OBJECT_0
    }
}

fn process_dead(pid: u32) -> bool {
    PeerHandle::open(pid).is_some_and(|peer| peer.is_dead())
}

pub(super) fn executable_basename(pid: u32) -> String {
    // SAFETY: OpenProcess validates PID and requests only image-query access.
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return String::new();
    };
    // SAFETY: successful OpenProcess transfers this handle to the owner.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let mut name = vec![0_u16; 32_768];
    let mut length = name.len() as u32;
    // SAFETY: owned query handle and writable UTF-16 buffer of length elements;
    // Windows writes the returned character count through length.
    if unsafe {
        QueryFullProcessImageNameW(
            HANDLE(handle.as_raw_handle()),
            PROCESS_NAME_WIN32,
            PWSTR(name.as_mut_ptr()),
            &mut length,
        )
    }
    .is_err()
    {
        return String::new();
    }
    let path = String::from_utf16_lossy(&name[..(length as usize).min(name.len())]);
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .to_owned()
}
