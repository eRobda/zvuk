//! Listing and picking the input device, plus choosing the closest supported
//! stream configuration.
//!
//! There is no output side. The tool never plays anything: `zvuk generate`
//! writes a track and the user plays it through the system being measured.

use anyhow::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{
    Device, Host, SampleFormat, SampleRate, SupportedStreamConfig, SupportedStreamConfigRange,
};

pub struct DeviceList {
    entries: Vec<(String, Device)>,
    pub default_index: Option<usize>,
}

impl DeviceList {
    pub fn inputs(host: &Host) -> Result<Self> {
        let devices: Vec<Device> = host.input_devices()?.collect();
        let default_name = host.default_input_device().and_then(|d| d.name().ok());

        let entries: Vec<(String, Device)> = devices
            .into_iter()
            .enumerate()
            .map(|(i, d)| {
                let name = d.name().unwrap_or_else(|_| format!("<device {i}>"));
                (name, d)
            })
            .collect();

        let default_index = default_name
            .as_deref()
            .and_then(|want| entries.iter().position(|(name, _)| name == want));

        Ok(Self {
            entries,
            default_index,
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn print(&self) {
        println!("Available input devices ({}):", self.entries.len());
        if self.entries.is_empty() {
            println!("  (none)");
            return;
        }
        for (i, (name, device)) in self.entries.iter().enumerate() {
            let mark = if Some(i) == self.default_index {
                "*"
            } else {
                " "
            };
            println!("  {mark}[{i}] {name}");
            println!("       {}", describe_default(device));
        }
        println!("  (* = system default)");
    }

    /// Accepts either an index or a case-insensitive fragment of the device name.
    pub fn resolve(&self, spec: &str) -> Result<usize> {
        let spec = spec.trim();
        if let Ok(i) = spec.parse::<usize>() {
            if i < self.entries.len() {
                return Ok(i);
            }
            bail!(
                "index {i} is out of range 0-{}",
                self.entries.len().saturating_sub(1)
            );
        }
        let needle = spec.to_lowercase();
        let hits: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, (name, _))| name.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect();
        match hits.len() {
            0 => bail!("no input device matches '{spec}'"),
            1 => Ok(hits[0]),
            _ => bail!("'{spec}' matches several devices: {hits:?}"),
        }
    }

    pub fn take(mut self, index: usize) -> Result<(String, Device)> {
        if index >= self.entries.len() {
            bail!("invalid device index {index}");
        }
        Ok(self.entries.remove(index))
    }
}

fn describe_default(device: &Device) -> String {
    match device.default_input_config() {
        Ok(c) => format!(
            "default: {} Hz, {} ch, {:?}",
            c.sample_rate().0,
            c.channels(),
            c.sample_format()
        ),
        Err(e) => format!("configuration unavailable ({e})"),
    }
}

/// Picks the input configuration closest to `target_rate`.
///
/// The user is never asked for a sample rate; we take what the device offers.
pub fn choose_input_config(device: &Device, target_rate: u32) -> Result<SupportedStreamConfig> {
    let usable: Vec<SupportedStreamConfigRange> = device
        .supported_input_configs()
        .map(|it| it.collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|r| format_rank(r.sample_format()).is_some())
        .collect();

    let best = usable.iter().min_by_key(|r| {
        let rate = clamp_rate(r, target_rate);
        let rate_penalty = (rate as i64 - target_rate as i64).unsigned_abs();
        let fmt_penalty = format_rank(r.sample_format()).unwrap_or(u32::MAX) as u64;
        let ch_penalty = r.channels() as u64;
        (rate_penalty, fmt_penalty, ch_penalty)
    });
    if let Some(r) = best {
        let rate = clamp_rate(r, target_rate);
        return Ok((*r).with_sample_rate(SampleRate(rate)));
    }

    // The device reports no usable range, so fall back to its default config.
    let fallback = device
        .default_input_config()
        .map_err(|e| anyhow!("device offers no usable configuration: {e}"))?;
    if format_rank(fallback.sample_format()).is_none() {
        bail!(
            "device only supports the {:?} sample format, which is not handled yet",
            fallback.sample_format()
        );
    }
    Ok(fallback)
}

fn clamp_rate(range: &SupportedStreamConfigRange, target: u32) -> u32 {
    target.clamp(range.min_sample_rate().0, range.max_sample_rate().0)
}

/// Sample formats we can handle; a lower number means a stronger preference.
fn format_rank(fmt: SampleFormat) -> Option<u32> {
    match fmt {
        SampleFormat::F32 => Some(0),
        SampleFormat::I16 => Some(1),
        SampleFormat::I32 => Some(2),
        SampleFormat::U16 => Some(3),
        SampleFormat::I8 => Some(4),
        SampleFormat::U8 => Some(5),
        _ => None,
    }
}
