//! The `left-right-balance` module: level difference between left and right.
//!
//! `zvuk generate` writes a stereo track: pink noise in the left channel, then
//! the same noise in the right, then the left one more time, separated by
//! silence. The user plays that track through the car - off a USB stick, a
//! phone, a CD - while `zvuk` records at the listening position.
//!
//! Because the tool never plays anything, it does not know when the track
//! started. It finds the bursts in the recording by their energy, which is
//! what the silences between them are for.
//!
//! Sanity check: the third burst is the left side again. If two measurements
//! of the same side disagree by more than 0.5 dB, something other than the car
//! was being measured - usually the microphone moved - and the result is
//! worthless.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::audio::signal::{self, Channel};
use crate::audio::AudioContext;
use crate::dsp::segment::{self, Burst};
use crate::dsp::{self as dsp, octave};
use crate::measurement::{Measurement, MeasurementResult, SignalParams, TestSignal};
use crate::prompt;

/// Seconds dropped from both ends of a detected burst: the fade ramps plus any
/// slop in the detected edges.
const EDGE_TRIM_S: f32 = 0.25;
/// Anything shorter than this is a door slam, not a measurement burst.
const MIN_BURST_S: f32 = 1.0;
/// How much the burst lengths may differ before the segmentation is suspect.
const LENGTH_TOLERANCE: f32 = 0.25;
/// Threshold above which two measurements of one side are irreconcilable.
const SANITY_LIMIT_DB: f32 = 0.5;
/// Broadband difference below which balancing is pointless.
const NEGLIGIBLE_DB: f32 = 0.5;
/// Spread of per-band differences above which this is not a level problem.
const TILT_WARN_DB: f32 = 4.0;
/// Fixed generator seed, so every generated track is identical.
const NOISE_SEED: u64 = 0xCAFE_BABE_1234_5678;

pub struct LeftRightBalance;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMeasurement {
    /// "left", "right" or "left-recheck".
    pub channel: String,
    /// Where the burst sat in the recording, in seconds.
    pub start_s: f32,
    pub length_s: f32,
    pub broadband_rms: f32,
    pub broadband_dbfs: f32,
    pub peak: f32,
    pub band_rms: Vec<f32>,
    pub band_dbfs: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeftRightBalanceResult {
    /// Input sample rate the analysis ran at.
    pub sample_rate: u32,
    pub capture_s: f32,
    pub band_centers_hz: Vec<f32>,
    pub left: ChannelMeasurement,
    pub right: ChannelMeasurement,
    /// Third burst, present when the generated track included the recheck.
    pub left_repeat: Option<ChannelMeasurement>,
    /// Broadband difference L - R in dB (positive means left is louder).
    pub broadband_diff_db: f32,
    pub band_diff_db: Vec<f32>,
    /// Difference between the two left-side measurements, if there were two.
    pub repeatability_db: Option<f32>,
    pub sanity_check_passed: Option<bool>,
    pub recommendation: String,
    pub warnings: Vec<String>,
}

impl Measurement for LeftRightBalance {
    fn name(&self) -> &str {
        "left-right-balance"
    }

    fn description(&self) -> &str {
        "Level difference between left and right, broadband + octave bands (125 Hz - 8 kHz)"
    }

    fn signal(&self, params: &SignalParams) -> Option<TestSignal> {
        let order: Vec<Channel> = if params.recheck {
            vec![Channel::Left, Channel::Right, Channel::Left]
        } else {
            vec![Channel::Left, Channel::Right]
        };

        let samples = signal::burst_track(
            params.sample_rate,
            params.duration_s,
            params.level_dbfs,
            params.lead_in_s,
            params.gap_s,
            &order,
            NOISE_SEED,
        );

        // Two columns: when the section starts, and how long it lasts.
        let mut layout = Vec::new();
        let mut at = 0.0f32;
        let mut section = |at: &mut f32, length: f32, what: String| {
            layout.push(format!("{:>6.1} s  {length:>4.1} s  {what}", *at));
            *at += length;
        };
        section(&mut at, params.lead_in_s, "silence (lead-in)".to_string());
        for (i, channel) in order.iter().enumerate() {
            if i > 0 {
                section(&mut at, params.gap_s, "silence".to_string());
            }
            let what = if i == 2 {
                format!(
                    "pink noise, {} channel only (repeat, for the sanity check)",
                    channel.label()
                )
            } else {
                format!("pink noise, {} channel only", channel.label())
            };
            section(&mut at, params.duration_s, what);
        }
        section(&mut at, params.gap_s, "silence (tail)".to_string());

        let instructions = vec![
            "1. Copy this file to whatever the car plays from: USB stick, phone, CD.".to_string(),
            "2. Put the microphone at the listening position and do not move it again.".to_string(),
            "3. Engine off, ventilation off, windows closed.".to_string(),
            "4. Set the volume to a normal listening level and DO NOT CHANGE IT".to_string(),
            "   while the track plays. Turn off loudness, DSP presets and any".to_string(),
            "   automatic volume that reacts to speed.".to_string(),
            "5. Set balance and fader to centre, so you measure the system and not".to_string(),
            "   the setting you are trying to find.".to_string(),
            "6. Start `zvuk` on the laptop, then press play. Sit still until the".to_string(),
            "   track ends, then stop the recording.".to_string(),
        ];

        Some(TestSignal {
            file_stem: format!("zvuk-{}", self.name()),
            sample_rate: params.sample_rate,
            channels: 2,
            samples,
            layout,
            instructions,
        })
    }

    fn run(&self, ctx: &AudioContext) -> Result<MeasurementResult> {
        println!();
        println!("== Left / right balance ==");
        println!("You need the generated track first:");
        println!("    zvuk generate --module {}", self.name());
        println!();
        println!("Before you start:");
        println!("  1) Microphone at the listening position, and DO NOT MOVE IT.");
        println!("  2) Engine off, ventilation off, windows closed.");
        println!("  3) Balance and fader centred, loudness and DSP presets off.");
        println!("  4) Volume at a normal listening level - and leave it there.");
        println!();

        let capture = ctx.record_while(|| {
            println!("Recording. Press play on the track now, then sit still.");
            prompt::wait_enter("Press Enter once the track has finished.")
        })?;

        let sample_rate = capture.sample_rate as f32;
        println!();
        println!(
            "Captured {:.1} s. Looking for the bursts...",
            capture.seconds()
        );

        let mut warnings = Vec::new();
        let mut measured = measure_bursts(&capture.samples, sample_rate, &mut warnings)?;

        let left = measured.remove(0);
        let right = measured.remove(0);
        let left_repeat = if measured.is_empty() {
            None
        } else {
            Some(measured.remove(0))
        };

        let centers: Vec<f32> = octave::OCTAVE_CENTERS_HZ.to_vec();
        let band_diff_db: Vec<f32> = left
            .band_rms
            .iter()
            .zip(right.band_rms.iter())
            .map(|(&l, &r)| dsp::db_ratio(l, r))
            .collect();
        let broadband_diff_db = dsp::db_ratio(left.broadband_rms, right.broadband_rms);

        let repeatability_db = left_repeat
            .as_ref()
            .map(|rep| dsp::db_ratio(left.broadband_rms, rep.broadband_rms).abs());
        let sanity_check_passed = repeatability_db.map(|d| d <= SANITY_LIMIT_DB);

        print_table(&centers, &left, &right, &band_diff_db, broadband_diff_db);

        match (repeatability_db, sanity_check_passed) {
            (Some(diff), Some(true)) => {
                println!();
                println!("Sanity check (left measured twice): {diff:.2} dB apart, broadband");
                println!("  OK, the measurement is repeatable (limit {SANITY_LIMIT_DB:.1} dB).");
            }
            (Some(diff), Some(false)) => {
                println!();
                println!("Sanity check (left measured twice): {diff:.2} dB apart, broadband");
                println!("  !! WARNING !!");
                println!(
                    "  Two measurements of THE SAME side differ by {diff:.2} dB, more than the \
                     {SANITY_LIMIT_DB:.1} dB limit."
                );
                println!("  The microphone, your body or the ambient noise most likely moved.");
                println!("  The L/R difference above is NOT TRUSTWORTHY - measure again.");
                warnings.push(format!(
                    "repeatability {diff:.2} dB exceeded the {SANITY_LIMIT_DB:.1} dB limit; \
                     the measurement is not trustworthy"
                ));
            }
            _ => {
                println!();
                println!("Sanity check: not available - the track had no repeat burst.");
                println!("  Generate without --no-recheck to get one.");
            }
        }

        let recommendation = build_recommendation(
            broadband_diff_db,
            &band_diff_db,
            sanity_check_passed,
            &mut warnings,
        );

        println!();
        println!("Recommendation: {recommendation}");
        if !warnings.is_empty() {
            println!();
            println!("Warnings:");
            for w in &warnings {
                println!("  - {w}");
            }
        }

        Ok(MeasurementResult::LeftRightBalance(
            LeftRightBalanceResult {
                sample_rate: capture.sample_rate,
                capture_s: capture.seconds(),
                band_centers_hz: centers,
                left,
                right,
                left_repeat,
                broadband_diff_db,
                band_diff_db,
                repeatability_db,
                sanity_check_passed,
                recommendation,
                warnings,
            },
        ))
    }
}

/// Segments a capture and measures every burst in it.
///
/// Split out of `run` so the whole chain - segmentation, trimming, band
/// analysis - can be tested against a synthetic recording.
fn measure_bursts(
    samples: &[f32],
    sample_rate: f32,
    warnings: &mut Vec<String>,
) -> Result<Vec<ChannelMeasurement>> {
    let bursts = segment::find_bursts(samples, sample_rate, MIN_BURST_S)?;
    let bursts = validate(&bursts, sample_rate)?;

    for (i, b) in bursts.iter().enumerate() {
        println!(
            "  burst {} at {:>6.1} s, {:.1} s long",
            i + 1,
            b.start as f32 / sample_rate,
            b.seconds(sample_rate)
        );
    }

    let labels = ["left", "right", "left-recheck"];
    bursts
        .iter()
        .zip(labels.iter())
        .map(|(b, label)| analyse(samples, sample_rate, *b, label, warnings))
        .collect()
}

/// Rejects a segmentation that cannot be the track we generated.
fn validate(bursts: &[Burst], sample_rate: f32) -> Result<Vec<Burst>> {
    match bursts.len() {
        0 => bail!(
            "no measurement burst found in the recording. Did the track actually play, and \
             is the selected input device the microphone in the car?"
        ),
        1 => bail!(
            "only one burst found. The recording probably started after the first one or \
             stopped before the second - start recording before pressing play, and stop it \
             after the track ends."
        ),
        2 | 3 => {}
        n => bail!(
            "found {n} loud sections, expected 2 or 3. Something else was making noise - \
             a passing car, the ventilation, a door - so the bursts cannot be told apart. \
             Measure again somewhere quieter."
        ),
    }

    let lengths: Vec<f32> = bursts.iter().map(|b| b.seconds(sample_rate)).collect();
    let longest = lengths.iter().cloned().fold(f32::MIN, f32::max);
    let shortest = lengths.iter().cloned().fold(f32::MAX, f32::min);
    if longest - shortest > longest * LENGTH_TOLERANCE {
        bail!(
            "the bursts have very different lengths ({shortest:.1} s to {longest:.1} s), so \
             they were probably not cut where the track actually changed channel. Measure \
             again with less background noise."
        );
    }

    Ok(bursts.to_vec())
}

/// Cuts one burst out of the recording and computes its levels.
fn analyse(
    samples: &[f32],
    sample_rate: f32,
    burst: Burst,
    label: &str,
    warnings: &mut Vec<String>,
) -> Result<ChannelMeasurement> {
    let trimmed = burst.trimmed(sample_rate, EDGE_TRIM_S).ok_or_else(|| {
        anyhow::anyhow!(
            "the {label} burst is too short to analyse ({:.2} s)",
            burst.seconds(sample_rate)
        )
    })?;
    let segment = &samples[trimmed.start..trimmed.end.min(samples.len())];

    let broadband_rms = dsp::rms(segment);
    let broadband_dbfs = dsp::db(broadband_rms);
    let peak = dsp::peak(segment);

    let bands = octave::octave_bands(segment, sample_rate, &octave::OCTAVE_CENTERS_HZ)?;
    let band_rms: Vec<f32> = bands.iter().map(|b| b.rms).collect();
    let band_dbfs: Vec<f32> = band_rms.iter().map(|&r| dsp::db(r)).collect();

    println!(
        "  {label:<13} {broadband_dbfs:>6.1} dBFS broadband, peak {:>6.1} dBFS",
        dsp::db(peak)
    );

    if peak >= 0.99 {
        warnings.push(format!(
            "{label}: input is clipping (peak {peak:.3}), lower the microphone gain"
        ));
        println!("   !! input is clipping - lower the gain on the microphone or interface");
    }
    if broadband_dbfs < -60.0 {
        warnings.push(format!(
            "{label}: very quiet capture ({broadband_dbfs:.1} dBFS), the result may be noise"
        ));
        println!("   !! very quiet capture - turn the car up or raise the microphone gain");
    }
    if let Some(b) = bands.iter().find(|b| b.bins == 0) {
        warnings.push(format!(
            "{label}: the {} Hz band has no FFT bin, its value is meaningless",
            b.center_hz
        ));
    }

    Ok(ChannelMeasurement {
        channel: label.to_string(),
        start_s: burst.start as f32 / sample_rate,
        length_s: burst.seconds(sample_rate),
        broadband_rms,
        broadband_dbfs,
        peak,
        band_rms,
        band_dbfs,
    })
}

fn print_table(
    centers: &[f32],
    left: &ChannelMeasurement,
    right: &ChannelMeasurement,
    band_diff_db: &[f32],
    broadband_diff_db: f32,
) {
    println!();
    println!(
        "{:>11} {:>12} {:>12} {:>12}   louder",
        "Band", "Left", "Right", "Diff"
    );
    println!("{}", "-".repeat(66));
    for (i, &center) in centers.iter().enumerate() {
        let l = left.band_dbfs.get(i).copied().unwrap_or(f32::NAN);
        let r = right.band_dbfs.get(i).copied().unwrap_or(f32::NAN);
        let d = band_diff_db.get(i).copied().unwrap_or(f32::NAN);
        println!(
            "{:>8.0} Hz {:>9.1} dB {:>9.1} dB {:>+9.1} dB   {}",
            center,
            l,
            r,
            d,
            louder_label(d)
        );
    }
    println!("{}", "-".repeat(66));
    println!(
        "{:>11} {:>9.1} dB {:>9.1} dB {:>+9.1} dB   {}",
        "broadband",
        left.broadband_dbfs,
        right.broadband_dbfs,
        broadband_diff_db,
        louder_label(broadband_diff_db)
    );
    println!();
    println!("Values are relative (dBFS at the input); do not read absolute SPL from them.");
}

fn louder_label(diff_db: f32) -> &'static str {
    if !diff_db.is_finite() {
        "?"
    } else if diff_db.abs() < NEGLIGIBLE_DB {
        "balanced"
    } else if diff_db > 0.0 {
        "left"
    } else {
        "right"
    }
}

fn build_recommendation(
    broadband_diff_db: f32,
    band_diff_db: &[f32],
    sanity_check_passed: Option<bool>,
    warnings: &mut Vec<String>,
) -> String {
    if sanity_check_passed == Some(false) {
        return "Measure again - the sanity check failed, so the numbers above cannot be trusted."
            .to_string();
    }

    let mut text = if broadband_diff_db.abs() < NEGLIGIBLE_DB {
        format!(
            "Balance is fine; the {broadband_diff_db:+.1} dB difference is below the audible \
             threshold. Change nothing."
        )
    } else if broadband_diff_db > 0.0 {
        format!(
            "The left side is {0:.1} dB louder. Cut the left channel by {0:.1} dB (or add \
             {0:.1} dB to the right).",
            broadband_diff_db
        )
    } else {
        let d = broadband_diff_db.abs();
        format!(
            "The right side is {d:.1} dB louder. Cut the right channel by {d:.1} dB (or add \
             {d:.1} dB to the left)."
        )
    };

    let finite: Vec<f32> = band_diff_db
        .iter()
        .copied()
        .filter(|d| d.is_finite())
        .collect();
    if finite.len() >= 2 {
        let max = finite.iter().cloned().fold(f32::MIN, f32::max);
        let min = finite.iter().cloned().fold(f32::MAX, f32::min);
        let spread = max - min;
        if spread > TILT_WARN_DB {
            warnings.push(format!("per-band differences span {spread:.1} dB"));
            text.push(' ');
            text.push_str(&format!(
                "The differences span {spread:.1} dB across the bands. That is not a level \
                 problem but different speaker placement, reflections, or a different driver \
                 on one side. Gain alone will not fix it."
            ));
        }
    }

    if sanity_check_passed.is_none() {
        text.push_str(" (No repeat burst in the track, so repeatability is unverified.)");
    }

    text
}

/// Prints a stored result; used by the `zvuk show` subcommand.
pub fn print_result(result: &LeftRightBalanceResult) {
    print_table(
        &result.band_centers_hz,
        &result.left,
        &result.right,
        &result.band_diff_db,
        result.broadband_diff_db,
    );
    if let Some(diff) = result.repeatability_db {
        let verdict = match result.sanity_check_passed {
            Some(true) => "OK",
            Some(false) => "FAILED - measurement is not trustworthy",
            None => "?",
        };
        println!("Sanity check: {diff:.2} dB apart, broadband -> {verdict}");
    } else {
        println!("Sanity check: not available (no repeat burst in the track)");
    }
    println!("Recommendation: {}", result.recommendation);
    if !result.warnings.is_empty() {
        println!("Warnings:");
        for w in &result.warnings {
            println!("  - {w}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn burst(start_s: f32, len_s: f32) -> Burst {
        Burst {
            start: (start_s * FS) as usize,
            end: ((start_s + len_s) * FS) as usize,
        }
    }

    #[test]
    fn two_or_three_equal_bursts_are_accepted() {
        assert!(validate(&[burst(5.0, 3.0), burst(10.0, 3.0)], FS).is_ok());
        assert!(validate(&[burst(5.0, 3.0), burst(10.0, 3.0), burst(15.0, 3.0)], FS).is_ok());
    }

    #[test]
    fn a_missing_burst_is_explained_not_silently_accepted() {
        let err = validate(&[burst(5.0, 3.0)], FS).unwrap_err().to_string();
        assert!(err.contains("only one burst"), "unexpected error: {err}");

        let err = validate(&[], FS).unwrap_err().to_string();
        assert!(
            err.contains("no measurement burst"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn extra_noise_sections_are_rejected() {
        let bursts = [
            burst(5.0, 3.0),
            burst(10.0, 3.0),
            burst(15.0, 3.0),
            burst(20.0, 3.0),
        ];
        let err = validate(&bursts, FS).unwrap_err().to_string();
        assert!(err.contains("4 loud sections"), "unexpected error: {err}");
    }

    /// Simulates a microphone in a car: it hears both speakers summed, each
    /// with its own gain, on top of a faint background.
    fn fake_recording(left_gain: f32, right_gain: f32) -> Vec<f32> {
        let track = signal::burst_track(
            FS as u32,
            2.0,
            -6.0,
            1.0,
            1.0,
            &[Channel::Left, Channel::Right, Channel::Left],
            NOISE_SEED,
        );
        let mut state = 0x5EED_1234_ABCD_9876u64;
        track
            .chunks(2)
            .map(|frame| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                let background =
                    ((state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / 8_388_608.0) - 1.0;
                frame[0] * left_gain + frame[1] * right_gain + background * 0.0005
            })
            .collect()
    }

    /// The end-to-end check: generate a track, pretend to record it through a
    /// system that is 2 dB louder on the left, and see the analysis say so.
    #[test]
    fn a_known_imbalance_comes_back_out_of_the_whole_chain() {
        let imbalance_db = 2.0f32;
        let samples = fake_recording(1.0, 10f32.powf(-imbalance_db / 20.0));

        let mut warnings = Vec::new();
        let measured = measure_bursts(&samples, FS, &mut warnings).unwrap();

        assert_eq!(measured.len(), 3, "expected left, right and the recheck");
        assert_eq!(measured[0].channel, "left");
        assert_eq!(measured[1].channel, "right");
        assert_eq!(measured[2].channel, "left-recheck");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

        let broadband = dsp::db_ratio(measured[0].broadband_rms, measured[1].broadband_rms);
        assert!(
            (broadband - imbalance_db).abs() < 0.1,
            "broadband difference came out as {broadband:.2} dB, expected {imbalance_db:.2} dB"
        );

        for (i, &center) in octave::OCTAVE_CENTERS_HZ.iter().enumerate() {
            let diff = dsp::db_ratio(measured[0].band_rms[i], measured[1].band_rms[i]);
            assert!(
                (diff - imbalance_db).abs() < 0.2,
                "band {center} Hz came out as {diff:.2} dB, expected {imbalance_db:.2} dB"
            );
        }

        // Both left bursts are the same signal at the same gain, so the sanity
        // check must be comfortably inside its limit.
        let repeat = dsp::db_ratio(measured[0].broadband_rms, measured[2].broadband_rms).abs();
        assert!(
            repeat < 0.1,
            "repeatability came out as {repeat:.2} dB on identical bursts"
        );
    }

    #[test]
    fn a_balanced_system_reads_as_balanced() {
        let samples = fake_recording(1.0, 1.0);
        let mut warnings = Vec::new();
        let measured = measure_bursts(&samples, FS, &mut warnings).unwrap();
        let broadband = dsp::db_ratio(measured[0].broadband_rms, measured[1].broadband_rms);
        assert!(
            broadband.abs() < 0.05,
            "expected 0 dB, got {broadband:.3} dB"
        );
        assert_eq!(louder_label(broadband), "balanced");
    }

    #[test]
    fn wildly_uneven_bursts_are_rejected() {
        let err = validate(&[burst(5.0, 3.0), burst(10.0, 1.5)], FS)
            .unwrap_err()
            .to_string();
        assert!(err.contains("different lengths"), "unexpected error: {err}");
    }
}
