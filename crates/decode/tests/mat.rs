#![forbid(unsafe_code)]

use bitstream::{Burst, DataType};
use decode::{DecodeConfig, DecodeError, Decoded, Decoders, DeferredInput, FrontDecoders};
use ssr_types::{BitstreamCodec, BitstreamFormat, SourceFormat};

fn bitstream_format(codec: BitstreamCodec) -> SourceFormat {
    SourceFormat::Bitstream(BitstreamFormat {
        codec,
        container_rate: 192_000,
        container_channels: 8,
        encoded_rate: 48_000,
        encoded_channels: 0,
    })
}

#[test]
fn mat_and_raw_truehd_select_the_deferred_truehd_adapter() {
    for (codec, expected) in [
        (BitstreamCodec::TrueHd, DeferredInput::TrueHdRaw),
        (BitstreamCodec::Mat20, DeferredInput::TrueHdMat),
        (BitstreamCodec::Mat21, DeferredInput::TrueHdMat),
    ] {
        let mut decoders = Decoders::new(DecodeConfig::default());
        assert!(decoders.select(&bitstream_format(codec), 7).is_ok());
        assert_eq!(decoders.deferred(), Some(expected));
        let burst = Burst {
            data_type: DataType::Mat,
            pc: 22,
            payload: &[],
            start: 0,
        };
        assert!(matches!(
            decoders.push_burst(&burst, 0, &mut |_| panic!("no scene")),
            Err(DecodeError::WrongRoute(_))
        ));
    }
}

#[test]
fn mat_without_mlp_is_rejected_on_the_decode_thread() {
    let mut mat = vec![0; decode::mat_frame::MAT_PAYLOAD_BYTES];
    mat[..decode::mat_frame::START_MARKER.len()].copy_from_slice(&decode::mat_frame::START_MARKER);
    let middle = decode::mat_frame::MIDDLE_MARKER_OFFSET;
    mat[middle..middle + decode::mat_frame::MIDDLE_MARKER.len()]
        .copy_from_slice(&decode::mat_frame::MIDDLE_MARKER);
    mat[decode::mat_frame::END_MARKER_OFFSET..].copy_from_slice(&decode::mat_frame::END_MARKER);

    let mut front = FrontDecoders::new();
    let mut decoded = Decoded::new();
    assert_eq!(
        front.decode(DeferredInput::TrueHdMat, &mat, &mut decoded),
        Err(DecodeError::Unsupported {
            codec: "MAT 2.x",
            detail: "PCM inside MAT is not decoded yet",
        })
    );
}
