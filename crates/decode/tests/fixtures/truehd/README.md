# Synthetic TrueHD fixtures

These files are synthetic audio generated for the decoder tests, not captured
commercial audio. They contain no object metadata and may be redistributed
under the repository's MIT license.

Generated on 2026-10-18 using `/usr/bin/ffmpeg` from Debian package 7.1.2.
The TrueHD encoder is experimental, so both commands enable experimental
encoders explicitly.

```sh
ffmpeg -y -f lavfi -i "aevalsrc=0.1*sin(2*PI*375*t)|0.1*sin(2*PI*562.5*t)|0.1*sin(2*PI*750*t)|0.1*sin(2*PI*62.5*t)|0.1*sin(2*PI*937.5*t)|0.1*sin(2*PI*1125*t):s=48000:d=0.08:c=5.1" -c:a truehd -strict -2 -f truehd crates/decode/tests/fixtures/truehd/tones-5.1-48k.thd
ffmpeg -y -f lavfi -i "aevalsrc=0.1*sin(2*PI*375*t)|0.1*sin(2*PI*562.5*t)|0.1*sin(2*PI*750*t)|0.1*sin(2*PI*62.5*t)|0.1*sin(2*PI*937.5*t)|0.1*sin(2*PI*1125*t):s=48000:d=0.08:c=5.1" -c:a pcm_s24le -f s24le crates/decode/tests/fixtures/truehd/tones-5.1-48k.pcm24le
ffmpeg -y -f lavfi -i "aevalsrc=0.1*sin(2*PI*750*t)|0.1*sin(2*PI*1125*t):s=96000:d=0.08:c=stereo" -c:a truehd -strict -2 -f truehd crates/decode/tests/fixtures/truehd/tones-stereo-96k.thd
ffmpeg -y -f lavfi -i "aevalsrc=0.1*sin(2*PI*750*t)|0.1*sin(2*PI*1125*t):s=96000:d=0.08:c=stereo" -c:a pcm_s24le -f s24le crates/decode/tests/fixtures/truehd/tones-stereo-96k.pcm24le
ffmpeg -y -i crates/decode/tests/fixtures/truehd/tones-5.1-48k.thd -c:a copy -f spdif crates/decode/tests/fixtures/truehd/tones-5.1-48k.spdif
ffmpeg -y -i crates/decode/tests/fixtures/truehd/tones-stereo-96k.thd -c:a copy -f spdif crates/decode/tests/fixtures/truehd/tones-stereo-96k.spdif
```

The 48 kHz file's WAV order is FL 375 Hz, FR 562.5 Hz, FC 750 Hz,
LFE 62.5 Hz, BL 937.5 Hz and BR 1125 Hz. The 96 kHz file is stereo,
with left 750 Hz and right 1125 Hz. Tests compare decoded integer samples
against the stored raw 24-bit PCM generated directly from these expressions;
source WAV files are not needed.

SHA-256:

- `tones-5.1-48k.thd`: `cb98c7e72e15366a8fd20e05b715673251ba0933d1ed0c7f2dfeec733f6e4946`
- `tones-5.1-48k.pcm24le`: `88f1ab42ede0474b39c62077cc057a12cd7d7c4047bc8df26799216fd5b2f0c8`
- `tones-stereo-96k.thd`: `18e4cda095a9844a11bd3119d8090eb2acf36cd8b4e0e281a4a39e1eee3152f0`
- `tones-stereo-96k.pcm24le`: `7952c0bdea204f3d004135b53295c969db35724bf0718775a3e03a803ee9b562`
- `tones-5.1-48k.spdif`: `89feef2cedbdc21644d8a6bd0488a12ab979f418cc672cd8268d34fc99f6ebf9`
- `tones-stereo-96k.spdif`: `69ad1d661f40826a78f5f707b4afc7a5b1804ad1ae259aa3af8308456de04cc9`

Each SPDIF file contains three 61,440-byte IEC 61937-9 repetition periods.
The 8-byte burst preamble and 61,424-byte MAT payload are stored as
little-endian carrier words; `bitstream::Deframer` swaps each word and exposes
the payload in codec byte order.

## Object coverage

No TrueHD object sample is redistributed here. The ignored integration test
reads a locally permitted raw TrueHD or MAT sample from
`SSR_TRUEHD_ATMOS_SAMPLE`. The object bitstream path remains partial until a
sample with established redistribution rights can be added.
