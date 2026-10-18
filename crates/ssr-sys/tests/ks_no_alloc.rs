#![forbid(unsafe_code)]
#![allow(dead_code)]

#[path = "../src/playback_clock.rs"]
mod playback_clock;
#[path = "../src/ks/sample.rs"]
mod sample;
#[path = "../src/ks/selection.rs"]
mod selection;
#[path = "../src/ks/timing.rs"]
mod timing;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
#[global_allocator]
static ALLOCATOR: AllocDisabler = AllocDisabler;

#[test]
fn ks_conversion_and_position_do_not_allocate() {
    let samples = [0.25; 960];
    let mut bytes = [0; 3840];
    let mut position = timing::RingPosition::default();
    assert_no_alloc(|| {
        for encoding in sample::SampleFormat::ALL {
            encoding.convert(&samples, &mut bytes[..samples.len() * encoding.bytes()]);
        }
        for tick in 0..1000 {
            assert!(position.update(tick % 100, 100, tick, 1000, 1000).is_some());
        }
    });
}
