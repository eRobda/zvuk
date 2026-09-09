//! Test signal generators.

/// Deterministic white noise source (xorshift64*), so that measurements are
/// reproducible and both channels get bit-for-bit the same signal.
struct Xorshift64(u64);

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    /// A sample in the range [-1, 1).
    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        // Top 24 bits, mapped symmetrically around zero.
        ((v >> 40) as f32 / 8_388_608.0) - 1.0
    }
}

/// Pink noise through Kellett's filter, an approximation of -3 dB/octave that
/// stays within 0.05 dB from 10 Hz to 20 kHz. The filter is warmed up first so
/// the burst does not start with a transient.
// The coefficients are quoted at their published precision on purpose; f32
// cannot hold the extra digits, but keeping them makes the source traceable.
#[allow(clippy::excessive_precision)]
fn pink_noise(len: usize, seed: u64) -> Vec<f32> {
    const WARMUP: usize = 4096;

    let mut rng = Xorshift64::new(seed);
    let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6) =
        (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);

    let mut out = Vec::with_capacity(len);
    for i in 0..(len + WARMUP) {
        let white = rng.next_f32();
        b0 = 0.99886 * b0 + white * 0.0555179;
        b1 = 0.99332 * b1 + white * 0.0750759;
        b2 = 0.96900 * b2 + white * 0.1538520;
        b3 = 0.86650 * b3 + white * 0.3104856;
        b4 = 0.55000 * b4 + white * 0.5329522;
        b5 = -0.7616 * b5 - white * 0.0168980;
        let pink = b0 + b1 + b2 + b3 + b4 + b5 + b6 + white * 0.5362;
        b6 = white * 0.115926;
        if i >= WARMUP {
            out.push(pink);
        }
    }
    out
}

/// Scales the buffer to the requested linear peak.
fn normalize_peak(buf: &mut [f32], target: f32) {
    let peak = buf.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
    if peak <= f32::EPSILON {
        return;
    }
    let gain = target / peak;
    for s in buf.iter_mut() {
        *s *= gain;
    }
}

/// Raised-cosine ramps so the speaker does not click.
fn apply_fades(buf: &mut [f32], fade_len: usize) {
    let n = fade_len.min(buf.len() / 2);
    if n == 0 {
        return;
    }
    for i in 0..n {
        let g = 0.5 - 0.5 * (std::f32::consts::PI * i as f32 / n as f32).cos();
        buf[i] *= g;
        let last = buf.len() - 1 - i;
        buf[last] *= g;
    }
}

/// A ready-to-play pink noise burst: normalised to `level_dbfs` peak, with
/// 30 ms fades on both ends.
pub fn pink_noise_burst(sample_rate: u32, seconds: f32, level_dbfs: f32, seed: u64) -> Vec<f32> {
    let len = ((sample_rate as f32) * seconds).round().max(1.0) as usize;
    let mut buf = pink_noise(len, seed);
    let target = 10f32.powf(level_dbfs / 20.0).clamp(0.0, 1.0);
    normalize_peak(&mut buf, target);
    apply_fades(&mut buf, (sample_rate as f32 * 0.030) as usize);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_has_requested_length_and_level() {
        let buf = pink_noise_burst(48_000, 3.0, -6.0, 42);
        assert_eq!(buf.len(), 144_000);

        let peak = buf.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        let expected = 10f32.powf(-6.0 / 20.0);
        assert!(
            (peak - expected).abs() < 1e-4,
            "peak {peak} instead of {expected}"
        );
    }

    #[test]
    fn burst_is_deterministic() {
        let a = pink_noise_burst(48_000, 0.5, -6.0, 7);
        let b = pink_noise_burst(48_000, 0.5, -6.0, 7);
        assert_eq!(a, b, "the same seed must produce the same signal");
    }

    #[test]
    fn fades_start_and_end_at_silence() {
        let buf = pink_noise_burst(48_000, 1.0, -6.0, 1);
        assert!(buf[0].abs() < 1e-6);
        assert!(buf[buf.len() - 1].abs() < 1e-6);
    }
}
