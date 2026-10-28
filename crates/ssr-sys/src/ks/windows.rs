#![allow(unsafe_code)]

use super::{
    discovery::render_pin,
    io::{self, Control, Handle, Packet},
    sample::SampleFormat,
    timing::{PlaybackTiming, RingPosition, WaveRtWrite},
};
use crate::{
    playback_clock::{ClockReading, scale},
    qpc,
};
use ::windows::{
    Win32::{
        Foundation::{GENERIC_WRITE, HANDLE, WIN32_ERROR},
        Media::{
            Audio::{WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0},
            KernelStreaming::*,
            Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        },
    },
    core::GUID,
};
use ssr_types::{
    OutputError, OutputFormat, OutputLayout, OutputMode, OutputOpener, OutputSink, ports::KsTarget,
};
use std::{
    fmt,
    mem::size_of,
    ptr,
    sync::atomic::{Ordering, fence},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct KsOpener {
    pub target: KsTarget,
    pub rate: u32,
    pub period: Option<Duration>,
}

pub struct KsSink {
    pin: Handle,
    control: Control,
    transport: Transport,
    format: OutputFormat,
    encoding: SampleFormat,
    samples: Box<[f32]>,
    converted: Box<[u8]>,
    running: bool,
    submitted: u64,
    played: Option<u64>,
    position_supported: bool,
    started: u64,
    frequency: u64,
    clock: PlaybackTiming,
    late_resyncs: u64,
}
impl fmt::Debug for KsSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KsSink")
            .field("format", &self.format)
            .field("encoding", &self.encoding)
            .field("interface", &self.interface())
            .finish_non_exhaustive()
    }
}

enum Transport {
    Standard {
        packets: [Packet; 2],
        next: usize,
    },
    WaveRt {
        buffer: KSRTAUDIO_BUFFER,
        event: Handle,
        notification: bool,
        register: Option<KSRTAUDIO_HWREGISTER>,
        position: RingPosition,
        latency_hns: u64,
        write_bytes: u64,
        guard_bytes: u64,
    },
}

/// Periods in a WaveRT ring. The writer refills a period once it is at most
/// `WAVERT_PERIODS - 1` periods ahead, and a wait can oversleep by the system
/// timer's resolution (15.6 ms unless a process raised it), which our virtual
/// driver's position timer shares. Two 10 ms periods left 10 ms of slack and
/// the writer resynced several times a second; three periods of slack cover it.
const WAVERT_PERIODS: usize = 4;

fn open_error(error: WIN32_ERROR) -> OutputError {
    OutputError::Io(format!("KS Win32 error {}", error.0))
}
// Value-only RT errors: no formatting or heap allocation on the output thread.
fn rt_error(_: WIN32_ERROR) -> OutputError {
    OutputError::Removed
}
fn rejected(reason: &str) -> OutputError {
    OutputError::FormatRejected(reason.into())
}

#[repr(C)]
struct ConnectFormat {
    connect: KSPIN_CONNECT,
    data: KSDATAFORMAT,
    wave: WAVEFORMATEXTENSIBLE,
}

const _: () = {
    assert!(std::mem::offset_of!(ConnectFormat, data) == size_of::<KSPIN_CONNECT>());
    assert!(
        std::mem::offset_of!(ConnectFormat, wave)
            == size_of::<KSPIN_CONNECT>() + size_of::<KSDATAFORMAT>()
    );
    assert!(size_of::<WAVEFORMATEXTENSIBLE>() == 40);
};

impl OutputOpener for KsOpener {
    type Sink = KsSink;
    fn set_period(&mut self, period: Duration) -> bool {
        self.period = Some(period);
        true
    }
    fn open(&mut self, layout: OutputLayout) -> Result<KsSink, OutputError> {
        self.open_detailed(layout).map_err(|failure| failure.error)
    }
}

pub(super) struct OpenFailure {
    pub reason: ssr_types::ports::KsFallbackReason,
    pub code: u32,
    pub error: OutputError,
}
impl From<OutputError> for OpenFailure {
    fn from(error: OutputError) -> Self {
        Self {
            reason: ssr_types::ports::KsFallbackReason::FormatRejected,
            code: 0,
            error,
        }
    }
}
fn failure(reason: ssr_types::ports::KsFallbackReason, code: WIN32_ERROR) -> OpenFailure {
    OpenFailure {
        reason,
        code: code.0,
        error: open_error(code),
    }
}
impl KsOpener {
    pub(super) fn open_detailed(&mut self, layout: OutputLayout) -> Result<KsSink, OpenFailure> {
        use ssr_types::ports::KsFallbackReason as Reason;
        if layout.channels == 0
            || layout.channels > 64
            || (layout.channel_mask != 0
                && layout.channel_mask.count_ones() != u32::from(layout.channels))
            || self.rate == 0
            || self.rate > 768_000
        {
            return Err(rejected("invalid KS rate/layout").into());
        }
        let period = self.period.unwrap_or(Duration::from_millis(10));
        let frames = super::selection::period_frames(period, self.rate)
            .ok_or_else(|| rejected("KS period outside one frame..one second"))?;
        let filter =
            Handle::filter(&self.target.filter).map_err(|e| failure(Reason::FilterOpen, e))?;
        let mut control = Control::new().map_err(|e| failure(Reason::BufferAlloc, e))?;
        if !render_pin(&mut control, &filter, self.target.pin) {
            return Err(OpenFailure {
                reason: Reason::NoSinkPin,
                code: 0,
                error: rejected("KS pin is not a render sink"),
            });
        }
        let interfaces = identifiers(
            &mut control,
            &filter,
            self.target.pin,
            KSPROPERTY_PIN_INTERFACES,
        )?;
        let mediums = identifiers(
            &mut control,
            &filter,
            self.target.pin,
            KSPROPERTY_PIN_MEDIUMS,
        )?;
        let medium = mediums
            .into_iter()
            .find(|m| m.Set == KSMEDIUMSETID_Standard && m.Id == KSMEDIUM_STANDARD_DEVIO)
            .ok_or_else(|| rejected("KS DEVIO medium unavailable"))?;
        let ranges = io::variable(
            &mut control,
            &filter,
            &io::pin_property(self.target.pin, KSPROPERTY_PIN_DATARANGES),
        )
        .map_err(open_error)?;
        let mut last: OpenFailure = rejected("no compatible KS data range/interface").into();
        for encoding in SampleFormat::ALL {
            if !supports(&ranges, encoding, self.rate, layout.channels) {
                continue;
            }
            for interface_id in [
                KSINTERFACE_STANDARD_LOOPED_STREAMING.0 as u32,
                KSINTERFACE_STANDARD_STREAMING.0 as u32,
            ] {
                let Some(interface) = interfaces
                    .iter()
                    .find(|i| i.Set == KSINTERFACESETID_Standard && i.Id == interface_id)
                else {
                    continue;
                };
                let block = usize::from(layout.channels) * encoding.bytes();
                let subtype = if encoding == SampleFormat::Float32 {
                    KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
                } else {
                    KSDATAFORMAT_SUBTYPE_PCM
                };
                let connect = ConnectFormat {
                    connect: KSPIN_CONNECT {
                        Interface: io::id(interface.Set, interface.Id, interface.Flags),
                        Medium: io::id(medium.Set, medium.Id, medium.Flags),
                        PinId: self.target.pin,
                        PinToHandle: HANDLE::default(),
                        Priority: KSPRIORITY {
                            PriorityClass: KSPRIORITY_NORMAL,
                            PrioritySubClass: 1,
                        },
                    },
                    data: KSDATAFORMAT {
                        Anonymous: KSDATAFORMAT_0 {
                            FormatSize: (size_of::<KSDATAFORMAT>()
                                + size_of::<WAVEFORMATEXTENSIBLE>())
                                as u32,
                            SampleSize: block as u32,
                            MajorFormat: KSDATAFORMAT_TYPE_AUDIO,
                            SubFormat: subtype,
                            Specifier: KSDATAFORMAT_SPECIFIER_WAVEFORMATEX,
                            ..Default::default()
                        },
                    },
                    wave: WAVEFORMATEXTENSIBLE {
                        Format: WAVEFORMATEX {
                            wFormatTag: 0xfffe,
                            nChannels: layout.channels,
                            nSamplesPerSec: self.rate,
                            nAvgBytesPerSec: self.rate * block as u32,
                            nBlockAlign: block as u16,
                            wBitsPerSample: encoding.bits(),
                            cbSize: 22,
                        },
                        Samples: WAVEFORMATEXTENSIBLE_0 {
                            wValidBitsPerSample: encoding.valid_bits(),
                        },
                        dwChannelMask: layout.channel_mask,
                        SubFormat: subtype,
                    },
                };
                let mut raw = HANDLE::default();
                // SAFETY: repr(C) connect is immediately followed by KSDATAFORMAT
                // and WAVEFORMATEXTENSIBLE; requested length is exactly those fields.
                let status =
                    unsafe { KsCreatePin(filter.0, &connect.connect, GENERIC_WRITE.0, &mut raw) };
                if status != 0 {
                    last = failure(Reason::PinCreate, WIN32_ERROR(status));
                    continue;
                }
                let pin = Handle(raw);
                let transport = if interface_id == KSINTERFACE_STANDARD_LOOPED_STREAMING.0 as u32 {
                    match wave_rt(&mut control, &pin, frames * block * WAVERT_PERIODS, block) {
                        Ok(transport) => transport,
                        Err(e) => {
                            last = failure(Reason::BufferAlloc, e);
                            continue;
                        }
                    }
                } else {
                    Transport::Standard {
                        packets: [
                            Packet::new(frames * block)
                                .map_err(|e| failure(Reason::BufferAlloc, e))?,
                            Packet::new(frames * block)
                                .map_err(|e| failure(Reason::BufferAlloc, e))?,
                        ],
                        next: 0,
                    }
                };
                let period_frames = match &transport {
                    Transport::WaveRt { buffer, .. } => {
                        buffer.ActualBufferSize as usize / block / WAVERT_PERIODS
                    }
                    _ => frames,
                };
                // Own the pin and mapping before changing state: every failure from
                // here uses Drop's reverse state transition and notification cleanup.
                let mut sink = KsSink {
                    pin,
                    control,
                    transport,
                    format: OutputFormat {
                        rate: self.rate,
                        channels: layout.channels,
                        channel_mask: layout.channel_mask,
                        period_frames,
                        mode: OutputMode::KernelStreaming,
                        fell_back: false,
                        ks_fallback: false,
                        ks_fallback_reason: None,
                        ks_fallback_code: 0,
                        mix_channels: layout.channels,
                    },
                    encoding,
                    samples: vec![0.0; period_frames * usize::from(layout.channels)]
                        .into_boxed_slice(),
                    converted: vec![0; period_frames * block].into_boxed_slice(),
                    running: false,
                    submitted: 0,
                    played: None,
                    position_supported: true,
                    started: 0,
                    frequency: qpc::frequency(),
                    clock: PlaybackTiming::default(),
                    late_resyncs: 0,
                };
                sink.control
                    .state(&sink.pin, KSSTATE_ACQUIRE)
                    .map_err(|e| failure(Reason::StateChange, e))?;
                sink.control
                    .state(&sink.pin, KSSTATE_PAUSE)
                    .map_err(|e| failure(Reason::StateChange, e))?;
                return Ok(sink);
            }
        }
        Err(last)
    }
}

fn identifiers(
    control: &mut Control,
    filter: &Handle,
    pin: u32,
    property: KSPROPERTY_PIN,
) -> Result<Vec<KSIDENTIFIER_0_0>, OutputError> {
    let bytes =
        io::variable(control, filter, &io::pin_property(pin, property)).map_err(open_error)?;
    let header = io::read::<KSMULTIPLE_ITEM>(&bytes, 0)
        .ok_or_else(|| rejected("truncated KS identifier list"))?;
    if header.Count > 1024 || header.Size as usize > bytes.len() {
        return Err(rejected("invalid KS identifier count"));
    }
    (0..header.Count as usize)
        .map(|i| {
            io::read(
                &bytes,
                size_of::<KSMULTIPLE_ITEM>() + i * size_of::<KSIDENTIFIER>(),
            )
            .ok_or_else(|| rejected("truncated KS identifier"))
        })
        .collect()
}
fn supports(bytes: &[u8], format: SampleFormat, rate: u32, channels: u16) -> bool {
    let Some(header) = io::read::<KSMULTIPLE_ITEM>(bytes, 0) else {
        return false;
    };
    if header.Count > 1024 || header.Size as usize > bytes.len() || header.Size < 8 {
        return false;
    }
    let bytes = &bytes[..header.Size as usize];
    let mut offset = size_of::<KSMULTIPLE_ITEM>();
    for _ in 0..header.Count {
        let Some(data) = io::read::<KSDATAFORMAT_0>(bytes, offset) else {
            return false;
        };
        let size = data.FormatSize as usize;
        if size < size_of::<KSDATAFORMAT>()
            || offset.checked_add(size).is_none_or(|end| end > bytes.len())
        {
            return false;
        }
        let subtype = if format == SampleFormat::Float32 {
            KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
        } else {
            KSDATAFORMAT_SUBTYPE_PCM
        };
        if (data.MajorFormat == KSDATAFORMAT_TYPE_AUDIO || data.MajorFormat == GUID::zeroed())
            && (data.SubFormat == subtype || data.SubFormat == GUID::zeroed())
            && (data.Specifier == KSDATAFORMAT_SPECIFIER_WAVEFORMATEX
                || data.Specifier == GUID::zeroed())
            && size >= size_of::<KSDATARANGE_AUDIO>()
        {
            // KSDATARANGE_AUDIO's five u32 fields follow the 64-byte data range.
            let range = &bytes[offset + size_of::<KSDATAFORMAT>()..offset + size];
            let values: [u32; 5] = std::array::from_fn(|i| {
                u32::from_le_bytes(range[i * 4..i * 4 + 4].try_into().unwrap())
            });
            if super::selection::audio_range(values, format.bits(), rate, channels) {
                return true;
            }
        }
        offset += (size + 7) & !7;
    }
    false
}

fn notification(
    control: &mut Control,
    pin: &Handle,
    event: &Handle,
    register: bool,
) -> Result<(), WIN32_ERROR> {
    let request = KSRTAUDIO_NOTIFICATION_EVENT_PROPERTY {
        Property: io::id(
            KSPROPSETID_RtAudio,
            if register {
                KSPROPERTY_RTAUDIO_REGISTER_NOTIFICATION_EVENT.0
            } else {
                KSPROPERTY_RTAUDIO_UNREGISTER_NOTIFICATION_EVENT.0
            } as u32,
            KSPROPERTY_TYPE_SET,
        ),
        NotificationEvent: event.0,
    };
    control.bytes(pin, &request, &mut []).map(|_| ())
}
fn wave_rt(
    control: &mut Control,
    pin: &Handle,
    requested: usize,
    block: usize,
) -> Result<Transport, WIN32_ERROR> {
    let event = Handle::event()?;
    let mut buffer = KSRTAUDIO_BUFFER::default();
    let request = KSRTAUDIO_BUFFER_PROPERTY_WITH_NOTIFICATION {
        Property: io::id(
            KSPROPSETID_RtAudio,
            KSPROPERTY_RTAUDIO_BUFFER_WITH_NOTIFICATION.0 as u32,
            KSPROPERTY_TYPE_GET,
        ),
        RequestedBufferSize: requested as u32,
        NotificationCount: 2,
        ..Default::default()
    };
    let notified = match control.query(pin, &request, &mut buffer) {
        Ok(n) if n as usize == size_of::<KSRTAUDIO_BUFFER>() => true,
        Ok(_) => return Err(WIN32_ERROR(13)),
        Err(_) => false,
    };
    if !notified {
        let request = KSRTAUDIO_BUFFER_PROPERTY {
            Property: io::id(
                KSPROPSETID_RtAudio,
                KSPROPERTY_RTAUDIO_BUFFER.0 as u32,
                KSPROPERTY_TYPE_GET,
            ),
            RequestedBufferSize: requested as u32,
            ..Default::default()
        };
        buffer = KSRTAUDIO_BUFFER::default();
        if control.query(pin, &request, &mut buffer)? as usize != size_of::<KSRTAUDIO_BUFFER>() {
            return Err(WIN32_ERROR(13));
        }
    }
    let size = buffer.ActualBufferSize as usize;
    if buffer.BufferAddress.is_null()
        || size < block * WAVERT_PERIODS
        || size > 16 * 1024 * 1024
        || !size.is_multiple_of(block * 2)
        || (buffer.BufferAddress as usize).checked_add(size).is_none()
    {
        return Err(WIN32_ERROR(13));
    }
    // SAFETY: WDK returned a valid contiguous mapped audio buffer of checked size;
    // pin is stopped and owns this mapping until close, no aliased Rust reference.
    unsafe { ptr::write_bytes(buffer.BufferAddress.cast::<u8>(), 0, size) };
    fence(Ordering::SeqCst);
    let mut register = KSRTAUDIO_HWREGISTER::default();
    let request = KSRTAUDIO_HWREGISTER_PROPERTY {
        Property: io::id(
            KSPROPSETID_RtAudio,
            KSPROPERTY_RTAUDIO_POSITIONREGISTER.0 as u32,
            KSPROPERTY_TYPE_GET,
        ),
        ..Default::default()
    };
    let register = control
        .query(pin, &request, &mut register)
        .ok()
        .filter(|&n| n as usize == size_of::<KSRTAUDIO_HWREGISTER>())
        .and_then(|_| {
            let width = register.Width;
            (!register.Register.is_null()
                && matches!(width, 32 | 64)
                && (register.Register as usize).is_multiple_of((width / 8) as usize)
                && register.Accuracy < buffer.ActualBufferSize / 2
                && (width != 64 || cfg!(target_pointer_width = "64")))
            .then_some(register)
        });
    let latency = control
        .get::<KSRTAUDIO_HWLATENCY>(
            pin,
            KSPROPSETID_RtAudio,
            KSPROPERTY_RTAUDIO_HWLATENCY.0 as u32,
        )
        .unwrap_or_default();
    let guard_bytes = u64::from(latency.FifoSize) + register.map_or(0, |r| u64::from(r.Accuracy));
    if !(WaveRtWrite {
        ring_bytes: size as u64,
        period_bytes: size as u64 / 2,
        frame_bytes: block as u64,
        guard_bytes,
    })
    .guard_fits()
    {
        return Err(WIN32_ERROR(13));
    }
    let notification = notified && notification(control, pin, &event, true).is_ok();
    Ok(Transport::WaveRt {
        buffer,
        event,
        notification,
        register,
        position: RingPosition::with_accuracy(register.map_or(0, |r| u64::from(r.Accuracy))),
        latency_hns: u64::from(latency.ChipsetDelay) + u64::from(latency.CodecDelay),
        write_bytes: size as u64,
        guard_bytes,
    })
}

struct PositionReading {
    bytes: u64,
    qpc: u64,
    latency_hns: u64,
    reserved_write: Option<u64>,
}

impl KsSink {
    pub fn interface(&self) -> &'static str {
        match self.transport {
            Transport::WaveRt { .. } => "wavert",
            Transport::Standard { .. } => "standard",
        }
    }
    pub fn sample_format(&self) -> &'static str {
        self.encoding.name()
    }
    fn start(&mut self) -> Result<(), OutputError> {
        if let Transport::Standard { packets, .. } = &mut self.transport {
            for packet in packets {
                packet.submit(&self.pin).map_err(rt_error)?;
            }
        }
        self.submitted = self.format.period_frames as u64 * 2;
        self.started = qpc::now();
        let bytes_per_second = u64::from(self.format.rate) * self.block_bytes() as u64;
        if let Transport::WaveRt {
            position, buffer, ..
        } = &mut self.transport
        {
            position.update(
                0,
                u64::from(buffer.ActualBufferSize),
                self.started,
                self.frequency,
                bytes_per_second,
            );
        }
        self.control
            .state(&self.pin, KSSTATE_RUN)
            .map_err(rt_error)?;
        self.running = true;
        Ok(())
    }
    fn block_bytes(&self) -> usize {
        usize::from(self.format.channels) * self.encoding.bytes()
    }
    /// Known WaveRT underruns recovered without restarting the pin. These are
    /// separate from stream restarts and the engine's input-ring underruns.
    pub fn late_resyncs(&self) -> u64 {
        self.late_resyncs
    }
    fn ready(
        &mut self,
        reading: &PositionReading,
        resynced: &mut bool,
    ) -> Result<bool, OutputError> {
        let block = self.block_bytes() as u64;
        match &mut self.transport {
            Transport::Standard { packets, next } => {
                packets[*next].ready(&self.pin, 1).map_err(rt_error)
            }
            Transport::WaveRt {
                buffer,
                write_bytes,
                guard_bytes,
                event,
                notification,
                ..
            } => {
                let plan = WaveRtWrite {
                    ring_bytes: u64::from(buffer.ActualBufferSize),
                    period_bytes: self.converted.len() as u64,
                    frame_bytes: block,
                    guard_bytes: *guard_bytes,
                }
                .plan(*write_bytes, reading.bytes, reading.reserved_write)
                .ok_or(OutputError::Removed)?;
                if plan.ready {
                    *write_bytes = plan.write_bytes;
                }
                if plan.underrun && !*resynced {
                    self.late_resyncs = self.late_resyncs.saturating_add(1);
                    *resynced = true;
                }
                if !plan.ready {
                    // Notifications are hints only; always re-read the hardware.
                    event.wait(1).map_err(rt_error)?;
                    if *notification {
                        event.reset().map_err(rt_error)?;
                    }
                }
                Ok(plan.ready)
            }
        }
    }
    fn position(&mut self) -> Result<PositionReading, OutputError> {
        let block = self.block_bytes() as u64;
        let before = qpc::now();
        let mut reserved_write = None;
        let raw = if let Transport::WaveRt {
            register: Some(register),
            ..
        } = &self.transport
        {
            if register.Width == 32 {
                // SAFETY: mapping belongs to the live pin; validated width and
                // alignment; volatile MMIO read, never an Atomic on driver memory.
                u64::from(unsafe { ptr::read_volatile(register.Register.cast::<u32>()) })
            } else {
                // SAFETY: validated aligned 64-bit register on 64-bit targets;
                // mapping remains live until pin closes after this sink's drop.
                unsafe { ptr::read_volatile(register.Register.cast::<u64>()) }
            }
        } else if !self.position_supported {
            return Ok(PositionReading {
                bytes: 0,
                qpc: before,
                latency_hns: 0,
                reserved_write: None,
            });
        } else {
            match self.control.get::<KSAUDIO_POSITION>(
                &self.pin,
                KSPROPSETID_Audio,
                KSPROPERTY_AUDIO_POSITION.0 as u32,
            ) {
                Ok(position) => {
                    reserved_write = Some(position.WriteOffset);
                    position.PlayOffset
                }
                Err(e)
                    if matches!(self.transport, Transport::Standard { .. })
                        && matches!(e.0, 1 | 50 | 87 | 1168) =>
                {
                    // Unsupported property is not device progress. Standard packets
                    // still render; timestamps become explicit clock estimates.
                    self.position_supported = false;
                    return Ok(PositionReading {
                        bytes: 0,
                        qpc: before,
                        latency_hns: 0,
                        reserved_write: None,
                    });
                }
                Err(e) => return Err(rt_error(e)),
            }
        };
        let now = before + qpc::now().saturating_sub(before) / 2;
        match &mut self.transport {
            Transport::WaveRt {
                position,
                buffer,
                latency_hns,
                register,
                ..
            } => {
                let bytes = position
                    .update(
                        raw,
                        u64::from(buffer.ActualBufferSize),
                        now,
                        self.frequency,
                        u64::from(self.format.rate) * block,
                    )
                    .ok_or(OutputError::Removed)?;
                // AUDIO_POSITION already reports DAC position. A mapped bus
                // register can precede the codec; only that path adds HWLATENCY.
                Ok(PositionReading {
                    bytes,
                    qpc: now,
                    latency_hns: if register.is_some() { *latency_hns } else { 0 },
                    reserved_write,
                })
            }
            _ => Ok(PositionReading {
                bytes: raw,
                qpc: now,
                latency_hns: 0,
                reserved_write,
            }),
        }
    }
}
impl OutputSink for KsSink {
    fn format(&self) -> OutputFormat {
        self.format
    }
    fn played_frames(&self) -> Option<u64> {
        self.played
    }
    fn late_resyncs(&self) -> u64 {
        self.late_resyncs
    }
    fn next_period(&mut self, fill: &mut dyn FnMut(&mut [f32])) -> Result<u64, OutputError> {
        if !self.running {
            self.start()?;
        }
        let block = self.block_bytes() as u64;
        let entered = qpc::now();
        let mut resynced = false;
        let mut reading = loop {
            let reading = self.position()?;
            if self.ready(&reading, &mut resynced)? {
                break reading;
            }
            if qpc::now().saturating_sub(entered) > self.frequency * 2 {
                return Err(OutputError::Removed);
            }
        };
        self.samples.fill(0.0);
        fill(&mut self.samples);
        self.encoding.convert(&self.samples, &mut self.converted);
        // Filling can take time (or the thread can be preempted). Re-read the
        // hardware before touching mapped memory, not merely before fill().
        if matches!(self.transport, Transport::WaveRt { .. }) {
            loop {
                reading = self.position()?;
                if qpc::now().saturating_sub(entered) > self.frequency * 2 {
                    return Err(OutputError::Removed);
                }
                if self.ready(&reading, &mut resynced)? {
                    break;
                }
                // Keep the already-filled block while the reserved region moves;
                // never call fill twice or overwrite only part of its span.
            }
        }
        let played = reading.bytes / block;
        self.played = self.position_supported.then_some(played);
        let first_frame = match &self.transport {
            Transport::WaveRt { write_bytes, .. } => *write_bytes / block,
            Transport::Standard { .. } => self.submitted.max(played),
        };
        match &mut self.transport {
            Transport::Standard { packets, next } => {
                packets[*next].bytes.copy_from_slice(&self.converted);
                packets[*next].submit(&self.pin).map_err(rt_error)?;
                *next = (*next + 1) % 2;
            }
            Transport::WaveRt {
                buffer,
                write_bytes,
                ..
            } => {
                let size = buffer.ActualBufferSize as usize;
                let offset = (*write_bytes % size as u64) as usize;
                let first = self.converted.len().min(size - offset);
                let destination = buffer.BufferAddress.cast::<u8>().wrapping_add(offset);
                // SAFETY: position-based free-space check reserves this span; it
                // is in the live mapped buffer and cannot overlap source storage.
                unsafe { ptr::copy_nonoverlapping(self.converted.as_ptr(), destination, first) };
                if first < self.converted.len() {
                    // SAFETY: remaining span wraps to buffer start, remains within
                    // the reserved free region and is disjoint from source storage.
                    unsafe {
                        ptr::copy_nonoverlapping(
                            self.converted[first..].as_ptr(),
                            buffer.BufferAddress.cast::<u8>(),
                            self.converted.len() - first,
                        )
                    };
                }
                fence(Ordering::SeqCst);
                *write_bytes += self.converted.len() as u64;
            }
        }
        self.submitted = first_frame.saturating_add(self.format.period_frames as u64);
        Ok(self.clock.next(
            first_frame,
            self.position_supported.then_some(ClockReading {
                position: played,
                qpc_100ns: scale(reading.qpc, 10_000_000, self.frequency)
                    .saturating_add(reading.latency_hns),
            }),
            self.started,
            self.format.rate,
            self.frequency,
            self.format.period_frames,
            resynced,
        ))
    }
}
impl Drop for KsSink {
    fn drop(&mut self) {
        let _ = self.control.state(&self.pin, KSSTATE_PAUSE);
        if let Transport::Standard { packets, .. } = &mut self.transport {
            for packet in packets {
                packet.drain(&self.pin);
            }
        }
        let _ = self.control.state(&self.pin, KSSTATE_ACQUIRE);
        let _ = self.control.state(&self.pin, KSSTATE_STOP);
        if let Transport::WaveRt {
            event,
            notification: true,
            ..
        } = &self.transport
        {
            let _ = notification(&mut self.control, &self.pin, event, false);
        }
        // pin closes before transport fields: mappings are not touched afterwards.
    }
}
