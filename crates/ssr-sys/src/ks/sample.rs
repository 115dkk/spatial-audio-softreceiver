#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SampleFormat {
    Float32,
    Int32,
    Int24In32,
    Packed24,
    Int16,
}

impl SampleFormat {
    pub const ALL: [Self; 5] = [
        Self::Float32,
        Self::Int32,
        Self::Int24In32,
        Self::Packed24,
        Self::Int16,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Float32 => "float32",
            Self::Int32 => "int32",
            Self::Int24In32 => "int24_in_32",
            Self::Packed24 => "packed24",
            Self::Int16 => "int16",
        }
    }
    pub fn bits(self) -> u16 {
        match self {
            Self::Packed24 => 24,
            Self::Int16 => 16,
            _ => 32,
        }
    }
    pub fn valid_bits(self) -> u16 {
        if self == Self::Int24In32 {
            24
        } else {
            self.bits()
        }
    }
    pub fn bytes(self) -> usize {
        usize::from(self.bits() / 8)
    }
    pub fn convert(self, samples: &[f32], bytes: &mut [u8]) {
        assert_eq!(bytes.len(), samples.len() * self.bytes());
        for (&sample, dst) in samples.iter().zip(bytes.chunks_exact_mut(self.bytes())) {
            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            let integer = |scale: f64| {
                (f64::from(sample) * scale)
                    .round()
                    .clamp(-scale, scale - 1.0) as i32
            };
            match self {
                Self::Float32 => dst.copy_from_slice(&sample.to_le_bytes()),
                Self::Int32 => dst.copy_from_slice(&integer(2_147_483_648.0).to_le_bytes()),
                Self::Int24In32 => dst.copy_from_slice(&(integer(8_388_608.0) << 8).to_le_bytes()),
                Self::Packed24 => dst.copy_from_slice(&integer(8_388_608.0).to_le_bytes()[..3]),
                Self::Int16 => dst.copy_from_slice(&(integer(32768.0) as i16).to_le_bytes()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ks_sample_order_is_float_then_integer_precision() {
        assert_eq!(
            SampleFormat::ALL.map(SampleFormat::name),
            ["float32", "int32", "int24_in_32", "packed24", "int16"]
        );
    }
    #[test]
    fn ks_conversion_clips_rounds_and_silences_nonfinite() {
        for format in SampleFormat::ALL {
            let input = [-2.0, -1.0, 0.0, 1.0, 2.0, f32::NAN, f32::INFINITY];
            let mut bytes = vec![0; input.len() * format.bytes()];
            format.convert(&input, &mut bytes);
            let chunks: Vec<_> = bytes.chunks_exact(format.bytes()).collect();
            assert_eq!(chunks[0], chunks[1]);
            assert_eq!(chunks[3], chunks[4]);
            assert!(chunks[2].iter().all(|&b| b == 0));
            assert_eq!(chunks[2], chunks[5]);
            assert_eq!(chunks[2], chunks[6]);
        }
        let mut bytes = [0; 6];
        SampleFormat::Packed24.convert(&[-1.0, 1.0], &mut bytes);
        assert_eq!(bytes, [0, 0, 128, 255, 255, 127]);
        let mut bytes = [0; 8];
        SampleFormat::Int24In32.convert(&[-1.0, 1.0], &mut bytes);
        assert_eq!(bytes, [0, 0, 0, 128, 0, 255, 255, 127]);
        let mut bytes = [0; 4];
        SampleFormat::Int16.convert(&[0.5 / 32768.0, -0.5 / 32768.0], &mut bytes);
        assert_eq!(bytes, [1, 0, 255, 255]);
    }
}
