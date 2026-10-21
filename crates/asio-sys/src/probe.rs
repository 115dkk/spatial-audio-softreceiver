// SPDX-License-Identifier: GPL-3.0-or-later
// Probe call sequence informed by EqualizerAPO-XT Tests/AsioProbe at
// 3a6a213cf83a0606f00b8cce03b6781ee48d8c03 (115dkk, authorized MIT port).
#![allow(unsafe_code)]
//! Actual DLL-path COM probe; never consults COM registration.
use crate::{
    com::{CLASS, FACTORY, FactoryVtable},
    sdk::*,
};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
    },
    core::{PCWSTR, s},
};
static CLAIMED: AtomicBool = AtomicBool::new(false);
static DRIVER: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static PLANES: [AtomicUsize; 36] = [const { AtomicUsize::new(0) }; 36];
static CHANNELS: AtomicU32 = AtomicU32::new(0);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static RATE: AtomicU32 = AtomicU32::new(48000);
static CALLBACKS: AtomicU64 = AtomicU64::new(0);
static OUTPUT_READY: AtomicU64 = AtomicU64::new(0);
static RESETS: AtomicU64 = AtomicU64::new(0);
static POSITION: AtomicU64 = AtomicU64::new(0);
static TIMESTAMPS: AtomicU64 = AtomicU64::new(0);
fn caught(call: impl FnOnce()) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(call));
}
unsafe fn vtable<'a>(object: *mut c_void) -> &'a AsioVtable {
    // SAFETY: object was created by DllGetClassObject/CreateInstance and is retained.
    let pointer = unsafe { object.cast::<*const AsioVtable>().read() };
    // SAFETY: IASIO object starts with a live immutable vtable pointer.
    unsafe { &*pointer }
}
unsafe extern "C" fn switch(half: i32, direct: i32) {
    caught(|| {
        if !(0..=1).contains(&half) || direct != 1 {
            return;
        }
        let frames = FRAMES.load(Ordering::Acquire) as usize;
        let channels = CHANNELS.load(Ordering::Acquire) as usize;
        let start = POSITION.fetch_add(frames as u64, Ordering::Relaxed);
        let rate = f64::from(RATE.load(Ordering::Acquire));
        for channel in 0..channels {
            let pointer = PLANES[channel * 2 + half as usize].load(Ordering::Acquire) as *mut f32;
            if pointer.is_null() {
                continue;
            }
            for frame in 0..frames {
                let value = if channel < 2 {
                    ((start + frame as u64) as f64 * std::f64::consts::TAU * 1000.0 / rate).sin()
                        as f32
                        * 10_f32.powf(-12.0 / 20.0)
                } else {
                    0.0
                };
                // SAFETY: createBuffers provided frames writable f32 samples; this callback
                // owns this half until outputReady below. No reference aliases these writes.
                unsafe {
                    pointer.wrapping_add(frame).write(value);
                }
            }
        }
        CALLBACKS.fetch_add(1, Ordering::Relaxed);
        let driver = DRIVER.load(Ordering::Acquire);
        if !driver.is_null() {
            // SAFETY: driver's COM reference remains held until stop has drained callbacks.
            let table = unsafe { vtable(driver) };
            // SAFETY: reentrant call during the host's bufferSwitch as required by ASIO.
            let _ = unsafe { (table.outputReady)(driver) };
            OUTPUT_READY.fetch_add(1, Ordering::Relaxed);
        }
    })
}
unsafe extern "C" fn time_switch(time: *mut Time, half: i32, direct: i32) -> *mut Time {
    caught(|| {
        if !time.is_null() {
            // SAFETY: callback lends one readable Time for this invocation.
            let value = unsafe { time.read_unaligned() };
            if value.timeInfo.flags & 3 == 3 {
                TIMESTAMPS.fetch_add(1, Ordering::Relaxed);
            }
        }
        // SAFETY: forwarding same valid callback arguments within their scope.
        unsafe {
            switch(half, direct);
        }
    });
    time
}
unsafe extern "C" fn rate_changed(_rate: f64) {
    caught(|| {});
}
unsafe extern "C" fn message(
    selector: i32,
    _value: i32,
    _message: *mut c_void,
    _opt: *mut f64,
) -> i32 {
    std::panic::catch_unwind(|| {
        if selector == kAsioSupportsTimeInfo {
            1
        } else if selector == kAsioEngineVersion {
            2
        } else if selector == kAsioResetRequest {
            RESETS.fetch_add(1, Ordering::Relaxed);
            1
        } else {
            0
        }
    })
    .unwrap_or(0)
}
pub use crate::probe_report::Report;
#[derive(Debug)]
struct Loaded {
    module: HMODULE,
    /// Written by CreateInstance through `Option<NonNull>`'s null niche, which
    /// has a raw pointer's layout, so a null result stays `None`.
    object: Option<NonNull<c_void>>,
}
impl Drop for Loaded {
    fn drop(&mut self) {
        if let Some(object) = self.object {
            let object = object.as_ptr();
            // SAFETY: retained live IASIO reference.
            let table = unsafe { vtable(object) };
            // SAFETY: idempotent best-effort teardown on failure path.
            let stopped = unsafe { (table.stop)(object) };
            // SAFETY: live object; disposal refuses when an active callback cannot drain.
            let disposed = unsafe { (table.disposeBuffers)(object) };
            if stopped == ASE_HWMalfunction || disposed == ASE_HWMalfunction {
                return;
            }
            DRIVER.store(std::ptr::null_mut(), Ordering::Release);
            // SAFETY: release our unique outstanding COM reference after draining.
            unsafe {
                (table.unknown.release)(object);
            }
        }
        // SAFETY: no active callbacks or objects remain in this loaded module.
        let _ = unsafe { FreeLibrary(self.module) };
        CLAIMED.store(false, Ordering::Release);
    }
}
/// Loads a trusted DLL by canonical absolute path and exercises its real COM
/// exports. Native DLL code executes in this process; use only your built DLL.
pub fn run(
    path: &std::path::Path,
    seconds: u32,
    frames: u32,
) -> std::result::Result<Report, String> {
    if seconds == 0 || seconds > 3600 || !asio_proto::accepts_buffer(frames) {
        return Err("ASIO_PROBE_ARGUMENT".into());
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if CLAIMED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("ASIO_PROBE_BUSY".into());
    }
    // SAFETY: canonical NUL-terminated DLL path supplied by explicit probe request.
    let module = match unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) } {
        Ok(v) => v,
        Err(e) => {
            CLAIMED.store(false, Ordering::Release);
            return Err(e.to_string());
        }
    };
    let mut loaded = Loaded {
        module,
        object: None,
    };
    // SAFETY: module is retained; export name is a fixed C string.
    let entry =
        unsafe { GetProcAddress(module, s!("DllGetClassObject")) }.ok_or("ASIO_PROBE_EXPORT")?;
    // The output is `Option<NonNull>`, a raw pointer's layout with None as null.
    type GetClass =
        unsafe extern "system" fn(*const Guid, *const Guid, *mut Option<NonNull<c_void>>) -> i32;
    // SAFETY: standard COM export's exact signature on x64.
    let get_class: GetClass = unsafe { std::mem::transmute(entry) };
    let mut factory = None;
    // SAFETY: valid class/IID and writable output pointer.
    let result = unsafe { get_class(&CLASS, &FACTORY, &mut factory) };
    let Some(factory) = factory.filter(|_| result == 0) else {
        return Err("ASIO_PROBE_FACTORY".into());
    };
    let factory = factory.as_ptr();
    // SAFETY: returned IClassFactory object starts with its vtable.
    let table = unsafe { factory.cast::<*const FactoryVtable>().read() };
    // SAFETY: live factory's immutable vtable.
    let table = unsafe { &*table };
    // SAFETY: valid output pointer, no aggregation, IASIO interface uses CLSID.
    let result = unsafe {
        (table.create)(
            factory,
            std::ptr::null_mut(),
            &CLASS,
            (&raw mut loaded.object).cast(),
        )
    };
    // SAFETY: release one returned factory reference.
    unsafe {
        (table.unknown.release)(factory);
    }
    let Some(object) = loaded.object.filter(|_| result == 0) else {
        return Err("ASIO_PROBE_INSTANCE".into());
    };
    let object = object.as_ptr();
    // SAFETY: retained IASIO reference.
    let table = unsafe { vtable(object) };
    let mut report = Report {
        call_order: vec!["DllGetClassObject", "CreateInstance"],
        callbacks: 0,
        output_ready_calls: 0,
        timeinfo_callbacks: 0,
        reset_requests: 0,
        input_latency: 0,
        output_latency: 0,
        clock_fault_observed: false,
        loaded_frames: None,
        completed_frames: None,
        seconds,
        sample_rate: 0.0,
        buffer_frames: frames,
        output_channels: 0,
    };
    macro_rules! check {
        ($name:literal,$call:expr) => {{
            report.call_order.push($name);
            let result = $call;
            if result != ASE_OK {
                return Err(format!("{}:{}", $name, result));
            }
        }};
    }
    // SAFETY: live IASIO object and optional null system window.
    if unsafe { (table.init)(object, std::ptr::null_mut()) } != 1 {
        return Err("ASIO_PROBE_INIT".into());
    }
    report.call_order.push("init");
    let (mut inputs, mut outputs) = (0, 0);
    // SAFETY: writable channel outputs.
    check!("getChannels", unsafe {
        (table.getChannels)(object, &mut inputs, &mut outputs)
    });
    if inputs != 0 || !(1..=18).contains(&outputs) {
        return Err("ASIO_PROBE_CHANNELS".into());
    }
    report.output_channels = outputs as u32;
    let (mut minimum, mut maximum, mut preferred, mut granularity) = (0, 0, 0, 0);
    // SAFETY: four writable outputs.
    check!("getBufferSize", unsafe {
        (table.getBufferSize)(
            object,
            &mut minimum,
            &mut maximum,
            &mut preferred,
            &mut granularity,
        )
    });
    if minimum != 64
        || maximum != 2048
        || granularity != -1
        || !asio_proto::accepts_buffer(preferred as u32)
    {
        return Err("ASIO_PROBE_BUFFER_SIZE".into());
    }
    let mut rate = 0.0;
    // SAFETY: writable sample rate output.
    check!("getSampleRate", unsafe {
        (table.getSampleRate)(object, &mut rate)
    });
    report.sample_rate = rate;
    // SAFETY: supported finite rate returned by driver.
    check!("canSampleRate", unsafe {
        (table.canSampleRate)(object, rate)
    });
    let callbacks = Callbacks {
        bufferSwitch: Some(switch),
        sampleRateDidChange: Some(rate_changed),
        asioMessage: Some(message),
        bufferSwitchTimeInfo: Some(time_switch),
    };
    let mut infos: Vec<BufferInfo> = (0..outputs)
        .map(|channel| BufferInfo {
            isInput: 0,
            channelNum: channel,
            buffers: [std::ptr::null_mut(); 2],
        })
        .collect();
    // SAFETY: bounded output array and callback functions remain live until stop.
    check!("createBuffers", unsafe {
        (table.createBuffers)(
            object,
            infos.as_mut_ptr(),
            outputs,
            frames as i32,
            &callbacks,
        )
    });
    for (channel, info) in infos.iter().enumerate() {
        let buffers = info.buffers;
        for half in 0..2 {
            PLANES[channel * 2 + half].store(buffers[half] as usize, Ordering::Release);
        }
    }
    CHANNELS.store(outputs as u32, Ordering::Release);
    FRAMES.store(frames, Ordering::Release);
    RATE.store(rate as u32, Ordering::Release);
    for count in [&CALLBACKS, &OUTPUT_READY, &RESETS, &POSITION, &TIMESTAMPS] {
        count.store(0, Ordering::Release);
    }
    DRIVER.store(object, Ordering::Release);
    // SAFETY: valid latency outputs on live object.
    check!("getLatencies", unsafe {
        (table.getLatencies)(
            object,
            &mut report.input_latency,
            &mut report.output_latency,
        )
    });
    // SAFETY: prepared object and callback table/storage remain retained.
    check!("start", unsafe { (table.start)(object) });
    let deadline = std::time::Instant::now() + Duration::from_secs(u64::from(seconds));
    while std::time::Instant::now() < deadline {
        use asio_proto::ControlMemory;
        if let Ok(control) = crate::ControlMapping::open() {
            report.clock_fault_observed |= control.control().clock_fault();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // SAFETY: stop drains callbacks before reading observations/disposal.
    check!("stop", unsafe { (table.stop)(object) });
    use asio_proto::ControlMemory;
    if let Ok(control) = crate::ControlMapping::open() {
        report.loaded_frames = Some(control.control().loaded_frames());
        report.completed_frames = Some(control.control().completed_frames());
        report.clock_fault_observed |= control.control().clock_fault();
    }
    // SAFETY: stopped object may dispose buffers.
    check!("disposeBuffers", unsafe { (table.disposeBuffers)(object) });
    report.callbacks = CALLBACKS.load(Ordering::Acquire);
    report.output_ready_calls = OUTPUT_READY.load(Ordering::Acquire);
    report.timeinfo_callbacks = TIMESTAMPS.load(Ordering::Acquire);
    report.reset_requests = RESETS.load(Ordering::Acquire);
    Ok(report)
}
