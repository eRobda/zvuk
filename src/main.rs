//! `zvuk` - a modular CLI for measuring and tuning car audio systems.

mod audio;
mod cli;
mod dsp;
mod measurement;
mod modules;
mod prompt;
mod registry;
mod storage;

use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;

use crate::audio::devices::{choose_input_config, DeviceList};
use crate::audio::AudioContext;
use crate::cli::{Cli, Command, GenerateArgs, RunArgs};
use crate::measurement::{
    Measurement, MeasurementRecord, MeasurementResult, SignalParams, TestSignal,
};

fn main() {
    if let Err(err) = run() {
        eprintln!();
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Generate(args)) => generate(args),
        Some(Command::Devices) => list_devices(),
        Some(Command::Modules) => {
            list_modules();
            Ok(())
        }
        Some(Command::Show { path }) => show(&path),
        None => measure(cli.run),
    }
}

fn list_devices() -> Result<()> {
    let host = cpal::default_host();
    println!("Audio host: {}", host.id().name());
    println!();
    DeviceList::inputs(&host)?.print();
    println!();
    println!("There is no output list: zvuk never plays anything. Use `zvuk generate`");
    println!("to write a track and play it through the car yourself.");
    Ok(())
}

fn list_modules() {
    println!("Measurement modules:");
    for (i, m) in registry::all().iter().enumerate() {
        println!("  [{i}] {:<20} {}", m.name(), m.description());
    }
}

fn show(path: &Path) -> Result<()> {
    let record = storage::load(path)?;
    println!("Module: {}", record.module);
    println!("Time:   {}", record.timestamp.format("%Y-%m-%d %H:%M:%S"));
    println!("Label:  {}", record.label.as_deref().unwrap_or("(none)"));
    println!(
        "Input:  {} @ {} Hz",
        record.context.input_device, record.context.input_sample_rate
    );
    match &record.result {
        MeasurementResult::LeftRightBalance(result) => {
            modules::left_right_balance::print_result(result)
        }
    }
    Ok(())
}

/// Picks a module by name, or interactively when no name was given.
fn pick_module(name: Option<&String>) -> Result<Box<dyn Measurement>> {
    match name {
        Some(name) => registry::find(name).ok_or_else(|| {
            anyhow!(
                "module '{name}' does not exist; available: {}",
                registry::all()
                    .iter()
                    .map(|m| m.name().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }),
        None => {
            println!();
            list_modules();
            let modules = registry::all();
            let index = prompt::select("Pick a module", modules.len(), Some(0))?;
            modules
                .into_iter()
                .nth(index)
                .ok_or_else(|| anyhow!("invalid module selection"))
        }
    }
}

fn generate(args: GenerateArgs) -> Result<()> {
    let module = pick_module(args.module.as_ref())?;
    let params = SignalParams {
        sample_rate: args.sample_rate,
        duration_s: args.duration.max(0.5),
        level_dbfs: args.level.min(0.0),
        lead_in_s: args.lead_in.max(0.0),
        gap_s: args.gap.max(0.5),
        recheck: !args.no_recheck,
    };

    let signal = module.signal(&params).ok_or_else(|| {
        anyhow!(
            "module '{}' needs no test signal, so there is nothing to generate",
            module.name()
        )
    })?;

    let wav_path = args.out_dir.join(format!("{}.wav", signal.file_stem));
    let txt_path = args.out_dir.join(format!("{}.txt", signal.file_stem));
    if !args.force {
        for path in [&wav_path, &txt_path] {
            if path.exists() {
                bail!(
                    "{} already exists; pass --force to overwrite",
                    path.display()
                );
            }
        }
    }

    audio::wav::write(
        &wav_path,
        &signal.samples,
        signal.sample_rate,
        signal.channels,
    )?;
    fs::write(&txt_path, readme_text(module.as_ref(), &signal))
        .with_context(|| format!("cannot write {}", txt_path.display()))?;

    println!("Wrote {}", wav_path.display());
    println!(
        "  {} Hz, {} channels, {:.1} s, peak {:.1} dBFS",
        signal.sample_rate,
        signal.channels,
        signal.duration_s(),
        params.level_dbfs
    );
    println!();
    println!("Track layout:");
    for line in &signal.layout {
        println!("  {line}");
    }
    println!();
    println!("What to do:");
    for line in &signal.instructions {
        println!("  {line}");
    }
    println!();
    println!(
        "The same instructions are in {}, so they travel with the file.",
        txt_path.display()
    );
    Ok(())
}

/// The text file written next to the WAV, for when you are in the car with a
/// USB stick and not with the terminal.
fn readme_text(module: &dyn Measurement, signal: &TestSignal) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}.wav\n", signal.file_stem));
    out.push_str(&format!(
        "Test track for the zvuk module '{}'.\n",
        module.name()
    ));
    out.push_str(&format!("{}\n\n", module.description()));
    out.push_str(&format!(
        "Format: {} Hz, {} channels, 16-bit PCM, {:.1} s total.\n\n",
        signal.sample_rate,
        signal.channels,
        signal.duration_s()
    ));
    out.push_str("Track layout:\n");
    for line in &signal.layout {
        out.push_str(&format!("  {line}\n"));
    }
    out.push_str("\nWhat to do:\n");
    for line in &signal.instructions {
        out.push_str(&format!("  {line}\n"));
    }
    out.push_str("\nDo not convert this file to MP3 or any other lossy format.\n");
    out.push_str("https://github.com/eRobda/zvuk\n");
    out
}

fn measure(args: RunArgs) -> Result<()> {
    let host = cpal::default_host();
    println!("Audio host: {}", host.id().name());

    let inputs = DeviceList::inputs(&host)?;
    if inputs.is_empty() {
        bail!("no input device found - plug in a microphone");
    }

    println!();
    inputs.print();
    println!();

    let input_index = match &args.input {
        Some(spec) => inputs.resolve(spec)?,
        None => prompt::select(
            "Pick the input (microphone)",
            inputs.len(),
            inputs.default_index,
        )?,
    };
    let (input_name, input_device) = inputs.take(input_index)?;

    let input_config = choose_input_config(&input_device, args.sample_rate)
        .with_context(|| format!("input device '{input_name}'"))?;

    let ctx = AudioContext::new(input_device, input_name, input_config, args.buffer);

    println!();
    ctx.describe();

    let module = pick_module(args.module.as_ref())?;

    let label = match args.label {
        Some(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Some(_) => None,
        None => {
            let text = prompt::read_line("Label, e.g. 'after door damping' (Enter = none): ")?;
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
    };

    let result = module.run(&ctx)?;
    let record = MeasurementRecord::new(module.name(), label, ctx.info(), result);

    if args.no_save {
        println!();
        println!("Result not saved (--no-save).");
    } else {
        let path = storage::save(&args.out_dir, &record)?;
        println!();
        println!("Saved: {}", path.display());
    }

    Ok(())
}
