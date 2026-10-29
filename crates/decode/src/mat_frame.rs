//! IEC 61937-9 MAT framing shared by the TrueHD adapter and diagnostics.
//!
//! [`bitstream::Deframer`] has already undone the little-endian carrier-word
//! storage: [`Burst::payload`] is in codec byte order (the high octet of each
//! IEC 61937 word first). The marker constants below are therefore compared as
//! published; this module must not swap each byte pair again.

use bitstream::{Burst, DataType};

use crate::DecodeError;

pub const MAT_PAYLOAD_BYTES: usize = 61_424;
pub const MAT_CONTENT_BYTES: usize = 61_376;

pub const START_MARKER: [u8; 20] = [
    0x07, 0x9e, 0x00, 0x03, 0x84, 0x01, 0x01, 0x01, 0x80, 0x00, 0x56, 0xa5, 0x3b, 0xf4, 0x81, 0x83,
    0x49, 0x80, 0x77, 0xe0,
];
pub const MIDDLE_MARKER_OFFSET: usize = 30_708;
pub const MIDDLE_MARKER: [u8; 12] = [
    0xc3, 0xc1, 0x42, 0x49, 0x3b, 0xfa, 0x82, 0x83, 0x49, 0x80, 0x77, 0xe0,
];
pub const END_MARKER_OFFSET: usize = 61_408;
pub const END_MARKER: [u8; 16] = [
    0xc3, 0xc2, 0xc0, 0xc4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x97, 0x11,
];

const FBA_MAJOR_SYNC: [u8; 4] = [0xf8, 0x72, 0x6f, 0xba];
pub const FIRST_READY_BYTES: usize = MIDDLE_MARKER_OFFSET + MIDDLE_MARKER.len();
pub const FIRST_CONTENT: std::ops::Range<usize> = START_MARKER.len()..MIDDLE_MARKER_OFFSET;
pub const SECOND_CONTENT: std::ops::Range<usize> = FIRST_READY_BYTES..END_MARKER_OFFSET;

/// The content carried between the three fixed MAT records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatPayload<'a> {
    /// Content containing an FBA TrueHD/MLP major sync.
    TrueHd(&'a [u8]),
    /// A MAT payload whose content syntax is not identified yet.
    Unknown(&'a [u8]),
}

impl MatPayload<'_> {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::TrueHd(bytes) | Self::Unknown(bytes) => bytes,
        }
    }
}

/// Validates the fixed records available at R-MAT and returns the first
/// content span. `payload` may contain bytes beyond the middle marker. No
/// allocation is performed.
pub fn first_content(payload: &[u8]) -> Result<&[u8], DecodeError> {
    if payload.len() < FIRST_READY_BYTES {
        return Err(corrupt("the payload has not reached the middle marker"));
    }
    if payload[..START_MARKER.len()] != START_MARKER {
        return Err(corrupt("the start marker is invalid"));
    }
    if payload[MIDDLE_MARKER_OFFSET..FIRST_READY_BYTES] != MIDDLE_MARKER {
        return Err(corrupt("the middle marker is invalid"));
    }
    Ok(&payload[FIRST_CONTENT])
}

/// Validates a complete IEC 61937-9 MAT payload and returns only its second
/// content span. This is the completion call paired with [`first_content`]. No
/// allocation is performed.
pub fn rest_content(payload: &[u8]) -> Result<&[u8], DecodeError> {
    if payload.len() != MAT_PAYLOAD_BYTES {
        return Err(corrupt("the payload is not 61,424 bytes"));
    }
    first_content(payload)?;
    if payload[END_MARKER_OFFSET..] != END_MARKER {
        return Err(corrupt("the end marker is invalid"));
    }
    Ok(&payload[SECOND_CONTENT])
}

/// Validates one complete IEC 61937-9 MAT payload and copies only its two
/// content spans into caller-owned storage. No allocation is performed.
pub fn extract<'a>(
    burst: &Burst<'_>,
    storage: &'a mut [u8; MAT_CONTENT_BYTES],
) -> Result<MatPayload<'a>, DecodeError> {
    if burst.data_type != DataType::Mat {
        return Err(DecodeError::WrongRoute("a non-MAT burst in a MAT epoch"));
    }
    let first = first_content(burst.payload)?;
    let rest = rest_content(burst.payload)?;
    let split = first.len();
    storage[..split].copy_from_slice(first);
    storage[split..].copy_from_slice(rest);
    let content = &storage[..];
    if contains_major_sync(content) {
        Ok(MatPayload::TrueHd(content))
    } else {
        Ok(MatPayload::Unknown(content))
    }
}

pub fn contains_major_sync(content: &[u8]) -> bool {
    content
        .windows(FBA_MAJOR_SYNC.len())
        .any(|window| window == FBA_MAJOR_SYNC)
}

fn corrupt(detail: &'static str) -> DecodeError {
    DecodeError::Corrupt {
        codec: "MAT 2.x",
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn valid_payload() -> Vec<u8> {
        let mut payload = vec![0; MAT_PAYLOAD_BYTES];
        payload[..START_MARKER.len()].copy_from_slice(&START_MARKER);
        payload[MIDDLE_MARKER_OFFSET..MIDDLE_MARKER_OFFSET + MIDDLE_MARKER.len()]
            .copy_from_slice(&MIDDLE_MARKER);
        payload[END_MARKER_OFFSET..].copy_from_slice(&END_MARKER);
        payload
    }

    fn burst(payload: &[u8]) -> Burst<'_> {
        Burst {
            data_type: DataType::Mat,
            pc: 22,
            payload,
            start: 0,
        }
    }

    #[test]
    fn ffmpeg_spdif_fixture_has_published_markers_after_deframing() {
        let stream = include_bytes!("../tests/fixtures/truehd/tones-5.1-48k.spdif");
        let mut deframer = bitstream::Deframer::new();
        let mut count = 0;
        deframer.push(stream, &mut |burst| {
            count += 1;
            assert_eq!(burst.data_type, DataType::Mat);
            let mut storage = [0; MAT_CONTENT_BYTES];
            let extracted = extract(&burst, &mut storage).unwrap();
            assert!(matches!(extracted, MatPayload::TrueHd(_)));
        });
        assert_eq!(count, 3);
    }

    #[test]
    fn extracts_both_content_spans_in_codec_byte_order() {
        let mut payload = valid_payload();
        for (i, byte) in payload[START_MARKER.len()..MIDDLE_MARKER_OFFSET]
            .iter_mut()
            .enumerate()
        {
            *byte = (i % 251) as u8;
        }
        for (i, byte) in payload[MIDDLE_MARKER_OFFSET + MIDDLE_MARKER.len()..END_MARKER_OFFSET]
            .iter_mut()
            .enumerate()
        {
            *byte = (255 - i % 251) as u8;
        }
        payload[START_MARKER.len() + 4..START_MARKER.len() + 8].copy_from_slice(&FBA_MAJOR_SYNC);

        let mut storage = [0; MAT_CONTENT_BYTES];
        let extracted = extract(&burst(&payload), &mut storage).unwrap();
        assert!(matches!(extracted, MatPayload::TrueHd(_)));
        assert_eq!(
            &extracted.bytes()[..FIRST_CONTENT.len()],
            &payload[FIRST_CONTENT]
        );
        assert_eq!(
            &extracted.bytes()[FIRST_CONTENT.len()..],
            &payload[SECOND_CONTENT]
        );
    }

    #[test]
    fn rejects_each_invalid_fixed_record() {
        for offset in [0, MIDDLE_MARKER_OFFSET, END_MARKER_OFFSET] {
            let mut payload = valid_payload();
            payload[offset] ^= 1;
            let mut storage = [0; MAT_CONTENT_BYTES];
            assert!(matches!(
                extract(&burst(&payload), &mut storage),
                Err(DecodeError::Corrupt {
                    codec: "MAT 2.x",
                    ..
                })
            ));
        }
    }

    #[test]
    fn content_without_fba_sync_is_unknown() {
        let payload = valid_payload();
        let mut storage = [0; MAT_CONTENT_BYTES];
        assert!(matches!(
            extract(&burst(&payload), &mut storage),
            Ok(MatPayload::Unknown(_))
        ));
    }

    proptest! {
        #[test]
        fn arbitrary_payload_bytes_never_panic(payload in proptest::collection::vec(any::<u8>(), 0..70_000)) {
            let mut storage = [0; MAT_CONTENT_BYTES];
            let _ = extract(&burst(&payload), &mut storage);
        }
    }
}
