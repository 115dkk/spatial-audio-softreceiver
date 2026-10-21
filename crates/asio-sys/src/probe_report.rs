// SPDX-License-Identifier: GPL-3.0-or-later
//! OS-independent acceptance criteria for the actual DLL probe.
#[derive(Debug)]
pub struct Report {
    pub call_order: Vec<&'static str>,
    pub callbacks: u64,
    pub output_ready_calls: u64,
    pub timeinfo_callbacks: u64,
    pub reset_requests: u64,
    pub input_latency: i32,
    pub output_latency: i32,
    pub clock_fault_observed: bool,
    pub loaded_frames: Option<u64>,
    pub completed_frames: Option<u64>,
    /// What the run asked for and was given, so a checker can tell how many
    /// callbacks the clock owed: seconds × sample_rate ÷ buffer_frames.
    pub seconds: u32,
    pub sample_rate: f64,
    pub buffer_frames: u32,
    pub output_channels: u32,
}
impl Report {
    pub fn accepted(&self) -> bool {
        self.call_order
            == [
                "DllGetClassObject",
                "CreateInstance",
                "init",
                "getChannels",
                "getBufferSize",
                "getSampleRate",
                "canSampleRate",
                "createBuffers",
                "getLatencies",
                "start",
                "stop",
                "disposeBuffers",
            ]
            && self.callbacks > 0
            && self.output_ready_calls == self.callbacks
            && self.timeinfo_callbacks == self.callbacks
            && self.input_latency == 0
            && self.output_latency >= 0
            && self
                .loaded_frames
                .zip(self.completed_frames)
                .is_none_or(|(loaded, completed)| completed <= loaded)
    }
    pub fn json(&self) -> String {
        format!(
            "{{\"accepted\":{},\"call_order\":[{}],\"callbacks\":{},\"output_ready_calls\":{},\"timeinfo_callbacks\":{},\"reset_requests\":{},\"input_latency\":{},\"output_latency\":{},\"clock_fault_observed\":{},\"loaded_frames\":{},\"completed_frames\":{},\"seconds\":{},\"sample_rate\":{},\"buffer_frames\":{},\"output_channels\":{}}}",
            self.accepted(),
            self.call_order
                .iter()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(","),
            self.callbacks,
            self.output_ready_calls,
            self.timeinfo_callbacks,
            self.reset_requests,
            self.input_latency,
            self.output_latency,
            self.clock_fault_observed,
            self.loaded_frames
                .map_or_else(|| "null".into(), |v| v.to_string()),
            self.completed_frames
                .map_or_else(|| "null".into(), |v| v.to_string()),
            self.seconds,
            if self.sample_rate.is_finite() {
                self.sample_rate
            } else {
                0.0
            },
            self.buffer_frames,
            self.output_channels
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_rejects_missing_callbacks_bad_order_and_counter_mismatch() {
        let mut report = Report {
            call_order: vec![
                "DllGetClassObject",
                "CreateInstance",
                "init",
                "getChannels",
                "getBufferSize",
                "getSampleRate",
                "canSampleRate",
                "createBuffers",
                "getLatencies",
                "start",
                "stop",
                "disposeBuffers",
            ],
            callbacks: 10,
            output_ready_calls: 10,
            timeinfo_callbacks: 10,
            reset_requests: 0,
            input_latency: 0,
            output_latency: 128,
            clock_fault_observed: false,
            loaded_frames: Some(640),
            completed_frames: Some(512),
            seconds: 1,
            sample_rate: 48_000.0,
            buffer_frames: 512,
            output_channels: 2,
        };
        assert!(report.accepted());
        report.callbacks = 0;
        assert!(!report.accepted());
        report.callbacks = 10;
        report.output_ready_calls = 11;
        assert!(!report.accepted());
        report.output_ready_calls = 10;
        report.completed_frames = Some(641);
        assert!(!report.accepted());
        report.completed_frames = Some(640);
        report.call_order.swap(9, 10);
        assert!(!report.accepted());
    }
}
