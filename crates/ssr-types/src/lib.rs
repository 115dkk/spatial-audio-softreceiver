#![forbid(unsafe_code)]

//! Types every other crate shares (CONTEXT.md names each of them).
//!
//! * [`scene`]: the [`SceneBlock`] every decoder writes and the renderer reads.
//! * [`layout`]: the directions of a BRIR set and the [`Layout`] they form.
//! * [`feeds`]: one signal per direction, the renderer's output.
//! * [`format`](mod@format): what a source writes to the virtual device.
//! * [`ports`]: the seams the engine is wired through ([`CaptureSource`],
//!   [`OutputOpener`]/[`OutputSink`]) and the supervisor's [`Host`].
//! * [`status`]: the engine state the control thread samples.
//! * [`app`]: the intent/snapshot pair between the screen and the supervisor.

pub mod app;
pub mod feeds;
pub mod format;
pub mod layout;
pub mod ports;
pub mod position;
pub mod scene;
pub mod scene_layout;
pub mod status;

pub use app::{AsioLayout, AsioSettings};
pub use feeds::Feeds;
pub use format::{BitstreamCodec, BitstreamFormat, PcmFormat, SampleType, SourceFormat};
pub use layout::{Direction, DirectionId, DirectionView, Layout, LayoutError};
pub use ports::{
    AppRoleOutputs, Arch, AudioApp, CaptureError, CaptureSource, DefaultRoles, DeviceId,
    DeviceState, DriverState, EngineThread, Host, HostError, KsFallbackReason, KsTarget,
    OutputDevice, OutputError, OutputFormat, OutputLayout, OutputMode, OutputOpener, OutputSink,
    Platform, PolicyState, Role, SecureBoot, SetupOutcome, SetupTask, TrimUnsupported,
};
pub use position::{Speaker, direction_to_room, room_to_direction};

/// Named channel layout shared by direct inputs and speaker-profile masks.
pub fn channel_layout_preset(name: &str) -> Option<&'static [Speaker]> {
    use Speaker::*;
    Some(match name.to_ascii_lowercase().as_str() {
        "mono" => &[C],
        "stereo" | "2.0" => &[L, R],
        "3.0" => &[L, R, C],
        "quad" => &[L, R, Lrs, Rrs],
        "5.0" => &[L, R, C, Lrs, Rrs],
        "5.1" => &[L, R, C, Lfe, Lrs, Rrs],
        "7.1" => &[L, R, C, Lfe, Lrs, Rrs, Lss, Rss],
        "5.1.2" => &[L, R, C, Lfe, Lrs, Rrs, Tfl, Tfr],
        "5.1.4" => &[L, R, C, Lfe, Lrs, Rrs, Tfl, Tfr, Trl, Trr],
        "7.1.2" => &[L, R, C, Lfe, Lrs, Rrs, Lss, Rss, Tfl, Tfr],
        "7.1.4" => &[L, R, C, Lfe, Lrs, Rrs, Lss, Rss, Tfl, Tfr, Trl, Trr],
        "9.1" => &[L, R, C, Lfe, Lrs, Rrs, Lss, Rss, Lw, Rw],
        "9.1.4" => &[L, R, C, Lfe, Lrs, Rrs, Lss, Rss, Lw, Rw, Tfl, Tfr, Trl, Trr],
        "9.1.6" => &[
            L, R, C, Lfe, Lrs, Rrs, Lss, Rss, Lw, Rw, Tfl, Tfr, Tsl, Tsr, Trl, Trr,
        ],
        "13.1" => &[
            L, R, C, Lfe, Lrs, Rrs, Lss, Rss, Tc, Tfl, Tfc, Tfr, Trl, Trr,
        ],
        "22.2" => &[
            Lw, Rw, C, Lfe, Lbs, Rbs, L, R, Cs, Lfe2, Lss, Rss, Tfl, Tfr, Tfc, Tc, Trl, Trr, Tsl,
            Tsr, Trc, Bfc, Bfl, Bfr,
        ],
        "24.1.10" => &[
            L, R, C, Lfe, Lss, Rss, Lbs, Rbs, Lw, Rw, Tfl, Tfr, Tsl, Tsr, Trl, Trr, Lc, Rc, Lsc,
            Rsc, Ls1, Rs1, Ls, Rs, Lrs1, Rrs1, Lrs, Rrs, Lcs, Rcs, Cs, Hfl, Hfr, Hrl, Hrr,
        ],
        "30.2" => &[
            C, L, R, Ls, Rs, Lfe, Cs, Lrs, Rrs, Lss, Rss, Lc, Rc, Hfl, Tfc, Hfr, Lfe2, Lw, Rw, Tc,
            Tsl, Tsr, Trc, Hrl, Hrr, Bfc, Bfl, Bfr, Tfl, Tfr, Trl, Trr,
        ],
        _ => return None,
    })
}
pub use scene::{MAX_OBJECTS, Object, Position, SceneBlock};
pub use scene_layout::{
    ObjectRole, SceneBedChannel, SceneDynamicObject, SceneLayout, SceneLayoutTable,
    SceneLogEndpoint, SceneLogLine, SceneLogObject, SceneSource,
};
pub use status::{
    AsioStatus, ChainErrorCounts, ChainErrorKind, ClockStatus, ENGINE_ENDPOINTS, EndpointStatus,
    EngineFault, EngineStatus, LatencyStats, SceneObject, UnderrunSnapshot, UnsupportedReason,
};

/// The fixed scene sample rate every decoder produces.
pub const SCENE_RATE: u32 = 48_000;
