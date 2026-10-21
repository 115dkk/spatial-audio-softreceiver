//! Fixed-carrier diagnostic playback; no decoding or device-name selection.

/// Copies a repeated prepared stream into whole endpoint buffers. Positions
/// advance only after WASAPI accepts a buffer, so failures never count a
/// partially submitted IEC burst.
#[derive(Debug)]
struct Transfer<'a> {
    bytes: &'a [u8],
    burst_bytes: usize,
    total_bytes: u64,
    submitted: u64,
}

impl<'a> Transfer<'a> {
    fn new(bytes: &'a [u8], burst_bytes: usize, bursts: u64) -> Option<Self> {
        if burst_bytes == 0
            || bytes.is_empty()
            || !bytes.len().is_multiple_of(burst_bytes)
            || bursts == 0
        {
            return None;
        }
        Some(Self {
            bytes,
            burst_bytes,
            total_bytes: bursts.checked_mul(burst_bytes as u64)?,
            submitted: 0,
        })
    }

    fn fill(&self, buffer: &mut [u8]) -> usize {
        buffer.fill(0);
        let count = (self.total_bytes - self.submitted).min(buffer.len() as u64) as usize;
        let mut filled = 0;
        while filled < count {
            let offset = ((self.submitted + filled as u64) % self.bytes.len() as u64) as usize;
            let take = (count - filled).min(self.bytes.len() - offset);
            buffer[filled..filled + take].copy_from_slice(&self.bytes[offset..offset + take]);
            filled += take;
        }
        count
    }

    fn commit(&mut self, count: usize) {
        self.submitted += count as u64;
    }
    fn sent_bursts(&self) -> u64 {
        self.submitted / self.burst_bytes as u64
    }
    fn done(&self) -> bool {
        self.submitted == self.total_bytes
    }
}

#[derive(Debug)]
pub struct CodecProbe {
    pub bit: u32,
    pub codec: &'static str,
    pub supported_hr: i32,
    pub initialize_hr: Option<i32>,
}

#[derive(Debug)]
pub struct EndpointProbe {
    pub endpoint: u16,
    pub id: String,
    pub codec_mask: u64,
    pub codecs: Vec<CodecProbe>,
}

#[derive(Debug)]
pub struct PlayReport {
    pub sent_bursts: u64,
    /// Each prepared burst contains exactly one raw E-AC-3 syncframe.
    pub sent_frames: u64,
    pub error_hr: Option<i32>,
}

#[cfg(windows)]
pub use win::{play_eac3, probe};

#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    use super::*;
    use crate::{devices::ComGuard, driver_ring::DriverRing, host::our_render_devices};
    use ssr_abi::wave::{CODEC_CARRIERS, CodecCarrier, IEC61937_FORMAT_BYTES, codec_name};
    use windows::{
        Win32::{
            Foundation::{CloseHandle, E_UNEXPECTED, HANDLE, WAIT_OBJECT_0},
            Media::Audio::{
                AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_SHAREMODE_EXCLUSIVE,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK, IAudioClient, IAudioClock, IAudioRenderClient,
                IMMDevice, WAVEFORMATEX,
            },
            System::{
                Com::CLSCTX_ALL,
                Threading::{CreateEventW, WaitForSingleObject},
            },
        },
        core::{HRESULT, Interface},
    };

    // A typed, four-byte-aligned owner for the packed 52-byte SDK wire layout.
    // Never cast a Vec<u8> (whose allocation only promises byte alignment).
    #[repr(C, align(4))]
    struct IecFormat([u8; IEC61937_FORMAT_BYTES]);
    impl IecFormat {
        fn new(carrier: CodecCarrier) -> Self {
            Self(carrier.wave_bytes())
        }
        fn pointer(&self) -> *const WAVEFORMATEX {
            self.0.as_ptr().cast()
        }
    }
    const _: () = assert!(std::mem::size_of::<IecFormat>() == 52);
    const _: () = assert!(std::mem::align_of::<IecFormat>() == 4);

    struct Event(HANDLE);
    impl Event {
        fn new() -> windows::core::Result<Self> {
            // SAFETY: unnamed auto-reset event, default security, initially clear.
            unsafe { CreateEventW(None, false, false, None) }.map(Self)
        }
        fn wait(&self) -> bool {
            // SAFETY: owned event remains live; bounded wait even after device loss.
            (unsafe { WaitForSingleObject(self.0, 5_000) }) == WAIT_OBJECT_0
        }
    }
    impl Drop for Event {
        fn drop(&mut self) {
            // SAFETY: sole owner; audio interfaces are dropped before this event.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    fn activate(device: &IMMDevice) -> windows::core::Result<IAudioClient> {
        // SAFETY: live device in this thread's apartment, no activation parameters.
        unsafe { device.Activate(CLSCTX_ALL, None) }
    }

    fn initialize(client: &IAudioClient, format: &IecFormat, period: i64) -> HRESULT {
        // SAFETY: live uninitialized client, aligned 52-byte format with cbSize 34;
        // event-exclusive duration and periodicity are equal and positive.
        unsafe {
            (client.vtable().Initialize)(
                client.as_raw(),
                AUDCLNT_SHAREMODE_EXCLUSIVE,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                period,
                period,
                format.pointer(),
                std::ptr::null(),
            )
        }
    }

    fn open(
        device: &IMMDevice,
        carrier: CodecCarrier,
        event: &Event,
    ) -> Result<IAudioClient, HRESULT> {
        let format = IecFormat::new(carrier);
        let mut client = activate(device).map_err(|e| e.code())?;
        let mut period = 0;
        // SAFETY: live client; minimum period output is a writable i64.
        unsafe { client.GetDevicePeriod(None, Some(&mut period)) }.map_err(|e| e.code())?;
        if period <= 0 {
            return Err(E_UNEXPECTED);
        }
        let mut hr = initialize(&client, &format, period);
        if hr == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
            // SAFETY: alignment-error path explicitly permits GetBufferSize.
            let frames = unsafe { client.GetBufferSize() }.map_err(|e| e.code())?;
            drop(client);
            period = ((10_000_000u64 * u64::from(frames) + u64::from(carrier.rate) / 2)
                / u64::from(carrier.rate)) as i64;
            if period <= 0 {
                return Err(E_UNEXPECTED);
            }
            client = activate(device).map_err(|e| e.code())?;
            hr = initialize(&client, &format, period);
        }
        if hr.0 != 0 {
            return Err(hr);
        }
        // SAFETY: successful event-exclusive initialization; event outlives client.
        unsafe { client.SetEventHandle(event.0) }.map_err(|e| e.code())?;
        Ok(client)
    }

    pub fn probe() -> Result<Vec<EndpointProbe>, String> {
        let _com = ComGuard::new().map_err(|e| e.to_string())?;
        // Close the sole control handle before trying any WASAPI streams.
        let masks = {
            let ring = DriverRing::open().map_err(|e| e.to_string())?;
            ring.info().endpoints
        };
        let devices = our_render_devices().map_err(|e| e.to_string())?;
        devices
            .into_iter()
            .map(|(endpoint, device)| {
                let id = wasapi::Device::from_immdevice(device.clone())
                    .and_then(|d| d.get_id())
                    .map_err(|e| e.to_string())?;
                let mut codecs = Vec::new();
                for carrier in CODEC_CARRIERS {
                    let format = IecFormat::new(carrier);
                    let supported = match activate(&device) {
                        Err(e) => e.code(),
                        Ok(client) => {
                            // SAFETY: aligned complete IEC descriptor, live client;
                            // exclusive queries require a null closest-match output.
                            unsafe {
                                client.IsFormatSupported(
                                    AUDCLNT_SHAREMODE_EXCLUSIVE,
                                    format.pointer(),
                                    None,
                                )
                            }
                        }
                    };
                    let initialize_hr = if endpoint == 2 && supported.0 == 0 {
                        // Event declared first and dropped last, including failed open.
                        Some(match Event::new() {
                            Err(e) => e.code().0,
                            Ok(event) => match open(&device, carrier, &event) {
                                Ok(_client) => 0,
                                Err(hr) => hr.0,
                            },
                        })
                    } else {
                        None
                    };
                    codecs.push(CodecProbe {
                        bit: carrier.bit(),
                        codec: codec_name(carrier.codec),
                        supported_hr: supported.0,
                        initialize_hr,
                    });
                }
                Ok(EndpointProbe {
                    endpoint,
                    id,
                    codec_mask: masks[usize::from(endpoint)].codecs,
                    codecs,
                })
            })
            .collect()
    }

    /// Input is prepared off-thread: whole 24,576-byte bursts, one validated
    /// 48 kHz/5.1/six-block syncframe each. Does not acquire the control device,
    /// allowing a simultaneous `record` command to own the capture ring.
    pub fn play_eac3(bytes: &[u8], bursts: u64) -> Result<PlayReport, String> {
        let mut transfer = Transfer::new(bytes, 24_576, bursts).ok_or("invalid_burst_stream")?;
        if bursts > 112_500 {
            return Err("duration_out_of_range".into());
        }
        let _com = ComGuard::new().map_err(|e| e.to_string())?;
        let mut devices = our_render_devices()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|(endpoint, _)| *endpoint == 2);
        let (_, device) = devices.next().ok_or("endpoint_not_found")?;
        if devices.next().is_some() {
            return Err("ambiguous_endpoint".into());
        }
        let carrier = CODEC_CARRIERS
            .iter()
            .copied()
            .find(|c| c.mask == ssr_abi::FORMAT_EAC3)
            .expect("EAC3 carrier");
        let event = Event::new().map_err(|e| format!("event:0x{:08X}", e.code().0))?;
        let client =
            open(&device, carrier, &event).map_err(|hr| format!("initialize:0x{:08X}", hr.0))?;
        // SAFETY: initialized live client, no outstanding buffer.
        let frames = unsafe { client.GetBufferSize() }.map_err(|e| e.to_string())?;
        if frames == 0 || frames > carrier.rate * 5 {
            return Err("invalid_buffer_size".into());
        }
        // SAFETY: render service on successfully initialized render client.
        let render: IAudioRenderClient =
            unsafe { client.GetService() }.map_err(|e| e.to_string())?;
        // SAFETY: clock service on successfully initialized client.
        let clock: IAudioClock = unsafe { client.GetService() }.map_err(|e| e.to_string())?;
        // SAFETY: live clock service; immutable frequency read before playback.
        let frequency = unsafe { clock.GetFrequency() }.map_err(|e| e.to_string())?;
        if frequency == 0 {
            return Err("invalid_clock_frequency".into());
        }
        let mut buffer = vec![0u8; frames as usize * 4];
        let _priority = crate::mmcss::promote("Pro Audio").map_err(|e| e.to_string())?;
        let hr = run(
            &client,
            &render,
            &clock,
            frequency,
            &event,
            frames,
            &mut buffer,
            &mut transfer,
        );
        Ok(PlayReport {
            sent_bursts: transfer.sent_bursts(),
            sent_frames: transfer.sent_bursts(),
            error_hr: (hr.0 != 0).then_some(hr.0),
        })
    }

    fn submit(render: &IAudioRenderClient, frames: u32, bytes: &[u8]) -> HRESULT {
        let mut pointer = std::ptr::null_mut();
        // SAFETY: live render service; output pointer is writable; requesting
        // exactly the entire negotiated endpoint buffer, no outstanding buffer.
        let hr = unsafe { (render.vtable().GetBuffer)(render.as_raw(), frames, &mut pointer) };
        if hr.0 != 0 {
            return hr;
        }
        if pointer.is_null() {
            return E_UNEXPECTED;
        }
        // SAFETY: GetBuffer succeeded for frames*4 bytes; source is equally sized,
        // nonoverlapping owned storage. No slice or pointer escapes ReleaseBuffer.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len()) };
        // SAFETY: matching full-size successful GetBuffer, all bytes initialized.
        unsafe { (render.vtable().ReleaseBuffer)(render.as_raw(), frames, 0) }
    }

    #[allow(clippy::too_many_arguments)]
    fn run(
        client: &IAudioClient,
        render: &IAudioRenderClient,
        clock: &IAudioClock,
        frequency: u64,
        event: &Event,
        frames: u32,
        buffer: &mut [u8],
        transfer: &mut Transfer<'_>,
    ) -> HRESULT {
        let first = transfer.fill(buffer);
        let hr = submit(render, frames, buffer);
        if hr.0 != 0 {
            return hr;
        }
        transfer.commit(first);
        let mut submitted_frames = u64::from(frames);
        // SAFETY: initialized event stream with its first full buffer submitted.
        let hr = unsafe { (client.vtable().Start)(client.as_raw()) };
        if hr.0 != 0 {
            return hr;
        }
        // No fallible Result<_, windows::Error> wrappers in this loop: their
        // error construction can allocate. Carry raw HRESULTs out instead.
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_secs_f64(
                transfer.total_bytes as f64 / 768_000.0 * 2.0 + 15.0,
            );
        let result = loop {
            if !event.wait() || std::time::Instant::now() >= deadline {
                break HRESULT(0x800705b4u32 as i32);
            }
            if transfer.done() {
                let mut position = 0;
                // SAFETY: live running clock; position output is writable, no QPC output.
                let hr = unsafe {
                    (clock.vtable().GetPosition)(
                        clock.as_raw(),
                        &mut position,
                        std::ptr::null_mut(),
                    )
                };
                if hr.is_err() {
                    break hr;
                }
                if u128::from(position) * 192_000
                    >= u128::from(submitted_frames) * u128::from(frequency)
                {
                    break HRESULT(0);
                }
                buffer.fill(0);
                let hr = submit(render, frames, buffer);
                if hr.0 != 0 {
                    break hr;
                }
            } else {
                let count = transfer.fill(buffer);
                let hr = submit(render, frames, buffer);
                if hr.0 != 0 {
                    break hr;
                }
                transfer.commit(count);
                submitted_frames += u64::from(frames);
            }
        };
        // SAFETY: started client, Stop is attempted on success and every failure.
        let stopped = unsafe { (client.vtable().Stop)(client.as_raw()) };
        if result.0 != 0 { result } else { stopped }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[global_allocator]
    static ALLOCATOR: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

    #[test]
    fn buffer_transfer_never_allocates() {
        let bytes = [1u8; 24_576];
        let mut transfer = Transfer::new(&bytes, bytes.len(), 250).unwrap();
        let mut buffer = [0; 3840];
        assert_no_alloc::assert_no_alloc(|| {
            while !transfer.done() {
                let count = transfer.fill(&mut buffer);
                transfer.commit(count);
            }
            assert_eq!(transfer.sent_bursts(), 250);
        });
    }

    #[test]
    fn whole_buffers_repeat_and_pad_without_counting_partial_bursts() {
        let bytes = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut transfer = Transfer::new(&bytes, 4, 3).unwrap();
        let mut buffer = [0; 5];
        assert_eq!(transfer.fill(&mut buffer), 5);
        assert_eq!(buffer, [1, 2, 3, 4, 5]);
        assert_eq!(transfer.sent_bursts(), 0);
        transfer.commit(5);
        assert_eq!(transfer.sent_bursts(), 1);
        assert_eq!(transfer.fill(&mut buffer), 5);
        assert_eq!(buffer, [6, 7, 8, 1, 2]);
        transfer.commit(5);
        assert_eq!(transfer.fill(&mut buffer), 2);
        assert_eq!(buffer, [3, 4, 0, 0, 0]);
        transfer.commit(2);
        assert_eq!(transfer.sent_bursts(), 3);
        assert!(transfer.done());
    }
}
