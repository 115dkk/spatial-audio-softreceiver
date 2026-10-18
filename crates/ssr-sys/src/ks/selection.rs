//! Validated driver-list parsing and connected-component selection, without FFI.
use std::collections::HashSet;

pub(super) fn list_extent(bytes: &[u8], item_size: usize, limit: usize) -> Option<usize> {
    let size = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let count = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    if count > limit
        || size < 8
        || size > bytes.len()
        || count.checked_mul(item_size)?.checked_add(8)? > size
    {
        return None;
    }
    Some(count)
}
pub(super) fn physical(bytes: &[u8]) -> Option<(String, u32)> {
    let size = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let pin = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    if size < 10 || !size.is_multiple_of(2) {
        return None;
    }
    let units: Vec<_> = bytes
        .get(8..size)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect();
    let end = units.iter().position(|&c| c == 0)?;
    if end == 0 {
        return None;
    }
    Some((String::from_utf16(&units[..end]).ok()?, pin))
}
pub(super) fn connected_pins(bytes: Option<&[u8]>, bridge: u32, count: u32) -> Vec<u32> {
    let Some(bytes) = bytes else {
        return vec![bridge];
    };
    let Some(n) = list_extent(bytes, 16, 4096) else {
        return vec![bridge];
    };
    let mut edges = Vec::with_capacity(n);
    for data in bytes[8..8 + n * 16].as_chunks::<16>().0 {
        let fields: [u32; 4] =
            std::array::from_fn(|i| u32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap()));
        let endpoint = |node, pin| {
            if node == u32::MAX {
                (node, pin)
            } else {
                (node, 0)
            }
        };
        edges.push((
            endpoint(fields[0], fields[1]),
            endpoint(fields[2], fields[3]),
        ));
    }
    let mut reached = HashSet::from([(u32::MAX, bridge)]);
    loop {
        let before = reached.len();
        for &(a, b) in &edges {
            if reached.contains(&a) {
                reached.insert(b);
            }
            if reached.contains(&b) {
                reached.insert(a);
            }
        }
        if before == reached.len() {
            break;
        }
    }
    (0..count)
        .filter(|&pin| reached.contains(&(u32::MAX, pin)))
        .collect()
}
pub(super) fn render_pin(flow: i32, communication: i32) -> bool {
    // WDK KSPIN_DATAFLOW_IN=1, KSPIN_COMMUNICATION_SINK=1, BOTH=3.
    flow == 1 && matches!(communication, 1 | 3)
}
pub(super) fn writable(play: u64, write: u64, start: u64, length: u64, ring: u64) -> bool {
    if ring == 0 || play >= ring || write >= ring || start >= ring || length > ring {
        return false;
    }
    let distance = |position| (position + ring - play) % ring;
    let start = distance(start);
    start >= distance(write) && start.checked_add(length).is_some_and(|end| end <= ring)
}
pub(super) fn audio_range(values: [u32; 5], bits: u16, rate: u32, channels: u16) -> bool {
    u32::from(channels) <= values[0]
        && u32::from(bits) >= values[1]
        && u32::from(bits) <= values[2]
        && rate >= values[3]
        && rate <= values[4]
}
pub(super) fn period_frames(period: std::time::Duration, rate: u32) -> Option<usize> {
    let frames = period
        .as_nanos()
        .checked_mul(u128::from(rate))?
        .div_ceil(1_000_000_000);
    (frames > 0 && frames <= u128::from(rate)).then_some(frames as usize)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ks_pin_and_write_region_reject_capture_and_reserved_dma_region() {
        assert!(render_pin(1, 1));
        assert!(render_pin(1, 3));
        assert!(!render_pin(2, 1));
        assert!(!render_pin(1, 4));
        assert!(writable(50, 60, 0, 50, 100));
        assert!(!writable(50, 60, 55, 10, 100));
        assert!(!writable(50, 60, 0, 51, 100));
        assert!(!writable(100, 0, 0, 10, 100));
    }
    #[test]
    fn ks_graph_does_not_select_unrelated_render_branch() {
        let mut bytes = Vec::new();
        for n in [40u32, 2, u32::MAX, 0, 0, 0, 0, 1, u32::MAX, 2] {
            bytes.extend(n.to_le_bytes());
        }
        assert_eq!(connected_pins(Some(&bytes), 0, 4), [0, 2]);
        assert_eq!(connected_pins(Some(&bytes), 1, 4), [1]);
        bytes[4..8].copy_from_slice(&5000u32.to_le_bytes());
        assert_eq!(connected_pins(Some(&bytes), 0, 4), [0]);
    }
    #[test]
    fn ks_physical_path_requires_terminated_bounded_utf16() {
        let mut bytes = vec![12, 0, 0, 0, 3, 0, 0, 0, 65, 0, 0, 0];
        assert_eq!(physical(&bytes), Some(("A".into(), 3)));
        bytes[10] = 66;
        assert_eq!(physical(&bytes), None);
        bytes[0] = 11;
        assert_eq!(physical(&bytes), None);
    }
    #[test]
    fn ks_audio_range_and_period_respect_requested_shape() {
        assert!(audio_range([8, 16, 32, 44100, 96000], 24, 48000, 8));
        assert!(!audio_range([8, 16, 32, 44100, 96000], 24, 48000, 9));
        assert!(!audio_range([8, 16, 32, 44100, 96000], 24, 192000, 2));
        assert_eq!(
            period_frames(std::time::Duration::from_micros(1001), 48000),
            Some(49)
        );
        assert_eq!(period_frames(std::time::Duration::ZERO, 48000), None);
        assert_eq!(period_frames(std::time::Duration::MAX, 48000), None);
    }
}
