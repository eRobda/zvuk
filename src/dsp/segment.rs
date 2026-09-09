//! Finding the measurement bursts inside a recording.
//!
//! The tool does not play anything, so it has no idea when the user hit play.
//! What it does know is the shape of the track it generated: loud bursts of
//! equal length separated by silence. Short-term RMS turns that into a simple
//! threshold problem.

use anyhow::{bail, Result};

/// Analysis frame length. Long enough to average out the noise waveform, short
/// enough to place a burst edge within a few tens of milliseconds.
const FRAME_S: f32 = 0.020;
/// Gaps shorter than this inside one burst are bridged rather than splitting it.
const BRIDGE_S: f32 = 0.5;
/// Minimum signal-to-noise ratio for the segmentation to mean anything.
const MIN_SNR_DB: f32 = 12.0;

/// A detected burst, as a half-open sample range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Burst {
    pub start: usize,
    pub end: usize,
}

impl Burst {
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Present because clippy asks for it next to `len`.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn seconds(&self, sample_rate: f32) -> f32 {
        self.len() as f32 / sample_rate
    }

    /// Drops `seconds` from both ends, which removes the fade ramps and any
    /// slop in the detected edges.
    pub fn trimmed(&self, sample_rate: f32, seconds: f32) -> Option<Burst> {
        let margin = (seconds * sample_rate) as usize;
        let start = self.start + margin;
        let end = self.end.checked_sub(margin)?;
        if end <= start {
            return None;
        }
        Some(Burst { start, end })
    }
}

/// Finds bursts at least `min_len_s` long.
///
/// The threshold sits halfway, in dB, between the noise floor and the loudest
/// frame, which adapts to whatever level the user happened to play at.
pub fn find_bursts(signal: &[f32], sample_rate: f32, min_len_s: f32) -> Result<Vec<Burst>> {
    if sample_rate <= 0.0 {
        bail!("invalid sample rate");
    }
    let frame = ((FRAME_S * sample_rate) as usize).max(16);
    let hop = (frame / 2).max(1);
    if signal.len() < frame * 4 {
        bail!(
            "recording is too short to contain anything ({} samples)",
            signal.len()
        );
    }

    let levels: Vec<f32> = signal
        .chunks(hop)
        .enumerate()
        .map(|(i, _)| {
            let start = i * hop;
            let end = (start + frame).min(signal.len());
            frame_db(&signal[start..end])
        })
        .collect();

    let loudest = levels.iter().cloned().fold(f32::MIN, f32::max);
    let floor = percentile(&levels, 0.10);

    if loudest < -80.0 {
        bail!("the recording is silent - check that the microphone is the selected input device");
    }
    if loudest - floor < MIN_SNR_DB {
        bail!(
            "the loudest part of the recording is only {:.1} dB above the background \
             ({MIN_SNR_DB:.0} dB needed). Turn the system up, move the microphone closer, \
             or find somewhere quieter.",
            loudest - floor
        );
    }

    let threshold = (loudest + floor) / 2.0;
    let bridge_frames = ((BRIDGE_S * sample_rate) / hop as f32) as usize;
    let min_len = (min_len_s * sample_rate) as usize;

    let mut bursts: Vec<Burst> = Vec::new();
    let mut open: Option<usize> = None;
    let mut quiet_since: Option<usize> = None;

    for (i, &level) in levels.iter().enumerate() {
        if level >= threshold {
            quiet_since = None;
            if open.is_none() {
                open = Some(i);
            }
        } else if let Some(start_frame) = open {
            let quiet_start = *quiet_since.get_or_insert(i);
            if i - quiet_start >= bridge_frames {
                bursts.push(frames_to_burst(
                    start_frame,
                    quiet_start,
                    hop,
                    frame,
                    signal.len(),
                ));
                open = None;
                quiet_since = None;
            }
        }
    }
    if let Some(start_frame) = open {
        let end_frame = quiet_since.unwrap_or(levels.len());
        bursts.push(frames_to_burst(
            start_frame,
            end_frame,
            hop,
            frame,
            signal.len(),
        ));
    }

    bursts.retain(|b| b.len() >= min_len);
    Ok(bursts)
}

fn frames_to_burst(
    start_frame: usize,
    end_frame: usize,
    hop: usize,
    frame: usize,
    total: usize,
) -> Burst {
    Burst {
        start: (start_frame * hop).min(total),
        end: (end_frame * hop + frame).min(total),
    }
}

fn frame_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -240.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum / samples.len() as f64).sqrt() as f32;
    20.0 * rms.max(1e-12).log10()
}

/// Linear-interpolation-free percentile; good enough for a noise floor estimate.
fn percentile(values: &[f32], fraction: f32) -> f32 {
    if values.is_empty() {
        return -240.0;
    }
    let mut sorted: Vec<f32> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((sorted.len() - 1) as f32 * fraction.clamp(0.0, 1.0)).round() as usize;
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    /// Builds silence with a faint noise floor and `count` louder bursts in it.
    fn track(count: usize, burst_s: f32, gap_s: f32, lead_s: f32, snr_db: f32) -> Vec<f32> {
        let mut state = 0xDEAD_BEEF_1234_5678u64;
        let mut noise = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            ((state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / 8_388_608.0) - 1.0
        };
        let floor = 0.001f32;
        let burst = floor * 10f32.powf(snr_db / 20.0);

        let mut out = Vec::new();
        let mut push = |seconds: f32, amplitude: f32, noise: &mut dyn FnMut() -> f32| {
            for _ in 0..((seconds * FS) as usize) {
                out.push(noise() * amplitude);
            }
        };
        push(lead_s, floor, &mut noise);
        for i in 0..count {
            if i > 0 {
                push(gap_s, floor, &mut noise);
            }
            push(burst_s, burst, &mut noise);
        }
        push(gap_s, floor, &mut noise);
        out
    }

    #[test]
    fn finds_three_bursts_of_the_right_length() {
        let signal = track(3, 3.0, 2.0, 5.0, 30.0);
        let bursts = find_bursts(&signal, FS, 1.0).unwrap();

        assert_eq!(bursts.len(), 3, "expected three bursts, got {bursts:?}");
        for b in &bursts {
            let seconds = b.seconds(FS);
            assert!(
                (seconds - 3.0).abs() < 0.2,
                "burst is {seconds:.2} s, expected about 3.0 s"
            );
        }
        // The first burst must start around the end of the 5 s lead-in.
        let first_start = bursts[0].start as f32 / FS;
        assert!(
            (first_start - 5.0).abs() < 0.2,
            "first burst starts at {first_start:.2} s, expected about 5.0 s"
        );
    }

    #[test]
    fn finds_two_bursts_when_the_recheck_is_omitted() {
        let signal = track(2, 3.0, 2.0, 1.0, 25.0);
        assert_eq!(find_bursts(&signal, FS, 1.0).unwrap().len(), 2);
    }

    #[test]
    fn short_blips_are_not_bursts() {
        let signal = track(3, 0.2, 2.0, 1.0, 30.0);
        assert!(find_bursts(&signal, FS, 1.0).unwrap().is_empty());
    }

    #[test]
    fn a_recording_that_is_all_background_is_rejected() {
        let signal = track(1, 3.0, 2.0, 1.0, 2.0);
        let err = find_bursts(&signal, FS, 1.0).unwrap_err().to_string();
        assert!(
            err.contains("above the background"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn silence_is_rejected_with_a_useful_message() {
        let signal = vec![0.0f32; 48_000];
        let err = find_bursts(&signal, FS, 1.0).unwrap_err().to_string();
        assert!(err.contains("silent"), "unexpected error: {err}");
    }

    #[test]
    fn trimming_removes_the_edges_and_refuses_to_invert() {
        let b = Burst {
            start: 1000,
            end: 5000,
        };
        let trimmed = b.trimmed(FS, 0.01).unwrap();
        assert_eq!(trimmed.start, 1480);
        assert_eq!(trimmed.end, 4520);
        assert!(b.trimmed(FS, 1.0).is_none());
    }
}
