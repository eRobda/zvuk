//! Core of the modular system: the `Measurement` trait, the test signal a
//! module asks the user to play, the result type, and the envelope a
//! measurement is stored in.

use anyhow::Result;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::audio::AudioContext;
use crate::modules::crossover_check::CrossoverCheckResult;
use crate::modules::left_right_balance::LeftRightBalanceResult;

/// Version of the on-disk JSON format. Bump it on a breaking change.
///
/// v2 dropped the output device fields: the tool no longer plays anything, the
/// user plays the generated track through the system being measured.
pub const SCHEMA_VERSION: u32 = 2;

/// Every measurement module implements this trait and registers itself in
/// [`crate::registry`].
pub trait Measurement {
    /// Short machine name, used as `--module` and in the output file name.
    fn name(&self) -> &str;

    /// One-line description shown in the CLI listing.
    fn description(&self) -> &str;

    /// The measurement itself. A module may talk to the user over stdin/stdout.
    fn run(&self, ctx: &AudioContext) -> Result<MeasurementResult>;

    /// The track `zvuk generate` should write for this module, if it needs one.
    ///
    /// The tool never plays audio itself, so a module that needs a stimulus
    /// describes it here and the user plays the file through the car.
    fn signal(&self, _params: &SignalParams) -> Option<TestSignal> {
        None
    }
}

/// How the generated track should be built.
#[derive(Debug, Clone, Copy)]
pub struct SignalParams {
    pub sample_rate: u32,
    /// Length of one measurement burst.
    pub duration_s: f32,
    /// Peak level of the burst in the file, in dBFS.
    pub level_dbfs: f32,
    /// Silence before the first burst, so you can sit down after pressing play.
    pub lead_in_s: f32,
    /// Silence between bursts. Also what the analysis uses to tell them apart.
    pub gap_s: f32,
    /// Append a repeat of the first burst so repeatability can be checked.
    pub recheck: bool,
}

impl Default for SignalParams {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            duration_s: 3.0,
            level_dbfs: -6.0,
            lead_in_s: 5.0,
            gap_s: 2.0,
            recheck: true,
        }
    }
}

/// A track for the user to play through the system under test.
pub struct TestSignal {
    /// File name without extension.
    pub file_stem: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved samples in the range [-1, 1].
    pub samples: Vec<f32>,
    /// What the track contains, one line per section.
    pub layout: Vec<String>,
    /// What the user has to do, one line per step.
    pub instructions: Vec<String>,
}

impl TestSignal {
    pub fn duration_s(&self) -> f32 {
        self.samples.len() as f32 / self.channels.max(1) as f32 / self.sample_rate as f32
    }
}

/// A measurement result. Each module adds its own variant with its own data
/// structure; the internal `kind` tag keeps the JSON readable and extensible.
///
/// Every payload is boxed. An enum is as large as its largest arm, and these
/// structs carry whole response curves, so an unboxed one would make every
/// result the size of the biggest module. Boxing is transparent to serde.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeasurementResult {
    LeftRightBalance(Box<LeftRightBalanceResult>),
    CrossoverCheck(Box<CrossoverCheckResult>),
    // Reserved for the planned modules, e.g.:
    // ArrivalTime(ArrivalTimeResult),
    // SubwooferPhase(SubwooferPhaseResult),
    // ClippingSweep(ClippingSweepResult),
}

/// The capture chain a measurement was taken with. Without this, a stored
/// measurement is worthless for later comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextInfo {
    pub input_device: String,
    pub input_sample_rate: u32,
    pub input_channels: u16,
    pub buffer_size: Option<u32>,
}

/// What actually gets serialized to a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasurementRecord {
    pub schema_version: u32,
    pub module: String,
    pub timestamp: DateTime<Local>,
    /// Optional user label, e.g. "after door damping".
    pub label: Option<String>,
    pub context: ContextInfo,
    pub result: MeasurementResult,
}

impl MeasurementRecord {
    pub fn new(
        module: &str,
        label: Option<String>,
        context: ContextInfo,
        result: MeasurementResult,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            module: module.to_string(),
            timestamp: Local::now(),
            label,
            context,
            result,
        }
    }
}
