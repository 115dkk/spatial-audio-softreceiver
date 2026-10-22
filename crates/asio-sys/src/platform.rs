// SPDX-License-Identifier: GPL-3.0-or-later
// Naming/launch adaptation: EqualizerAPO-XT asio/HostProtocol.h,
// asio/AsioRegistration.cpp and asio/Clock.h, commit
// 3a6a213cf83a0606f00b8cce03b6781ee48d8c03 (115dkk, authorized MIT port).
// Atomic view isolation follows the mapping/events of plughost-sys (removed by
// ADR 0039; its successor is vst-audio-transport-sys).
#![allow(unsafe_code)]
use asio_proto::{CONTROL_BYTES, CONTROL_NAME, CONTROL_WORDS, Control, ControlMemory};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::{
    Arc,
    atomic::{AtomicU32, AtomicU64},
};
use std::time::Duration;
use streamring::{Geometry, Region, RegionMemory, Signal, Wait, Waited};
use windows::{
    Win32::{
        Foundation::{
            ERROR_ALREADY_EXISTS, FreeLibrary, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
            WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        System::{
            LibraryLoader::{
                FreeLibraryAndExitThread, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                GetModuleHandleExW,
            },
            Memory::*,
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            Threading::*,
        },
    },
    core::{PCWSTR, w},
};
/// Native thread trampoline owns a loader reference until all Rust captures are
/// destroyed. The final call never returns into the potentially unloaded DLL.
pub fn spawn_worker(work: impl FnOnce() + Send + 'static) -> windows::core::Result<()> {
    struct Work {
        run: Box<dyn FnOnce() + Send>,
        module: usize,
        guard: crate::WorkerGuard,
    }
    unsafe extern "system" fn entry(context: *mut std::ffi::c_void) -> u32 {
        // SAFETY: CreateThread receives exactly one Box allocated below.
        let Work { run, module, guard } = *unsafe { Box::from_raw(context.cast::<Work>()) };
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
            std::mem::forget(payload);
        }
        drop(guard);
        // SAFETY: our retained module reference is the thread's last resource;
        // this API atomically releases it and exits without returning to DLL code.
        unsafe {
            FreeLibraryAndExitThread(windows::Win32::Foundation::HMODULE(module as *mut _), 0)
        }
    }
    let mut module = windows::Win32::Foundation::HMODULE::default();
    // SAFETY: FROM_ADDRESS treats this pointer as a code address in this module.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            PCWSTR(entry as *const () as *const u16),
            &mut module,
        )
    }?;
    let work = Box::new(Work {
        run: Box::new(work),
        module: module.0 as usize,
        guard: crate::WorkerGuard::new(),
    });
    let raw = Box::into_raw(work);
    // SAFETY: entry owns raw exactly once on success; it is reclaimed below on failure.
    match unsafe {
        CreateThread(
            None,
            0,
            Some(entry),
            Some(raw.cast()),
            THREAD_CREATION_FLAGS(0),
            None,
        )
    } {
        Ok(handle) => {
            // SAFETY: unique thread handle; closing it does not stop the worker.
            drop(unsafe { OwnedHandle::from_raw_handle(handle.0) });
            Ok(())
        }
        Err(error) => {
            // SAFETY: no thread was created, so raw is still uniquely owned here.
            drop(unsafe { Box::from_raw(raw) });
            // SAFETY: release the module reference acquired above on spawn failure.
            let _ = unsafe { FreeLibrary(module) };
            Err(error)
        }
    }
}

fn invalid() -> windows::core::Error {
    windows::core::Error::from_hresult(windows::Win32::Foundation::E_INVALIDARG)
}
fn wide(name: &str) -> windows::core::Result<Vec<u16>> {
    if name.contains('\0') {
        return Err(invalid());
    }
    Ok(name.encode_utf16().chain(Some(0)).collect())
}
#[derive(Debug)]
struct Mapping {
    _handle: OwnedHandle,
    address: usize,
    bytes: usize,
}
impl Mapping {
    fn named(name: &str, bytes: usize, create: bool) -> windows::core::Result<Self> {
        let name = wide(name)?;
        let handle = if create {
            // SAFETY: fixed validated byte count and NUL-terminated name, no inheritance.
            let handle = unsafe {
                CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    None,
                    PAGE_READWRITE,
                    0,
                    bytes as u32,
                    PCWSTR(name.as_ptr()),
                )
            }?;
            // SAFETY: read thread-local last error immediately after creation.
            let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            // SAFETY: newly returned unique handle is transferred to RAII.
            let owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
            if exists {
                return Err(invalid());
            }
            owned
        } else {
            // SAFETY: valid NUL-terminated name, no inherited handle.
            let handle =
                unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, PCWSTR(name.as_ptr())) }?;
            // SAFETY: successful open returns unique owned handle.
            unsafe { OwnedHandle::from_raw_handle(handle.0) }
        };
        // SAFETY: Windows validates requested view fits the underlying mapping.
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
            return Err(windows::core::Error::from_thread());
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
        // SAFETY: uniquely owned view, all borrowed atomic references have expired.
        let _ = unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.address as *mut _,
            })
        };
    }
}
/// Control names and ring names are separate namespaces. This API never exposes
/// an untyped map: a control byte can only be AtomicU64, never AtomicU32.
#[derive(Debug)]
pub struct ControlMapping(Mapping);
impl ControlMapping {
    pub fn open() -> windows::core::Result<Self> {
        let value = Self(Mapping::named(CONTROL_NAME, CONTROL_BYTES, false)?);
        value.control().validate().map_err(|_| invalid())?;
        Ok(value)
    }
    pub fn create() -> windows::core::Result<Self> {
        Self::create_for_pid(std::process::id())
    }
    pub fn create_for_pid(pid: u32) -> windows::core::Result<Self> {
        let value = Self(Mapping::named(CONTROL_NAME, CONTROL_BYTES, true)?);
        value.control().initialize(pid).map_err(|_| invalid())?;
        Ok(value)
    }
}
impl ControlMemory for ControlMapping {
    fn control(&self) -> Control<'_> {
        // SAFETY: page-aligned, fixed CONTROL_BYTES view; every access has width u64.
        let words = unsafe {
            std::slice::from_raw_parts(self.0.address as *const AtomicU64, CONTROL_WORDS)
        };
        Control::from_slice(words).expect("fixed control geometry")
    }
}
/// Geometry is private and immutable per named view. All SHARED ring accesses
/// are AtomicU32, even if a peer announces a smaller geometry. The reserved
/// mapped 128-byte suffix is unused: streamring's u64 counters are process-local.
/// ASIO heartbeat/errors live in the separate fixed-u64 control mapping. There
/// is deliberately no generic map/split API or mutable byte slice.
#[derive(Debug)]
pub struct RingMapping {
    mapping: Mapping,
    geometry: Geometry,
    local_counters: [AtomicU64; 16],
}
impl RingMapping {
    pub fn create(pid: u32, serial: u32, geometry: Geometry) -> windows::core::Result<Self> {
        Self::map(pid, serial, geometry)
    }
    fn map(pid: u32, serial: u32, geometry: Geometry) -> windows::core::Result<Self> {
        let bytes = geometry.total_bytes().map_err(|_| invalid())?;
        let name = asio_proto::ring_name(pid, serial);
        // Creation is exclusive; all views use only shared u32 accesses.
        let mapping = Mapping::named(&name, bytes, true)?;
        Ok(Self {
            mapping,
            geometry,
            local_counters: std::array::from_fn(|_| AtomicU64::new(0)),
        })
    }
    pub fn geometry(&self) -> Geometry {
        self.geometry
    }
}
impl RegionMemory for RingMapping {
    fn region(&self) -> Region<'_> {
        let prefix = self.mapping.bytes - 128;
        // SAFETY: immutable geometry fixes the aligned u32 prefix for this name.
        let words32 = unsafe {
            std::slice::from_raw_parts(self.mapping.address as *const AtomicU32, prefix / 4)
        };
        Region::new(words32, &self.local_counters)
    }
}
#[derive(Clone, Debug)]
pub struct Event(Arc<OwnedHandle>);
impl Event {
    pub fn named(name: &str) -> windows::core::Result<Self> {
        let name = wide(name)?;
        // SAFETY: valid name; auto-reset noninheritable event, open-or-create semantics.
        let handle = unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) }?;
        // SAFETY: transfer owned event handle.
        Ok(Self(Arc::new(unsafe {
            OwnedHandle::from_raw_handle(handle.0)
        })))
    }
}
impl Event {
    pub fn wait_either(&self, other: &Self, timeout: Duration) -> Waited {
        let handles = [
            HANDLE(self.0.as_raw_handle()),
            HANDLE(other.0.as_raw_handle()),
        ];
        // SAFETY: both owned events stay live through this finite wait.
        let result =
            unsafe { WaitForMultipleObjects(&handles, false, asio_proto::wait_millis(timeout)) };
        if result == WAIT_OBJECT_0 || result.0 == WAIT_OBJECT_0.0 + 1 {
            Waited::Signalled
        } else if result == WAIT_TIMEOUT {
            Waited::TimedOut
        } else {
            Waited::PeerGone
        }
    }
}
impl Signal for Event {
    fn signal(&self) {
        // SAFETY: owned handle remains live throughout call.
        let _ = unsafe { SetEvent(HANDLE(self.0.as_raw_handle())) };
    }
}
impl Wait for Event {
    fn wait(&self, timeout: Duration) -> Waited {
        // SAFETY: live event; finite timeout never INFINITE.
        let result = unsafe {
            WaitForSingleObject(
                HANDLE(self.0.as_raw_handle()),
                asio_proto::wait_millis(timeout),
            )
        };
        if result == WAIT_OBJECT_0 {
            Waited::Signalled
        } else if result == WAIT_TIMEOUT {
            Waited::TimedOut
        } else {
            Waited::PeerGone
        }
    }
}
#[derive(Debug)]
pub struct Peer(OwnedHandle);
impl Peer {
    pub fn open(pid: u32) -> windows::core::Result<Self> {
        // SAFETY: OpenProcess validates PID; synchronization-only access.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }?;
        // SAFETY: transfer newly opened process handle.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle.0) }))
    }
    pub fn alive(&self) -> bool {
        // SAFETY: owned process handle, zero timeout is nonblocking.
        (unsafe { WaitForSingleObject(HANDLE(self.0.as_raw_handle()), 0) }) == WAIT_TIMEOUT
    }
}
#[derive(Debug)]
pub struct Mmcss {
    handle: HANDLE,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Mmcss {
    pub fn pro_audio() -> windows::core::Result<Self> {
        let mut index = 0;
        // SAFETY: constant task name and writable index on current thread.
        let handle = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut index) }?;
        Ok(Self {
            handle,
            _thread: std::marker::PhantomData,
        })
    }
}
impl Drop for Mmcss {
    fn drop(&mut self) {
        // SAFETY: registration is reverted on its creating thread (guard is !Send).
        let _ = unsafe { AvRevertMmThreadCharacteristics(self.handle) };
    }
}
pub fn qpc_nanos() -> u64 {
    static FREQUENCY: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    let frequency = *FREQUENCY.get_or_init(|| {
        let mut value = 0;
        // SAFETY: writable local LARGE_INTEGER.
        let _ = unsafe { QueryPerformanceFrequency(&mut value) };
        value.max(1)
    });
    let mut ticks = 0;
    // SAFETY: writable local LARGE_INTEGER.
    let _ = unsafe { QueryPerformanceCounter(&mut ticks) };
    ((ticks.max(0) as u128 * 1_000_000_000) / frequency as u128).min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn done_event_wakes_combined_wait_without_demand() {
        let prefix = format!("Local\\ssr-asio-wait-test-{}", std::process::id());
        let demand = Event::named(&format!("{prefix}-demand")).unwrap();
        let done = Event::named(&format!("{prefix}-done")).unwrap();
        done.signal();
        assert_eq!(
            demand.wait_either(&done, Duration::from_millis(100)),
            Waited::Signalled
        );
        demand.signal();
        assert_eq!(
            demand.wait_either(&done, Duration::from_millis(100)),
            Waited::Signalled
        );
    }
}
