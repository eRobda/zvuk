//! Core of the modular system: the `Measurement` trait, the result type, and
//! the envelope a measurement is stored in.

use anyhow::Result;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::audio::AudioContext;
use crate::modules::left_right_balance::LeftRightBalanceResult;

/// Version of the on-disk JSON format. Bump it on a breaking change.
pub const SCHEMA_VERSION: u32 = 1;

/// Every measurement module implements this trait and registers itself in
/// [`crate::registry`].
pub trait Measurement {
    /// Short machine name, used as `--module` and in the output file name.
    fn name(&self) -> &str;

    /// One-line description shown in the CLI listing.
    fn description(&self) -> &str;

    /// The measurement itself. A module may talk to the user over stdin/stdout.
    fn run(&self, ctx: &AudioContext) -> Result<MeasurementResult>;
}

/// A measurement result. Each module adds its own variant with its own data
/// structure; the internal `kind` tag keeps the JSON readable and extensible.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeasurementResult {
    LeftRightBalance(LeftRightBalanceResult),
    // Reserved for the planned modules, e.g.:
    // ArrivalTime(ArrivalTimeResult),
    // SubwooferPhase(SubwooferPhaseResult),
    // ClippingSweep(ClippingSweepResult),
}

/// The signal chain a measurement was taken with. Without this, a stored
/// measurement is worthless for later comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextInfo {
    pub input_device: String,
    pub output_device: String,
    pub input_sample_rate: u32,
    pub output_sample_rate: u32,
    pub input_channels: u16,
    pub output_channels: u16,
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
