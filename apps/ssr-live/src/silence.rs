//! A paced, never-ending capture clock for ASIO without the kernel driver.

use std::time::Duration;

use ssr_types::{CaptureError, CaptureSource, TrimUnsupported};

pub struct SilenceCapture {
    now: fn() -> u64,
}

impl SilenceCapture {
    pub fn new(now: fn() -> u64) -> Self {
        Self { now }
    }
}

fn read_wait(wait: Duration) -> Duration {
    wait.min(Duration::from_millis(10))
}

impl CaptureSource for SilenceCapture {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> Result<usize, CaptureError> {
        // No records means the engine advances its existing idle clock. Never
        // report EOF: the independent ASIO entrance may be active indefinitely.
        std::thread::sleep(read_wait(wait));
        Ok(0)
    }

    fn now_qpc(&self) -> Option<u64> {
        Some((self.now)())
    }

    fn set_rate_trim(&mut self, _ppm: f64) -> Result<(), TrimUnsupported> {
        Err(TrimUnsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_wait_is_bounded_by_request_and_capture_tick() {
        for millis in [0, 1, 5, 10, 100, 1000] {
            let wait = Duration::from_millis(millis);
            assert!(read_wait(wait) <= wait);
            assert!(read_wait(wait) <= Duration::from_millis(10));
            if millis > 0 {
                assert!(!read_wait(wait).is_zero());
            }
        }
    }

    #[test]
    fn silence_keeps_the_engine_running_without_a_driver() {
        use ssr_types::{OutputError, OutputFormat, OutputLayout, OutputOpener, OutputSink};
        use std::sync::OnceLock;
        use std::time::Instant;

        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        fn now() -> u64 {
            ORIGIN.get_or_init(Instant::now).elapsed().as_micros() as u64
        }
        struct PacedOutput;
        struct PacedSink(engine::adapters::NullSink);
        impl OutputOpener for PacedOutput {
            type Sink = PacedSink;
            fn open(&mut self, layout: OutputLayout) -> Result<Self::Sink, OutputError> {
                engine::adapters::NullOpener {
                    rate: 48_000,
                    period_frames: 480,
                }
                .open(layout)
                .map(PacedSink)
            }
        }
        impl OutputSink for PacedSink {
            fn format(&self) -> OutputFormat {
                self.0.format()
            }
            fn next_period(
                &mut self,
                fill: &mut dyn FnMut(&mut [f32]),
            ) -> Result<u64, OutputError> {
                std::thread::sleep(Duration::from_millis(10));
                self.0.next_period(fill)?;
                Ok(now())
            }
        }
        let engine = engine::start_with_clock(
            engine::EngineConfig::default(),
            SilenceCapture::new(now),
            PacedOutput,
            1_000_000,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let status = engine.status();
            if status.capture_reads >= 3 && status.output_periods >= 3 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "silence engine stopped advancing"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let before = engine.status();
        std::thread::sleep(Duration::from_millis(50));
        let after = engine.status();
        assert!(after.capture_reads > before.capture_reads);
        assert!(after.output_periods > before.output_periods);
        // A busy-spinning source would run thousands of reads in this interval.
        assert!(after.capture_reads - before.capture_reads < 100);
        engine.stop();
    }

    #[test]
    fn silence_capture_is_paced_and_does_not_end_or_touch_records() {
        let mut capture = SilenceCapture::new(|| 42);
        let mut buffer = [7; 16];
        let started = std::time::Instant::now();
        for _ in 0..3 {
            assert_eq!(capture.read(&mut buffer, Duration::from_millis(1)), Ok(0));
        }
        assert!(started.elapsed() >= Duration::from_millis(3));
        assert_eq!(buffer, [7; 16]);
        assert_eq!(capture.now_qpc(), Some(42));
        assert_eq!(capture.set_rate_trim(0.0), Err(TrimUnsupported));
    }
}
