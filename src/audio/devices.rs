//! Listing and picking audio devices, plus choosing the closest supported
//! stream configuration.

use anyhow::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{
    Device, Host, SampleFormat, SampleRate, SupportedStreamConfig, SupportedStreamConfigRange,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Input,
    Output,
}

impl Direction {
    fn label(self) -> &'static str {
        match self {
            Direction::Input => "input",
            Direction::Output => "output",
        }
    }
}

pub struct DeviceList {
    pub direction: Direction,
    entries: Vec<(String, Device)>,
    pub default_index: Option<usize>,
}

impl DeviceList {
    pub fn enumerate(host: &Host, direction: Direction) -> Result<Self> {
        let devices: Vec<Device> = match direction {
            Direction::Input => host.input_devices()?.collect(),
            Direction::Output => host.output_devices()?.collect(),
        };

        let default_name = match direction {
            Direction::Input => host.default_input_device().and_then(|d| d.name().ok()),
            Direction::Output => host.default_output_device().and_then(|d| d.name().ok()),
        };

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
            direction,
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
        println!(
            "Available {} devices ({}):",
            self.direction.label(),
            self.entries.len()
        );
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
            let detail = describe_default(device, self.direction);
            println!("  {mark}[{i}] {name}");
            println!("       {detail}");
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
            0 => bail!("no {} device matches '{spec}'", self.direction.label()),
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

fn describe_default(device: &Device, direction: Direction) -> String {
    let cfg = match direction {
        Direction::Input => device.default_input_config(),
        Direction::Output => device.default_output_config(),
    };
    match cfg {
        Ok(c) => format!(
            "default: {} Hz, {} ch, {:?}",
            c.sample_rate().0,
            c.channels(),
            c.sample_format()
        ),
        Err(e) => format!("configuration unavailable ({e})"),
    }
}

/// Picks the configuration closest to `target_rate` with at least
/// `min_channels` channels.
///
/// The user is never asked for a sample rate; we take what the device offers.
pub fn choose_config(
    device: &Device,
    direction: Direction,
    target_rate: u32,
    min_channels: u16,
) -> Result<SupportedStreamConfig> {
    let ranges: Vec<SupportedStreamConfigRange> = match direction {
        Direction::Input => device.supported_input_configs().map(|it| it.collect()),
        Direction::Output => device.supported_output_configs().map(|it| it.collect()),
    }
    .unwrap_or_default();

    let usable: Vec<SupportedStreamConfigRange> = ranges
        .into_iter()
        .filter(|r| format_rank(r.sample_format()).is_some())
        .collect();

    // Prefer a device with enough channels, then fall back to anything.
    for required in [min_channels, 1] {
        let best = usable
            .iter()
            .filter(|r| r.channels() >= required)
            .min_by_key(|r| {
                let rate = clamp_rate(r, target_rate);
                let rate_penalty = (rate as i64 - target_rate as i64).unsigned_abs();
                let fmt_penalty = format_rank(r.sample_format()).unwrap_or(u32::MAX);
                let ch_penalty = (r.channels() as i64 - required as i64).unsigned_abs();
                (rate_penalty, fmt_penalty as u64, ch_penalty)
            });
        if let Some(r) = best {
            let rate = clamp_rate(r, target_rate);
            return Ok((*r).with_sample_rate(SampleRate(rate)));
        }
    }

    // The device reports no usable range, so fall back to its default config.
    let fallback = match direction {
        Direction::Input => device.default_input_config(),
        Direction::Output => device.default_output_config(),
    }
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
