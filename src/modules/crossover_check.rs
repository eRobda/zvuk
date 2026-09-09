//! The `crossover-check` module: where two speaker groups hand over, and
//! whether their levels match when they do.
//!
//! You measure the same track three times with the same microphone and the
//! same volume, changing only what is allowed to play:
//!
//! 1. group A alone - typically the door speakers, sub muted,
//! 2. group B alone - typically the sub, doors muted,
//! 3. group A again.
//!
//! From A and B you get each group's working bandwidth, the frequency where
//! they cross, whether the crossing leaves a hole, and how their levels
//! compare - the gain staging. The third pass exists because all of that is
//! only meaningful if nothing moved and, above all, if the volume knob did not
//! move between passes. If A and A-again disagree, they did.
//!
//! Everything is in thirds of an octave from 20 Hz, because a subwoofer
//! crosses somewhere around 80 Hz and whole octaves put their nearest centre
//! at 125 Hz.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use crate::audio::signal::{self, Channel};
use crate::audio::AudioContext;
use crate::dsp::segment;
use crate::dsp::{self as dsp, octave};
use crate::measurement::{Measurement, MeasurementResult, SignalParams, TestSignal};
use crate::prompt;

/// Seconds dropped from both ends of a detected burst.
const EDGE_TRIM_S: f32 = 0.25;
/// Anything shorter than this is not a measurement burst.
const MIN_BURST_S: f32 = 1.0;
/// Third-octave resolution at 20 Hz needs a long transform, which needs a long
/// burst to average over. Shorter than this and the bottom bands are guesswork.
const MIN_DURATION_S: f32 = 4.0;
/// Broadband disagreement between the two passes of group A that we tolerate.
const SANITY_LIMIT_DB: f32 = 0.5;
/// Drop from the passband that defines the usable band edge.
const CORNER_DROP_DB: f32 = 6.0;
/// A second, deeper edge, for how far the driver still contributes at all.
const DEEP_DROP_DB: f32 = 10.0;
/// A dip in the summed response bigger than this is a hole worth fixing.
const DIP_WARN_DB: f32 = 3.0;
/// Level difference between the groups worth mentioning.
const GAIN_NOTE_DB: f32 = 3.0;
/// Bands with fewer bins than this are too coarsely resolved to trust.
const MIN_BINS: usize = 3;
/// Fixed generator seed, so every generated track is identical.
const NOISE_SEED: u64 = 0x0CE7_A5E1_9B3D_4F62;

pub struct CrossoverCheck;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetMeasurement {
    /// What the user called this group, e.g. "doors" or "sub".
    pub name: String,
    pub start_s: f32,
    pub length_s: f32,
    pub broadband_dbfs: f32,
    pub peak: f32,
    /// Level per third-octave band, in dBFS.
    pub band_dbfs: Vec<f32>,
    /// FFT bins that fell into each band; low numbers mean low confidence.
    pub band_bins: Vec<usize>,
    /// Reference level the band edges are measured from: the peak of the
    /// curve after smoothing over three bands.
    pub passband_ref_dbfs: f32,
    pub passband_peak_hz: f32,
    /// Frequencies where the response falls `CORNER_DROP_DB` below the
    /// reference, below and above the peak.
    pub corner_low_hz: Option<f32>,
    pub corner_high_hz: Option<f32>,
    /// The same at `DEEP_DROP_DB`.
    pub deep_low_hz: Option<f32>,
    pub deep_high_hz: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossoverCheckResult {
    pub sample_rate: u32,
    pub band_centers_hz: Vec<f32>,
    pub bands_per_octave: u32,
    pub set_a: SetMeasurement,
    pub set_b: SetMeasurement,
    pub set_a_repeat: Option<SetMeasurement>,
    /// Where the two responses cross, if they do.
    pub crossover_hz: Option<f32>,
    /// How far each group sits below its own passband at that frequency.
    /// Textbook is about -6 dB for both.
    pub crossover_a_rel_db: Option<f32>,
    pub crossover_b_rel_db: Option<f32>,
    /// Power sum of the two responses, band by band.
    pub combined_dbfs: Vec<f32>,
    /// How deep the summed response dips around the crossover. Positive is a
    /// hole. Magnitude only - phase can make the real sum worse.
    pub combined_dip_db: Option<f32>,
    /// Passband level of A minus passband level of B: the gain staging.
    pub level_offset_db: f32,
    pub repeatability_db: Option<f32>,
    pub sanity_check_passed: Option<bool>,
    pub recommendation: String,
    pub warnings: Vec<String>,
}

impl Measurement for CrossoverCheck {
    fn name(&self) -> &str {
        "crossover-check"
    }

    fn description(&self) -> &str {
        "Bandwidth of two speaker groups, where they cross, and how their levels compare"
    }

    fn signal(&self, params: &SignalParams) -> Option<TestSignal> {
        let duration = params.duration_s.max(MIN_DURATION_S);

        let samples = signal::burst_track(
            params.sample_rate,
            duration,
            params.level_dbfs,
            params.lead_in_s,
            params.gap_s,
            &[Channel::Both],
            NOISE_SEED,
        );

        let layout = vec![
            format!(
                "{:>6.1} s  {:>4.1} s  silence (lead-in)",
                0.0, params.lead_in_s
            ),
            format!(
                "{:>6.1} s  {duration:>4.1} s  pink noise, both channels",
                params.lead_in_s
            ),
            format!(
                "{:>6.1} s  {:>4.1} s  silence (tail)",
                params.lead_in_s + duration,
                params.gap_s
            ),
        ];

        let instructions = vec![
            "This track is played THREE times, with the system set up differently".to_string(),
            "each time. zvuk prompts you before each pass.".to_string(),
            "".to_string(),
            "  pass 1: group A only  (e.g. doors playing, sub muted)".to_string(),
            "  pass 2: group B only  (e.g. sub playing, doors muted)".to_string(),
            "  pass 3: group A again (the same as pass 1)".to_string(),
            "".to_string(),
            "1. Copy this file to whatever the car plays from: USB stick, phone, CD.".to_string(),
            "2. Microphone where the driver's head sits. Do not move it, not even".to_string(),
            "   between passes - that is what pass 3 checks.".to_string(),
            "3. Engine off, ventilation off, windows closed.".to_string(),
            "4. DO NOT TOUCH THE VOLUME between passes. Comparing the levels of the".to_string(),
            "   two groups is the whole point, and the volume knob destroys it.".to_string(),
            "5. Mute a group at the amplifier or with fader/sub level - whatever".to_string(),
            "   your system offers - but leave every gain and EQ setting alone.".to_string(),
            "6. Turn off loudness and any automatic volume that reacts to speed.".to_string(),
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
        println!("== Crossover and gain staging ==");
        println!("You need the generated track first:");
        println!("    zvuk generate --module {}", self.name());
        println!();
        println!("The same track gets played three times, with the system set up");
        println!("differently each time. Between passes, change ONLY which speakers");
        println!("are allowed to play - never the volume.");
        println!();

        let name_a = ask_name("Name for group A", "doors")?;
        let name_b = ask_name("Name for group B", "sub")?;

        let mut warnings = Vec::new();

        let set_a = record_pass(
            ctx,
            &name_a,
            &format!("Set the system so that ONLY '{name_a}' plays."),
            &mut warnings,
        )?;
        let set_b = record_pass(
            ctx,
            &name_b,
            &format!("Now set the system so that ONLY '{name_b}' plays. Do NOT touch the volume."),
            &mut warnings,
        )?;
        let set_a_repeat = record_pass(
            ctx,
            &format!("{name_a}-recheck"),
            &format!("Last pass: set it back to ONLY '{name_a}', exactly as in pass 1."),
            &mut warnings,
        )?;

        let centers: Vec<f32> = octave::THIRD_OCTAVE_CENTERS_HZ.to_vec();
        let combined_dbfs = power_sum(&set_a.band_dbfs, &set_b.band_dbfs);

        let crossing = find_crossover(&centers, &set_a, &set_b);
        let (crossover_hz, crossover_a_rel_db, crossover_b_rel_db) = match crossing {
            Some(hz) => (
                Some(hz),
                interpolate_at(&centers, &set_a.band_dbfs, hz).map(|l| l - set_a.passband_ref_dbfs),
                interpolate_at(&centers, &set_b.band_dbfs, hz).map(|l| l - set_b.passband_ref_dbfs),
            ),
            None => (None, None, None),
        };
        let combined_dip_db = crossover_hz.and_then(|hz| dip_around(&centers, &combined_dbfs, hz));
        let level_offset_db = set_a.passband_ref_dbfs - set_b.passband_ref_dbfs;

        let repeatability_db = Some((set_a.broadband_dbfs - set_a_repeat.broadband_dbfs).abs());
        let sanity_check_passed = repeatability_db.map(|d| d <= SANITY_LIMIT_DB);

        print_table(&centers, &set_a, &set_b, &combined_dbfs, crossover_hz);
        print_bandwidth(&set_a);
        print_bandwidth(&set_b);

        if let (Some(diff), Some(passed)) = (repeatability_db, sanity_check_passed) {
            println!();
            println!("Sanity check ('{name_a}' measured twice): {diff:.2} dB apart, broadband");
            if passed {
                println!("  OK, nothing moved between passes (limit {SANITY_LIMIT_DB:.1} dB).");
            } else {
                println!("  !! WARNING !!");
                println!(
                    "  The two passes of '{name_a}' differ by {diff:.2} dB, more than the \
                     {SANITY_LIMIT_DB:.1} dB limit."
                );
                println!("  The volume, the microphone or the setup changed along the way, so");
                println!("  comparing '{name_a}' with '{name_b}' is meaningless. Measure again.");
                warnings.push(format!(
                    "repeatability {diff:.2} dB exceeded the {SANITY_LIMIT_DB:.1} dB limit; \
                     the comparison between the two groups is not trustworthy"
                ));
            }
        }

        let recommendation = build_recommendation(
            &set_a,
            &set_b,
            crossover_hz,
            crossover_a_rel_db,
            crossover_b_rel_db,
            combined_dip_db,
            level_offset_db,
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

        Ok(MeasurementResult::CrossoverCheck(CrossoverCheckResult {
            sample_rate: ctx.input.sample_rate(),
            band_centers_hz: centers,
            bands_per_octave: 3,
            set_a,
            set_b,
            set_a_repeat: Some(set_a_repeat),
            crossover_hz,
            crossover_a_rel_db,
            crossover_b_rel_db,
            combined_dbfs,
            combined_dip_db,
            level_offset_db,
            repeatability_db,
            sanity_check_passed,
            recommendation,
            warnings,
        }))
    }
}

fn ask_name(question: &str, default: &str) -> Result<String> {
    let answer = prompt::read_line(&format!("{question} [{default}]: "))?;
    Ok(if answer.is_empty() {
        default.to_string()
    } else {
        answer
    })
}

/// Records one pass and turns it into a third-octave response.
fn record_pass(
    ctx: &AudioContext,
    name: &str,
    setup: &str,
    warnings: &mut Vec<String>,
) -> Result<SetMeasurement> {
    println!();
    println!("--- {name} ---");
    println!("{setup}");
    prompt::wait_enter("Press Enter when the system is set up, then play the track.")?;

    let capture = ctx.record_while(|| {
        println!("Recording. Play the track now, then sit still.");
        prompt::wait_enter("Press Enter once the track has finished.")
    })?;

    let sample_rate = capture.sample_rate as f32;
    let bursts = segment::find_bursts(&capture.samples, sample_rate, MIN_BURST_S)?;
    let burst = match bursts.len() {
        1 => bursts[0],
        0 => bail!(
            "no burst found in the '{name}' pass. Did the track actually play, and is the \
             group you meant to mute really muted?"
        ),
        n => bail!(
            "found {n} bursts in the '{name}' pass, expected one. The track was played more \
             than once, or something else made a noise. Measure that pass again."
        ),
    };

    let trimmed = burst
        .trimmed(sample_rate, EDGE_TRIM_S)
        .ok_or_else(|| anyhow!("the '{name}' burst is too short to analyse"))?;
    let segment_samples = &capture.samples[trimmed.start..trimmed.end.min(capture.samples.len())];

    let broadband_dbfs = dsp::db(dsp::rms(segment_samples));
    let peak = dsp::peak(segment_samples);

    let bands = octave::fractional_octave_bands(
        segment_samples,
        sample_rate,
        &octave::THIRD_OCTAVE_CENTERS_HZ,
        3,
        octave::THIRD_OCTAVE_FFT_LEN,
    )?;
    let band_dbfs: Vec<f32> = bands.iter().map(|b| dsp::db(b.rms)).collect();
    let band_bins: Vec<usize> = bands.iter().map(|b| b.bins).collect();

    println!(
        "  captured {:.1} s, burst at {:.1} s: {broadband_dbfs:.1} dBFS broadband, peak {:.1} dBFS",
        capture.seconds(),
        burst.start as f32 / sample_rate,
        dsp::db(peak)
    );

    if peak >= 0.99 {
        warnings.push(format!(
            "{name}: input is clipping (peak {peak:.3}), lower the microphone gain"
        ));
        println!("   !! input is clipping - lower the gain on the microphone or interface");
    }
    if broadband_dbfs < -60.0 {
        warnings.push(format!(
            "{name}: very quiet capture ({broadband_dbfs:.1} dBFS), the result may be noise"
        ));
        println!("   !! very quiet capture - turn the car up or raise the microphone gain");
    }
    if let Some(i) = band_bins.iter().position(|&b| b < MIN_BINS) {
        warnings.push(format!(
            "{name}: the {} Hz band only has {} FFT bins; play a longer burst \
             (--duration) if the bottom end matters",
            octave::THIRD_OCTAVE_CENTERS_HZ[i],
            band_bins[i]
        ));
    }

    let smoothed = smooth(&band_dbfs);
    let peak_index = argmax(&smoothed);
    let passband_ref_dbfs = smoothed[peak_index];
    let centers = &octave::THIRD_OCTAVE_CENTERS_HZ;

    Ok(SetMeasurement {
        name: name.to_string(),
        start_s: burst.start as f32 / sample_rate,
        length_s: burst.seconds(sample_rate),
        broadband_dbfs,
        peak,
        passband_ref_dbfs,
        passband_peak_hz: centers[peak_index],
        corner_low_hz: corner(
            centers,
            &smoothed,
            peak_index,
            passband_ref_dbfs,
            CORNER_DROP_DB,
            true,
        ),
        corner_high_hz: corner(
            centers,
            &smoothed,
            peak_index,
            passband_ref_dbfs,
            CORNER_DROP_DB,
            false,
        ),
        deep_low_hz: corner(
            centers,
            &smoothed,
            peak_index,
            passband_ref_dbfs,
            DEEP_DROP_DB,
            true,
        ),
        deep_high_hz: corner(
            centers,
            &smoothed,
            peak_index,
            passband_ref_dbfs,
            DEEP_DROP_DB,
            false,
        ),
        band_dbfs,
        band_bins,
    })
}

/// Three-band moving average, so a single cabin mode does not become "the
/// passband" and drag every edge with it.
fn smooth(levels: &[f32]) -> Vec<f32> {
    (0..levels.len())
        .map(|i| {
            let lo = i.saturating_sub(1);
            let hi = (i + 2).min(levels.len());
            levels[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
        })
        .collect()
}

fn argmax(levels: &[f32]) -> usize {
    levels
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Walks outward from the peak and returns the frequency where the response
/// first drops `drop_db` below `reference`, interpolated in log frequency.
fn corner(
    centers: &[f32],
    levels: &[f32],
    peak_index: usize,
    reference: f32,
    drop_db: f32,
    downward: bool,
) -> Option<f32> {
    let threshold = reference - drop_db;
    let mut i = peak_index;
    loop {
        let next = if downward {
            if i == 0 {
                return None;
            }
            i - 1
        } else {
            if i + 1 >= levels.len() {
                return None;
            }
            i + 1
        };
        if levels[next] < threshold {
            return Some(log_interp(
                centers[i],
                levels[i],
                centers[next],
                levels[next],
                threshold,
            ));
        }
        i = next;
    }
}

/// Frequency at which a straight line between two bands, drawn on log-frequency
/// against dB, reaches `target`.
fn log_interp(f0: f32, l0: f32, f1: f32, l1: f32, target: f32) -> f32 {
    if (l0 - l1).abs() < f32::EPSILON {
        return f0;
    }
    let t = ((l0 - target) / (l0 - l1)).clamp(0.0, 1.0);
    f0 * (f1 / f0).powf(t)
}

/// Reads a band curve at an arbitrary frequency.
fn interpolate_at(centers: &[f32], levels: &[f32], hz: f32) -> Option<f32> {
    if centers.len() < 2 || levels.len() != centers.len() {
        return None;
    }
    if hz <= centers[0] {
        return Some(levels[0]);
    }
    if hz >= centers[centers.len() - 1] {
        return Some(levels[levels.len() - 1]);
    }
    let i = centers.iter().position(|&c| c > hz)? - 1;
    let t = (hz / centers[i]).log2() / (centers[i + 1] / centers[i]).log2();
    Some(levels[i] + t * (levels[i + 1] - levels[i]))
}

/// Power sum of two band curves, in dB.
fn power_sum(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| 10.0 * (10f32.powf(x / 10.0) + 10f32.powf(y / 10.0)).log10())
        .collect()
}

/// Finds where the two responses cross, searching only between the two peaks -
/// outside that range any crossing is noise meeting noise.
fn find_crossover(centers: &[f32], a: &SetMeasurement, b: &SetMeasurement) -> Option<f32> {
    let smooth_a = smooth(&a.band_dbfs);
    let smooth_b = smooth(&b.band_dbfs);
    let peak_a = argmax(&smooth_a);
    let peak_b = argmax(&smooth_b);
    let (lo, hi) = (peak_a.min(peak_b), peak_a.max(peak_b));
    if lo == hi {
        return None;
    }

    for i in lo..hi {
        let d0 = smooth_a[i] - smooth_b[i];
        let d1 = smooth_a[i + 1] - smooth_b[i + 1];
        if d0 == 0.0 {
            return Some(centers[i]);
        }
        if d0.signum() != d1.signum() {
            let t = (d0 / (d0 - d1)).clamp(0.0, 1.0);
            return Some(centers[i] * (centers[i + 1] / centers[i]).powf(t));
        }
    }
    None
}

/// How far the summed response dips around the crossover, measured against the
/// average of the levels an octave or two either side.
fn dip_around(centers: &[f32], combined: &[f32], crossover_hz: f32) -> Option<f32> {
    let mean_between = |lo: f32, hi: f32| -> Option<f32> {
        let vals: Vec<f32> = centers
            .iter()
            .zip(combined.iter())
            .filter(|(&c, _)| c >= lo && c <= hi)
            .map(|(_, &v)| v)
            .collect();
        if vals.is_empty() {
            None
        } else {
            Some(vals.iter().sum::<f32>() / vals.len() as f32)
        }
    };

    let below = mean_between(crossover_hz / 4.0, crossover_hz / 2.0)?;
    let above = mean_between(crossover_hz * 2.0, crossover_hz * 4.0)?;
    let reference = (below + above) / 2.0;

    let lowest = centers
        .iter()
        .zip(combined.iter())
        .filter(|(&c, _)| c >= crossover_hz / 1.5 && c <= crossover_hz * 1.5)
        .map(|(_, &v)| v)
        .fold(f32::MAX, f32::min);
    if lowest == f32::MAX {
        return None;
    }
    Some(reference - lowest)
}

fn print_table(
    centers: &[f32],
    a: &SetMeasurement,
    b: &SetMeasurement,
    combined: &[f32],
    crossover_hz: Option<f32>,
) {
    const BAR_WIDTH: usize = 24;
    const BAR_RANGE_DB: f32 = 36.0;

    let top = combined.iter().cloned().fold(f32::MIN, f32::max);

    println!();
    println!(
        "{:>7} {:>10} {:>10} {:>10}   summed response",
        "Hz",
        truncate(&a.name, 10),
        truncate(&b.name, 10),
        "sum"
    );
    println!("{}", "-".repeat(76));

    // The band nearest the crossover gets an arrow, so the row is easy to find.
    let marked = crossover_hz.map(|hz| {
        centers
            .iter()
            .enumerate()
            .min_by(|x, y| {
                (x.1.log2() - hz.log2())
                    .abs()
                    .partial_cmp(&(y.1.log2() - hz.log2()).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(usize::MAX)
    });

    for (i, &center) in centers.iter().enumerate() {
        let bar = bar(combined[i], top, BAR_RANGE_DB, BAR_WIDTH);
        let mark = if Some(i) == marked {
            " <- crossover"
        } else {
            ""
        };
        println!(
            "{:>7} {:>10.1} {:>10.1} {:>10.1}   |{bar:<BAR_WIDTH$}|{mark}",
            hz_column(center),
            a.band_dbfs[i],
            b.band_dbfs[i],
            combined[i]
        );
    }
    println!("{}", "-".repeat(76));
    println!(
        "All levels are dBFS at the input: relative, not SPL. Bar spans {BAR_RANGE_DB:.0} dB."
    );
}

/// Band centres are ISO preferred numbers, and one of them is 31.5 Hz. Printing
/// that as "32" in a measurement table is not acceptable.
fn hz_column(hz: f32) -> String {
    if (hz - hz.round()).abs() > 1e-3 {
        format!("{hz:.1}")
    } else {
        format!("{hz:.0}")
    }
}

fn bar(level_db: f32, top_db: f32, range_db: f32, width: usize) -> String {
    let t = ((level_db - (top_db - range_db)) / range_db).clamp(0.0, 1.0);
    "=".repeat((t * width as f32).round() as usize)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect()
    }
}

fn print_bandwidth(set: &SetMeasurement) {
    println!();
    println!(
        "{}: peaks at {:.0} Hz ({:.1} dBFS)",
        set.name, set.passband_peak_hz, set.passband_ref_dbfs
    );
    println!(
        "  -{CORNER_DROP_DB:.0} dB from {} to {}",
        format_hz(set.corner_low_hz),
        format_hz(set.corner_high_hz)
    );
    println!(
        "  -{DEEP_DROP_DB:.0} dB from {} to {}",
        format_hz(set.deep_low_hz),
        format_hz(set.deep_high_hz)
    );
}

fn format_hz(hz: Option<f32>) -> String {
    match hz {
        Some(f) if f >= 1000.0 => format!("{:.1} kHz", f / 1000.0),
        Some(f) => format!("{f:.0} Hz"),
        None => "(off the measured range)".to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_recommendation(
    a: &SetMeasurement,
    b: &SetMeasurement,
    crossover_hz: Option<f32>,
    a_rel: Option<f32>,
    b_rel: Option<f32>,
    dip_db: Option<f32>,
    level_offset_db: f32,
    sanity_check_passed: Option<bool>,
    warnings: &mut Vec<String>,
) -> String {
    if sanity_check_passed == Some(false) {
        return "Measure again - the two passes of the same group disagree, so nothing below \
                can be compared."
            .to_string();
    }

    let mut parts: Vec<String> = Vec::new();

    match crossover_hz {
        Some(hz) => {
            parts.push(format!(
                "'{}' and '{}' cross at {}.",
                a.name,
                b.name,
                format_hz(Some(hz))
            ));
            if let (Some(ra), Some(rb)) = (a_rel, b_rel) {
                parts.push(format!(
                    "There each sits {ra:.1} dB and {rb:.1} dB below its own passband \
                     (about -6 dB each is the usual target)."
                ));
            }
        }
        None => {
            parts.push(format!(
                "'{}' and '{}' never cross inside 20 Hz to 16 kHz, so they do not hand over \
                 to each other at all in the measured range.",
                a.name, b.name
            ));
            warnings.push("no crossover found between the two groups".to_string());
        }
    }

    match dip_db {
        Some(dip) if dip > DIP_WARN_DB => {
            parts.push(format!(
                "Summed, the response dips {dip:.1} dB around the crossover: there is a hole \
                 between them. Raise the crossover of the lower group, lower the crossover of \
                 the upper one, or close the gap with level."
            ));
            warnings.push(format!(
                "{dip:.1} dB hole in the summed response at the crossover"
            ));
        }
        Some(dip) if dip < -DIP_WARN_DB => {
            parts.push(format!(
                "Summed, the response bulges {:.1} dB around the crossover: the two overlap \
                 more than they should.",
                -dip
            ));
            warnings.push(format!(
                "{:.1} dB bump in the summed response at the crossover",
                -dip
            ));
        }
        Some(dip) => parts.push(format!(
            "Summed, the response stays within {:.1} dB through the crossover.",
            dip.abs()
        )),
        None => {}
    }

    if level_offset_db.abs() > GAIN_NOTE_DB {
        let (louder, quieter, amount) = if level_offset_db > 0.0 {
            (&a.name, &b.name, level_offset_db)
        } else {
            (&b.name, &a.name, -level_offset_db)
        };
        parts.push(format!(
            "Gain staging: '{louder}' runs {amount:.1} dB hotter than '{quieter}'. Trim the \
             gain rather than the volume, so the difference stays fixed as you turn it up."
        ));
    } else {
        parts.push(format!(
            "Gain staging: the two passbands are within {:.1} dB of each other.",
            level_offset_db.abs()
        ));
    }

    parts.push(
        "The sum above is magnitude only. If it already shows a hole, phase cannot rescue it; \
         if it looks flat, phase can still ruin it."
            .to_string(),
    );

    parts.join(" ")
}

/// Prints a stored result; used by the `zvuk show` subcommand.
pub fn print_result(result: &CrossoverCheckResult) {
    print_table(
        &result.band_centers_hz,
        &result.set_a,
        &result.set_b,
        &result.combined_dbfs,
        result.crossover_hz,
    );
    print_bandwidth(&result.set_a);
    print_bandwidth(&result.set_b);
    println!();
    if let Some(diff) = result.repeatability_db {
        let verdict = match result.sanity_check_passed {
            Some(true) => "OK",
            Some(false) => "FAILED - the comparison is not trustworthy",
            None => "?",
        };
        println!("Sanity check: {diff:.2} dB apart, broadband -> {verdict}");
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
    const CENTERS: [f32; 30] = octave::THIRD_OCTAVE_CENTERS_HZ;

    /// Cascaded one-pole low-pass: -6 dB at `cutoff`, 12 dB/octave beyond.
    fn low_pass(input: &[f32], cutoff: f32) -> Vec<f32> {
        let a = 1.0 - (-std::f32::consts::TAU * cutoff / FS).exp();
        let mut y1 = 0.0f32;
        let mut y2 = 0.0f32;
        input
            .iter()
            .map(|&x| {
                y1 += a * (x - y1);
                y2 += a * (y1 - y2);
                y2
            })
            .collect()
    }

    /// Cascaded one-pole high-pass, the mirror image of `low_pass`.
    fn high_pass(input: &[f32], cutoff: f32) -> Vec<f32> {
        let a = 1.0 - (-std::f32::consts::TAU * cutoff / FS).exp();
        let mut lp1 = 0.0f32;
        let mut lp2 = 0.0f32;
        input
            .iter()
            .map(|&x| {
                lp1 += a * (x - lp1);
                let h1 = x - lp1;
                lp2 += a * (h1 - lp2);
                h1 - lp2
            })
            .collect()
    }

    fn levels(signal: &[f32]) -> Vec<f32> {
        octave::fractional_octave_bands(signal, FS, &CENTERS, 3, octave::THIRD_OCTAVE_FFT_LEN)
            .unwrap()
            .iter()
            .map(|b| dsp::db(b.rms))
            .collect()
    }

    fn set(name: &str, band_dbfs: Vec<f32>) -> SetMeasurement {
        let smoothed = smooth(&band_dbfs);
        let peak_index = argmax(&smoothed);
        SetMeasurement {
            name: name.to_string(),
            start_s: 0.0,
            length_s: 5.0,
            broadband_dbfs: -20.0,
            peak: 0.1,
            passband_ref_dbfs: smoothed[peak_index],
            passband_peak_hz: CENTERS[peak_index],
            corner_low_hz: corner(
                &CENTERS,
                &smoothed,
                peak_index,
                smoothed[peak_index],
                CORNER_DROP_DB,
                true,
            ),
            corner_high_hz: corner(
                &CENTERS,
                &smoothed,
                peak_index,
                smoothed[peak_index],
                CORNER_DROP_DB,
                false,
            ),
            deep_low_hz: None,
            deep_high_hz: None,
            band_bins: vec![10; CENTERS.len()],
            band_dbfs,
        }
    }

    fn noise(len: usize) -> Vec<f32> {
        signal::pink_noise_burst(FS as u32, len as f32 / FS, -6.0, NOISE_SEED)
    }

    /// A sub crossed at 80 Hz against doors high-passed at the same frequency:
    /// the analysis has to find the handover where the filters put it.
    #[test]
    fn a_known_crossover_is_found_where_the_filters_put_it() {
        let cutoff = 80.0f32;
        let source = noise(FS as usize * 6);
        let sub = set("sub", levels(&low_pass(&source, cutoff)));
        let doors = set("doors", levels(&high_pass(&source, cutoff)));

        let hz = find_crossover(&CENTERS, &doors, &sub).expect("no crossover found");
        let error_octaves = (hz / cutoff).log2().abs();
        assert!(
            error_octaves < 0.34,
            "crossover found at {hz:.0} Hz, expected around {cutoff:.0} Hz"
        );
    }

    /// The sub must report an upper edge and the doors a lower one, both near
    /// the crossover, and neither must claim to work where it does not.
    #[test]
    fn each_group_reports_the_bandwidth_its_filter_gives_it() {
        let cutoff = 80.0f32;
        let source = noise(FS as usize * 6);
        let sub = set("sub", levels(&low_pass(&source, cutoff)));
        let doors = set("doors", levels(&high_pass(&source, cutoff)));

        let sub_high = sub.corner_high_hz.expect("sub has no upper edge");
        assert!(
            (sub_high / cutoff).log2().abs() < 1.0,
            "sub rolls off at {sub_high:.0} Hz, expected within an octave of {cutoff:.0} Hz"
        );
        assert!(
            sub.passband_peak_hz < cutoff,
            "sub should peak below the crossover"
        );

        let doors_low = doors.corner_low_hz.expect("doors have no lower edge");
        assert!(
            (doors_low / cutoff).log2().abs() < 1.0,
            "doors roll off at {doors_low:.0} Hz, expected within an octave of {cutoff:.0} Hz"
        );
        assert!(
            doors.passband_peak_hz > cutoff,
            "doors should peak above the crossover"
        );
    }

    /// A matched pair sums flat; pulling the two filters apart opens a hole and
    /// the dip detector has to see it.
    #[test]
    fn a_gap_between_the_two_shows_up_as_a_dip() {
        let source = noise(FS as usize * 6);

        let matched = power_sum(
            &levels(&high_pass(&source, 80.0)),
            &levels(&low_pass(&source, 80.0)),
        );
        let matched_dip = dip_around(&CENTERS, &matched, 80.0).expect("no dip figure");

        // Sub stops two octaves below where the doors start.
        let split = power_sum(
            &levels(&high_pass(&source, 160.0)),
            &levels(&low_pass(&source, 40.0)),
        );
        let split_dip = dip_around(&CENTERS, &split, 80.0).expect("no dip figure");

        assert!(
            matched_dip.abs() < DIP_WARN_DB,
            "a matched crossover should sum flat, got {matched_dip:.1} dB"
        );
        assert!(
            split_dip > DIP_WARN_DB,
            "a two-octave gap should show as a hole, got only {split_dip:.1} dB"
        );
    }

    /// Gain staging: attenuating one group must come back as exactly that.
    #[test]
    fn a_level_offset_between_the_groups_is_reported() {
        let source = noise(FS as usize * 6);
        let offset_db = 6.0f32;
        let attenuation = 10f32.powf(-offset_db / 20.0);

        let doors = set("doors", levels(&high_pass(&source, 80.0)));
        let quiet_sub: Vec<f32> = low_pass(&source, 80.0)
            .iter()
            .map(|&s| s * attenuation)
            .collect();
        let sub = set("sub", levels(&quiet_sub));

        let measured = doors.passband_ref_dbfs - sub.passband_ref_dbfs;
        let loud_sub = set("sub", levels(&low_pass(&source, 80.0)));
        let baseline = doors.passband_ref_dbfs - loud_sub.passband_ref_dbfs;

        assert!(
            (measured - baseline - offset_db).abs() < 0.2,
            "expected the offset to grow by {offset_db:.1} dB, it grew by {:.2} dB",
            measured - baseline
        );
    }

    #[test]
    fn interpolation_reads_between_bands_and_clamps_outside_them() {
        let centers = [100.0f32, 200.0, 400.0];
        let levels = [-10.0f32, -20.0, -30.0];
        assert_eq!(interpolate_at(&centers, &levels, 100.0), Some(-10.0));
        assert_eq!(interpolate_at(&centers, &levels, 10.0), Some(-10.0));
        assert_eq!(interpolate_at(&centers, &levels, 4000.0), Some(-30.0));
        let mid = interpolate_at(&centers, &levels, 141.42).unwrap();
        assert!((mid - -15.0).abs() < 0.1, "midpoint read back as {mid}");
    }

    #[test]
    fn fractional_band_centres_keep_their_decimal() {
        assert_eq!(hz_column(31.5), "31.5");
        assert_eq!(hz_column(20.0), "20");
        assert_eq!(hz_column(16000.0), "16000");
    }

    #[test]
    fn power_sum_of_two_equal_levels_is_three_db_higher() {
        let sum = power_sum(&[-20.0, -40.0], &[-20.0, -40.0]);
        assert!((sum[0] - -16.9897).abs() < 0.01);
        assert!((sum[1] - -36.9897).abs() < 0.01);
    }
}
