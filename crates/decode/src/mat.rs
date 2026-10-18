//! Placeholder for uncompressed PCM carried in MAT 2.x (clean room, ADR 0005).
//!
//! MLP/TrueHD carried in MAT is decoded by `mat_frame` and the TrueHD adapter.
//! The uncompressed PCM layout remains unknown and will be inferred only from
//! ring dumps observed as a black box. Until those dumps exist this adapter
//! refuses that syntax.

use bitstream::Burst;
use ssr_types::SceneBlock;

use crate::{DecodeConfig, DecodeError};

/// Whether uncompressed PCM MAT content is supported.
pub const PCM_AVAILABLE: bool = false;

#[derive(Debug)]
pub struct MatDecoder {
    _config: DecodeConfig,
}

impl MatDecoder {
    pub fn new(config: DecodeConfig) -> Self {
        Self { _config: config }
    }

    pub fn push(
        &mut self,
        _burst: &Burst<'_>,
        _qpc: u64,
        _epoch: u32,
        _out: &mut dyn FnMut(&SceneBlock<'_>),
    ) -> Result<(), DecodeError> {
        Err(DecodeError::Unsupported {
            codec: "MAT 2.x",
            detail: "PCM inside MAT is not decoded yet",
        })
    }

    pub fn reset(&mut self) {}
}
