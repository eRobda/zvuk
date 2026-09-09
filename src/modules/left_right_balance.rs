//! The `left-right-balance` module: level difference between left and right.
//!
//! Pink noise (the same buffer for both sides, so we compare like with like) is
//! played into the left channel only, then into the right, while the microphone
//! records at the listening position. The steady-state part of each capture
//! yields a broadband RMS and per-octave-band RMS values computed with an FFT.
//!
//! Sanity check: optionally the left side is measured a second time at the end.
//! If two measurements of the same side disagree by more than 0.5 dB, something
//! other than the car was being measured - usually the microphone moved - and
//! the result is worthless.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::audio::{signal, AudioContext};
use crate::dsp::{self, octave};
use crate::measurement::{Measurement, MeasurementResult};
use crate::prompt;

/// Seconds dropped from each end of the capture (output latency, room decay).
const GUARD_S: f32 = 0.4;
/// How long recording continues after the signal has finished.
const TAIL_S: f32 = 0.4;
/// Threshold above which two measurements of one side are irreconcilable.
const SANITY_LIMIT_DB: f32 = 0.5;
/// Broadband difference below which balancing is pointless.
const NEGLIGIBLE_DB: f32 = 0.5;
/// Spread of per-band differences above which this is not a level problem.
const TILT_WARN_DB: f32 = 4.0;
/// Fixed generator seed, so measurements are reproducible.
const NOISE_SEED: u64 = 0xCAFE_BABE_1234_5678;

pub struct LeftRightBalance;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMeasurement {
    /// "left", "right" or "left-recheck".
    pub channel: String,
    pub broadband_rms: f32,
    pub broadband_dbfs: f32,
    pub peak: f32,
    pub band_rms: Vec<f32>,
    pub band_dbfs: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeftRightBalanceResult {
    pub duration_s: f32,
    pub level_dbfs: f32,
    /// Input sample rate the analysis ran at.
    pub sample_rate: u32,
    pub band_centers_hz: Vec<f32>,
    pub left: ChannelMeasurement,
    pub right: ChannelMeasurement,
    /// Second measurement of the left side, if the sanity check was enabled.
    pub left_repeat: Option<ChannelMeasurement>,
    /// Broadband difference L - R in dB (positive means left is louder).
    pub broadband_diff_db: f32,
    pub band_diff_db: Vec<f32>,
    /// Difference between the two left-side measurements, if one was taken.
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

    fn run(&self, ctx: &AudioContext) -> Result<MeasurementResult> {
        if ctx.output.channels() < 2 {
            bail!(
                "output device has only {} channel(s); an L/R measurement needs at least 2",
                ctx.output.channels()
            );
        }

        println!();
        println!("== Left / right balance ==");
        println!("Before you start:");
        println!("  1) Put the microphone at the listening position and DO NOT MOVE IT.");
        println!("  2) Engine off, ventilation off, windows closed.");
        println!("  3) Stay quiet and still while it runs.");
        println!(
            "  Pink noise will play for {:.1} s per channel at {:.1} dBFS peak.",
            ctx.params.duration_s, ctx.params.level_dbfs
        );
        println!();

        let sanity = prompt::confirm(
            "Measure the left side twice to verify the result is repeatable (recommended)?",
            true,
        )?;
        if !sanity {
            println!("  Sanity check skipped. Trust the result only as far as you trust");
            println!("  that neither the microphone nor the car moved during the run.");
        }

        prompt::wait_enter("Press Enter once it is quiet and the microphone is in place.")?;

        // The same signal for both sides, otherwise we would not be comparing
        // the same thing twice.
        let noise = signal::pink_noise_burst(
            ctx.output.sample_rate(),
            ctx.params.duration_s,
            ctx.params.level_dbfs,
            NOISE_SEED,
        );

        let mut warnings = Vec::new();

        let left = capture(ctx, &noise, 0, "left", &mut warnings)?;
        let right = capture(ctx, &noise, 1, "right", &mut warnings)?;
        let left_repeat = if sanity {
            Some(capture(ctx, &noise, 0, "left-recheck", &mut warnings)?)
        } else {
            None
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

        if let (Some(diff), Some(passed)) = (repeatability_db, sanity_check_passed) {
            println!();
            println!("Sanity check (left channel measured twice): {diff:.2} dB apart, broadband");
            if passed {
                println!("  OK, the measurement is repeatable (limit {SANITY_LIMIT_DB:.1} dB).");
            } else {
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
                duration_s: ctx.params.duration_s,
                level_dbfs: ctx.params.level_dbfs,
                sample_rate: ctx.input.sample_rate(),
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

/// Plays noise into one channel, records the microphone and computes levels.
fn capture(
    ctx: &AudioContext,
    noise: &[f32],
    channel: usize,
    label: &str,
    warnings: &mut Vec<String>,
) -> Result<ChannelMeasurement> {
    println!();
    println!("-> Measuring the {label} channel, hold still...");

    let interleaved = ctx.mono_to_channel(noise, channel)?;
    let recording = ctx.play_and_record(&interleaved, TAIL_S)?;
    let segment = recording.steady_state(GUARD_S)?;

    let broadband_rms = dsp::rms(segment);
    let broadband_dbfs = dsp::db(broadband_rms);
    let peak = dsp::peak(segment);

    let bands = octave::octave_bands(
        segment,
        recording.sample_rate as f32,
        &octave::OCTAVE_CENTERS_HZ,
    )?;
    let band_rms: Vec<f32> = bands.iter().map(|b| b.rms).collect();
    let band_dbfs: Vec<f32> = band_rms.iter().map(|&r| dsp::db(r)).collect();

    println!(
        "   done: {broadband_dbfs:.1} dBFS broadband, peak {:.1} dBFS, {} samples analysed",
        dsp::db(peak),
        segment.len()
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
        println!("   !! very quiet capture - raise the volume or the microphone gain");
    }
    if let Some(b) = bands.iter().find(|b| b.bins == 0) {
        warnings.push(format!(
            "{label}: the {} Hz band has no FFT bin, its value is meaningless",
            b.center_hz
        ));
    }

    Ok(ChannelMeasurement {
        channel: label.to_string(),
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
        text.push_str(" (No sanity check was run, so repeatability is unverified.)");
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
        println!("Sanity check: not run");
    }
    println!("Recommendation: {}", result.recommendation);
    if !result.warnings.is_empty() {
        println!("Warnings:");
        for w in &result.warnings {
            println!("  - {w}");
        }
    }
}
