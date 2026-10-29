//! Platform-independent translation of process-loopback packets into ABI records.

use std::mem::size_of;

use ssr_abi::{MAX_DATA_PAYLOAD, RecordWriter, record_size, wave_format_bytes};
use ssr_types::{PcmFormat, SampleType, SourceFormat};

pub const STOP_IDLE_MS: u64 = 200;
pub const LOOPBACK_RATE: u32 = 48_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopbackLayout {
    pub channels: u16,
    pub channel_mask: u32,
}

impl LoopbackLayout {
    pub const STEREO: Self = Self {
        channels: 2,
        channel_mask: 0x3,
    };

    pub fn from_mix(channels: u16, channel_mask: u32) -> Self {
        if (1..=8).contains(&channels)
            && channel_mask != 0
            && channel_mask.count_ones() == u32::from(channels)
        {
            Self {
                channels,
                channel_mask,
            }
        } else {
            Self::STEREO
        }
    }

    pub(crate) fn bytes_per_frame(self) -> usize {
        usize::from(self.channels) * size_of::<f32>()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PacketFlags {
    pub silent: bool,
    pub discontinuity: bool,
    pub timestamp_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StageError {
    #[error("a packet is still pending")]
    Pending,
    #[error("packet of {frames} frames exceeds the staging buffer of {capacity}")]
    TooLarge { frames: u32, capacity: usize },
    #[error("packet data length {len} does not match {frames} frames")]
    Length { len: usize, frames: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingStage {
    Start,
    Format,
    Overrun,
    Data,
    Done,
}

#[derive(Clone, Copy, Debug)]
struct PendingPacket {
    len: usize,
    offset: usize,
    qpc: u64,
    stage: PendingStage,
    format_repeat: bool,
    discontinuity: bool,
}

#[derive(Debug)]
pub struct LoopbackTranslator {
    endpoint: u16,
    qpc_frequency: u64,
    format: Vec<u8>,
    bytes_per_frame: usize,
    staging: Vec<u8>,
    pending: Option<PendingPacket>,
    epoch: u32,
    byte_position: u64,
    streaming: bool,
    published_format: bool,
    last_packet_qpc: Option<u64>,
}

impl LoopbackTranslator {
    /// `endpoint` is the lane (0 main, 1 music). All storage is allocated here.
    pub fn new(
        endpoint: u16,
        layout: LoopbackLayout,
        qpc_frequency: u64,
        max_packet_frames: usize,
    ) -> Self {
        let layout = LoopbackLayout::from_mix(layout.channels, layout.channel_mask);
        let format = SourceFormat::Pcm(PcmFormat {
            channels: layout.channels,
            sample_rate: LOOPBACK_RATE,
            bits_per_sample: 32,
            valid_bits: 32,
            sample_type: SampleType::Float,
            channel_mask: layout.channel_mask,
        });
        let bytes_per_frame = layout.bytes_per_frame();
        Self {
            endpoint,
            qpc_frequency: qpc_frequency.max(1),
            format: wave_format_bytes(&format),
            bytes_per_frame,
            staging: vec![0; max_packet_frames.saturating_mul(bytes_per_frame)],
            pending: None,
            epoch: 0,
            byte_position: 0,
            streaming: false,
            published_format: false,
            last_packet_qpc: None,
        }
    }

    pub fn endpoint(&self) -> u16 {
        self.endpoint
    }

    /// WASAPI's process-loopback QPC position is in 100 ns units.
    pub fn qpc_from_hns(&self, hns: u64) -> u64 {
        u64::try_from(u128::from(hns).saturating_mul(u128::from(self.qpc_frequency)) / 10_000_000)
            .unwrap_or(u64::MAX)
    }

    pub fn stage(
        &mut self,
        frames: u32,
        data: Option<&[u8]>,
        flags: PacketFlags,
        qpc: u64,
    ) -> Result<(), StageError> {
        if self.pending.is_some() {
            return Err(StageError::Pending);
        }
        let frames_usize = frames as usize;
        if frames_usize > self.staging.len() / self.bytes_per_frame {
            return Err(StageError::TooLarge {
                frames,
                capacity: self.staging.len() / self.bytes_per_frame,
            });
        }
        let len = frames_usize * self.bytes_per_frame;
        if flags.silent {
            self.staging[..len].fill(0);
        } else {
            let actual = data.map_or(0, <[u8]>::len);
            if actual != len {
                return Err(StageError::Length {
                    len: actual,
                    frames,
                });
            }
            self.staging[..len].copy_from_slice(data.expect("length checked above"));
        }
        let restarting = !self.streaming;
        self.pending = Some(PendingPacket {
            len,
            offset: 0,
            qpc,
            stage: if restarting {
                PendingStage::Start
            } else if flags.discontinuity {
                PendingStage::Overrun
            } else {
                PendingStage::Data
            },
            format_repeat: restarting && self.published_format,
            discontinuity: flags.discontinuity,
        });
        Ok(())
    }

    pub fn drain(&mut self, out: &mut RecordWriter<'_>) -> usize {
        let previous_endpoint = out.endpoint();
        out.set_endpoint(self.endpoint);
        let mut count = 0;
        while let Some(pending) = self.pending.as_mut() {
            match pending.stage {
                PendingStage::Start => {
                    let epoch = if self.published_format {
                        self.epoch
                    } else {
                        self.epoch.wrapping_add(1)
                    };
                    if !out.stream_start(epoch, pending.qpc) {
                        break;
                    }
                    self.epoch = epoch;
                    self.byte_position = 0;
                    self.streaming = true;
                    pending.stage = PendingStage::Format;
                    count += 1;
                }
                PendingStage::Format => {
                    let written = if pending.format_repeat {
                        out.format_repeat(self.epoch, pending.qpc, &self.format)
                    } else {
                        out.format_change(self.epoch, pending.qpc, &self.format)
                    };
                    if !written {
                        break;
                    }
                    self.published_format = true;
                    pending.stage = if pending.discontinuity {
                        PendingStage::Overrun
                    } else {
                        PendingStage::Data
                    };
                    count += 1;
                }
                PendingStage::Overrun => {
                    if !out.overrun(self.byte_position, pending.qpc, 0) {
                        break;
                    }
                    pending.stage = PendingStage::Data;
                    count += 1;
                }
                PendingStage::Data => {
                    let remaining = pending.len - pending.offset;
                    if remaining == 0 {
                        pending.stage = PendingStage::Done;
                        continue;
                    }
                    let header = record_size(0);
                    if out.remaining() <= header {
                        break;
                    }
                    let chunk = remaining
                        .min(MAX_DATA_PAYLOAD as usize)
                        .min(out.remaining() - header);
                    if chunk == 0 {
                        break;
                    }
                    let frame_offset = pending.offset / self.bytes_per_frame;
                    let qpc_offset = u64::try_from(
                        (frame_offset as u128).saturating_mul(self.qpc_frequency as u128)
                            / LOOPBACK_RATE as u128,
                    )
                    .unwrap_or(u64::MAX);
                    let bytes = &self.staging[pending.offset..pending.offset + chunk];
                    if !out.data(
                        self.epoch,
                        self.byte_position,
                        pending.qpc.saturating_add(qpc_offset),
                        bytes,
                    ) {
                        break;
                    }
                    pending.offset += chunk;
                    self.byte_position = self.byte_position.saturating_add(chunk as u64);
                    if pending.offset == pending.len {
                        pending.stage = PendingStage::Done;
                    }
                    count += 1;
                }
                PendingStage::Done => {
                    let finished = self.pending.take().expect("matched pending packet");
                    self.last_packet_qpc = Some(finished.qpc);
                    break;
                }
            }
        }
        out.set_endpoint(previous_endpoint);
        count
    }

    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn is_streaming(&self) -> bool {
        self.streaming
    }

    pub fn idle(&mut self, now_qpc: u64, out: &mut RecordWriter<'_>) -> bool {
        let stop_ticks = self.qpc_frequency.saturating_mul(STOP_IDLE_MS) / 1_000;
        if !self.streaming
            || self.pending.is_some()
            || !self
                .last_packet_qpc
                .is_some_and(|last| now_qpc.saturating_sub(last) >= stop_ticks)
        {
            return false;
        }
        self.write_stop(now_qpc, out)
    }

    pub fn end(&mut self, now_qpc: u64, out: &mut RecordWriter<'_>) -> bool {
        if !self.streaming || self.pending.is_some() {
            return false;
        }
        self.write_stop(now_qpc, out)
    }

    fn write_stop(&mut self, qpc: u64, out: &mut RecordWriter<'_>) -> bool {
        let previous_endpoint = out.endpoint();
        out.set_endpoint(self.endpoint);
        let written = out.stream_stop(self.epoch, qpc);
        out.set_endpoint(previous_endpoint);
        if written {
            self.streaming = false;
        }
        written
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssr_abi::{Record, records};

    const QPC_FREQUENCY: u64 = 10_000_000;

    fn drain_all(translator: &mut LoopbackTranslator, capacity: usize) -> Vec<Record<'static>> {
        let mut bytes = vec![0; capacity];
        let len = {
            let mut writer = RecordWriter::new(&mut bytes);
            translator.drain(&mut writer);
            writer.len()
        };
        let leaked = Box::leak(bytes[..len].to_vec().into_boxed_slice());
        records(leaked).collect::<Result<Vec<_>, _>>().unwrap()
    }

    #[test]
    fn mix_layout_accepts_valid_masks_and_falls_back_to_stereo() {
        assert_eq!(
            LoopbackLayout::from_mix(1, 0x4),
            LoopbackLayout {
                channels: 1,
                channel_mask: 0x4,
            }
        );
        assert_eq!(
            LoopbackLayout::from_mix(8, 0x63f),
            LoopbackLayout {
                channels: 8,
                channel_mask: 0x63f,
            }
        );
        for invalid in [(0, 0), (9, 0x1ff), (2, 0), (2, 0x7), (8, 0x3f)] {
            assert_eq!(
                LoopbackLayout::from_mix(invalid.0, invalid.1),
                LoopbackLayout::STEREO
            );
        }
    }

    #[test]
    fn mix_layout_sets_format_and_frame_width() {
        let layout = LoopbackLayout::from_mix(8, 0x63f);
        let mut translator = LoopbackTranslator::new(0, layout, QPC_FREQUENCY, 1);
        translator
            .stage(1, Some(&[1; 32]), PacketFlags::default(), 100)
            .unwrap();
        let records = drain_all(&mut translator, 512);
        assert!(matches!(
            records[1],
            Record::FormatChange {
                format: SourceFormat::Pcm(PcmFormat {
                    channels: 8,
                    channel_mask: 0x63f,
                    ..
                }),
                ..
            }
        ));
        assert!(matches!(records[2], Record::Data { bytes, .. } if bytes == [1; 32]));
    }

    #[test]
    fn first_packet_gives_start_format_and_endpoint_one_data() {
        let mut translator = LoopbackTranslator::new(1, LoopbackLayout::STEREO, QPC_FREQUENCY, 8);
        translator
            .stage(1, Some(&[1; 8]), PacketFlags::default(), 100)
            .unwrap();
        let records = drain_all(&mut translator, 512);
        assert!(matches!(
            records[0],
            Record::StreamStart {
                endpoint: 1,
                epoch: 1,
                ..
            }
        ));
        assert!(matches!(
            records[1],
            Record::FormatChange {
                endpoint: 1,
                epoch: 1,
                repeat: false,
                format: SourceFormat::Pcm(PcmFormat {
                    sample_rate: LOOPBACK_RATE,
                    channels: 2,
                    channel_mask: 0x3,
                    sample_type: SampleType::Float,
                    ..
                }),
                ..
            }
        ));
        assert!(
            matches!(records[2], Record::Data { endpoint: 1, epoch: 1, bytes, .. } if bytes == [1; 8])
        );
    }

    #[test]
    fn silent_packet_is_zero_and_discontinuity_precedes_data() {
        let mut translator = LoopbackTranslator::new(0, LoopbackLayout::STEREO, QPC_FREQUENCY, 8);
        translator
            .stage(
                1,
                None,
                PacketFlags {
                    silent: true,
                    discontinuity: true,
                    timestamp_error: false,
                },
                100,
            )
            .unwrap();
        let records = drain_all(&mut translator, 512);
        assert!(matches!(records[2], Record::Overrun { lost_bytes: 0, .. }));
        assert!(matches!(records[3], Record::Data { bytes, .. } if bytes == [0; 8]));
    }

    #[test]
    fn idle_stops_and_restart_keeps_epoch_and_repeats_format() {
        let mut translator = LoopbackTranslator::new(0, LoopbackLayout::STEREO, QPC_FREQUENCY, 8);
        translator
            .stage(1, Some(&[2; 8]), PacketFlags::default(), 100)
            .unwrap();
        let _ = drain_all(&mut translator, 512);
        let mut bytes = [0; 128];
        let stop_len = {
            let mut writer = RecordWriter::new(&mut bytes);
            assert!(!translator.idle(100 + QPC_FREQUENCY / 5 - 1, &mut writer));
            assert!(translator.idle(100 + QPC_FREQUENCY / 5, &mut writer));
            writer.len()
        };
        assert!(matches!(
            records(&bytes[..stop_len]).next().unwrap().unwrap(),
            Record::StreamStop { epoch: 1, .. }
        ));
        translator
            .stage(1, Some(&[3; 8]), PacketFlags::default(), 300)
            .unwrap();
        let records = drain_all(&mut translator, 512);
        assert!(matches!(records[0], Record::StreamStart { epoch: 1, .. }));
        assert!(matches!(
            records[1],
            Record::FormatChange {
                epoch: 1,
                repeat: true,
                ..
            }
        ));
    }

    #[test]
    fn converts_hns_to_qpc_with_saturation() {
        let translator = LoopbackTranslator::new(0, LoopbackLayout::STEREO, 3_000_000, 1);
        assert_eq!(translator.qpc_from_hns(20_000_000), 6_000_000);
        let translator = LoopbackTranslator::new(0, LoopbackLayout::STEREO, u64::MAX, 1);
        assert_eq!(translator.qpc_from_hns(u64::MAX), u64::MAX);
    }

    #[test]
    fn large_packet_drains_across_small_writers_without_loss() {
        let bytes_per_frame = LoopbackLayout::STEREO.bytes_per_frame();
        let frames = (MAX_DATA_PAYLOAD as usize / bytes_per_frame) + 2;
        let input: Vec<u8> = (0..frames * bytes_per_frame).map(|i| i as u8).collect();
        let mut translator =
            LoopbackTranslator::new(0, LoopbackLayout::STEREO, QPC_FREQUENCY, frames);
        translator
            .stage(frames as u32, Some(&input), PacketFlags::default(), 500)
            .unwrap();
        let first = drain_all(
            &mut translator,
            record_size(0) * 2 + record_size(40) + record_size(MAX_DATA_PAYLOAD as usize),
        );
        assert!(translator.has_pending());
        let second = drain_all(
            &mut translator,
            record_size(input.len() - MAX_DATA_PAYLOAD as usize),
        );
        assert!(!translator.has_pending());
        let actual: Vec<u8> = first
            .into_iter()
            .chain(second)
            .filter_map(|record| match record {
                Record::Data { bytes, .. } => Some(bytes),
                _ => None,
            })
            .flatten()
            .copied()
            .collect();
        assert_eq!(actual, input);
    }

    #[test]
    fn writer_too_small_keeps_pending_and_end_stops_once() {
        let mut translator = LoopbackTranslator::new(0, LoopbackLayout::STEREO, QPC_FREQUENCY, 1);
        translator
            .stage(1, Some(&[4; 8]), PacketFlags::default(), 1)
            .unwrap();
        let mut tiny = [0; 1];
        let mut writer = RecordWriter::new(&mut tiny);
        assert_eq!(translator.drain(&mut writer), 0);
        assert!(translator.has_pending());
        let _ = drain_all(&mut translator, 512);
        let mut bytes = [0; 128];
        let len = {
            let mut writer = RecordWriter::new(&mut bytes);
            assert!(translator.end(2, &mut writer));
            assert!(!translator.end(3, &mut writer));
            writer.len()
        };
        assert_eq!(records(&bytes[..len]).count(), 1);
    }
}
