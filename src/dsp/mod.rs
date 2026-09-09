//! Shared DSP helpers for measurement modules.

pub mod octave;
pub mod segment;

/// Lowest level we still convert to dB (about -240 dBFS).
const FLOOR: f32 = 1e-12;

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()))
}

/// Linear amplitude to dB.
pub fn db(amplitude: f32) -> f32 {
    20.0 * amplitude.max(FLOOR).log10()
}

/// Ratio of two amplitudes in dB (positive means `a` is louder).
pub fn db_ratio(a: f32, b: f32) -> f32 {
    db(a) - db(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_sine_is_amplitude_over_sqrt_two() {
        let samples: Vec<f32> = (0..48_000)
            .map(|n| (std::f32::consts::TAU * 100.0 * n as f32 / 48_000.0).sin())
            .collect();
        let expected = 1.0 / std::f32::consts::SQRT_2;
        assert!((rms(&samples) - expected).abs() < 1e-3);
    }

    #[test]
    fn db_ratio_is_signed_and_symmetric() {
        assert!((db_ratio(2.0, 1.0) - 6.0206).abs() < 1e-3);
        assert!((db_ratio(1.0, 2.0) + 6.0206).abs() < 1e-3);
        assert_eq!(db_ratio(1.0, 1.0), 0.0);
    }

    #[test]
    fn silence_does_not_produce_nan() {
        assert!(db(0.0).is_finite());
        assert!(rms(&[]).is_finite());
    }
}
