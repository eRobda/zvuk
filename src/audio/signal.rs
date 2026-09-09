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

/// Which output channel a burst is placed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Left,
    Right,
}

impl Channel {
    fn index(self) -> usize {
        match self {
            Channel::Left => 0,
            Channel::Right => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Channel::Left => "left",
            Channel::Right => "right",
        }
    }
}

/// Builds the stereo track the user plays through the car: lead-in silence,
/// then one burst per entry in `order`, separated by silence.
///
/// Every burst is the **same** pink noise buffer, so the sides are compared
/// like with like rather than against two different noise realisations.
pub fn burst_track(
    sample_rate: u32,
    duration_s: f32,
    level_dbfs: f32,
    lead_in_s: f32,
    gap_s: f32,
    order: &[Channel],
    seed: u64,
) -> Vec<f32> {
    const CHANNELS: usize = 2;

    let burst = pink_noise_burst(sample_rate, duration_s, level_dbfs, seed);
    let silence = |seconds: f32| vec![0.0f32; frames(sample_rate, seconds) * CHANNELS];

    let mut track = silence(lead_in_s.max(0.0));
    for (i, channel) in order.iter().enumerate() {
        if i > 0 {
            track.extend_from_slice(&silence(gap_s.max(0.0)));
        }
        for &s in &burst {
            let mut frame = [0.0f32; CHANNELS];
            frame[channel.index()] = s;
            track.extend_from_slice(&frame);
        }
    }
    // Trailing silence, so a player that stops abruptly does not clip the last
    // burst and the analysis still sees a gap after it.
    track.extend_from_slice(&silence(gap_s.max(0.0)));
    track
}

fn frames(sample_rate: u32, seconds: f32) -> usize {
    ((sample_rate as f32) * seconds).round().max(0.0) as usize
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

    #[test]
    fn burst_track_has_the_expected_length_and_channel_placement() {
        let fs = 48_000;
        let track = burst_track(
            fs,
            1.0,
            -6.0,
            2.0,
            0.5,
            &[Channel::Left, Channel::Right, Channel::Left],
            1,
        );

        // lead-in + 3 bursts + 2 inner gaps + trailing gap, times two channels.
        let expected_frames = (2.0 + 3.0 * 1.0 + 2.0 * 0.5 + 0.5) * fs as f32;
        assert_eq!(track.len(), expected_frames as usize * 2);

        let energy = |channel: usize, from_s: f32, to_s: f32| -> f32 {
            let from = (from_s * fs as f32) as usize * 2 + channel;
            let to = (to_s * fs as f32) as usize * 2 + channel;
            track[from..to].iter().step_by(2).map(|s| s.abs()).sum()
        };

        // Layout: 2.0 s silence | 1.0 s left | 0.5 s | 1.0 s right | 0.5 s |
        //         1.0 s left | 0.5 s silence.
        assert!(
            energy(0, 2.1, 2.9) > 0.0,
            "first burst missing from the left channel"
        );
        assert_eq!(
            energy(1, 2.1, 2.9),
            0.0,
            "first burst leaked into the right channel"
        );
        assert!(
            energy(1, 3.6, 4.4) > 0.0,
            "second burst missing from the right channel"
        );
        assert_eq!(
            energy(0, 3.6, 4.4),
            0.0,
            "second burst leaked into the left channel"
        );
        assert!(
            energy(0, 5.1, 5.9) > 0.0,
            "third burst missing from the left channel"
        );
        assert_eq!(
            energy(1, 5.1, 5.9),
            0.0,
            "third burst leaked into the right channel"
        );
    }

    #[test]
    fn every_burst_carries_the_identical_signal() {
        let fs = 8_000;
        let track = burst_track(fs, 1.0, -6.0, 0.0, 0.5, &[Channel::Left, Channel::Right], 3);
        let first: Vec<f32> = track[0..fs as usize * 2]
            .iter()
            .step_by(2)
            .copied()
            .collect();
        let offset = (1.5 * fs as f32) as usize * 2 + 1;
        let second: Vec<f32> = track[offset..offset + fs as usize * 2]
            .iter()
            .step_by(2)
            .copied()
            .collect();
        assert_eq!(first, second);
    }
}
