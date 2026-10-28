#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use brir::{AnglePreset, BrirError, BrirSource, LfePolicy, Profile, layout, prepare};
use hound::{SampleFormat, WavSpec, WavWriter};
use ssr_types::channel_layout_preset;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ssr-brir-high-channel-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_float_wav(path: &Path, tracks: usize) {
    let mut writer = WavWriter::create(
        path,
        WavSpec {
            channels: tracks as u16,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        },
    )
    .unwrap();
    for frame in 0..8 {
        for track in 0..tracks {
            writer
                .write_sample(if frame == 0 {
                    0.01 + track as f32 / 1000.0
                } else {
                    0.0
                })
                .unwrap();
        }
    }
    writer.finalize().unwrap();
}

fn append_chunk(path: &Path, id: &[u8; 4], payload: &[u8], declared_size: Option<u32>) {
    let mut bytes = fs::read(path).unwrap();
    bytes.extend_from_slice(id);
    bytes.extend_from_slice(&declared_size.unwrap_or(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
    if !payload.len().is_multiple_of(2) {
        bytes.push(0);
    }
    let riff_size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
    fs::write(path, bytes).unwrap();
}

fn append_layout_chunk(path: &Path, labels: &[&str]) {
    let tracks = labels
        .iter()
        .flat_map(|label| [format!("\"{label}-left\""), format!("\"{label}-right\"")])
        .collect::<Vec<_>>()
        .join(",");
    let payload = format!("{{\"version\":1,\"tracks\":[{tracks}]}}");
    append_chunk(path, b"ICHL", payload.as_bytes(), None);
}

fn profile(source: BrirSource) -> Profile {
    Profile {
        name: "synthetic".into(),
        brir: source,
        gain_db: 0.0,
        limiter_ceiling_dbfs: -1.0,
        lfe: LfePolicy::Mute,
        angles: AnglePreset::Standard,
        angle_overrides: Vec::new(),
        truncate: None,
        trim_leading_delay: false,
    }
}

const LAYOUTS: [(&str, &[&str]); 4] = [
    (
        "13.1",
        &[
            "L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs", "T", "HL", "HC", "HR", "HLs", "HRs",
        ],
    ),
    (
        "22.2",
        &[
            "FL", "FR", "FC", "LFE1", "BL", "BR", "FLc", "FRc", "BC", "LFE2", "SiL", "SiR", "TpFL",
            "TpFR", "TpFC", "TpC", "TpBL", "TpBR", "TpSiL", "TpSiR", "TpBC", "BtFC", "BtFL",
            "BtFR",
        ],
    ),
    (
        "24.1.10",
        &[
            "L", "R", "C", "LFE", "Ls", "Rs", "Lrs", "Rrs", "Lw", "Rw", "Ltf", "Rtf", "Ltm", "Rtm",
            "Ltr", "Rtr", "Lc", "Rc", "Lsc", "Rsc", "Ls1", "Rs1", "Ls2", "Rs2", "Lrs1", "Rrs1",
            "Lrs2", "Rrs2", "Lcs", "Rcs", "Cs", "Lfh", "Rfh", "Lrh", "Rrh",
        ],
    ),
    (
        "30.2",
        &[
            "C", "L", "R", "Ls", "Rs", "LFE1", "Cs", "Lsr", "Rsr", "Lss", "Rss", "Lc", "Rc", "Lh",
            "Ch", "Rh", "LFE2", "Lw", "Rw", "Oh", "Lhs", "Rhs", "Chr", "Lhr", "Rhr", "Cb", "Lb",
            "Rb", "Ltf", "Rtf", "Ltr", "Rtr",
        ],
    ),
];

#[test]
fn ordered_three_x_files_accept_full_track_counts() {
    let directory = TestDirectory::new("ordered-full");
    for (name, tracks, source) in [
        (
            "hrir.wav",
            82,
            BrirSource::Hrir as fn(PathBuf) -> BrirSource,
        ),
        (
            "hesuvi.wav",
            80,
            BrirSource::Hesuvi as fn(PathBuf) -> BrirSource,
        ),
    ] {
        let path = directory.join(name);
        write_float_wav(&path, tracks);
        let input = profile(source(path));
        let from_headers = layout(&input).unwrap();
        let prepared = prepare(&input, 48_000, 64).unwrap();
        assert_eq!(from_headers.directions(), prepared.layout.directions());
        assert_eq!(from_headers.len(), 40);
        assert_eq!(from_headers.directions().last().unwrap().name, "DFC");
    }
}

#[test]
fn first_extended_pair_selects_three_x_angles_and_overrides_still_win() {
    let directory = TestDirectory::new("ordered-34");
    let path = directory.join("hrir.wav");
    write_float_wav(&path, 34);
    let mut input = profile(BrirSource::Hrir(path));
    input.angle_overrides = vec![("BL".into(), 140.0, 2.0)];
    let directions = layout(&input).unwrap();
    let prepared = prepare(&input, 48_000, 64).unwrap();
    assert_eq!(directions.directions(), prepared.layout.directions());
    let bl = directions
        .directions()
        .iter()
        .find(|direction| direction.name == "BL")
        .unwrap();
    let tsl = directions
        .directions()
        .iter()
        .find(|direction| direction.name == "TSL")
        .unwrap();
    let fcl = directions
        .directions()
        .iter()
        .find(|direction| direction.name == "FCL")
        .unwrap();
    assert_eq!((bl.azimuth_deg, bl.elevation_deg), (140.0, 2.0));
    assert_eq!((tsl.azimuth_deg, tsl.elevation_deg), (90.0, 65.0));
    assert_eq!((fcl.azimuth_deg, fcl.elevation_deg), (15.0, 0.0));
}

#[test]
fn every_layout_file_uses_ssr_direction_names_and_angles() {
    let directory = TestDirectory::new("layout-files");
    for (name, labels) in LAYOUTS {
        let path = directory.join(&format!("layout-{name}.wav"));
        write_float_wav(&path, labels.len() * 2);
        append_layout_chunk(&path, labels);
        assert!(brir::is_layout_wav(&path).unwrap());
        let input = profile(BrirSource::LayoutWav(path));
        let from_headers = layout(&input).unwrap();
        let prepared = prepare(&input, 48_000, 64).unwrap();
        assert_eq!(from_headers.directions(), prepared.layout.directions());
        let expected: Vec<_> = channel_layout_preset(name)
            .unwrap()
            .iter()
            .filter_map(|speaker| {
                speaker
                    .angles()
                    .map(|(azimuth, elevation)| (speaker.name(), azimuth, elevation))
            })
            .collect();
        let actual: Vec<_> = from_headers
            .directions()
            .iter()
            .map(|direction| {
                (
                    direction.name.as_str(),
                    direction.azimuth_deg,
                    direction.elevation_deg,
                )
            })
            .collect();
        assert_eq!(actual, expected, "{name}");
    }
}

#[test]
fn unmatched_layout_labels_still_allow_a_csv_sidecar() {
    let directory = TestDirectory::new("layout-fallback");
    let path = directory.join("mapped.wav");
    write_float_wav(&path, 4);
    append_layout_chunk(&path, &["Unknown1", "Unknown2"]);
    assert!(!brir::is_layout_wav(&path).unwrap());
    let map = directory.join("channels.csv");
    fs::write(
        &map,
        "speaker,BRIR_left_1based,BRIR_right_1based\nFL,1,2\nFR,3,4\n",
    )
    .unwrap();
    let directions = layout(&profile(BrirSource::Mapped { wav: path, map })).unwrap();
    assert_eq!(
        directions
            .directions()
            .iter()
            .map(|direction| direction.name.as_str())
            .collect::<Vec<_>>(),
        ["FL", "FR"]
    );
}

#[test]
fn truncated_layout_chunk_is_rejected_without_panicking() {
    let directory = TestDirectory::new("layout-truncated");
    let path = directory.join("truncated.wav");
    write_float_wav(&path, 28);
    append_chunk(&path, b"ICHL", b"{}", Some(200));
    let error = layout(&profile(BrirSource::LayoutWav(path))).unwrap_err();
    assert!(matches!(error, BrirError::LayoutChunk { .. }));
    assert!(error.to_string().contains("truncated or oversized chunk"));

    let oversized = directory.join("oversized.wav");
    write_float_wav(&oversized, 28);
    append_chunk(&oversized, b"ICHL", &[b' '; 4097], None);
    let error = layout(&profile(BrirSource::LayoutWav(oversized))).unwrap_err();
    assert!(matches!(error, BrirError::LayoutChunk { .. }));
    assert!(error.to_string().contains("oversized ICHL chunk"));
}

#[test]
fn extensible_pcm32_with_zero_mask_and_fact_is_read() {
    let directory = TestDirectory::new("wavex");
    let path = directory.join("extensible.wav");
    let channels = 4u16;
    let frames = 2u32;
    let align = channels * 4;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&0xfffeu16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&48_000u32.to_le_bytes());
    bytes.extend_from_slice(&(48_000u32 * u32::from(align)).to_le_bytes());
    bytes.extend_from_slice(&align.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&22u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]);
    bytes.extend_from_slice(b"fact");
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&frames.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(frames * u32::from(align)).to_le_bytes());
    for sample in [
        i32::MAX,
        0,
        i32::MIN,
        1 << 30,
        0,
        -(1 << 30),
        1 << 29,
        -(1 << 29),
    ] {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let riff_size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
    fs::write(&path, bytes).unwrap();

    let wav = brir::ir::read_wav(&path, 48_000).unwrap();
    assert_eq!(wav.channels.len(), 4);
    assert_eq!(wav.channels[0].len(), 2);
    assert!((wav.channels[0][0] - (i32::MAX as f32 / 2_147_483_648.0)).abs() < 1e-6);
    assert_eq!(wav.channels[2][0], -1.0);
    assert_eq!(wav.channels[1][1], -0.5);
}
