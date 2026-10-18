//! The supervisor's [`Launcher`] on Windows: the engine on our driver's ring,
//! rendering to the chosen output through WASAPI or Kernel Streaming.
//!
//! The launcher reads no profile. The supervisor resolves it (the state
//! directory's `profile.toml`, else the imported BRIR set, else stereo
//! speakers) and hands it over with the output (ADR 0001, 2026-10-15
//! addendum).

use std::sync::Arc;

use engine::{EngineConfig, OutputProfile};
use ssr_sys::driver_ring::DriverRing;
use ssr_sys::mmcss;
use ssr_sys::output::WasapiOpener;
use ssr_types::OutputDevice;
use supervisor::{EngineHandle, Launcher, Session, SessionConfig, SessionError};

struct DiscoveryFallback<T>(T);

impl ssr_types::OutputOpener for DiscoveryFallback<WasapiOpener> {
    type Sink = DiscoveryFallback<<WasapiOpener as ssr_types::OutputOpener>::Sink>;

    fn open(
        &mut self,
        layout: ssr_types::OutputLayout,
    ) -> Result<Self::Sink, ssr_types::OutputError> {
        self.0.open(layout).map(DiscoveryFallback)
    }

    fn set_period(&mut self, period: std::time::Duration) -> bool {
        self.0.set_period(period)
    }
}

impl<T: ssr_types::OutputSink> ssr_types::OutputSink for DiscoveryFallback<T> {
    fn format(&self) -> ssr_types::OutputFormat {
        ssr_types::OutputFormat {
            ks_fallback: true,
            ks_fallback_reason: Some(ssr_types::ports::KsFallbackReason::NoTarget),
            ks_fallback_code: 0,
            ..self.0.format()
        }
    }

    fn next_period(
        &mut self,
        fill: &mut dyn FnMut(&mut [f32]),
    ) -> Result<u64, ssr_types::OutputError> {
        self.0.next_period(fill)
    }

    fn restarts(&self) -> u64 {
        self.0.restarts()
    }
    fn played_frames(&self) -> Option<u64> {
        self.0.played_frames()
    }
}

/// Starts the engine for the supervisor on Windows.
#[derive(Clone, Debug, Default)]
pub struct WindowsLauncher;

impl Launcher for WindowsLauncher {
    /// Opens the driver ring and starts a session with a copy of `profile`
    /// on `output`: 48 kHz exclusive, falling back to shared when the device
    /// refuses, each engine thread registered with MMCSS. The supervisor
    /// keeps the profile and answers the engine's re-prepares from it.
    fn launch(
        &mut self,
        output: &OutputDevice,
        profile: &OutputProfile,
    ) -> Result<Box<dyn EngineHandle>, SessionError> {
        launch_session(output, profile, None, false)
    }

    fn launch_with_ks(
        &mut self,
        output: &OutputDevice,
        profile: &OutputProfile,
        ks_target: Option<&ssr_types::ports::KsTarget>,
    ) -> Result<Box<dyn EngineHandle>, SessionError> {
        launch_session(output, profile, ks_target, true)
    }
}

fn launch_session(
    output: &OutputDevice,
    profile: &OutputProfile,
    ks_target: Option<&ssr_types::ports::KsTarget>,
    ks_requested: bool,
) -> Result<Box<dyn EngineHandle>, SessionError> {
    let ring = DriverRing::open()?;
    let opener = WasapiOpener {
        endpoint: Some(output.id.0.clone()),
        rate: 48_000,
        exclusive: true,
        shared_fallback: !ks_requested,
        period: None,
    };
    let engine = EngineConfig {
        thread_hook: Some(Arc::new(mmcss::promote_engine_thread)),
        ..EngineConfig::default()
    };
    let qpc_frequency = ring.info().qpc_frequency;
    let observer = ring.observer();
    let config = SessionConfig {
        engine,
        profile: match profile {
            OutputProfile::Headphones(profile) => OutputProfile::Headphones(profile.clone()),
            OutputProfile::Speakers(profile) => OutputProfile::Speakers(profile.clone()),
            OutputProfile::Correction(profile) => OutputProfile::Correction(profile.clone()),
        },
        qpc_frequency,
        mute: false,
        recorder: None,
    };
    let session = if let Some(target) = ks_target {
        let opener = ssr_sys::ks::KsOrWasapiOpener {
            ks: ssr_sys::ks::KsOpener {
                target: target.clone(),
                rate: 48_000,
                period: None,
            },
            wasapi: WasapiOpener {
                shared_fallback: false,
                ..opener
            },
            endpoint: output.id.0.clone(),
        };
        Session::start(config, ring, opener, Some(Box::new(observer)))
    } else if ks_requested {
        Session::start(
            config,
            ring,
            DiscoveryFallback(opener),
            Some(Box::new(observer)),
        )
    } else {
        Session::start(config, ring, opener, Some(Box::new(observer)))
    }?;
    Ok(Box::new(session))
}
