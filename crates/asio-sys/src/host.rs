// SPDX-License-Identifier: GPL-3.0-or-later
// Ported pending-output semantics: EqualizerAPO-XT asio/AsioWrapper.cpp and
// asio/CallbackTrampolines.{h,cpp}, commit 3a6a213cf83a0606f00b8cce03b6781ee48d8c03.
// Copyright 2026 115dkk; authorized MIT port, SDK boundary under GPL.
#![allow(unsafe_code)]
use crate::{Position, sdk};
use std::cell::{Cell, UnsafeCell};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
struct Storage {
    planes: Box<[UnsafeCell<Box<[f32]>>]>,
    channels: Box<[u32]>,
    frames: usize,
    busy: AtomicBool,
}
// SAFETY: Rust never exposes references to foreign-owned samples. Only the thread
// owning busy calls the host and reads them afterwards/at synchronous outputReady.
// ASIO's host contract forbids writes outside that callback or after outputReady.
unsafe impl Send for Storage {}
// SAFETY: the busy CAS excludes simultaneous switches; pointers are private to COM.
unsafe impl Sync for Storage {}

/// Double buffers owned independently of the COM object. There is deliberately
/// no public slice/pointer accessor. Foreign writes are allowed only in the
/// callback, ending at outputReady or callback return. Hung callbacks retain the
/// Arc and DLL module; disposal never frees their storage underneath them.
#[derive(Clone, Debug)]
pub struct HostBuffers(Arc<Storage>);
impl HostBuffers {
    pub(crate) fn new(channels: &[u32], frames: usize) -> Self {
        Self(Arc::new(Storage {
            planes: (0..channels.len() * 2)
                .map(|_| UnsafeCell::new(vec![0.; frames].into_boxed_slice()))
                .collect(),
            channels: channels.into(),
            frames,
            busy: AtomicBool::new(false),
        }))
    }
    pub(crate) fn pointer(&self, channel: usize, half: usize) -> *mut std::ffi::c_void {
        // SAFETY: allocation stays fixed for the storage lifetime; no reference escapes.
        unsafe {
            (*self.0.planes[channel * 2 + half].get())
                .as_mut_ptr()
                .cast()
        }
    }
    fn copy(&self, half: usize, target: &mut [f32]) {
        let frames = self.0.frames;
        target.fill(0.0);
        for (slot, &channel) in self.0.channels.iter().enumerate() {
            let start = channel as usize * frames;
            if start + frames > target.len() {
                continue;
            }
            // SAFETY: only switch's busy owner calls copy after host commits this half;
            // no Rust mutable reference to that allocation exists and target is disjoint.
            let source = unsafe { &*self.0.planes[slot * 2 + half].get() };
            target[start..start + frames].copy_from_slice(source);
        }
    }
}

thread_local! { static PENDING: Cell<Option<*mut Pending<'static>>> = const { Cell::new(None) }; }
struct Pending<'a> {
    commit: Option<&'a mut dyn FnMut()>,
}
fn finish_pending() {
    PENDING.with(|pending| {
        if let Some(ptr) = pending.get() {
            // SAFETY: installed only for the synchronous scope of Host::switch on this
            // thread. Taking the closure before calling it makes reentry idempotent.
            let callback = unsafe { (&mut *ptr).commit.take() };
            if let Some(callback) = callback {
                callback();
            }
        }
    });
}
struct ClearPending;
impl Drop for ClearPending {
    fn drop(&mut self) {
        PENDING.with(|p| p.set(None));
    }
}
struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Debug)]
struct HostInner {
    callbacks: sdk::Callbacks,
    buffers: HostBuffers,
    retired: AtomicBool,
    deferred_object: std::sync::atomic::AtomicPtr<std::ffi::c_void>,
}
#[derive(Clone, Debug)]
pub struct Host(Arc<HostInner>);
impl Host {
    /// COM exclusively creates this capability while the foreign callback table
    /// is live. Revocation stops safe trait implementations retaining a clone
    /// from calling the host after dispose/release.
    pub(crate) fn from_callbacks(callbacks: sdk::Callbacks, buffers: HostBuffers) -> Self {
        Self(Arc::new(HostInner {
            callbacks,
            buffers,
            retired: AtomicBool::new(false),
            deferred_object: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()),
        }))
    }
    /// # Safety
    /// `object` is the uniquely owned final COM reference, reclaimed only here.
    pub(crate) unsafe fn defer_object(&self, object: *mut std::ffi::c_void) {
        self.0.deferred_object.store(object, Ordering::Release);
    }
    /// Called after the worker has left every foreign callback.
    pub fn worker_finished(&self) {
        if self.drained() {
            let object = self
                .0
                .deferred_object
                .swap(std::ptr::null_mut(), Ordering::AcqRel);
            if !object.is_null() {
                // SAFETY: defer_object transfers the sole final COM reference.
                unsafe { crate::com::reclaim_object(object) };
            }
        }
    }
    pub(crate) fn retire(&self) {
        self.0.retired.store(true, Ordering::Release);
    }
    pub(crate) fn retired_and_drained(&self) -> bool {
        self.0.retired.load(Ordering::Acquire) && self.drained()
    }
    pub(crate) fn drained(&self) -> bool {
        !self.0.buffers.0.busy.load(Ordering::Acquire)
    }
    pub fn output_ready() {
        finish_pending();
    }
    pub fn reset_request(&self) {
        self.notify(sdk::kAsioResetRequest);
    }
    pub fn latencies_changed(&self) {
        self.notify(sdk::kAsioLatenciesChanged);
    }
    fn notify(&self, selector: i32) {
        if self.0.retired.load(Ordering::Acquire)
            || self
                .0
                .buffers
                .0
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
        let _busy = Busy(&self.0.buffers.0.busy);
        if self.0.retired.load(Ordering::Acquire) {
            return;
        }
        if let Some(callback) = self.0.callbacks.asioMessage {
            // SAFETY: Host can only be constructed by COM from a live host callback table.
            unsafe {
                callback(selector, 0, std::ptr::null_mut(), std::ptr::null_mut());
            }
        }
    }
    /// Calls the foreign host without any driver mutex. `publish` runs exactly once
    /// at synchronous outputReady, or on return if outputReady was omitted. Samples
    /// are copied into Rust-owned scratch before the publishing closure sees them.
    /// A nested switch is rejected, so safe callers cannot alias buffer storage.
    pub fn switch(
        &self,
        buffers: &HostBuffers,
        half: usize,
        position: Position,
        rate: u32,
        scratch: &mut [f32],
        mut publish: impl FnMut(&[f32]),
    ) -> bool {
        if half > 1
            || self.0.retired.load(Ordering::Acquire)
            || !Arc::ptr_eq(&buffers.0, &self.0.buffers.0)
            || PENDING.with(|p| p.get().is_some())
            || buffers
                .0
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return false;
        }
        let _busy = Busy(&buffers.0.busy);
        if self.0.retired.load(Ordering::Acquire) {
            return false;
        }
        let supports = if let Some(message) = self.0.callbacks.asioMessage {
            // SAFETY: callback table was supplied by the live COM host.
            unsafe {
                message(
                    sdk::kAsioSupportsTimeInfo,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) == 1
            }
        } else {
            false
        };
        if self.0.retired.load(Ordering::Acquire) {
            return false;
        }
        let mut commit = || {
            buffers.copy(half, scratch);
            publish(scratch);
        };
        let mut pending = Pending {
            commit: Some(&mut commit),
        };
        // The thread-local pointer is removed before the borrowed closure dies.
        PENDING.with(|p| p.set(Some((&mut pending as *mut Pending<'_>).cast())));
        let _clear = ClearPending;
        if let Some(callback) = self.0.callbacks.bufferSwitchTimeInfo.filter(|_| supports) {
            let mut time = sdk::Time::new(position.frames, position.nanos, f64::from(rate));
            // SAFETY: time remains live through the synchronous callback; half is 0 or 1.
            unsafe {
                callback(&mut time, half as i32, sdk::ASIOTrue);
            }
        } else if let Some(callback) = self.0.callbacks.bufferSwitch {
            // SAFETY: valid callback and prepared buffers; directProcess=true requires
            // all host buffer writes to finish before this callback returns.
            unsafe {
                callback(half as i32, sdk::ASIOTrue);
            }
        }
        finish_pending();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn no_ready(_half: i32, _direct: i32) {}
    unsafe extern "C" fn twice(_half: i32, _direct: i32) {
        Host::output_ready();
        Host::output_ready();
    }
    fn host(callback: unsafe extern "C" fn(i32, i32)) -> (Host, HostBuffers) {
        let buffers = HostBuffers::new(&[0, 1], 64);
        let host = Host::from_callbacks(
            sdk::Callbacks {
                bufferSwitch: Some(callback),
                sampleRateDidChange: None,
                asioMessage: None,
                bufferSwitchTimeInfo: None,
            },
            buffers.clone(),
        );
        (host, buffers)
    }
    #[test]
    fn output_ready_and_callback_return_commit_exactly_once() {
        for callback in [no_ready as unsafe extern "C" fn(i32, i32), twice] {
            let (host, buffers) = host(callback);
            let mut scratch = [0.0; 128];
            let mut count = 0;
            assert!(host.switch(
                &buffers,
                0,
                Position::default(),
                48000,
                &mut scratch,
                |_| count += 1
            ));
            assert_eq!(count, 1);
        }
    }
    #[test]
    fn retired_and_mismatched_capabilities_cannot_invoke_host() {
        let (host, buffers) = host(no_ready);
        let wrong = HostBuffers::new(&[0, 1], 64);
        let mut scratch = [0.0; 128];
        assert!(!host.switch(
            &wrong,
            0,
            Position::default(),
            48000,
            &mut scratch,
            |_| panic!("unexpected")
        ));
        host.retire();
        assert!(!host.switch(
            &buffers,
            0,
            Position::default(),
            48000,
            &mut scratch,
            |_| panic!("unexpected")
        ));
        assert!(host.drained());
    }
}
