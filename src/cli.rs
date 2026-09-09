//! Command line definition.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "zvuk",
    version,
    about = "Measure and tune car audio systems",
    long_about = "A modular measurement tool for car audio.\n\
                  With no subcommand it runs a measurement: lists devices, then \
                  lets you pick one and choose a module.",
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
    /// List the available input and output audio devices.
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
    /// Module name; without it you get an interactive picker.
    #[arg(short, long)]
    pub module: Option<String>,

    /// User label for the measurement, e.g. "after door damping".
    #[arg(short, long)]
    pub label: Option<String>,

    /// Input device: index from the listing or part of the name.
    #[arg(short = 'i', long)]
    pub input: Option<String>,

    /// Output device: index from the listing or part of the name.
    #[arg(short = 'o', long)]
    pub output: Option<String>,

    /// Desired sample rate; the closest supported one is used.
    #[arg(long, default_value_t = 48_000)]
    pub sample_rate: u32,

    /// Fixed buffer size in samples (driver default otherwise).
    #[arg(long)]
    pub buffer: Option<u32>,

    /// Length of the test signal in seconds.
    #[arg(long, default_value_t = 3.0)]
    pub duration: f32,

    /// Test signal level in dBFS (peak).
    #[arg(long, default_value_t = -6.0)]
    pub level: f32,

    /// Directory measurements are written to.
    #[arg(long, default_value = "measurements")]
    pub out_dir: PathBuf,

    /// Do not write the result to disk.
    #[arg(long)]
    pub no_save: bool,
}
