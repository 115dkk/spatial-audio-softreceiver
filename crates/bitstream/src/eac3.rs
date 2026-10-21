//! Strict syncframe boundaries for the fixed 48 kHz / 5.1 / 32 ms test sender.
//! This is not a decoder: unsupported substreams and shorter blocks must not
//! silently become one 32 ms IEC burst each.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    Empty,
    Truncated,
    Syncword,
    StreamType,
    Substream,
    FrameSize,
    SampleRate,
    Blocks,
    Channels,
    Bsid,
}

/// Validates every frame, including the last boundary, before returning slices.
/// Only independent substream zero, six blocks, 48 kHz and 5.1 are supported.
pub fn frames_48k_51(bytes: &[u8]) -> Result<Vec<&[u8]>, FrameError> {
    if bytes.is_empty() {
        return Err(FrameError::Empty);
    }
    let mut remaining = bytes;
    let mut frames = Vec::new();
    while !remaining.is_empty() {
        if remaining.len() < 7 {
            return Err(FrameError::Truncated);
        }
        if remaining[..2] != [0x0b, 0x77] {
            return Err(FrameError::Syncword);
        }
        if remaining[2] >> 6 != 0 {
            return Err(FrameError::StreamType);
        }
        if (remaining[2] >> 3) & 7 != 0 {
            return Err(FrameError::Substream);
        }
        let size = (usize::from(u16::from_be_bytes([remaining[2], remaining[3]]) & 0x7ff) + 1) * 2;
        if size < 8 {
            return Err(FrameError::FrameSize);
        }
        if size > remaining.len() {
            return Err(FrameError::Truncated);
        }
        if remaining[4] >> 6 != 0 {
            return Err(FrameError::SampleRate);
        }
        if (remaining[4] >> 4) & 3 != 3 {
            return Err(FrameError::Blocks);
        }
        if (remaining[4] >> 1) & 7 != 7 || remaining[4] & 1 == 0 {
            return Err(FrameError::Channels);
        }
        if !(11..=16).contains(&(remaining[5] >> 3)) {
            return Err(FrameError::Bsid);
        }
        frames.push(&remaining[..size]);
        remaining = &remaining[size..];
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> [u8; 16] {
        [
            0x0b,
            0x77,
            0,
            7,
            0x3f,
            16 << 3,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ]
    }

    #[test]
    fn complete_frames_and_no_trailing_garbage() {
        let one = frame();
        let bytes = [one, one].concat();
        assert_eq!(frames_48k_51(&bytes).unwrap(), vec![&one[..], &one[..]]);
        for end in 1..one.len() {
            assert!(frames_48k_51(&one[..end]).is_err());
        }
        assert_eq!(frames_48k_51(&[]), Err(FrameError::Empty));
        let mut bytes = bytes;
        bytes.push(0);
        assert_eq!(frames_48k_51(&bytes), Err(FrameError::Truncated));
    }

    #[test]
    fn rejects_mistimed_and_unsupported_headers() {
        for (index, value, error) in [
            (0, 0, FrameError::Syncword),
            (2, 0x40, FrameError::StreamType),
            (2, 0x80, FrameError::StreamType),
            (2, 8, FrameError::Substream),
            (3, 0, FrameError::FrameSize),
            (3, 8, FrameError::Truncated),
            (4, 0x7f, FrameError::SampleRate),
            (4, 0xff, FrameError::SampleRate),
            (4, 0x0f, FrameError::Blocks),
            (4, 0x1f, FrameError::Blocks),
            (4, 0x2f, FrameError::Blocks),
            (4, 0x3e, FrameError::Channels),
            (4, 0x35, FrameError::Channels),
            (5, 8 << 3, FrameError::Bsid),
            (5, 17 << 3, FrameError::Bsid),
        ] {
            let mut bytes = frame();
            bytes[index] = value;
            assert_eq!(frames_48k_51(&bytes), Err(error));
        }
    }
}
