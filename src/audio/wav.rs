//! Writing generated test signals to WAV.
//!
//! 16-bit PCM, because that is what car head units read off a USB stick without
//! complaining. The samples come in as interleaved floats in [-1, 1].

use std::path::Path;

use anyhow::{Context, Result};

pub fn write(path: &Path, samples: &[f32], sample_rate: u32, channels: u16) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create directory {}", parent.display()))?;
        }
    }

    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(path, spec)
        .with_context(|| format!("cannot create {}", path.display()))?;
    for &s in samples {
        writer.write_sample(to_i16(s))?;
    }
    writer
        .finalize()
        .with_context(|| format!("cannot finish writing {}", path.display()))?;
    Ok(())
}

/// Scales by 32767 rather than 32768 so that +1.0 and -1.0 stay symmetric and
/// neither wraps around.
fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scale_maps_to_the_extremes_without_wrapping() {
        assert_eq!(to_i16(1.0), i16::MAX);
        assert_eq!(to_i16(-1.0), -i16::MAX);
        assert_eq!(to_i16(0.0), 0);
    }

    #[test]
    fn out_of_range_input_is_clamped() {
        assert_eq!(to_i16(4.2), i16::MAX);
        assert_eq!(to_i16(-4.2), -i16::MAX);
    }

    #[test]
    fn a_written_file_reads_back_with_the_same_shape_and_samples() {
        let path = std::env::temp_dir().join("zvuk-wav-roundtrip.wav");
        let _ = std::fs::remove_file(&path);

        // Two frames of stereo, values chosen to survive 16-bit exactly.
        let samples: Vec<f32> = vec![0.0, 0.5, -0.5, 1.0];
        write(&path, &samples, 44_100, 2).unwrap();

        let mut reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.sample_rate, 44_100);
        assert_eq!(spec.bits_per_sample, 16);

        let read: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        let expected: Vec<i16> = samples.iter().map(|&s| to_i16(s)).collect();
        assert_eq!(read, expected);

        std::fs::remove_file(&path).unwrap();
    }
}
