//! SDK-independent bridge from the Windows host to the engine's ASIO worker.
//!
//! Only the engine may publish ready, after preparing the renderer and DAC.

use std::sync::Arc;
use std::time::Duration;

use engine::asio::{
    AsioClient, AsioTransport, AsioTransportError, AsioTransportProvider, ControlMemory, Signal,
};
use ssr_sys::asio_host::AsioHost;

pub fn provider() -> Arc<dyn AsioTransportProvider> {
    Arc::new(open)
}

fn open() -> Result<Box<dyn AsioTransport>, AsioTransportError> {
    let host = AsioHost::create().map_err(transport_error)?;
    Ok(Box::new(WindowsTransport {
        host,
        attached: false,
    }))
}

struct WindowsTransport {
    host: AsioHost,
    attached: bool,
}

impl AsioTransport for WindowsTransport {
    fn control_memory(&self) -> Arc<dyn ControlMemory> {
        self.host.control_memory()
    }

    fn demand_signal(&self) -> Arc<dyn Signal> {
        self.host.demand_signal()
    }

    fn configure(&mut self, config: engine::asio::OutputConfig) -> Result<(), AsioTransportError> {
        self.host
            .configure(ssr_sys::asio_host::OutputConfig {
                sample_rate: config.sample_rate,
                dac_period_frames: config.dac_period_frames,
                layout: config.layout,
                latency_frames: config.latency_frames,
            })
            .map_err(transport_error)
    }

    fn set_ready(&mut self, ready: bool) -> Result<(), AsioTransportError> {
        self.host.set_ready(ready).map_err(transport_error)
    }

    fn wait_client(
        &mut self,
        timeout: Duration,
    ) -> Result<engine::asio::ClientState, AsioTransportError> {
        self.host.wait_client(timeout).map_err(transport_error)
    }

    fn poll_client(&mut self, timeout: Duration) -> Result<Option<AsioClient>, AsioTransportError> {
        if self.attached {
            // The event is only a hint; poll again to validate lease/generation.
            let _ = self.host.wait_work(timeout);
        }
        let mut client = self.host.poll_client().map_err(transport_error)?;
        if client.is_none() && !self.attached && !timeout.is_zero() {
            // wait_work returns immediately without a client. Keep discovery
            // bounded without spinning while the entrance has no DAW.
            std::thread::sleep(timeout);
            client = self.host.poll_client().map_err(transport_error)?;
        }
        self.attached = client.is_some();
        Ok(client.map(|client| AsioClient {
            memory: client.memory,
            geometry: client.geometry,
            lease: client.lease,
            executable: client.executable,
            done: client.done,
        }))
    }
}

fn transport_error(error: ssr_sys::asio_host::Error) -> AsioTransportError {
    AsioTransportError::Transport(error.to_string())
}
