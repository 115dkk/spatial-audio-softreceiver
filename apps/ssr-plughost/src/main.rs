#![forbid(unsafe_code)]

//! The plugin host process (ADR 0021): vst-audio's worker, built and shipped
//! under this name beside the app (ADR 0039).

fn main() {
    vst_audio_worker_runtime::host::main();
}
