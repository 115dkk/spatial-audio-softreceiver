//! TrueHD adapter fixtures and deferred placement checks.
#![forbid(unsafe_code)]

use decode::DecodeConfig;

#[cfg(debug_assertions)]
#[global_allocator]
static ALLOCATOR: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

use decode::truehd::{TrueHdBack, TrueHdDecoded, TrueHdDecoder, TrueHdFront};
use ssr_types::{SceneSource, Speaker};

const SURROUND: &[u8] = include_bytes!("fixtures/truehd/tones-5.1-48k.thd");
const SURROUND_SOURCE: &[u8] = include_bytes!("fixtures/truehd/tones-5.1-48k.pcm24le");
const STEREO: &[u8] = include_bytes!("fixtures/truehd/tones-stereo-96k.thd");
const STEREO_SOURCE: &[u8] = include_bytes!("fixtures/truehd/tones-stereo-96k.pcm24le");

fn pcm24_sample(bytes: &[u8], sample: usize) -> i32 {
    let offset = sample * 3;
    i32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        if bytes[offset + 2] & 0x80 == 0 {
            0
        } else {
            0xff
        },
    ])
}

#[test]
fn raw_fixtures_preserve_rate_channels_samples_and_positions() {
    for (bytes, rate, samples, speakers, lfe) in [
        (
            SURROUND,
            48_000,
            3840,
            vec![Speaker::L, Speaker::R, Speaker::C, Speaker::Ls, Speaker::Rs],
            true,
        ),
        (STEREO, 96_000, 7680, vec![Speaker::L, Speaker::R], false),
    ] {
        let mut front = TrueHdFront::new();
        let mut decoded = TrueHdDecoded::new();
        front.decode(bytes, &mut decoded).unwrap();
        assert_eq!(decoded.frames(), samples);
        assert_eq!(decoded.frames_at_mix_rate(), 3840);
        assert_eq!(decoded.error(), None);
        assert!(!decoded.is_empty());
        for au in decoded.access_units() {
            assert_eq!(au.sampling_frequency, rate);
            assert_eq!(au.channel_count, speakers.len() + usize::from(lfe));
        }
        let mut back = TrueHdBack::new(DecodeConfig::default());
        let mut count = 0;
        back.place(&decoded, 99, 4, &mut |scene| {
            count += scene.frames;
            assert_eq!((scene.rate, scene.epoch, scene.source_qpc), (rate, 4, 99));
            assert_eq!(scene.lfe.is_some(), lfe);
            for (object, speaker) in scene.objects.iter().zip(&speakers) {
                assert_eq!(object.position, speaker.position().unwrap());
                assert!(
                    object
                        .signal
                        .iter()
                        .all(|v| v.is_finite() && v.abs() <= 1.0)
                );
            }
        })
        .unwrap();
        assert_eq!(count, samples);
        assert_eq!(back.layout().source, SceneSource::TrueHd);
        assert_eq!(usize::from(back.layout().bed_count()), speakers.len());
        decoded.clear();
        assert!(decoded.is_empty());
        front.reset();
        back.reset();
    }
}

#[test]
fn fragmented_front_back_matches_offline_and_splits_max_frames() {
    let config = DecodeConfig {
        max_frames: 17,
        ..DecodeConfig::default()
    };
    let mut offline = TrueHdDecoder::new(config);
    let mut expected = Vec::new();
    offline
        .push(STEREO, 1, 1, &mut |scene| {
            assert!(scene.frames <= 17);
            for i in 0..scene.frames {
                expected.push([scene.objects[0].signal[i], scene.objects[1].signal[i]]);
            }
        })
        .unwrap();
    assert_eq!(offline.layout().source, SceneSource::TrueHd);
    let mut front = TrueHdFront::new();
    let mut decoded = TrueHdDecoded::new();
    let mut back = TrueHdBack::new(config);
    let mut actual = Vec::new();
    for part in STEREO.chunks(97) {
        front.decode(part, &mut decoded).unwrap();
        back.place(&decoded, 1, 1, &mut |scene| {
            assert!(scene.frames <= 17);
            for i in 0..scene.frames {
                actual.push([scene.objects[0].signal[i], scene.objects[1].signal[i]]);
            }
        })
        .unwrap();
    }
    assert_eq!(actual, expected);
    offline.reset();
}

#[test]
fn placement_and_reset_do_not_allocate() {
    let mut front = TrueHdFront::new();
    let mut decoded = TrueHdDecoded::new();
    front.decode(SURROUND, &mut decoded).unwrap();
    let mut back = TrueHdBack::new(DecodeConfig {
        max_frames: 13,
        ..DecodeConfig::default()
    });
    let mut frames = 0;
    let mut place = || {
        back.place(&decoded, 1, 1, &mut |scene| frames += scene.frames)
            .unwrap();
        back.reset();
    };
    #[cfg(debug_assertions)]
    assert_no_alloc::assert_no_alloc(&mut place);
    #[cfg(not(debug_assertions))]
    place();
    assert_eq!(frames, 3840);
}

#[test]
fn mat_content_uses_the_same_front_without_marker_bytes() {
    let bytes = include_bytes!("fixtures/truehd/tones-5.1-48k.spdif");
    let mut deframer = bitstream::Deframer::new();
    let mut storage = [0; decode::mat_frame::MAT_CONTENT_BYTES];
    let mut front = TrueHdFront::new();
    let mut decoded = TrueHdDecoded::new();
    let mut frames = 0;
    deframer.push(bytes, &mut |burst| {
        let content = decode::mat_frame::extract(&burst, &mut storage).unwrap();
        front.decode(content.bytes(), &mut decoded).unwrap();
        frames += decoded.frames();
    });
    // The muxer fixture contains three complete 20 ms MAT periods.
    assert_eq!(frames, 2880);
}

#[test]
fn decoded_pcm_matches_the_generated_source_sample_for_sample() {
    for (encoded, source, channels) in [(SURROUND, SURROUND_SOURCE, 6), (STEREO, STEREO_SOURCE, 2)]
    {
        let mut front = TrueHdFront::new();
        let mut decoded = TrueHdDecoded::new();
        front.decode(encoded, &mut decoded).unwrap();
        let mut frame = 0;
        for access_unit in decoded.access_units() {
            assert_eq!(access_unit.channel_count, channels);
            for row in &access_unit.pcm_data[..access_unit.sample_length] {
                for (channel, actual) in row[..channels].iter().enumerate() {
                    assert_eq!(
                        *actual,
                        pcm24_sample(source, frame * channels + channel),
                        "frame {frame}, channel {channel}"
                    );
                }
                frame += 1;
            }
        }
        assert_eq!(source.len(), frame * channels * 3);
    }
}

/// Private raw TrueHD or IEC 61937 MAT stream. Redistribution rights for an
/// object-bearing sample have not been established, so this remains explicit.
#[test]
#[ignore = "requires a locally permitted SSR_TRUEHD_ATMOS_SAMPLE raw TrueHD or MAT stream"]
fn truehd_external_object_sample_has_positioned_objects() {
    let path = std::env::var_os("SSR_TRUEHD_ATMOS_SAMPLE")
        .expect("set SSR_TRUEHD_ATMOS_SAMPLE to raw TrueHD or IEC 61937 MAT");
    let input = std::fs::read(path).unwrap();
    let mut object_scenes = 0;
    let mut inspect = |scene: &ssr_types::SceneBlock<'_>| {
        assert!(scene.is_consistent());
        assert!(scene.objects.iter().all(|object| {
            object.position.direction().is_some()
                || (object.position == ssr_types::Position::default() && object.spread == 1.0)
        }));
        object_scenes += 1;
    };

    let mut deframer = bitstream::Deframer::new();
    let mut storage = [0; decode::mat_frame::MAT_CONTENT_BYTES];
    let mut front = TrueHdFront::new();
    let mut decoded = TrueHdDecoded::new();
    let mut back = TrueHdBack::new(DecodeConfig::default());
    let mut mat_periods = 0;
    deframer.push(&input, &mut |burst| {
        let content = decode::mat_frame::extract(&burst, &mut storage).unwrap();
        front.decode(content.bytes(), &mut decoded).unwrap();
        back.place(&decoded, 0, 1, &mut inspect).unwrap();
        assert_eq!(back.layout().source, SceneSource::TrueHdObjects);
        mat_periods += 1;
    });
    if mat_periods == 0 {
        let mut decoder = TrueHdDecoder::new(DecodeConfig::default());
        decoder.push(&input, 0, 1, &mut inspect).unwrap();
        assert_eq!(decoder.layout().source, SceneSource::TrueHdObjects);
    }
    assert!(
        object_scenes > 0,
        "sample contains no decoded TrueHD objects"
    );
}

#[test]
fn integer_accessor_matches_scene_normalization() {
    let mut front = TrueHdFront::new();
    let mut decoded = TrueHdDecoded::new();
    front.decode(STEREO, &mut decoded).unwrap();
    let raw: Vec<_> = decoded
        .access_units()
        .flat_map(|au| {
            au.pcm_data[..au.sample_length]
                .iter()
                .map(|row| [row[0], row[1]])
        })
        .collect();
    let mut back = TrueHdBack::new(DecodeConfig::default());
    let mut offset = 0;
    back.place(&decoded, 0, 0, &mut |scene| {
        for i in 0..scene.frames {
            for (channel, &sample) in raw[offset + i].iter().enumerate() {
                assert_eq!(
                    scene.objects[channel].signal[i],
                    sample as f32 / 8_388_608.0
                );
            }
        }
        offset += scene.frames;
    })
    .unwrap();
    assert_eq!(offset, raw.len());
}
