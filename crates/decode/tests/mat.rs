#![forbid(unsafe_code)]

use bitstream::{Burst, DataType};
use decode::{DecodeConfig, DecodeError, Decoded, Decoders, DeferredInput, FrontDecoders, MatPart};
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
        (
            BitstreamCodec::Mat20,
            DeferredInput::TrueHdMat(MatPart::Whole),
        ),
        (
            BitstreamCodec::Mat21,
            DeferredInput::TrueHdMat(MatPart::Whole),
        ),
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

fn first_mat_payload() -> Vec<u8> {
    let stream = include_bytes!("fixtures/truehd/tones-5.1-48k.spdif");
    let mut deframer = bitstream::Deframer::new();
    let mut payload = Vec::new();
    deframer.push(&stream[..61_440], &mut |burst| {
        payload.extend_from_slice(burst.payload)
    });
    payload
}

fn place_samples(
    back: &mut Decoders,
    decoded: &Decoded,
    qpc: u64,
    samples: &mut Vec<Vec<i32>>,
    qpcs: &mut Vec<u64>,
) {
    back.place_deferred(decoded, qpc, 1, &mut |scene| {
        for frame in 0..scene.frames {
            qpcs.push(scene.source_qpc);
            let mut row = scene
                .objects
                .iter()
                .map(|object| (object.signal[frame] * 8_388_608.0) as i32)
                .collect::<Vec<_>>();
            if let Some(lfe) = scene.lfe {
                row.push((lfe[frame] * 8_388_608.0) as i32);
            }
            samples.push(row);
        }
    })
    .unwrap();
}

#[test]
fn mat_first_and_rest_match_whole_burst_pcm_and_keep_delivery_qpcs() {
    let payload = first_mat_payload();
    let first = decode::mat_frame::first_content(&payload).unwrap();
    let rest = decode::mat_frame::rest_content(&payload).unwrap();

    let mut whole_front = FrontDecoders::new();
    let mut whole_decoded = Decoded::new();
    whole_front
        .decode(
            DeferredInput::TrueHdMat(MatPart::Whole),
            &payload,
            &mut whole_decoded,
        )
        .unwrap();
    let mut whole_back = Decoders::new(DecodeConfig::default());
    whole_back
        .select(&bitstream_format(BitstreamCodec::Mat21), 1)
        .unwrap();
    let mut expected = Vec::new();
    let mut whole_qpcs = Vec::new();
    place_samples(
        &mut whole_back,
        &whole_decoded,
        99,
        &mut expected,
        &mut whole_qpcs,
    );

    let mut split_front = FrontDecoders::new();
    let mut split_decoded = Decoded::new();
    let mut split_back = Decoders::new(DecodeConfig::default());
    split_back
        .select(&bitstream_format(BitstreamCodec::Mat21), 1)
        .unwrap();
    let mut actual = Vec::new();
    let mut split_qpcs = Vec::new();
    split_front
        .decode(
            DeferredInput::TrueHdMat(MatPart::First),
            first,
            &mut split_decoded,
        )
        .unwrap();
    assert!(split_decoded.frames() > 0);
    place_samples(
        &mut split_back,
        &split_decoded,
        11,
        &mut actual,
        &mut split_qpcs,
    );
    let first_frames = actual.len();
    split_front
        .decode(
            DeferredInput::TrueHdMat(MatPart::Rest),
            rest,
            &mut split_decoded,
        )
        .unwrap();
    place_samples(
        &mut split_back,
        &split_decoded,
        22,
        &mut actual,
        &mut split_qpcs,
    );

    assert_eq!(actual, expected);
    assert!(first_frames > 0 && first_frames < actual.len());
    assert!(split_qpcs[..first_frames].iter().all(|qpc| *qpc == 11));
    assert!(split_qpcs[first_frames..].iter().all(|qpc| *qpc == 22));
    assert!(whole_qpcs.iter().all(|qpc| *qpc == 99));
}

#[test]
fn access_unit_straddling_mat_reference_decodes_once_after_rest() {
    let payload = first_mat_payload();
    let mut storage = [0; decode::mat_frame::MAT_CONTENT_BYTES];
    let burst = Burst {
        data_type: DataType::Mat,
        pc: 22,
        payload: &payload,
        start: 0,
    };
    let extracted = decode::mat_frame::extract(&burst, &mut storage).unwrap();
    let content = extracted.bytes();
    // The first access unit is 516 bytes. This split leaves it incomplete in
    // the first job, as an AU crossing R-MAT would be.
    let split = 100;
    let mut front = FrontDecoders::new();
    let mut decoded = Decoded::new();
    let mut back = Decoders::new(DecodeConfig::default());
    back.select(&bitstream_format(BitstreamCodec::Mat21), 1)
        .unwrap();
    let mut actual = Vec::new();
    let mut qpcs = Vec::new();
    front
        .decode(
            DeferredInput::TrueHdMat(MatPart::First),
            &content[..split],
            &mut decoded,
        )
        .unwrap();
    assert_eq!(decoded.frames(), 0);
    place_samples(&mut back, &decoded, 1, &mut actual, &mut qpcs);
    front
        .decode(
            DeferredInput::TrueHdMat(MatPart::Rest),
            &content[split..],
            &mut decoded,
        )
        .unwrap();
    place_samples(&mut back, &decoded, 2, &mut actual, &mut qpcs);

    let mut whole_front = FrontDecoders::new();
    let mut whole_decoded = Decoded::new();
    whole_front
        .decode(
            DeferredInput::TrueHdMat(MatPart::Whole),
            &payload,
            &mut whole_decoded,
        )
        .unwrap();
    let mut whole_back = Decoders::new(DecodeConfig::default());
    whole_back
        .select(&bitstream_format(BitstreamCodec::Mat21), 1)
        .unwrap();
    let mut expected = Vec::new();
    let mut expected_qpcs = Vec::new();
    place_samples(
        &mut whole_back,
        &whole_decoded,
        3,
        &mut expected,
        &mut expected_qpcs,
    );
    assert_eq!(actual, expected);
    assert!(qpcs.iter().all(|qpc| *qpc == 2));
}

#[test]
fn mat_without_mlp_is_rejected_on_the_decode_thread() {
    let mut mat = vec![0; decode::mat_frame::MAT_PAYLOAD_BYTES];
    mat[..decode::mat_frame::START_MARKER.len()].copy_from_slice(&decode::mat_frame::START_MARKER);
    let middle = decode::mat_frame::MIDDLE_MARKER_OFFSET;
    mat[middle..middle + decode::mat_frame::MIDDLE_MARKER.len()]
        .copy_from_slice(&decode::mat_frame::MIDDLE_MARKER);
    mat[decode::mat_frame::END_MARKER_OFFSET..].copy_from_slice(&decode::mat_frame::END_MARKER);

    let pcm_inside = Err(DecodeError::Unsupported {
        codec: "MAT 2.x",
        detail: "PCM inside MAT is not decoded yet",
    });
    let mut front = FrontDecoders::new();
    let mut decoded = Decoded::new();
    assert_eq!(
        front.decode(DeferredInput::TrueHdMat(MatPart::Whole), &mat, &mut decoded),
        pcm_inside
    );

    // Split at R-MAT: the first part decodes nothing, the rest says why.
    let mut front = FrontDecoders::new();
    let first = decode::mat_frame::first_content(&mat).unwrap();
    let rest = decode::mat_frame::rest_content(&mat).unwrap();
    assert_eq!(
        front.decode(
            DeferredInput::TrueHdMat(MatPart::First),
            first,
            &mut decoded
        ),
        Ok(())
    );
    assert_eq!(decoded.frames(), 0);
    assert_eq!(
        front.decode(DeferredInput::TrueHdMat(MatPart::Rest), rest, &mut decoded),
        pcm_inside
    );
}
