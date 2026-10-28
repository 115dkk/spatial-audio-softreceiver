// SPDX-License-Identifier: MIT
// IASIO lifecycle and pending-output port: EqualizerAPO-XT
// asio/AsioWrapper.{h,cpp}, commit 3a6a213cf83a0606f00b8cce03b6781ee48d8c03.
// Copyright 2026 115dkk. Ported with copyright holder's MIT authorization.
#![forbid(unsafe_code)]
use asio_proto::Layout;
use asio_sys::{AsioDriver, Channel, Error, Host, HostBuffers, Position, Result};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, AtomicU64, Ordering},
};
#[cfg(windows)]
mod running;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum State {
    Loaded,
    Initialized,
    Prepared,
    Running,
}
#[derive(Debug)]
pub struct Driver {
    state: AtomicU8,
    error: AtomicU8,
    messages: [&'static str; 5],
    active_channels: AtomicU64,
    frames: Arc<AtomicU64>,
    nanos: Arc<AtomicU64>,
    #[cfg(windows)]
    prepared: Mutex<Option<running::Prepared>>,
    #[cfg(not(windows))]
    prepared: Mutex<()>,
}
impl Default for Driver {
    fn default() -> Self {
        // ASIO char buffers are ANSI: this boundary deliberately uses ASCII English.
        let language = ssr_types::app::Language::En;
        Self {
            state: AtomicU8::new(0),
            error: AtomicU8::new(0),
            messages: [
                ssr_i18n::live::asio_driver_ok(language),
                ssr_i18n::live::asio_driver_lease_busy(language),
                ssr_i18n::live::asio_driver_app_not_ready(language),
                ssr_i18n::live::asio_driver_callback_not_drained(language),
                ssr_i18n::live::asio_driver_panicked(language),
            ],
            active_channels: AtomicU64::new(0),
            frames: Arc::new(AtomicU64::new(0)),
            nanos: Arc::new(AtomicU64::new(0)),
            #[cfg(windows)]
            prepared: Mutex::new(None),
            #[cfg(not(windows))]
            prepared: Mutex::new(()),
        }
    }
}
impl Driver {
    pub fn state(&self) -> State {
        match self.state.load(Ordering::Acquire) {
            0 => State::Loaded,
            1 => State::Initialized,
            2 => State::Prepared,
            _ => State::Running,
        }
    }
    fn layout(&self) -> Layout {
        #[cfg(windows)]
        {
            use asio_proto::ControlMemory;
            if let Ok(control) = asio_sys::ControlMapping::open()
                && let Some(layout) = control.control().layout()
            {
                return layout;
            }
        }
        Layout::Surround714
    }
    fn control_value(&self, field: impl FnOnce(u32, u32, u32) -> u32, default: u32) -> u32 {
        #[cfg(windows)]
        {
            use asio_proto::ControlMemory;
            if let Ok(control) = asio_sys::ControlMapping::open() {
                let c = control.control();
                return field(c.sample_rate(), c.dac_period_frames(), c.latency_frames());
            }
        }
        let _ = field;
        default
    }
}
impl AsioDriver for Driver {
    fn name(&self) -> &'static str {
        ssr_i18n::names::product(ssr_types::app::Language::En)
    }
    fn init(&self) -> bool {
        let _ = self.state.compare_exchange(
            State::Loaded as u8,
            State::Initialized as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        true
    }
    fn error_code(&self) -> &'static str {
        self.messages[usize::from(self.error.load(Ordering::Acquire)).min(4)]
    }
    fn panic_message(&self) -> &'static str {
        self.messages[4]
    }
    fn channels(&self) -> u32 {
        self.layout().channels()
    }
    fn latency(&self) -> u32 {
        self.control_value(|_, _, latency| latency, 0)
    }
    fn preferred_frames(&self) -> u32 {
        asio_proto::preferred_buffer(self.control_value(|_, period, _| period, 480))
    }
    fn sample_rate(&self) -> u32 {
        self.control_value(|rate, _, _| rate, 48000)
    }
    fn can_sample_rate(&self, rate: u32) -> bool {
        rate == self.sample_rate()
    }
    fn set_sample_rate(&self, rate: u32) -> Result {
        if self.can_sample_rate(rate) {
            return Ok(());
        }
        Err(Error::NoClock)
    }
    fn position(&self) -> Position {
        Position {
            frames: self.frames.load(Ordering::Acquire),
            nanos: self.nanos.load(Ordering::Acquire),
        }
    }
    fn channel(&self, index: u32) -> Result<Channel> {
        let layout = self.layout();
        let name = layout
            .channel_names()
            .get(index as usize)
            .ok_or(Error::InvalidParameter)?;
        Ok(Channel {
            name: (*name).into(),
            active: self.active_channels.load(Ordering::Acquire) & (1_u64 << index) != 0,
        })
    }
    fn create_buffers(
        &self,
        channels: &[u32],
        frames: u32,
        host: Host,
        buffers: HostBuffers,
    ) -> Result {
        if self.state() != State::Initialized {
            return Err(Error::InvalidMode);
        }
        validate_channels(channels, frames, self.channels())?;
        #[cfg(windows)]
        {
            // Only control-plane preparation is serialized. No host callback occurs here.
            let mut prepared = self.prepared.try_lock().map_err(|_| Error::Hardware)?;
            if prepared.is_some() {
                return Err(Error::InvalidMode);
            }
            let value = running::Prepared::create(
                channels,
                frames,
                host,
                buffers,
                self.frames.clone(),
                self.nanos.clone(),
            )
            .inspect_err(|error| {
                self.error.store(
                    if *error == Error::InvalidMode { 1 } else { 2 },
                    Ordering::Release,
                )
            })?;
            *prepared = Some(value);
            self.active_channels.store(
                channels
                    .iter()
                    .fold(0_u64, |mask, channel| mask | (1_u64 << channel)),
                Ordering::Release,
            );
            self.state.store(State::Prepared as u8, Ordering::Release);
            self.error.store(0, Ordering::Release);
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = (&self.prepared, host, buffers);
            Err(Error::NotPresent)
        }
    }
    fn start(&self) -> Result {
        if self.state() != State::Prepared {
            return Err(Error::InvalidMode);
        }
        #[cfg(windows)]
        {
            let mut prepared = self.prepared.try_lock().map_err(|_| Error::Hardware)?;
            let stream = prepared.as_mut().ok_or(Error::InvalidMode)?;
            stream.start()?;
            let token = stream.stop_token();
            self.state.store(State::Running as u8, Ordering::Release);
            drop(prepared);
            // The worker may enter a foreign callback only after the control
            // mutex has been released, including the very first callback.
            token.arm();
            Ok(())
        }
        #[cfg(not(windows))]
        {
            Err(Error::NotPresent)
        }
    }
    fn stop(&self) -> Result {
        if self.state() != State::Running {
            return Err(Error::InvalidMode);
        }
        #[cfg(windows)]
        {
            // Take only a clone of the stop token under the mutex. Drain outside it;
            // reentrant outputReady never needs this mutex or the COM object storage.
            let stop = self
                .prepared
                .try_lock()
                .map_err(|_| Error::Hardware)?
                .as_ref()
                .ok_or(Error::InvalidMode)?
                .stop_token();
            if !stop.stop() {
                self.error.store(3, Ordering::Release);
                return Err(Error::Hardware);
            }
            self.state.store(State::Prepared as u8, Ordering::Release);
            Ok(())
        }
        #[cfg(not(windows))]
        {
            Err(Error::NotPresent)
        }
    }
    fn dispose_buffers(&self) -> Result {
        if self.state() == State::Running {
            self.stop()?;
        }
        if self.state() != State::Prepared {
            return Err(Error::InvalidMode);
        }
        #[cfg(windows)]
        {
            let prepared = self
                .prepared
                .try_lock()
                .map_err(|_| Error::Hardware)?
                .take();
            drop(prepared);
        }
        self.active_channels.store(0, Ordering::Release);
        self.state
            .store(State::Initialized as u8, Ordering::Release);
        Ok(())
    }
    fn output_ready(&self) -> Result {
        Host::output_ready();
        Ok(())
    }
}
fn validate_channels(channels: &[u32], frames: u32, total: u32) -> Result {
    if !asio_proto::accepts_buffer(frames) || channels.is_empty() || total > 64 {
        return Err(Error::InvalidParameter);
    }
    let mut seen = 0_u64;
    for &channel in channels {
        if channel >= total || channel >= 64 || seen & (1_u64 << channel) != 0 {
            return Err(Error::InvalidParameter);
        }
        seen |= 1_u64 << channel;
    }
    Ok(())
}
fn factory() -> Arc<dyn AsioDriver> {
    let _ = asio_sys::retain_exports();
    Arc::new(Driver::default())
}
asio_sys::submit! {asio_sys::Factory(factory)}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_init_is_idempotent_without_ipc() {
        let driver = Driver::default();
        assert_eq!(driver.state(), State::Loaded);
        assert!(driver.init());
        assert!(driver.init());
        assert_eq!(driver.state(), State::Initialized);
    }
    #[test]
    fn driver_name_and_ansi_errors_come_from_english_catalog() {
        let driver = Driver::default();
        assert_eq!(
            driver.name(),
            ssr_i18n::names::product(ssr_types::app::Language::En)
        );
        assert!(driver.name().len() < 32);
        for message in driver.messages {
            assert!(message.is_ascii());
        }
    }
    #[test]
    fn set_sample_rate_accepts_only_current_dac_rate() {
        let driver = Driver::default();
        let rate = driver.sample_rate();
        assert_eq!(driver.set_sample_rate(rate), Ok(()));
        assert_eq!(driver.set_sample_rate(rate + 1), Err(Error::NoClock));
        assert!(!driver.can_sample_rate(rate + 1));
    }
    #[test]
    fn invalid_state_never_starts() {
        let driver = Driver::default();
        assert_eq!(driver.start(), Err(Error::InvalidMode));
        assert_eq!(driver.stop(), Err(Error::InvalidMode));
        assert_eq!(driver.dispose_buffers(), Err(Error::InvalidMode));
    }
    #[test]
    fn channel_validation_requires_power_of_two_and_rejects_duplicates() {
        assert_eq!(validate_channels(&[0, 1], 512, 12), Ok(()));
        assert_eq!(
            validate_channels(&[0, 1], 480, 12),
            Err(Error::InvalidParameter)
        );
        assert_eq!(
            validate_channels(&[1, 1], 64, 12),
            Err(Error::InvalidParameter)
        );
        assert_eq!(
            validate_channels(&[12], 64, 12),
            Err(Error::InvalidParameter)
        );
        assert_eq!(
            validate_channels(&[0], 63, 12),
            Err(Error::InvalidParameter)
        );
    }

    #[test]
    fn channel_validation_accepts_thirty_five_and_sixty_four_but_not_more() {
        assert_eq!(
            validate_channels(&(0..35).collect::<Vec<_>>(), 64, 35),
            Ok(())
        );
        assert_eq!(
            validate_channels(&(0..64).collect::<Vec<_>>(), 2_048, 64),
            Ok(())
        );
        assert_eq!(
            validate_channels(&[0], 64, 65),
            Err(Error::InvalidParameter)
        );
        assert_eq!(
            validate_channels(&[64], 64, 65),
            Err(Error::InvalidParameter)
        );
    }
}
