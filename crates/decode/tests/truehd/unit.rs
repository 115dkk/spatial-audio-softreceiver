use super::*;
use truehd::structs::oamd::{
    BlockUpdateInfo, MDUpdateInfo, ObjectElement, ObjectInfoBlock, ProgramAssignment,
};

fn object_pcm() -> DecodedAccessUnit {
    let mut first = ObjectInfoBlock::default();
    first.object_basic_info.object_gain = 0;
    first.object_render_info.pos3d = [0.0, 0.0, 0.0];
    let mut second = first.clone();
    second.object_render_info.pos3d = [1.0, 1.0, 1.0];
    second.object_render_info.b_differential_position_specified = true;
    DecodedAccessUnit {
        sample_length: 160,
        sampling_frequency: 48_000,
        channel_count: 1,
        pcm_data: [[4_194_304; CHANNELS]; 160],
        oamd: vec![ObjectAudioMetadataPayload {
            object_count: 1,
            program_assignment: ProgramAssignment {
                num_dynamic_objects: 1,
                ..Default::default()
            },
            object_element: Some(ObjectElement {
                md_update_info: MDUpdateInfo {
                    sample_offset: 8,
                    num_obj_info_blocks: 2,
                    block_update_info: vec![
                        BlockUpdateInfo::default(),
                        BlockUpdateInfo {
                            block_offset_factor_bits: 1,
                            ramp_duration: 512,
                            ..Default::default()
                        },
                    ],
                },
                object_data: vec![vec![first, second]],
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn group(pcm: DecodedAccessUnit) -> DecodedGroup {
    let shape = Shape::from_pcm(&pcm).unwrap();
    let mut events = Vec::new();
    for payload in &pcm.oamd {
        translate_oamd(payload, &shape, &mut events).unwrap();
    }
    DecodedGroup { pcm, shape, events }
}

#[test]
fn oamd_offsets_resolved_deltas_ramps_and_raw_layout_cross_access_units() {
    let decoded = TrueHdDecoded {
        groups: vec![group(object_pcm())],
        error: None,
    };
    let mut back = TrueHdBack::new(DecodeConfig::default());
    let mut cuts = Vec::new();
    back.place(&decoded, 5, 1, &mut |scene| {
        cuts.push(scene.frames);
        assert!(scene.objects[0].signal.iter().all(|&s| s == 0.5));
        if cuts.len() == 1 {
            assert_eq!(scene.objects[0].spread, 1.0);
        }
        if cuts.len() == 2 {
            assert_eq!(
                scene.objects[0].position,
                ssr_types::room_to_direction(-1.0, 1.0, 0.0)
            );
        }
    })
    .unwrap();
    assert_eq!(cuts, [8, 32, 120]);
    assert_eq!(back.layout().source, SceneSource::TrueHdObjects);
    assert_eq!(back.layout().room[0], Some([1.0, 1.0, 1.0]));
    assert_eq!(back.layout().dynamic_count(), 1);
    let mut continuation = object_pcm();
    continuation.oamd.clear();
    let continuation = TrueHdDecoded {
        groups: vec![group(continuation)],
        error: None,
    };
    back.place(&continuation, 6, 1, &mut |scene| {
        // At sample 160, 120 of the 512-sample ramp has elapsed.
        assert_eq!(
            scene.objects[0].position,
            ssr_types::room_to_direction(-0.53125, 0.53125, 0.234375)
        );
    })
    .unwrap();
    assert_eq!(back.layout().room[0], Some([1.0, 1.0, 1.0]));
}

#[test]
fn object_placement_and_failure_paths_do_not_allocate() {
    let decoded = TrueHdDecoded {
        groups: vec![group(object_pcm())],
        error: None,
    };
    let mut back = TrueHdBack::new(DecodeConfig::default());
    let mut place = || {
        back.place(&decoded, 0, 1, &mut |_| {}).unwrap();
        back.reset();
        back.timeline.discontinuity(0).unwrap();
    };
    #[cfg(debug_assertions)]
    assert_no_alloc::assert_no_alloc(&mut place);
    #[cfg(not(debug_assertions))]
    place();
    let bad = TrueHdDecoded {
        groups: vec![],
        error: Some(corrupt()),
    };
    let mut reject = || {
        assert_eq!(back.place(&bad, 0, 1, &mut |_| {}), Err(corrupt()));
    };
    #[cfg(debug_assertions)]
    assert_no_alloc::assert_no_alloc(&mut reject);
    #[cfg(not(debug_assertions))]
    reject();
}

#[test]
fn two_lfe_channels_sum_and_surround_direct_is_explicitly_unsupported() {
    let pcm = DecodedAccessUnit {
        channel_count: 2,
        channel_labels: vec![ChannelLabel::LFE, ChannelLabel::LFE2],
        sample_length: 40,
        sampling_frequency: 48_000,
        pcm_data: [[2_097_152; CHANNELS]; 160],
        ..Default::default()
    };
    let decoded = TrueHdDecoded {
        groups: vec![group(pcm)],
        error: None,
    };
    let mut back = TrueHdBack::new(DecodeConfig::default());
    back.place(&decoded, 0, 0, &mut |scene| {
        assert!(scene.objects.is_empty());
        assert_eq!(scene.lfe.unwrap(), &[0.5; 40]);
    })
    .unwrap();
    assert!(matches!(
        channel_speaker(ChannelLabel::Lsd),
        Err(DecodeError::Unsupported { .. })
    ));
    assert_eq!(channel_speaker(ChannelLabel::Lsc).unwrap(), Speaker::Lc);
    assert_eq!(channel_speaker(ChannelLabel::Tbl).unwrap(), Speaker::Trl);
}

#[test]
fn extraction_and_parse_errors_reset_and_recover_at_major_sync() {
    let bytes = include_bytes!("../fixtures/truehd/tones-5.1-48k.thd");
    let mut front = TrueHdFront::new();
    let mut decoded = TrueHdDecoded::new();
    let mut broken = bytes.to_vec();
    broken[8] ^= 0x40;
    assert!(front.decode(&broken, &mut decoded).is_err());
    assert!(decoded.error().is_some());
    front.decode(bytes, &mut decoded).unwrap();
    assert_eq!(decoded.frames(), 3840);
    assert_eq!(decoded.error(), None);
    let mut broken = bytes.to_vec();
    // Major-sync CRC remains valid; corrupt the compressed first frame instead.
    broken[40] ^= 0x80;
    assert!(front.decode(&broken, &mut decoded).is_err());
    front.decode(bytes, &mut decoded).unwrap();
    assert_eq!(decoded.frames(), 3840);
}

#[test]
fn frames_at_mix_rate_rounds_runs_not_each_access_unit() {
    let mut pcm = object_pcm();
    pcm.oamd.clear();
    pcm.sampling_frequency = 44_100;
    pcm.sample_length = 40;
    let decoded = TrueHdDecoded {
        groups: vec![group(pcm.clone()), group(pcm)],
        error: None,
    };
    assert_eq!(decoded.frames_at_mix_rate(), 88);
}
