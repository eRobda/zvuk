//! Fractional-octave band analysis via FFT (Welch's method). No IIR filters.
//!
//! The signal is cut into overlapping windows, each window goes through a real
//! FFT, the power spectra are averaged, and finally the power of every bin
//! falling inside a band is summed. The normalisation is chosen so that the sum
//! of power over all bins equals the mean square of the signal, which makes a
//! band RMS directly comparable to the broadband RMS.
//!
//! Two band resolutions are used. Whole octaves are enough to tell left from
//! right; crossover work needs thirds, because a sub crosses somewhere around
//! 80 Hz and whole octaves put their nearest centre at 125 Hz.

use anyhow::{anyhow, bail, Result};
use realfft::RealFftPlanner;

/// Standard octave-band centres used by the `left-right-balance` module.
pub const OCTAVE_CENTERS_HZ: [f32; 7] = [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];

/// ISO preferred third-octave centres, 20 Hz to 16 kHz.
///
/// The range covers a subwoofer at the bottom and a tweeter at the top, and it
/// spans the whole-octave centres completely, so thirds always sum back to
/// octaves.
pub const THIRD_OCTAVE_CENTERS_HZ: [f32; 30] = [
    20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0,
    500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0,
    8000.0, 10000.0, 12500.0, 16000.0,
];

/// FFT length for whole octaves: ~5.9 Hz per bin at 48 kHz.
pub const OCTAVE_FFT_LEN: usize = 8192;
/// FFT length for thirds. The 20 Hz third spans 17.8-22.4 Hz, so the bins have
/// to be well under 1.5 Hz for the band to contain more than one of them.
pub const THIRD_OCTAVE_FFT_LEN: usize = 32768;

#[derive(Debug, Clone, Copy)]
pub struct Band {
    pub center_hz: f32,
    #[allow(dead_code)] // band edges, kept for display and debugging
    pub low_hz: f32,
    #[allow(dead_code)]
    pub high_hz: f32,
    /// RMS amplitude inside the band, linear and in the same unit as the samples.
    pub rms: f32,
    /// How many FFT bins fell into the band. Zero means the band is out of range.
    pub bins: usize,
}

/// Computes the RMS inside whole-octave bands around the given centres.
pub fn octave_bands(signal: &[f32], sample_rate: f32, centers: &[f32]) -> Result<Vec<Band>> {
    fractional_octave_bands(signal, sample_rate, centers, 1, OCTAVE_FFT_LEN)
}

/// Computes the RMS inside 1/`bands_per_octave`-octave bands.
///
/// Band edges are the standard `fc * 2^(-+1 / (2 * bands_per_octave))`, so
/// `bands_per_octave = 1` gives the familiar `fc / sqrt(2)` to `fc * sqrt(2)`.
pub fn fractional_octave_bands(
    signal: &[f32],
    sample_rate: f32,
    centers: &[f32],
    bands_per_octave: u32,
    max_fft_len: usize,
) -> Result<Vec<Band>> {
    if bands_per_octave == 0 {
        bail!("bands_per_octave must be at least 1");
    }
    let spectrum = power_spectrum_with_len(signal, sample_rate, max_fft_len)?;
    let bin_hz = sample_rate / spectrum.fft_len as f32;
    let edge = 2f32.powf(1.0 / (2.0 * bands_per_octave as f32));

    let bands = centers
        .iter()
        .map(|&center| {
            let low = center / edge;
            let high = center * edge;
            let mut power = 0.0f64;
            let mut bins = 0usize;
            for (k, &p) in spectrum.power.iter().enumerate() {
                let f = k as f32 * bin_hz;
                if f >= low && f < high {
                    power += p as f64;
                    bins += 1;
                }
            }
            Band {
                center_hz: center,
                low_hz: low,
                high_hz: high,
                rms: power.sqrt() as f32,
                bins,
            }
        })
        .collect();

    Ok(bands)
}

pub struct PowerSpectrum {
    /// One-sided power per bin; the sum over bins equals the signal mean square.
    pub power: Vec<f32>,
    pub fft_len: usize,
}

/// Welch power spectrum estimate with a Hann window and 50% overlap.
///
/// `max_fft_len` is an upper bound: a shorter signal gets a shorter transform,
/// because a window that does not fit produces no frames at all.
pub fn power_spectrum_with_len(
    signal: &[f32],
    sample_rate: f32,
    max_fft_len: usize,
) -> Result<PowerSpectrum> {
    if signal.len() < 256 {
        bail!(
            "signal too short for spectral analysis ({} samples)",
            signal.len()
        );
    }
    if sample_rate <= 0.0 {
        bail!("invalid sample rate");
    }

    let fft_len = max_fft_len
        .max(256)
        .min(previous_power_of_two(signal.len()))
        .max(256);
    let hop = fft_len / 2;

    let window: Vec<f32> = (0..fft_len)
        .map(|n| {
            let x = std::f32::consts::TAU * n as f32 / fft_len as f32;
            0.5 - 0.5 * x.cos()
        })
        .collect();
    // Window power, used to normalise back to the true signal level.
    let window_power: f64 = window.iter().map(|&w| (w as f64) * (w as f64)).sum();

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_len);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();

    let mut accum = vec![0.0f64; output.len()];
    let mut frames = 0usize;

    let mut start = 0usize;
    while start + fft_len <= signal.len() {
        for i in 0..fft_len {
            input[i] = signal[start + i] * window[i];
        }
        fft.process(&mut input, &mut output)
            .map_err(|e| anyhow!("FFT failed: {e}"))?;
        for (acc, c) in accum.iter_mut().zip(output.iter()) {
            *acc += c.norm_sqr() as f64;
        }
        frames += 1;
        start += hop;
    }

    if frames == 0 {
        bail!("could not fit a single FFT window");
    }

    // Parseval normalisation to a one-sided spectrum:
    //   P[k] = |X[k]|^2 / (N * sum(w^2)), doubled outside DC and Nyquist.
    let norm = 1.0 / (fft_len as f64 * window_power);
    let last = accum.len() - 1;
    let power = accum
        .iter()
        .enumerate()
        .map(|(k, &acc)| {
            let mean = acc / frames as f64;
            let one_sided = if k == 0 || k == last { 1.0 } else { 2.0 };
            (mean * norm * one_sided) as f32
        })
        .collect();

    Ok(PowerSpectrum { power, fft_len })
}

fn previous_power_of_two(n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut p = 1usize;
    while p * 2 <= n {
        p *= 2;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    /// White noise of a known amplitude: the power summed over all bins must
    /// match the mean square of the signal (Parseval normalisation).
    #[test]
    fn spectrum_preserves_total_power() {
        let signal = white_noise(FS as usize * 2, 0.25);
        let ms_time: f64 =
            signal.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / signal.len() as f64;

        let spectrum = power_spectrum_with_len(&signal, FS, OCTAVE_FFT_LEN).unwrap();
        let ms_freq: f64 = spectrum.power.iter().map(|&p| p as f64).sum();

        let ratio_db = 10.0 * (ms_freq / ms_time).log10();
        assert!(
            ratio_db.abs() < 0.2,
            "time-domain and frequency-domain power differ by {ratio_db:.3} dB"
        );
    }

    /// A sine of known frequency and amplitude must land in the right band with
    /// the right RMS value (A/sqrt(2)).
    #[test]
    fn sine_lands_in_correct_band() {
        let amplitude = 0.5f32;
        let signal: Vec<f32> = (0..FS as usize)
            .map(|n| amplitude * (std::f32::consts::TAU * 1000.0 * n as f32 / FS).sin())
            .collect();

        let bands = octave_bands(&signal, FS, &OCTAVE_CENTERS_HZ).unwrap();
        let khz = bands.iter().find(|b| b.center_hz == 1000.0).unwrap();

        let expected = amplitude / std::f32::consts::SQRT_2;
        let error_db = 20.0 * (khz.rms / expected).log10();
        assert!(
            error_db.abs() < 0.2,
            "1 kHz band RMS is off by {error_db:.3} dB"
        );

        // Neighbouring bands must sit far below the one holding the sine.
        for band in bands.iter().filter(|b| b.center_hz != 1000.0) {
            let leak_db = 20.0 * (band.rms.max(1e-12) / expected).log10();
            assert!(
                leak_db < -40.0,
                "band {} Hz leaks at {leak_db:.1} dB",
                band.center_hz
            );
        }
    }

    /// An attenuated signal must show up as exactly that attenuation in every
    /// band, which is precisely what the left-right-balance module reports.
    #[test]
    fn gain_difference_shows_in_every_band() {
        let left = white_noise(FS as usize * 2, 0.4);
        let attenuation = 10f32.powf(-3.0 / 20.0);
        let right: Vec<f32> = left.iter().map(|&s| s * attenuation).collect();

        let bands_l = octave_bands(&left, FS, &OCTAVE_CENTERS_HZ).unwrap();
        let bands_r = octave_bands(&right, FS, &OCTAVE_CENTERS_HZ).unwrap();

        for (l, r) in bands_l.iter().zip(bands_r.iter()) {
            let diff = 20.0 * (l.rms / r.rms).log10();
            assert!(
                (diff - 3.0).abs() < 0.01,
                "band {} Hz: expected 3.00 dB, computed {diff:.3} dB",
                l.center_hz
            );
        }
    }

    #[test]
    fn every_octave_band_has_bins() {
        let signal = white_noise(FS as usize, 0.2);
        let bands = octave_bands(&signal, FS, &OCTAVE_CENTERS_HZ).unwrap();
        for band in &bands {
            assert!(band.bins > 0, "band {} Hz has no bins", band.center_hz);
        }
    }

    fn white_noise(len: usize, amplitude: f32) -> Vec<f32> {
        let mut state = 0x1234_5678_9ABC_DEF0u64;
        (0..len)
            .map(|_| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                let v = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
                amplitude * (((v >> 40) as f32 / 8_388_608.0) - 1.0)
            })
            .collect()
    }

    /// A sine must land in its third-octave band too, where the bands are
    /// narrow enough that the neighbours have to be much further down.
    #[test]
    fn a_sine_lands_in_its_third_octave_band() {
        let amplitude = 0.5f32;
        let target_hz = 80.0f32;
        let signal: Vec<f32> = (0..(FS as usize * 4))
            .map(|n| amplitude * (std::f32::consts::TAU * target_hz * n as f32 / FS).sin())
            .collect();

        let bands = fractional_octave_bands(
            &signal,
            FS,
            &THIRD_OCTAVE_CENTERS_HZ,
            3,
            THIRD_OCTAVE_FFT_LEN,
        )
        .unwrap();

        let hit = bands.iter().find(|b| b.center_hz == target_hz).unwrap();
        let expected = amplitude / std::f32::consts::SQRT_2;
        let error_db = 20.0 * (hit.rms / expected).log10();
        assert!(
            error_db.abs() < 0.3,
            "80 Hz third is off by {error_db:.3} dB"
        );

        for band in bands.iter().filter(|b| b.center_hz != target_hz) {
            let leak_db = 20.0 * (band.rms.max(1e-12) / expected).log10();
            assert!(
                leak_db < -30.0,
                "third at {} Hz leaks at {leak_db:.1} dB",
                band.center_hz
            );
        }
    }

    /// The bottom third is the one at risk of having no bins to sum, which is
    /// exactly the band a subwoofer measurement lives in.
    #[test]
    fn the_lowest_third_octave_band_still_has_bins() {
        let signal = white_noise(FS as usize * 5, 0.2);
        let bands = fractional_octave_bands(
            &signal,
            FS,
            &THIRD_OCTAVE_CENTERS_HZ,
            3,
            THIRD_OCTAVE_FFT_LEN,
        )
        .unwrap();
        for band in &bands {
            assert!(band.bins > 0, "third at {} Hz has no bins", band.center_hz);
        }
        assert!(
            bands[0].bins >= 3,
            "20 Hz third only has {} bins, too few to trust",
            bands[0].bins
        );
    }

    /// Thirds and whole octaves have to agree: summing the three thirds inside
    /// an octave must reproduce that octave.
    #[test]
    fn thirds_sum_back_to_whole_octaves() {
        let signal = white_noise(FS as usize * 4, 0.3);
        let octaves = octave_bands(&signal, FS, &OCTAVE_CENTERS_HZ).unwrap();
        let thirds = fractional_octave_bands(
            &signal,
            FS,
            &THIRD_OCTAVE_CENTERS_HZ,
            3,
            THIRD_OCTAVE_FFT_LEN,
        )
        .unwrap();

        for octave in &octaves {
            let power: f32 = thirds
                .iter()
                .filter(|t| {
                    t.center_hz > octave.center_hz / 1.5 && t.center_hz < octave.center_hz * 1.5
                })
                .map(|t| t.rms * t.rms)
                .sum();
            let diff_db = 20.0 * (power.sqrt() / octave.rms).log10();
            assert!(
                diff_db.abs() < 0.5,
                "octave {} Hz differs from its thirds by {diff_db:.2} dB",
                octave.center_hz
            );
        }
    }
}
