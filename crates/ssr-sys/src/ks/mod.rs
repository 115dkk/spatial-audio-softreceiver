//! Direct Kernel Streaming render output (ADR 0028, ADR 0037).
//!
//! Discovery runs while the endpoint is enabled. The pure opener never changes
//! visibility; the combined opener hides it on every open and restores it before
//! trying WASAPI exclusive. The supervisor/guard own final restoration.

#[cfg(windows)]
mod console;
#[cfg(windows)]
mod discovery;
#[cfg(windows)]
mod io;
mod sample;
mod selection;
mod timing;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use crate::host::set_endpoint_visible;
#[cfg(windows)]
pub use console::ConsoleCancel;
#[cfg(windows)]
pub use discovery::kernel_target;
#[cfg(windows)]
pub use windows::{KsOpener, KsSink};

#[cfg(windows)]
use crate::output::{WasapiOpener, WasapiSink};
#[cfg(windows)]
use ssr_types::{OutputError, OutputFormat, OutputLayout, OutputOpener, OutputSink};

#[cfg(windows)]
#[derive(Debug, Clone)]
pub struct KsOrWasapiOpener {
    pub ks: KsOpener,
    pub wasapi: WasapiOpener,
    pub endpoint: String,
}

#[cfg(windows)]
#[derive(Debug)]
pub enum KsOrWasapiSink {
    Ks(KsSink),
    Wasapi {
        sink: WasapiSink,
        reason: ssr_types::ports::KsFallbackReason,
        code: u32,
    },
}

#[cfg(windows)]
impl OutputOpener for KsOrWasapiOpener {
    type Sink = KsOrWasapiSink;

    fn open(&mut self, layout: OutputLayout) -> Result<Self::Sink, OutputError> {
        // A previous open may have restored visibility for WASAPI fallback.
        let attempt = set_endpoint_visible(&self.endpoint, false)
            .map_err(|e| windows::OpenFailure {
                reason: ssr_types::ports::KsFallbackReason::Other,
                code: 0,
                error: OutputError::Io(e.to_string()),
            })
            .and_then(|()| self.ks.open_detailed(layout));
        match attempt {
            Ok(sink) => Ok(KsOrWasapiSink::Ks(sink)),
            Err(error) => {
                // Structured reason/code is returned in OutputFormat. Callers
                // own localized presentation and persistent diagnostic logging.
                set_endpoint_visible(&self.endpoint, true)
                    .map_err(|e| OutputError::Io(e.to_string()))?;
                self.wasapi.endpoint = Some(self.endpoint.clone());
                self.wasapi.exclusive = true;
                self.wasapi.shared_fallback = false;
                self.wasapi.open(layout).map(|sink| KsOrWasapiSink::Wasapi {
                    sink,
                    reason: error.reason,
                    code: error.code,
                })
            }
        }
    }

    fn set_period(&mut self, period: std::time::Duration) -> bool {
        self.ks.set_period(period);
        self.wasapi.set_period(period);
        true
    }
}

#[cfg(windows)]
impl OutputSink for KsOrWasapiSink {
    fn format(&self) -> OutputFormat {
        match self {
            Self::Ks(sink) => sink.format(),
            Self::Wasapi { sink, reason, code } => OutputFormat {
                ks_fallback: true,
                ks_fallback_reason: Some(*reason),
                ks_fallback_code: *code,
                ..sink.format()
            },
        }
    }

    fn next_period(&mut self, fill: &mut dyn FnMut(&mut [f32])) -> Result<u64, OutputError> {
        match self {
            Self::Ks(sink) => sink.next_period(fill),
            Self::Wasapi { sink, .. } => sink.next_period(fill),
        }
    }

    fn restarts(&self) -> u64 {
        match self {
            Self::Ks(sink) => sink.restarts(),
            Self::Wasapi { sink, .. } => sink.restarts(),
        }
    }

    fn played_frames(&self) -> Option<u64> {
        match self {
            Self::Ks(sink) => sink.played_frames(),
            Self::Wasapi { sink, .. } => sink.played_frames(),
        }
    }

    fn late_resyncs(&self) -> u64 {
        match self {
            Self::Ks(sink) => sink.late_resyncs(),
            Self::Wasapi { sink, .. } => sink.late_resyncs(),
        }
    }
}
