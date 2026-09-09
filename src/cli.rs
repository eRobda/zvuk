//! Command line definition.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "zvuk",
    version,
    about = "Measure and tune car audio systems",
    long_about = "A modular measurement tool for car audio.\n\n\
                  The tool never plays audio itself. `zvuk generate` writes a test \
                  track for you to play through the car - off a USB stick, a phone, \
                  a CD - and `zvuk` with no subcommand records it and does the maths.",
    allow_negative_numbers = true
)]
pub struct Cli {
    #[command(flatten)]
    pub run: RunArgs,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Write the test track a module needs you to play.
    Generate(GenerateArgs),
    /// List the available input devices.
    Devices,
    /// List the registered measurement modules.
    Modules,
    /// Print a stored measurement from a file.
    Show {
        /// Path to the measurement JSON.
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Args)]
#[command(allow_negative_numbers = true)]
pub struct RunArgs {
    /// Module to run; without it you get an interactive picker.
    #[arg(short, long)]
    pub module: Option<String>,

    /// User label for the measurement, e.g. "after door damping".
    #[arg(short, long)]
    pub label: Option<String>,

    /// Input device: index from the listing or part of the name.
    #[arg(short = 'i', long)]
    pub input: Option<String>,

    /// Desired capture sample rate; the closest supported one is used.
    #[arg(long, default_value_t = 48_000)]
    pub sample_rate: u32,

    /// Fixed buffer size in samples (driver default otherwise).
    #[arg(long)]
    pub buffer: Option<u32>,

    /// Directory measurements are written to.
    #[arg(long, default_value = "measurements")]
    pub out_dir: PathBuf,

    /// Do not write the result to disk.
    #[arg(long)]
    pub no_save: bool,
}

#[derive(Debug, Clone, Args)]
#[command(allow_negative_numbers = true)]
pub struct GenerateArgs {
    /// Module to generate the track for; without it you get a picker.
    #[arg(short, long)]
    pub module: Option<String>,

    /// Directory the track is written to.
    #[arg(long, default_value = "signals")]
    pub out_dir: PathBuf,

    /// Sample rate of the generated file. Try 44100 for an older head unit.
    #[arg(long, default_value_t = 48_000)]
    pub sample_rate: u32,

    /// Length of one measurement burst, in seconds.
    #[arg(long, default_value_t = 3.0)]
    pub duration: f32,

    /// Peak level of the burst in the file, in dBFS.
    #[arg(long, default_value_t = -6.0)]
    pub level: f32,

    /// Silence before the first burst, so you can sit down after pressing play.
    #[arg(long, default_value_t = 5.0)]
    pub lead_in: f32,

    /// Silence between bursts. This is what lets the analysis tell them apart.
    #[arg(long, default_value_t = 2.0)]
    pub gap: f32,

    /// Leave out the repeated burst, and with it the repeatability check.
    #[arg(long)]
    pub no_recheck: bool,

    /// Overwrite an existing file instead of refusing.
    #[arg(long)]
    pub force: bool,
}
