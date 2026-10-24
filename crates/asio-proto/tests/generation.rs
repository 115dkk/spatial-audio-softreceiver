#![forbid(unsafe_code)]
//! Fake transports exercise the protocol ordering without SDK/Windows clocks.
use asio_proto::{ControlBuf, Layout, next_sequence};
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use streamring::{
    Consumer, Geometry, InProcessEvent, Producer, Publish, Region, RegionBuf, Waited,
    sequence_at_least,
};

fn geometry() -> Geometry {
    Geometry {
        lanes: 1,
        input_channels: 2,
        output_channels: 2,
        max_frames: 64,
        sample_rate: 48_000,
        latency_bound_frames: 128,
    }
}

#[test]
fn preparation_ack_does_not_require_start_or_a_callback() {
    let memory = ControlBuf::new();
    let c = memory.control();
    c.initialize(1).unwrap();
    c.set_dac_generation(1);
    c.set_layout(Layout::Stereo);
    c.set_demand(128);
    let ring = RegionBuf::new(geometry()).unwrap();
    let event = InProcessEvent::new();
    let mut producer = Producer::new(ring.region(), [None, None], &event);
    producer.announce(geometry(), 2).unwrap();
    c.set_generation_barrier_sequence(0);
    c.set_loaded_frames(0);
    c.set_client_generation_ack(1);
    c.set_buffer_frames(64);
    assert!(!c.client_running());
    assert_eq!(c.client_generation_ack(), 1);
    let mut consumer = Consumer::new(ring.region(), &event, [None, None]);
    consumer.validate_and_ready(1).unwrap();
    c.set_completed_frames(0);
    c.set_ready_for_generation(1);
    assert_eq!(producer.wait_ready(Duration::ZERO), Waited::Signalled);
    assert!(c.ready());
    // Preparation time cannot turn into catch-up demand.
    assert_eq!(c.demand(), 128);
}

#[test]
fn late_old_callback_publish_is_covered_by_quiescent_ack() {
    let memory = ControlBuf::new();
    let c = memory.control();
    c.initialize(1).unwrap();
    c.set_dac_generation(10);
    c.set_client_generation_ack(10);
    c.set_ready_for_generation(10);
    let ring = RegionBuf::new(geometry()).unwrap();
    let event = InProcessEvent::new();
    let mut producer = Producer::new(ring.region(), [None, None], &event);
    producer.announce(geometry(), 2).unwrap();
    let mut consumer = Consumer::new(ring.region(), &event, [None, None]);
    consumer.validate_and_ready(1).unwrap();
    assert_eq!(producer.try_publish(0, 1, |_| {}), Publish::Published);
    c.set_loaded_frames(64);
    // The callback already checked generation 10. Output starts epoch 11 before
    // that callback's final publication; the check cannot prevent this race.
    c.set_ready(false);
    c.set_demand(128);
    c.set_dac_generation(11);
    assert_eq!(producer.try_publish(0, 2, |_| {}), Publish::Published);
    c.set_loaded_frames(128);
    assert_ne!(c.client_generation_ack(), 11);
    // ONLY after returning from that callback can the DLL publish this ACK.
    c.set_generation_barrier_sequence(2);
    c.set_loaded_frames(0);
    c.set_client_generation_ack(11);
    assert_eq!(c.client_generation_ack(), 11);
    let barrier = c.generation_barrier_sequence();
    let mut discarded = 0;
    for _ in 0..2 {
        if let Some(sequence) = consumer.next_published(0) {
            assert!(sequence_at_least(barrier, sequence));
            consumer.complete(0, sequence);
            discarded += 1;
        }
    }
    assert_eq!(discarded, 2);
    assert!(consumer.next_published(0).is_none());
    // DSP/queue reset is now safe; counters are not compared during transition.
    c.set_completed_frames(0);
    c.set_ready_for_generation(11);
    let next = next_sequence(barrier);
    assert_eq!(next, 3);
    assert_eq!(producer.try_publish(0, next, |_| {}), Publish::Published);
    assert_eq!(consumer.next_published(0), Some(3));
}

#[test]
fn late_ready_store_cannot_enable_another_epoch() {
    let memory = ControlBuf::new();
    let c = memory.control();
    c.initialize(1).unwrap();
    c.set_dac_generation(5);
    c.set_ready_for_generation(5);
    assert!(c.ready());
    // Consumer checked generation 5; output advances before its ready store.
    c.set_ready(false);
    c.set_dac_generation(6);
    c.set_ready_for_generation(5);
    assert!(!c.ready());
    c.set_ready_for_generation(6);
    assert!(c.ready());
}

#[test]
fn run_serial_detects_stop_start_between_output_polls() {
    let memory = ControlBuf::new();
    let c = memory.control();
    c.initialize(1).unwrap();
    c.set_client_run_serial(1);
    c.set_client_running(true);
    let observed = c.client_run_serial();
    c.set_client_running(false);
    c.set_client_run_serial(observed + 1);
    c.set_client_running(true);
    assert!(c.client_running());
    assert_ne!(c.client_run_serial(), observed);
    assert_eq!(c.snapshot().unwrap().client_run_serial, 2);
}

#[test]
fn asio_local_suffix_never_uses_shared_samples_as_u64() {
    // Model BOTH platform adapters: even a falsely smaller announced geometry
    // exposes shared words only as u32 and supplies its own local u64 suffix.
    let larger = Geometry {
        max_frames: 128,
        ..geometry()
    };
    let prefix: Vec<_> = (0..(larger.total_bytes().unwrap() - 128) / 4)
        .map(|_| std::sync::atomic::AtomicU32::new(0))
        .collect();
    let producer_counters = std::array::from_fn::<_, 16, _>(|_| AtomicU64::new(0));
    let consumer_counters = std::array::from_fn::<_, 16, _>(|_| AtomicU64::new(0));
    let producer_region = Region::new(&prefix, &producer_counters);
    let consumer_region = Region::new(
        &prefix[..(geometry().total_bytes().unwrap() - 128) / 4],
        &consumer_counters,
    );
    let event = InProcessEvent::new();
    let mut producer = Producer::new(producer_region, [None, None], &event);
    producer.announce(geometry(), 2).unwrap();
    let mut consumer = Consumer::new(consumer_region, &event, [None, None]);
    consumer.validate_and_ready(1).unwrap();
    assert_eq!(consumer.beat(), 1);
    assert_eq!(consumer_region.heartbeat(), 1);
    assert_eq!(producer_region.heartbeat(), 0);
    assert_eq!(larger.total_bytes().unwrap(), 2688);
    assert_eq!(geometry().total_bytes().unwrap(), 1664);
}
