#![allow(unsafe_code)]

use std::{ffi::c_void, mem::size_of, ptr};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0,
            WAIT_TIMEOUT, WIN32_ERROR,
        },
        Media::KernelStreaming::*,
        Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
        System::{
            IO::{CancelIoEx, DeviceIoControl, GetOverlappedResult, OVERLAPPED},
            Threading::{CreateEventW, ResetEvent, WaitForSingleObject},
        },
    },
    core::{GUID, PCWSTR},
};

#[derive(Debug)]
pub(super) struct Handle(pub HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this owner closes its uniquely owned kernel handle exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}
impl Handle {
    pub fn filter(path: &str) -> Result<Self, WIN32_ERROR> {
        if path.contains('\0') {
            return Err(WIN32_ERROR(87));
        }
        let path: Vec<u16> = path.encode_utf16().chain([0]).collect();
        // SAFETY: terminated path lives through the call; output is uniquely owned.
        unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                None,
            )
        }
        .map(Self)
        .map_err(code)
    }
    pub fn event() -> Result<Self, WIN32_ERROR> {
        // SAFETY: unnamed manual-reset event with no borrowed security descriptor.
        unsafe { CreateEventW(None, true, false, None) }
            .map(Self)
            .map_err(code)
    }
    pub fn wait(&self, ms: u32) -> Result<bool, WIN32_ERROR> {
        // SAFETY: this owner keeps the event alive throughout the bounded wait.
        match unsafe { WaitForSingleObject(self.0, ms) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(WIN32_ERROR(6)),
        }
    }
    pub fn reset(&self) -> Result<(), WIN32_ERROR> {
        // SAFETY: the owned handle is an event.
        unsafe { ResetEvent(self.0) }.map_err(code)
    }
}

mod sealed {
    pub trait Sealed {}
}
/// Restricted to WDK POD outputs: all bit patterns are valid, no references,
/// Rust bools, owning pointers, or enum discriminants can be written by a driver.
pub(super) trait Pod: Copy + sealed::Sealed {}
macro_rules! pod { ($($t:ty),* $(,)?) => { $(impl sealed::Sealed for $t {} impl Pod for $t {})* }; }
pod!(
    u32,
    KSSTATE,
    KSPIN_DATAFLOW,
    KSPIN_COMMUNICATION,
    KSAUDIO_POSITION,
    KSRTAUDIO_BUFFER,
    KSRTAUDIO_HWREGISTER,
    KSRTAUDIO_HWLATENCY,
    KSMULTIPLE_ITEM,
    KSIDENTIFIER_0_0,
    KSDATAFORMAT_0
);

pub(super) fn code(e: windows::core::Error) -> WIN32_ERROR {
    WIN32_ERROR(e.code().0 as u32 & 0xffff)
}
pub(super) fn id(set: GUID, id: u32, flags: u32) -> KSIDENTIFIER {
    KSIDENTIFIER {
        Anonymous: KSIDENTIFIER_0 {
            Anonymous: KSIDENTIFIER_0_0 {
                Set: set,
                Id: id,
                Flags: flags,
            },
        },
    }
}
pub(super) fn pin_property(pin: u32, property: KSPROPERTY_PIN) -> KSP_PIN {
    KSP_PIN {
        Property: id(KSPROPSETID_Pin, property.0 as u32, KSPROPERTY_TYPE_GET),
        PinId: pin,
        ..Default::default()
    }
}

/// One preallocated OVERLAPPED and event. Every operation is reaped before reuse
/// or return, including timeout/cancellation; pointers into a stack request never
/// escape this method. Used only with plain WDK numeric/pointer structures.
pub(super) struct Control {
    event: Handle,
    overlapped: Box<OVERLAPPED>,
}
impl Control {
    pub fn new() -> Result<Self, WIN32_ERROR> {
        let event = Handle::event()?;
        Ok(Self {
            overlapped: Box::new(OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            }),
            event,
        })
    }
    pub fn query<I, O: Pod>(
        &mut self,
        handle: &Handle,
        request: &I,
        output: &mut O,
    ) -> Result<u32, WIN32_ERROR> {
        self.raw(
            handle,
            ptr::from_ref(request).cast(),
            size_of::<I>(),
            ptr::from_mut(output).cast(),
            size_of::<O>(),
        )
    }
    pub fn bytes<I>(
        &mut self,
        handle: &Handle,
        request: &I,
        output: &mut [u8],
    ) -> Result<u32, WIN32_ERROR> {
        self.raw(
            handle,
            ptr::from_ref(request).cast(),
            size_of::<I>(),
            output.as_mut_ptr().cast(),
            output.len(),
        )
    }
    fn raw(
        &mut self,
        handle: &Handle,
        request: *const c_void,
        request_len: usize,
        output: *mut c_void,
        output_len: usize,
    ) -> Result<u32, WIN32_ERROR> {
        self.event.reset()?;
        *self.overlapped = OVERLAPPED {
            hEvent: self.event.0,
            ..Default::default()
        };
        let mut returned = 0;
        // SAFETY: private callers provide valid WDK request/output buffers with
        // lengths checked from their storage. All pending I/O is reaped below.
        let result = unsafe {
            DeviceIoControl(
                handle.0,
                IOCTL_KS_PROPERTY,
                Some(request),
                request_len as u32,
                (output_len != 0).then_some(output),
                output_len as u32,
                Some(&mut returned),
                Some(&mut *self.overlapped),
            )
        };
        if let Err(error) = result {
            let error = code(error);
            if error != ERROR_IO_PENDING {
                return Err(error);
            }
            if self.event.wait(2000) != Ok(true) {
                // SAFETY: exactly this outstanding request belongs to this handle.
                let _ = unsafe { CancelIoEx(handle.0, Some(&*self.overlapped)) };
                // SAFETY: must drain even a broken driver before releasing request
                // memory; cancellation is not completion. Blocking here is teardown.
                let _ = unsafe {
                    GetOverlappedResult(handle.0, &*self.overlapped, &mut returned, true)
                };
                return Err(WIN32_ERROR(1460));
            }
            // SAFETY: request/event remain owned and signalled; retrieves completion.
            unsafe { GetOverlappedResult(handle.0, &*self.overlapped, &mut returned, true) }
                .map_err(code)?;
        }
        Ok(returned)
    }
    pub fn get<O: Default + Pod>(
        &mut self,
        handle: &Handle,
        set: GUID,
        property: u32,
    ) -> Result<O, WIN32_ERROR> {
        let mut output = O::default();
        let size = self.query(handle, &id(set, property, KSPROPERTY_TYPE_GET), &mut output)?;
        if size as usize != size_of::<O>() {
            return Err(WIN32_ERROR(13));
        }
        Ok(output)
    }
    pub fn state(&mut self, pin: &Handle, state: KSSTATE) -> Result<(), WIN32_ERROR> {
        let mut state = state;
        self.query(
            pin,
            &id(
                KSPROPSETID_Connection,
                KSPROPERTY_CONNECTION_STATE.0 as u32,
                KSPROPERTY_TYPE_SET,
            ),
            &mut state,
        )
        .map(|_| ())
    }
}

/// The buffer, header, OVERLAPPED and event retain fixed heap addresses until
/// completion. The stream owner cancels/drains every slot before dropping pin.
pub(super) struct Packet {
    pub bytes: Box<[u8]>,
    header: Box<KSSTREAM_HEADER>,
    overlapped: Box<OVERLAPPED>,
    event: Handle,
    pending: bool,
}
impl Packet {
    pub fn new(size: usize) -> Result<Self, WIN32_ERROR> {
        let event = Handle::event()?;
        let mut bytes = vec![0; size].into_boxed_slice();
        let header = Box::new(KSSTREAM_HEADER {
            Size: size_of::<KSSTREAM_HEADER>() as u32,
            FrameExtent: size as u32,
            DataUsed: size as u32,
            Data: bytes.as_mut_ptr().cast(),
            PresentationTime: KSTIME {
                Time: 0,
                Numerator: 1,
                Denominator: 1,
            },
            ..Default::default()
        });
        Ok(Self {
            bytes,
            header,
            overlapped: Box::new(OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            }),
            event,
            pending: false,
        })
    }
    pub fn submit(&mut self, pin: &Handle) -> Result<(), WIN32_ERROR> {
        debug_assert!(!self.pending);
        self.event.reset()?;
        *self.overlapped = OVERLAPPED {
            hEvent: self.event.0,
            ..Default::default()
        };
        self.header.DataUsed = self.bytes.len() as u32;
        let mut returned = 0;
        // SAFETY: KS write's header is its output buffer (METHOD_NEITHER). Header,
        // data, OVERLAPPED and event have fixed addresses until reaped by drain.
        let result = unsafe {
            DeviceIoControl(
                pin.0,
                IOCTL_KS_WRITE_STREAM,
                None,
                0,
                Some(ptr::from_mut(&mut *self.header).cast()),
                size_of::<KSSTREAM_HEADER>() as u32,
                Some(&mut returned),
                Some(&mut *self.overlapped),
            )
        };
        match result {
            Ok(()) => Ok(()),
            Err(e) => {
                let e = code(e);
                if e == ERROR_IO_PENDING {
                    self.pending = true;
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }
    pub fn ready(&mut self, pin: &Handle, ms: u32) -> Result<bool, WIN32_ERROR> {
        if !self.pending {
            return Ok(true);
        }
        if !self.event.wait(ms)? {
            return Ok(false);
        }
        let mut returned = 0;
        // SAFETY: outstanding request storage remains live; event signalled.
        let result = unsafe { GetOverlappedResult(pin.0, &*self.overlapped, &mut returned, false) };
        match result {
            Ok(()) => {
                self.pending = false;
                Ok(true)
            }
            Err(e) => {
                let e = code(e);
                if e.0 == 996 {
                    return Ok(false);
                } // ERROR_IO_INCOMPLETE: still owns pointers.
                self.pending = false;
                Err(e)
            }
        }
    }
    pub fn drain(&mut self, pin: &Handle) {
        if self.pending {
            // SAFETY: cancels only the request whose storage this packet owns.
            let _ = unsafe { CancelIoEx(pin.0, Some(&*self.overlapped)) };
            let mut returned = 0;
            // SAFETY: cancellation may be asynchronous; wait for final completion
            // before any buffer/header/OVERLAPPED/event is released.
            let _ = unsafe { GetOverlappedResult(pin.0, &*self.overlapped, &mut returned, true) };
            self.pending = false;
        }
    }
}

/// Copy only validated driver response bytes into an all-bit-patterns-valid WDK
/// numeric structure. Private callers never instantiate Rust enums or references.
pub(super) fn read<T: Pod>(bytes: &[u8], offset: usize) -> Option<T> {
    let end = offset.checked_add(size_of::<T>())?;
    let bytes = bytes.get(offset..end)?;
    // SAFETY: range is exactly sizeof(T); unaligned reads support packed driver
    // responses. All callers choose plain numeric WDK types, not Rust references.
    Some(unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<T>()) })
}

pub(super) fn variable<I>(
    control: &mut Control,
    handle: &Handle,
    request: &I,
) -> Result<Vec<u8>, WIN32_ERROR> {
    // Bounded open-time allocation, no driver-controlled allocation sizes.
    let mut bytes = vec![0; 65536];
    let size = control.bytes(handle, request, &mut bytes)? as usize;
    if size > bytes.len() {
        return Err(WIN32_ERROR(13));
    }
    bytes.truncate(size);
    Ok(bytes)
}
