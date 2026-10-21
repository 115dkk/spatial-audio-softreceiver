// SPDX-License-Identifier: GPL-3.0-or-later
// Ported COM identity/refcount: EqualizerAPO-XT asio/ComObject.h and
// asio/AsioWrapper.cpp at 3a6a213cf83a0606f00b8cce03b6781ee48d8c03.
// Copyright 2026 115dkk; authorized MIT port, SDK boundary under GPL.
#![allow(unsafe_code, non_snake_case)]
use crate::{AsioDriver, Error, Factory, Host, HostBuffers, sdk::*};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

pub(crate) static LIVE: AtomicU32 = AtomicU32::new(0);
static LOCKS: AtomicU32 = AtomicU32::new(0);
pub(crate) const CLASS: Guid = Guid {
    data1: 0x39d4f08a,
    data2: 0x7b4c,
    data3: 0x4b32,
    data4: [0x94, 0xa7, 0xc0, 0x74, 0xbb, 0x51, 0xbf, 0xf9],
};
const UNKNOWN: Guid = Guid {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
pub(crate) const FACTORY: Guid = Guid {
    data1: 1,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
const E_POINTER: i32 = 0x80004003_u32 as i32;
const E_FAIL: i32 = 0x80004005_u32 as i32;
const E_NOINTERFACE: i32 = 0x80004002_u32 as i32;
const CLASS_NOT_AVAILABLE: i32 = 0x80040111_u32 as i32;
fn guard<T>(fallback: T, call: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(call)) {
        Ok(value) => value,
        Err(payload) => {
            // An arbitrary panic payload may itself panic when dropped.
            std::mem::forget(payload);
            fallback
        }
    }
}
macro_rules! object_guard {
    ($this:expr, $fallback:expr, $call:expr) => {{
        // SAFETY: each object entry retains its caller's COM reference until return.
        // release is deliberately excluded: it can destroy that reference.
        let current = unsafe { object($this) };
        guard($fallback, || match catch_unwind(AssertUnwindSafe($call)) {
            Ok(value) => value,
            Err(payload) => {
                current.panicked.store(true, Ordering::Release);
                std::mem::forget(payload);
                $fallback
            }
        })
    }};
}
fn error(value: crate::Result) -> i32 {
    match value {
        Ok(()) => ASE_OK,
        Err(e) => match e {
            Error::NotPresent => ASE_NotPresent,
            Error::Hardware => ASE_HWMalfunction,
            Error::InvalidParameter => ASE_InvalidParameter,
            Error::InvalidMode => ASE_InvalidMode,
            Error::NotAdvancing => ASE_SPNotAdvancing,
            Error::NoClock => ASE_NoClock,
            Error::NoMemory => ASE_NoMemory,
        },
    }
}
#[repr(C)]
struct Object {
    vtable: &'static AsioVtable,
    refs: AtomicU32,
    panicked: AtomicBool,
    panic_message: &'static str,
    driver: Arc<dyn AsioDriver>,
    host: std::sync::Mutex<Option<Host>>,
    // Fields drop in declaration order, including during unwinding.
    _live: LiveObject,
}
struct LiveObject;
impl Drop for LiveObject {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Drop for Object {
    fn drop(&mut self) {
        if let Some(host) = self
            .host
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            host.retire();
        }
        guard((), || {
            let _ = self.driver.stop();
        });
        guard((), || {
            let _ = self.driver.dispose_buffers();
        });
    }
}
// These helpers are private and unsafe: no safe public function accepts a raw
// COM pointer. The host must honor COM lifetime and writable buffer contracts.
unsafe fn object<'a>(this: *mut c_void) -> &'a Object {
    // SAFETY: every caller is a COM entry on a live Object with an outstanding ref.
    unsafe { &*this.cast::<Object>() }
}
unsafe fn put<T>(target: *mut T, value: T) -> bool {
    if target.is_null() {
        return false;
    }
    // SAFETY: foreign method contract provides a writable output of type T.
    unsafe {
        target.write_unaligned(value);
    }
    true
}
unsafe fn text(target: *mut u8, capacity: usize, value: &str) {
    if target.is_null() {
        return;
    }
    let mut count = value.len().min(capacity.saturating_sub(1));
    while !value.is_char_boundary(count) {
        count -= 1;
    }
    // SAFETY: host provides the SDK-specified writable fixed-size string buffer.
    unsafe {
        std::ptr::copy_nonoverlapping(value.as_ptr(), target, count);
    }
    // SAFETY: count < capacity and target denotes the host's writable buffer.
    unsafe {
        target.wrapping_add(count).write(0);
    }
}
unsafe extern "system" fn query(this: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32 {
    object_guard!(this, E_FAIL, || {
        if iid.is_null() || out.is_null() {
            return E_POINTER;
        }
        // SAFETY: COM caller supplies a readable IID for this invocation.
        let iid = unsafe { iid.read_unaligned() };
        // SAFETY: COM output pointer is valid and writable.
        unsafe {
            put(out, std::ptr::null_mut());
        }
        if iid != UNKNOWN && iid != CLASS {
            return E_NOINTERFACE;
        }
        // SAFETY: this is the live object owning this vtable.
        unsafe {
            add_ref(this);
        }
        // SAFETY: validated host output.
        unsafe {
            put(out, this);
        }
        0
    })
}
unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    object_guard!(this, 0, || {
        // SAFETY: live COM object protected by caller's current reference.
        unsafe { object(this) }.refs.fetch_add(1, Ordering::Relaxed) + 1
    })
}
unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    guard(0, || {
        // SAFETY: caller releases one live reference, at most once.
        let remaining = unsafe { object(this) }.refs.fetch_sub(1, Ordering::AcqRel) - 1;
        if remaining == 0 {
            // SAFETY: final reference owns the object until it is reclaimed below.
            let current = unsafe { object(this) };
            let host = match current.host.try_lock() {
                Ok(host) => host.clone(),
                Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner().clone(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    current.refs.store(1, Ordering::Release);
                    return 0;
                }
            };
            if let Some(host) = &host {
                host.retire();
            }
            object_guard!(this, (), || {
                let _ = current.driver.stop();
            });
            // A hung/reentrant host may still call outputReady through this COM
            // pointer. Quarantine the object itself, not only its sample buffers.
            // Retained LIVE count prevents DLL unloading; leaking on this exceptional
            // path is preferable to executing freed DAW/DLL state.
            if let Some(host) = host.as_ref().filter(|host| !host.drained()) {
                // SAFETY: final reference is transferred, not made visible to callers.
                unsafe { host.defer_object(this) };
                // Cover a worker that finished between the drain check and transfer.
                host.worker_finished();
                return 0;
            }
            // SAFETY: final reference uniquely reclaims the Box allocated by create_instance.
            drop(unsafe { Box::from_raw(this.cast::<Object>()) });
        }
        remaining
    })
}
/// # Safety
/// The caller owns the final deferred reference and all callbacks have returned.
pub(crate) unsafe fn reclaim_object(this: *mut c_void) {
    guard((), || {
        // SAFETY: the deferred reference uniquely owns this allocation.
        drop(unsafe { Box::from_raw(this.cast::<Object>()) });
    });
}
macro_rules! simple {
    ($name:ident,$method:ident) => {
        unsafe extern "system" fn $name(this: *mut c_void) -> i32 {
            object_guard!(this, ASE_HWMalfunction, || {
                // SAFETY: COM dispatch keeps the object alive throughout this entry.
                error(unsafe { object(this) }.driver.$method())
            })
        }
    };
}
simple!(start, start);
simple!(stop, stop);
unsafe extern "system" fn dispose(this: *mut c_void) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        // SAFETY: COM caller retains this object for the entry.
        let object = unsafe { object(this) };
        let host = match object.host.try_lock() {
            Ok(host) => host.clone(),
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner().clone(),
            Err(std::sync::TryLockError::WouldBlock) => return ASE_HWMalfunction,
        };
        if let Some(host) = &host {
            host.retire();
        }
        let result = object.driver.dispose_buffers();
        if result.is_ok()
            && host.as_ref().is_none_or(Host::drained)
            && let Ok(mut slot) = object.host.try_lock()
        {
            *slot = None;
        }
        error(result)
    })
}
simple!(output, output_ready);
unsafe extern "system" fn init(this: *mut c_void, _window: *mut c_void) -> i32 {
    object_guard!(this, ASIOFalse, || {
        // SAFETY: COM dispatch keeps this object alive.
        i32::from(unsafe { object(this) }.driver.init())
    })
}
unsafe extern "system" fn name(this: *mut c_void, out: *mut u8) {
    object_guard!(this, (), || {
        // SAFETY: caller retains the live COM object throughout this entry.
        let name = unsafe { object(this) }.driver.name();
        // SAFETY: IASIO getDriverName specifies a writable 32-byte buffer.
        unsafe {
            text(out, 32, name);
        }
    })
}
unsafe extern "system" fn version(this: *mut c_void) -> i32 {
    object_guard!(this, 0, || 1)
}
unsafe extern "system" fn message(this: *mut c_void, out: *mut u8) {
    // SAFETY: the caller retains a live COM reference through this entry.
    let current = unsafe { object(this) };
    let code = object_guard!(this, current.panic_message, || {
        if current.panicked.load(Ordering::Acquire) {
            current.panic_message
        } else {
            current.driver.error_code()
        }
    });
    // SAFETY: IASIO getErrorMessage specifies a writable 124-byte buffer.
    unsafe {
        text(out, 124, code);
    }
}
unsafe extern "system" fn channels(this: *mut c_void, input: *mut i32, output: *mut i32) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        if input.is_null() || output.is_null() {
            return ASE_InvalidParameter;
        }
        // SAFETY: live COM instance.
        let count = unsafe { object(this) }.driver.channels();
        // SAFETY: checked SDK output pointers.
        unsafe {
            put(input, 0);
        }
        // SAFETY: checked SDK output pointers.
        unsafe {
            put(output, count as i32);
        }
        ASE_OK
    })
}
unsafe extern "system" fn latencies(this: *mut c_void, input: *mut i32, output: *mut i32) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        if input.is_null() || output.is_null() {
            return ASE_InvalidParameter;
        }
        // SAFETY: live COM instance.
        let latency = unsafe { object(this) }
            .driver
            .latency()
            .min(i32::MAX as u32);
        // SAFETY: checked SDK output pointer.
        unsafe {
            put(input, 0);
        }
        // SAFETY: checked SDK output pointer.
        unsafe {
            put(output, latency as i32);
        }
        ASE_OK
    })
}
unsafe extern "system" fn buffer_size(
    this: *mut c_void,
    min: *mut i32,
    max: *mut i32,
    preferred: *mut i32,
    granularity: *mut i32,
) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        if [min, max, preferred, granularity]
            .iter()
            .any(|p| p.is_null())
        {
            return ASE_InvalidParameter;
        }
        // SAFETY: live COM instance.
        let value = unsafe { object(this) }.driver.preferred_frames();
        for (ptr, value) in [
            (min, 64),
            (max, 2048),
            (preferred, value as i32),
            (granularity, -1),
        ] {
            // SAFETY: each pointer is a validated SDK output.
            unsafe {
                put(ptr, value);
            }
        }
        ASE_OK
    })
}
fn rate(value: f64) -> Option<u32> {
    (value.is_finite() && (1000.0..=1_000_000.0).contains(&value) && value.fract() == 0.0)
        .then_some(value as u32)
}
unsafe extern "system" fn can_rate(this: *mut c_void, value: f64) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        // SAFETY: live COM instance.
        let driver = &unsafe { object(this) }.driver;
        if rate(value).is_some_and(|r| driver.can_sample_rate(r)) {
            ASE_OK
        } else {
            ASE_NoClock
        }
    })
}
unsafe extern "system" fn get_rate(this: *mut c_void, out: *mut f64) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        // SAFETY: live COM instance.
        let rate = unsafe { object(this) }.driver.sample_rate();
        // SAFETY: nullable SDK output checked by put.
        if unsafe { put(out, f64::from(rate)) } {
            ASE_OK
        } else {
            ASE_InvalidParameter
        }
    })
}
unsafe extern "system" fn set_rate(this: *mut c_void, value: f64) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        let Some(rate) = rate(value) else {
            return ASE_NoClock;
        };
        // SAFETY: live COM instance.
        error(unsafe { object(this) }.driver.set_sample_rate(rate))
    })
}
unsafe extern "system" fn clocks(this: *mut c_void, out: *mut ClockSource, count: *mut i32) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        if count.is_null() || out.is_null() {
            return ASE_InvalidParameter;
        }
        // SAFETY: count is a readable/writable host in-out parameter.
        if unsafe { count.read_unaligned() } < 1 {
            return ASE_InvalidParameter;
        }
        let mut clock = ClockSource {
            associatedChannel: -1,
            associatedGroup: -1,
            isCurrentSource: 1,
            ..Default::default()
        };
        clock.name[..3].copy_from_slice(b"DAC");
        // SAFETY: caller has capacity for one ClockSource.
        unsafe {
            put(out, clock);
        }
        // SAFETY: checked host output.
        unsafe {
            put(count, 1);
        }
        ASE_OK
    })
}
unsafe extern "system" fn clock_source(this: *mut c_void, value: i32) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        if value == 0 {
            ASE_OK
        } else {
            ASE_InvalidParameter
        }
    })
}
unsafe extern "system" fn position(
    this: *mut c_void,
    frames: *mut Samples,
    nanos: *mut Samples,
) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        if frames.is_null() || nanos.is_null() {
            return ASE_InvalidParameter;
        }
        // SAFETY: live COM instance.
        let position = unsafe { object(this) }.driver.position();
        // SAFETY: checked host output.
        unsafe {
            put(frames, position.frames.into());
        }
        // SAFETY: checked host output.
        unsafe {
            put(nanos, position.nanos.into());
        }
        ASE_OK
    })
}
unsafe extern "system" fn channel(this: *mut c_void, info: *mut ChannelInfo) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        if info.is_null() {
            return ASE_InvalidParameter;
        }
        // SAFETY: host owns a readable/writable ChannelInfo.
        let mut value = unsafe { info.read_unaligned() };
        if value.isInput != 0 || value.channel < 0 {
            return ASE_InvalidParameter;
        }
        // SAFETY: live COM instance.
        let driver = &unsafe { object(this) }.driver;
        let data = match driver.channel(value.channel as u32) {
            Ok(v) => v,
            Err(e) => return error(Err(e)),
        };
        value.isActive = i32::from(data.active);
        value.channelGroup = 0;
        value.sample_type = ASIOSTFloat32LSB;
        value.name = [0; 32];
        let bytes = data.name.as_bytes();
        let count = bytes.len().min(31);
        value.name[..count].copy_from_slice(&bytes[..count]);
        // SAFETY: host owns writable ChannelInfo.
        unsafe {
            put(info, value);
        }
        ASE_OK
    })
}
unsafe extern "system" fn create(
    this: *mut c_void,
    infos: *mut BufferInfo,
    count: i32,
    frames: i32,
    callbacks: *const Callbacks,
) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        if infos.is_null()
            || callbacks.is_null()
            || !(1..=18).contains(&count)
            || !asio_proto::accepts_buffer(frames as u32)
        {
            return ASE_InvalidParameter;
        }
        let mut indices = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            // SAFETY: host provides count readable BufferInfo records, count is bounded.
            let info = unsafe { infos.wrapping_add(index).read_unaligned() };
            if info.isInput != 0
                || info.channelNum < 0
                || indices.contains(&(info.channelNum as u32))
            {
                return ASE_InvalidParameter;
            }
            indices.push(info.channelNum as u32);
        }
        // SAFETY: host provides a readable callback table with lifetime through disposal.
        let callbacks = unsafe { callbacks.read_unaligned() };
        let switch = callbacks.bufferSwitch;
        let time_switch = callbacks.bufferSwitchTimeInfo;
        if switch.is_none() && time_switch.is_none() {
            return ASE_InvalidParameter;
        }
        let buffers = HostBuffers::new(&indices, frames as usize);
        // SAFETY: live COM instance.
        let object = unsafe { object(this) };
        let mut slot = match object.host.try_lock() {
            Ok(slot) => slot,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return ASE_HWMalfunction,
        };
        if slot.as_ref().is_some_and(|host| host.retired_and_drained()) {
            *slot = None;
        }
        if slot.is_some() {
            return ASE_InvalidMode;
        }
        let host = Host::from_callbacks(callbacks, buffers.clone());
        struct RetireOnFailure(Option<Host>);
        impl Drop for RetireOnFailure {
            fn drop(&mut self) {
                if let Some(host) = self.0.take() {
                    host.retire();
                }
            }
        }
        let mut cleanup = RetireOnFailure(Some(host.clone()));
        let result =
            object
                .driver
                .create_buffers(&indices, frames as u32, host.clone(), buffers.clone());
        if let Err(err) = result {
            host.retire();
            return error(Err(err));
        }
        *slot = Some(host);
        cleanup.0 = None;
        for (index, &channel) in indices.iter().enumerate() {
            let info = BufferInfo {
                isInput: 0,
                channelNum: channel as i32,
                buffers: [buffers.pointer(index, 0), buffers.pointer(index, 1)],
            };
            // SAFETY: caller provides count writable records; it retains buffer pointers
            // only until disposeBuffers, and writes only under ASIO callback ownership.
            unsafe {
                infos.wrapping_add(index).write_unaligned(info);
            }
        }
        ASE_OK
    })
}
unsafe extern "system" fn panel(this: *mut c_void) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || ASE_NotPresent)
}
unsafe extern "system" fn future(this: *mut c_void, selector: i32, _data: *mut c_void) -> i32 {
    object_guard!(this, ASE_HWMalfunction, || {
        if selector == kAsioCanTimeInfo {
            ASE_SUCCESS
        } else {
            ASE_InvalidParameter
        }
    })
}
static VTABLE: AsioVtable = AsioVtable {
    unknown: UnknownVtable {
        query,
        add_ref,
        release,
    },
    init,
    getDriverName: name,
    getDriverVersion: version,
    getErrorMessage: message,
    start,
    stop,
    getChannels: channels,
    getLatencies: latencies,
    getBufferSize: buffer_size,
    canSampleRate: can_rate,
    getSampleRate: get_rate,
    setSampleRate: set_rate,
    getClockSources: clocks,
    setClockSource: clock_source,
    getSamplePosition: position,
    getChannelInfo: channel,
    createBuffers: create,
    disposeBuffers: dispose,
    controlPanel: panel,
    future,
    outputReady: output,
};

#[repr(C)]
#[derive(Debug)]
pub(crate) struct FactoryVtable {
    pub unknown: UnknownVtable,
    pub create:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, *mut *mut c_void) -> i32,
    pub lock: unsafe extern "system" fn(*mut c_void, i32) -> i32,
}
#[repr(C)]
struct ClassFactory {
    vtable: &'static FactoryVtable,
    refs: AtomicU32,
}
unsafe extern "system" fn factory_query(
    this: *mut c_void,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> i32 {
    guard(E_FAIL, || {
        if iid.is_null() || out.is_null() {
            return E_POINTER;
        }
        // SAFETY: readable COM IID.
        let id = unsafe { iid.read_unaligned() };
        // SAFETY: writable COM output.
        unsafe {
            put(out, std::ptr::null_mut());
        }
        if id != UNKNOWN && id != FACTORY {
            return E_NOINTERFACE;
        }
        // SAFETY: live class factory.
        unsafe {
            factory_add(this);
        }
        // SAFETY: writable COM output.
        unsafe {
            put(out, this);
        }
        0
    })
}
unsafe extern "system" fn factory_add(this: *mut c_void) -> u32 {
    guard(0, || {
        // SAFETY: live class factory vtable dispatch.
        unsafe { &*this.cast::<ClassFactory>() }
            .refs
            .fetch_add(1, Ordering::Relaxed)
            + 1
    })
}
unsafe extern "system" fn factory_release(this: *mut c_void) -> u32 {
    guard(0, || {
        // SAFETY: caller releases one existing reference.
        let count = unsafe { &*this.cast::<ClassFactory>() }
            .refs
            .fetch_sub(1, Ordering::AcqRel)
            - 1;
        if count == 0 {
            // SAFETY: final reference reclaims uniquely allocated factory.
            drop(unsafe { Box::from_raw(this.cast::<ClassFactory>()) });
            LIVE.fetch_sub(1, Ordering::AcqRel);
        }
        count
    })
}
unsafe extern "system" fn create_instance(
    _this: *mut c_void,
    outer: *mut c_void,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> i32 {
    guard(E_FAIL, || {
        if out.is_null() || iid.is_null() {
            return E_POINTER;
        }
        // SAFETY: writable COM output.
        unsafe {
            put(out, std::ptr::null_mut());
        }
        if !outer.is_null() {
            return 0x80040110_u32 as i32;
        }
        // SAFETY: readable COM IID.
        let id = unsafe { iid.read_unaligned() };
        if id != UNKNOWN && id != CLASS {
            return E_NOINTERFACE;
        }
        let Some(factory) = inventory::iter::<Factory>.into_iter().next() else {
            return CLASS_NOT_AVAILABLE;
        };
        let driver = (factory.0)();
        let panic_message = driver.panic_message();
        let object = Box::new(Object {
            vtable: &VTABLE,
            refs: AtomicU32::new(1),
            panicked: AtomicBool::new(false),
            panic_message,
            driver,
            host: std::sync::Mutex::new(None),
            _live: LiveObject,
        });
        LIVE.fetch_add(1, Ordering::AcqRel);
        // SAFETY: writable output receives ownership of initial COM ref.
        unsafe {
            put(out, Box::into_raw(object).cast());
        }
        0
    })
}
unsafe extern "system" fn lock(_this: *mut c_void, value: i32) -> i32 {
    guard(E_FAIL, || {
        if value != 0 {
            LOCKS.fetch_add(1, Ordering::AcqRel);
        } else {
            let _ = LOCKS.try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1));
        }
        0
    })
}
static FACTORY_VTABLE: FactoryVtable = FactoryVtable {
    unknown: UnknownVtable {
        query: factory_query,
        add_ref: factory_add,
        release: factory_release,
    },
    create: create_instance,
    lock,
};
// Exports intentionally remain private unsafe functions. No safe Rust caller can
// manufacture a valid foreign output pointer or bypass COM lifetime contracts.
#[unsafe(no_mangle)]
unsafe extern "system" fn DllGetClassObject(
    class: *const Guid,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> i32 {
    guard(E_FAIL, || {
        if class.is_null() || iid.is_null() || out.is_null() {
            return E_POINTER;
        }
        // SAFETY: valid caller-provided CLSID.
        let requested = unsafe { class.read_unaligned() };
        // SAFETY: valid caller-provided IID.
        let interface = unsafe { iid.read_unaligned() };
        // SAFETY: writable COM output.
        unsafe {
            put(out, std::ptr::null_mut());
        }
        if requested != CLASS {
            return CLASS_NOT_AVAILABLE;
        }
        if interface != UNKNOWN && interface != FACTORY {
            return E_NOINTERFACE;
        }
        let factory = Box::new(ClassFactory {
            vtable: &FACTORY_VTABLE,
            refs: AtomicU32::new(1),
        });
        LIVE.fetch_add(1, Ordering::AcqRel);
        // SAFETY: output transfers initial class-factory reference to caller.
        unsafe {
            put(out, Box::into_raw(factory).cast());
        }
        0
    })
}
#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> i32 {
    guard(1, || {
        i32::from(LIVE.load(Ordering::Acquire) != 0 || LOCKS.load(Ordering::Acquire) != 0)
    })
}
pub(crate) fn export_anchor() -> usize {
    (DllGetClassObject as *const () as usize) ^ (DllCanUnloadNow as *const () as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::ptr::NonNull;
    use std::sync::Mutex;

    /// A COM output slot. `Option<NonNull<c_void>>` has the layout of a raw
    /// pointer with `None` as null, so the callee writes it directly and a
    /// pointer is only taken out of it once it is known to be non-null.
    fn slot(out: &mut Option<NonNull<c_void>>) -> *mut *mut c_void {
        (out as *mut Option<NonNull<c_void>>).cast()
    }

    static TEST_LOCK: Mutex<()> = Mutex::new(());
    thread_local! {
        static CREATED: RefCell<Option<Arc<Fake>>> = const { RefCell::new(None) };
        static CALLBACK_OBJECT: std::cell::Cell<Option<NonNull<c_void>>> = const { std::cell::Cell::new(None) };
    }
    const PANIC_INIT: u32 = 1;
    const PANIC_START: u32 = 2;
    const PANIC_MESSAGE: u32 = 4;
    const PANIC_STOP: u32 = 8;
    const PANIC_DISPOSE: u32 = 16;
    const PANIC_DROP: u32 = 32;
    #[derive(Debug, Default)]
    struct Fake {
        panic: AtomicU32,
        running: AtomicBool,
        prepared: Mutex<Option<(Host, HostBuffers)>>,
        commits: AtomicU32,
        stops: AtomicU32,
        check_unload_on_drop: AtomicBool,
    }
    impl Fake {
        fn check(&self, flag: u32) {
            assert_eq!(
                self.panic.load(Ordering::Acquire) & flag,
                0,
                "injected panic"
            );
        }
        fn pump(&self) -> bool {
            let (host, buffers) = self.prepared.lock().unwrap().clone().unwrap();
            host.switch(
                &buffers,
                0,
                crate::Position::default(),
                48000,
                &mut [0.; 128],
                |_| {
                    self.commits.fetch_add(1, Ordering::Relaxed);
                },
            )
        }
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            if self.check_unload_on_drop.load(Ordering::Acquire) {
                assert_eq!(DllCanUnloadNow(), 1);
            }
            self.check(PANIC_DROP);
        }
    }
    impl AsioDriver for Fake {
        fn name(&self) -> &'static str {
            "Test"
        }
        fn init(&self) -> bool {
            self.check(PANIC_INIT);
            true
        }
        fn error_code(&self) -> &'static str {
            self.check(PANIC_MESSAGE);
            "OK"
        }
        fn panic_message(&self) -> &'static str {
            "TEST_PANIC"
        }
        fn channels(&self) -> u32 {
            2
        }
        fn latency(&self) -> u32 {
            0
        }
        fn preferred_frames(&self) -> u32 {
            64
        }
        fn sample_rate(&self) -> u32 {
            48000
        }
        fn can_sample_rate(&self, rate: u32) -> bool {
            rate == 48000
        }
        fn set_sample_rate(&self, _: u32) -> crate::Result {
            Ok(())
        }
        fn position(&self) -> crate::Position {
            crate::Position::default()
        }
        fn channel(&self, _: u32) -> crate::Result<crate::Channel> {
            Ok(crate::Channel {
                name: "L".into(),
                active: true,
            })
        }
        fn create_buffers(
            &self,
            _: &[u32],
            _: u32,
            host: Host,
            buffers: HostBuffers,
        ) -> crate::Result {
            *self.prepared.lock().unwrap() = Some((host, buffers));
            Ok(())
        }
        fn dispose_buffers(&self) -> crate::Result {
            self.check(PANIC_DISPOSE);
            self.stop()?;
            self.prepared.lock().unwrap().take();
            Ok(())
        }
        fn start(&self) -> crate::Result {
            self.check(PANIC_START);
            self.running.store(true, Ordering::Release);
            Ok(())
        }
        fn stop(&self) -> crate::Result {
            self.check(PANIC_STOP);
            self.running.store(false, Ordering::Release);
            self.stops.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn output_ready(&self) -> crate::Result {
            Host::output_ready();
            Ok(())
        }
    }
    fn fake_factory() -> Arc<dyn AsioDriver> {
        let driver = Arc::new(Fake::default());
        CREATED.with(|slot| *slot.borrow_mut() = Some(driver.clone()));
        driver
    }
    inventory::submit! { Factory(fake_factory) }

    // Each invocation performs one ABI call with the test's live factory/object
    // reference and stack-owned SDK argument/output storage.
    macro_rules! invoke {
        ($call:expr) => {{
            // SAFETY: test retains the matching COM reference and argument storage.
            unsafe { $call }
        }};
    }
    fn instance() -> (*mut c_void, Arc<Fake>) {
        let mut factory = None;
        assert_eq!(
            invoke!(DllGetClassObject(&CLASS, &FACTORY, slot(&mut factory))),
            0
        );
        let factory = factory.expect("DllGetClassObject wrote a factory").as_ptr();
        let mut driver = None;
        assert_eq!(
            invoke!((FACTORY_VTABLE.create)(
                factory,
                std::ptr::null_mut(),
                &CLASS,
                slot(&mut driver)
            )),
            0
        );
        assert_eq!(invoke!(factory_release(factory)), 0);
        (
            driver.expect("CreateInstance wrote an object").as_ptr(),
            CREATED.with(|slot| slot.borrow_mut().take().unwrap()),
        )
    }
    fn error_message(driver: *mut c_void) -> String {
        let mut message = [0_u8; 124];
        invoke!((VTABLE.getErrorMessage)(driver, message.as_mut_ptr()));
        let end = message.iter().position(|&byte| byte == 0).unwrap();
        String::from_utf8(message[..end].to_vec()).unwrap()
    }
    unsafe extern "C" fn callback_return(_half: i32, _direct: i32) {}
    unsafe extern "C" fn callback_release(_half: i32, _direct: i32) {
        if let Some(object) = CALLBACK_OBJECT.with(std::cell::Cell::get) {
            assert_eq!(invoke!(dispose(object.as_ptr())), ASE_OK);
            assert_eq!(invoke!(release(object.as_ptr())), 0);
        }
    }
    #[test]
    fn callback_dispose_release_is_reclaimed_after_worker_drains() {
        let _serial = TEST_LOCK.lock().unwrap();
        let before = LIVE.load(Ordering::Acquire);
        let (object, driver) = instance();
        CALLBACK_OBJECT.with(|slot| slot.set(NonNull::new(object)));
        let mut info = BufferInfo {
            isInput: 0,
            channelNum: 0,
            buffers: [std::ptr::null_mut(); 2],
        };
        let callbacks = Callbacks {
            bufferSwitch: Some(callback_release),
            sampleRateDidChange: None,
            asioMessage: None,
            bufferSwitchTimeInfo: None,
        };
        assert_eq!(
            invoke!(create(object, &mut info, 1, 64, &callbacks)),
            ASE_OK
        );
        let retained = driver.prepared.lock().unwrap().clone().unwrap();
        assert!(driver.pump());
        assert_eq!(LIVE.load(Ordering::Acquire), before + 1);
        retained.0.worker_finished();
        assert_eq!(LIVE.load(Ordering::Acquire), before);
        retained.0.worker_finished();
        CALLBACK_OBJECT.with(|slot| slot.set(None));
    }
    unsafe extern "C" fn callback_twice(_half: i32, _direct: i32) {
        if let Some(object) = CALLBACK_OBJECT.with(std::cell::Cell::get) {
            invoke!((VTABLE.outputReady)(object.as_ptr()));
            invoke!((VTABLE.outputReady)(object.as_ptr()));
        }
    }
    #[test]
    fn actual_com_entries_contain_panics_and_preserve_error_messages() {
        let _serial = TEST_LOCK.lock().unwrap();
        for (flag, init_panic) in [
            (PANIC_INIT, true),
            (PANIC_START, false),
            (PANIC_MESSAGE, false),
        ] {
            let (object, driver) = instance();
            driver.panic.store(flag, Ordering::Release);
            if init_panic {
                assert_eq!(
                    invoke!((VTABLE.init)(object, std::ptr::null_mut())),
                    ASIOFalse
                );
            } else if flag == PANIC_START {
                assert_eq!(invoke!((VTABLE.start)(object)), ASE_HWMalfunction);
            }
            assert_eq!(error_message(object), "TEST_PANIC");
            assert_eq!(error_message(object), "TEST_PANIC");
            assert_eq!(invoke!(release(object)), 0);
        }
        let before = LIVE.load(Ordering::Acquire);
        let (object, driver) = instance();
        driver
            .panic
            .store(PANIC_STOP | PANIC_DISPOSE | PANIC_DROP, Ordering::Release);
        drop(driver);
        assert_eq!(invoke!(release(object)), 0);
        assert_eq!(LIVE.load(Ordering::Acquire), before);
    }
    #[test]
    fn actual_com_disposes_running_and_commits_each_half_once() {
        let _serial = TEST_LOCK.lock().unwrap();
        for callback in [
            callback_return as unsafe extern "C" fn(i32, i32),
            callback_twice,
        ] {
            let (object, driver) = instance();
            CALLBACK_OBJECT.with(|slot| slot.set(NonNull::new(object)));
            assert_eq!(
                invoke!((VTABLE.init)(object, std::ptr::null_mut())),
                ASIOTrue
            );
            let mut infos = [BufferInfo {
                isInput: 0,
                channelNum: 0,
                buffers: [std::ptr::null_mut(); 2],
            }];
            let callbacks = Callbacks {
                bufferSwitch: Some(callback),
                sampleRateDidChange: None,
                asioMessage: None,
                bufferSwitchTimeInfo: None,
            };
            assert_eq!(
                invoke!((VTABLE.createBuffers)(
                    object,
                    infos.as_mut_ptr(),
                    1,
                    64,
                    &callbacks
                )),
                ASE_OK
            );
            let retained = driver.prepared.lock().unwrap().clone().unwrap();
            assert_eq!(invoke!((VTABLE.start)(object)), ASE_OK);
            assert!(driver.pump());
            assert_eq!(driver.commits.load(Ordering::Acquire), 1);
            assert_eq!(invoke!((VTABLE.disposeBuffers)(object)), ASE_OK);
            assert!(!driver.running.load(Ordering::Acquire));
            assert_eq!(driver.stops.load(Ordering::Acquire), 1);
            assert!(!retained.0.switch(
                &retained.1,
                0,
                crate::Position::default(),
                48000,
                &mut [0.; 128],
                |_| panic!("retired")
            ));
            retained.0.reset_request();
            assert_eq!(invoke!(release(object)), 0);
            CALLBACK_OBJECT.with(|slot| slot.set(None));
        }
    }
    #[test]
    fn unload_stays_blocked_through_driver_field_destruction_and_worker() {
        let _serial = TEST_LOCK.lock().unwrap();
        let (object, driver) = instance();
        driver.check_unload_on_drop.store(true, Ordering::Release);
        drop(driver);
        assert_eq!(invoke!(release(object)), 0);
        let worker = crate::WorkerGuard::new();
        assert_eq!(DllCanUnloadNow(), 1);
        drop(worker);
        assert_eq!(DllCanUnloadNow(), 0);
    }
    #[test]
    fn com_factory_identity_and_reference_lifetime() {
        let _serial = TEST_LOCK.lock().unwrap();
        let mut output = None;
        assert_eq!(
            // SAFETY: valid CLSID/IID and live writable output.
            unsafe { DllGetClassObject(&CLASS, &FACTORY, slot(&mut output)) },
            0
        );
        let output = output.expect("DllGetClassObject wrote a factory").as_ptr();
        assert_eq!(DllCanUnloadNow(), 1);
        let mut unknown = None;
        assert_eq!(
            // SAFETY: live factory reference and writable query output.
            unsafe { factory_query(output, &UNKNOWN, slot(&mut unknown)) },
            0
        );
        let unknown = unknown.expect("QueryInterface wrote the factory").as_ptr();
        assert_eq!(unknown, output);
        // SAFETY: release the QueryInterface reference.
        assert_eq!(unsafe { factory_release(unknown) }, 1);
        // SAFETY: release the original factory reference.
        assert_eq!(unsafe { factory_release(output) }, 0);
    }
    #[test]
    fn text_truncation_keeps_utf8_boundaries_and_buffer_size_is_power_of_two() {
        let _serial = TEST_LOCK.lock().unwrap();
        let mut out = [255; 5];
        invoke!(text(out.as_mut_ptr(), 5, "가나다"));
        assert_eq!(&out[..4], &[0xea, 0xb0, 0x80, 0]);
        let (object, _) = instance();
        let (mut min, mut max, mut preferred, mut granularity) = (0, 0, 0, 0);
        assert_eq!(
            invoke!(buffer_size(
                object,
                &mut min,
                &mut max,
                &mut preferred,
                &mut granularity
            )),
            ASE_OK
        );
        assert_eq!((min, max, preferred, granularity), (64, 2048, 64, -1));
        assert_eq!(invoke!(release(object)), 0);
    }
    #[test]
    fn foreign_errors_keep_hresult_and_asio_return_domains() {
        assert_eq!(
            // SAFETY: null is explicitly supported as an invalid COM argument.
            unsafe { DllGetClassObject(std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) },
            E_POINTER
        );
        assert_eq!(guard(ASIOFalse, || panic!("init")), ASIOFalse);
        assert_eq!(
            guard(ASE_HWMalfunction, || panic!("asio")),
            ASE_HWMalfunction
        );
        assert_eq!(guard(E_FAIL, || panic!("com")), E_FAIL);
    }
}
