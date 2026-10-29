#![allow(unsafe_code)]

//! WASAPI process-loopback capture (ADR 0043).

use std::{
    mem::{ManuallyDrop, size_of},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use ssr_abi::RecordWriter;
use ssr_types::{CaptureError, CaptureSource, TrimUnsupported};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_EVENT, WAIT_FAILED, WAIT_TIMEOUT},
        Media::{
            Audio::{
                AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY, AUDCLNT_BUFFERFLAGS_SILENT,
                AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR, AUDCLNT_E_DEVICE_INVALIDATED,
                AUDCLNT_E_RESOURCES_INVALIDATED, AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
                AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
                ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
                IActivateAudioInterfaceCompletionHandler,
                IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
                PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
                WAVEFORMATEXTENSIBLE_0,
            },
            KernelStreaming::WAVE_FORMAT_EXTENSIBLE,
            Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        },
        System::{
            Com::{
                BLOB, IAgileObject, IAgileObject_Impl,
                StructuredStorage::{
                    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                },
            },
            Threading::{
                CreateEventW, OpenProcess, PROCESS_SYNCHRONIZE, WaitForMultipleObjects,
                WaitForSingleObject,
            },
            Variant::VT_BLOB,
        },
    },
    core::{HRESULT, IUnknown, Interface, Ref},
};
use windows_core::implement;

use crate::{
    devices::ComGuard,
    loopback_format::{LOOPBACK_RATE, LoopbackLayout, LoopbackTranslator, PacketFlags},
    qpc,
};

const MAX_PACKET_FRAMES: usize = LOOPBACK_RATE as usize;
const BUFFER_DURATION_HNS: i64 = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppTarget {
    pub pid: u32,
    pub endpoint: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoopbackError {
    #[error("process loopback is unsupported: {0}")]
    Unsupported(String),
    #[error("activation timed out for process {pid}")]
    ActivationTimedOut { pid: u32 },
    #[error("activation failed for process {pid}: HRESULT {hresult:#010x}")]
    Activation { pid: u32, hresult: i32 },
    #[error("process {pid} capture failed: {detail}")]
    Stream { pid: u32, detail: String },
}

impl From<LoopbackError> for CaptureError {
    fn from(error: LoopbackError) -> Self {
        match error {
            LoopbackError::Unsupported(_) => Self::NotPresent,
            other => Self::Io(other.to_string()),
        }
    }
}

struct ActivationState {
    completed: Mutex<bool>,
    condition: Condvar,
    _params: Box<AUDIOCLIENT_ACTIVATION_PARAMS>,
    prop: ManuallyDrop<PROPVARIANT>,
}

// SAFETY: `_params` owns the stable allocation addressed by `prop`'s VT_BLOB.
// Neither value is mutated after construction, and the OS only reads the blob.
unsafe impl Send for ActivationState {}
// SAFETY: the completion flag is protected by its mutex; the activation blob is immutable.
unsafe impl Sync for ActivationState {}

#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct Completion(Arc<ActivationState>);

impl IAgileObject_Impl for Completion_Impl {}

impl IActivateAudioInterfaceCompletionHandler_Impl for Completion_Impl {
    fn ActivateCompleted(
        &self,
        _operation: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows_core::Result<()> {
        let mut completed = self
            .0
            .completed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *completed = true;
        drop(completed);
        self.0.condition.notify_one();
        Ok(())
    }
}

struct Event(HANDLE);

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: CreateEventW returned this owned handle and it is closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct Client {
    target: AppTarget,
    audio: IAudioClient,
    capture: IAudioCaptureClient,
    event: Event,
    process: Event,
    translator: LoopbackTranslator,
    live: bool,
    process_exited: bool,
}

impl Drop for Client {
    fn drop(&mut self) {
        if self.live {
            // SAFETY: this initialized client was started by open_client; Stop is idempotent here.
            let _ = unsafe { self.audio.Stop() };
        }
    }
}

pub struct AppLoopback {
    clients: Vec<Client>,
    targets: Vec<AppTarget>,
    layout: LoopbackLayout,
    activation_timeout: Duration,
}

// SAFETY: WASAPI audio clients and auto-reset event handles are created in the
// process MTA, moved as owned values, and then used by only one capture thread.
unsafe impl Send for AppLoopback {}

impl std::fmt::Debug for AppLoopback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppLoopback")
            .field("targets", &self.targets())
            .finish_non_exhaustive()
    }
}

impl AppLoopback {
    pub fn open(
        targets: &[AppTarget],
        layout: LoopbackLayout,
        activation_timeout: Duration,
    ) -> Result<Self, LoopbackError> {
        validate_targets(targets)?;
        let layout = LoopbackLayout::from_mix(layout.channels, layout.channel_mask);
        let _com =
            ComGuard::new().map_err(|error| LoopbackError::Unsupported(error.to_string()))?;
        crate::apartment::pin();
        let frequency = qpc::frequency();
        let mut clients = Vec::with_capacity(targets.len());
        for &target in targets {
            clients.push(open_client(target, layout, activation_timeout, frequency)?);
        }
        Ok(Self {
            clients,
            targets: targets.to_vec(),
            layout,
            activation_timeout,
        })
    }

    pub fn targets(&self) -> Vec<(AppTarget, bool)> {
        self.clients
            .iter()
            .map(|client| (client.target, client.live))
            .collect()
    }

    fn all_dead_and_drained(&self) -> bool {
        self.clients.iter().all(|client| {
            !client.live && !client.translator.has_pending() && !client.translator.is_streaming()
        })
    }
}

fn validate_targets(targets: &[AppTarget]) -> Result<(), LoopbackError> {
    if targets.is_empty() || targets.len() > 2 {
        return Err(LoopbackError::Unsupported(
            "one or two app targets are required".into(),
        ));
    }
    for (index, target) in targets.iter().enumerate() {
        if target.pid == 0 || target.endpoint > 1 {
            return Err(LoopbackError::Unsupported(format!(
                "invalid target pid {} endpoint {}",
                target.pid, target.endpoint
            )));
        }
        if targets[..index]
            .iter()
            .any(|old| old.pid == target.pid || old.endpoint == target.endpoint)
        {
            return Err(LoopbackError::Unsupported(
                "app targets must have distinct process ids and endpoints".into(),
            ));
        }
    }
    Ok(())
}

fn activate(target: AppTarget, timeout: Duration) -> Result<IAudioClient, LoopbackError> {
    let mut params = Box::new(AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: target.pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    });
    let prop = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_BLOB,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    blob: BLOB {
                        cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                        pBlobData: (&mut *params as *mut AUDIOCLIENT_ACTIVATION_PARAMS).cast(),
                    },
                },
            }),
        },
    };
    let state = Arc::new(ActivationState {
        completed: Mutex::new(false),
        condition: Condvar::new(),
        _params: params,
        prop: ManuallyDrop::new(prop),
    });
    let callback: IActivateAudioInterfaceCompletionHandler = Completion(state.clone()).into();
    // SAFETY: the callback owns the heap allocation addressed by its VT_BLOB and
    // keeps both the parameters and ManuallyDrop PROPVARIANT alive until the OS
    // releases the callback, including after this function times out.
    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*state.prop),
            &callback,
        )
    }
    .map_err(|error| LoopbackError::Activation {
        pid: target.pid,
        hresult: error.code().0,
    })?;
    let completed = state
        .completed
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (completed, wait) = state
        .condition
        .wait_timeout_while(completed, timeout, |completed| !*completed)
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !*completed && wait.timed_out() {
        return Err(LoopbackError::ActivationTimedOut { pid: target.pid });
    }
    drop(completed);
    let mut status = HRESULT::default();
    let mut unknown: Option<IUnknown> = None;
    // SAFETY: completion was signaled and both writable outputs are initialized.
    unsafe { operation.GetActivateResult(&mut status, &mut unknown) }.map_err(|error| {
        LoopbackError::Activation {
            pid: target.pid,
            hresult: error.code().0,
        }
    })?;
    status.ok().map_err(|error| LoopbackError::Activation {
        pid: target.pid,
        hresult: error.code().0,
    })?;
    unknown
        .ok_or(LoopbackError::Activation {
            pid: target.pid,
            hresult: status.0,
        })?
        .cast()
        .map_err(|error| LoopbackError::Activation {
            pid: target.pid,
            hresult: error.code().0,
        })
}

fn open_client(
    target: AppTarget,
    layout: LoopbackLayout,
    timeout: Duration,
    qpc_frequency: u64,
) -> Result<Client, LoopbackError> {
    let audio = activate(target, timeout)?;
    let bytes_per_frame = layout.bytes_per_frame();
    let format = WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_EXTENSIBLE as u16,
            nChannels: layout.channels,
            nSamplesPerSec: LOOPBACK_RATE,
            nAvgBytesPerSec: LOOPBACK_RATE * bytes_per_frame as u32,
            nBlockAlign: bytes_per_frame as u16,
            wBitsPerSample: 32,
            cbSize: 22,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 {
            wValidBitsPerSample: 32,
        },
        dwChannelMask: layout.channel_mask,
        SubFormat: KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
    };
    let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
        | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
    // SAFETY: live process-loopback client; format points to a complete extensible structure.
    unsafe {
        audio.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            flags,
            BUFFER_DURATION_HNS,
            0,
            (&raw const format.Format).cast(),
            None,
        )
    }
    .map_err(|error| stream_error(target.pid, "IAudioClient::Initialize", error))?;
    // SAFETY: unnamed auto-reset event with default security.
    let event = Event(
        unsafe { CreateEventW(None, false, false, None) }
            .map_err(|error| stream_error(target.pid, "CreateEventW", error))?,
    );
    // SAFETY: initialized event-driven client and a live event handle.
    unsafe { audio.SetEventHandle(event.0) }
        .map_err(|error| stream_error(target.pid, "IAudioClient::SetEventHandle", error))?;
    // SAFETY: initialized capture client; process loopback exposes IAudioCaptureClient.
    let capture = unsafe { audio.GetService::<IAudioCaptureClient>() }
        .map_err(|error| stream_error(target.pid, "GetService<IAudioCaptureClient>", error))?;
    // SAFETY: requests only synchronization rights for a nonzero target pid.
    let process = Event(
        unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, target.pid) }
            .map_err(|error| stream_error(target.pid, "OpenProcess", error))?,
    );
    // SAFETY: client has been initialized and has its event handle.
    unsafe { audio.Start() }
        .map_err(|error| stream_error(target.pid, "IAudioClient::Start", error))?;
    Ok(Client {
        target,
        audio,
        capture,
        event,
        process,
        translator: LoopbackTranslator::new(
            target.endpoint,
            layout,
            qpc_frequency,
            MAX_PACKET_FRAMES,
        ),
        live: true,
        process_exited: false,
    })
}

fn stream_error(pid: u32, operation: &str, error: windows::core::Error) -> LoopbackError {
    LoopbackError::Stream {
        pid,
        detail: format!("{operation}: {error}"),
    }
}

fn ended_error(error: &windows::core::Error) -> bool {
    matches!(
        error.code(),
        AUDCLNT_E_DEVICE_INVALIDATED | AUDCLNT_E_RESOURCES_INVALIDATED
    )
}

/// Drains every packet currently queued by WASAPI. `Ok(true)` means that the
/// capture queue reached zero or the client ended; `Ok(false)` means the ABI
/// writer filled while one staged packet still needs to be drained.
fn drain_packets(
    client: &mut Client,
    writer: &mut RecordWriter<'_>,
    now: u64,
    bytes_per_frame: usize,
) -> Result<bool, CaptureError> {
    loop {
        // SAFETY: live capture interface; returns a value and does not expose memory.
        let packet_frames = match unsafe { client.capture.GetNextPacketSize() } {
            Ok(frames) => frames,
            Err(error) if ended_error(&error) => {
                client.live = false;
                return Ok(true);
            }
            Err(error) => {
                return Err(stream_error(client.target.pid, "GetNextPacketSize", error).into());
            }
        };
        if packet_frames == 0 {
            return Ok(true);
        }
        let mut data: *mut u8 = std::ptr::null_mut();
        let mut frames = 0_u32;
        let mut raw_flags = 0_u32;
        let mut device_position = 0_u64;
        let mut qpc_hns = 0_u64;
        // SAFETY: all outputs are writable; returned data remains valid until ReleaseBuffer.
        if let Err(error) = unsafe {
            client.capture.GetBuffer(
                &mut data,
                &mut frames,
                &mut raw_flags,
                Some(&mut device_position),
                Some(&mut qpc_hns),
            )
        } {
            if ended_error(&error) {
                client.live = false;
                return Ok(true);
            }
            return Err(
                stream_error(client.target.pid, "IAudioCaptureClient::GetBuffer", error).into(),
            );
        }
        let silent = raw_flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
        let byte_len = frames as usize * bytes_per_frame;
        let packet = if silent {
            None
        } else {
            // SAFETY: WASAPI returned `frames` complete frames and keeps data valid until release.
            Some(unsafe { std::slice::from_raw_parts(data.cast_const(), byte_len) })
        };
        let flags = PacketFlags {
            silent,
            discontinuity: raw_flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0,
            timestamp_error: raw_flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 != 0,
        };
        let packet_qpc = if flags.timestamp_error {
            now
        } else {
            client.translator.qpc_from_hns(qpc_hns)
        };
        let staged = client.translator.stage(frames, packet, flags, packet_qpc);
        // SAFETY: balances the successful GetBuffer exactly once with its frame count.
        let released = unsafe { client.capture.ReleaseBuffer(frames) };
        if let Err(error) = released {
            if ended_error(&error) {
                client.live = false;
                return Ok(true);
            }
            return Err(stream_error(
                client.target.pid,
                "IAudioCaptureClient::ReleaseBuffer",
                error,
            )
            .into());
        }
        staged.map_err(|error| LoopbackError::Stream {
            pid: client.target.pid,
            detail: error.to_string(),
        })?;
        client.translator.drain(writer);
        if client.translator.has_pending() {
            return Ok(false);
        }
    }
}

impl CaptureSource for AppLoopback {
    fn read(&mut self, buf: &mut [u8], wait: Duration) -> Result<usize, CaptureError> {
        let mut writer = RecordWriter::new(buf);
        for client in &mut self.clients {
            client.translator.drain(&mut writer);
        }
        let initial_now = qpc::now();
        let bytes_per_frame = self.layout.bytes_per_frame();
        for client in &mut self.clients {
            let process_exited = client.live && !client.process_exited && {
                // SAFETY: owned process synchronization handle; a zero-timeout status query.
                (unsafe { WaitForSingleObject(client.process.0, 0) }) != WAIT_TIMEOUT
            };
            if process_exited {
                client.process_exited = true;
            }
            if client.process_exited && !client.translator.has_pending() {
                let queue_empty = drain_packets(client, &mut writer, initial_now, bytes_per_frame)?;
                if queue_empty {
                    client.live = false;
                }
            }
            if !client.live {
                client.translator.end(initial_now, &mut writer);
            }
        }
        if self.all_dead_and_drained() {
            return if writer.is_empty() {
                Err(CaptureError::Ended)
            } else {
                Ok(writer.len())
            };
        }
        if self
            .clients
            .iter()
            .any(|client| client.translator.has_pending())
        {
            return Ok(writer.len());
        }

        let mut handles = [HANDLE::default(); 2];
        let mut count = 0;
        for client in &self.clients {
            if client.live && !client.process_exited {
                handles[count] = client.event.0;
                count += 1;
            }
        }
        if count == 0 {
            if writer.is_empty() {
                std::thread::sleep(wait.min(Duration::from_millis(2)));
            }
            return Ok(writer.len());
        }
        let timeout_ms = wait
            .as_nanos()
            .div_ceil(1_000_000)
            .min(u128::from(u32::MAX)) as u32;
        // SAFETY: the slice contains live event handles owned by self for the whole wait.
        let waited = unsafe { WaitForMultipleObjects(&handles[..count], false, timeout_ms) };
        if waited == WAIT_FAILED {
            return Err(CaptureError::Io("WaitForMultipleObjects failed".into()));
        }
        if waited != WAIT_TIMEOUT && !wait_result_is_handle(waited, count) {
            return Err(CaptureError::Io(format!(
                "WaitForMultipleObjects returned {}",
                waited.0
            )));
        }

        let now = qpc::now();
        for client in &mut self.clients {
            if !client.live || client.process_exited {
                continue;
            }
            drain_packets(client, &mut writer, now, bytes_per_frame)?;
            if client.live {
                client.translator.idle(now, &mut writer);
            } else {
                client.translator.end(now, &mut writer);
            }
        }
        if self.all_dead_and_drained() && writer.is_empty() {
            Err(CaptureError::Ended)
        } else {
            Ok(writer.len())
        }
    }

    fn now_qpc(&self) -> Option<u64> {
        Some(qpc::now())
    }

    fn set_rate_trim(&mut self, _ppm: f64) -> Result<(), TrimUnsupported> {
        Err(TrimUnsupported)
    }

    fn reconnect(&mut self) -> Result<(), CaptureError> {
        *self = Self::open(&self.targets, self.layout, self.activation_timeout)
            .map_err(CaptureError::from)?;
        Ok(())
    }
}

fn wait_result_is_handle(result: WAIT_EVENT, count: usize) -> bool {
    result.0 < count as u32
}
