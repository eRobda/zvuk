//! `zvuk` - a modular CLI for measuring and tuning car audio systems.

mod audio;
mod cli;
mod dsp;
mod measurement;
mod modules;
mod prompt;
mod registry;
mod storage;

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;

use crate::audio::devices::{choose_config, DeviceList, Direction};
use crate::audio::{AudioContext, RunParams};
use crate::cli::{Cli, Command, RunArgs};
use crate::measurement::{MeasurementRecord, MeasurementResult};

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
    DeviceList::enumerate(&host, Direction::Input)?.print();
    println!();
    DeviceList::enumerate(&host, Direction::Output)?.print();
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
    println!(
        "Output: {} @ {} Hz",
        record.context.output_device, record.context.output_sample_rate
    );
    match &record.result {
        MeasurementResult::LeftRightBalance(result) => {
            modules::left_right_balance::print_result(result)
        }
    }
    Ok(())
}

fn measure(args: RunArgs) -> Result<()> {
    let host = cpal::default_host();
    println!("Audio host: {}", host.id().name());

    let inputs = DeviceList::enumerate(&host, Direction::Input)?;
    let outputs = DeviceList::enumerate(&host, Direction::Output)?;
    if inputs.is_empty() {
        bail!("no input device found - plug in a microphone");
    }
    if outputs.is_empty() {
        bail!("no output device found");
    }

    println!();
    inputs.print();
    println!();
    outputs.print();
    println!();

    let input_index = match &args.input {
        Some(spec) => inputs.resolve(spec)?,
        None => prompt::select(
            "Pick the input (microphone)",
            inputs.len(),
            inputs.default_index,
        )?,
    };
    let output_index = match &args.output {
        Some(spec) => outputs.resolve(spec)?,
        None => prompt::select(
            "Pick the output (head unit)",
            outputs.len(),
            outputs.default_index,
        )?,
    };

    let (input_name, input_device) = inputs.take(input_index)?;
    let (output_name, output_device) = outputs.take(output_index)?;

    if input_name.eq_ignore_ascii_case(&output_name) {
        println!();
        println!("Note: input and output are the same device ({input_name}).");
        println!("      On Windows that may fail - if the stream errors out, pick another output.");
    }

    let input_config = choose_config(&input_device, Direction::Input, args.sample_rate, 1)
        .with_context(|| format!("input device '{input_name}'"))?;
    let output_config = choose_config(&output_device, Direction::Output, args.sample_rate, 2)
        .with_context(|| format!("output device '{output_name}'"))?;

    let ctx = AudioContext::new(
        input_device,
        input_name,
        input_config,
        output_device,
        output_name,
        output_config,
        args.buffer,
        RunParams {
            duration_s: args.duration.max(0.5),
            level_dbfs: args.level.min(0.0),
        },
    );

    println!();
    ctx.describe();

    let module = match &args.module {
        Some(name) => registry::find(name).ok_or_else(|| {
            anyhow!(
                "module '{name}' does not exist; available: {}",
                registry::all()
                    .iter()
                    .map(|m| m.name().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?,
        None => {
            println!();
            list_modules();
            let modules = registry::all();
            let index = prompt::select("Pick a module", modules.len(), Some(0))?;
            modules
                .into_iter()
                .nth(index)
                .ok_or_else(|| anyhow!("invalid module selection"))?
        }
    };

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
