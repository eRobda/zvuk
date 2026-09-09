//! `AudioContext` holds the selected devices and offers modules three
//! primitives: play a buffer, record N seconds, play and record at once.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{
    BufferSize, Device, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
    SupportedStreamConfig,
};

use crate::measurement::ContextInfo;

/// Run parameters modules read (signal length, level).
#[derive(Debug, Clone, Copy)]
pub struct RunParams {
    pub duration_s: f32,
    pub level_dbfs: f32,
}

impl Default for RunParams {
    fn default() -> Self {
        Self {
            duration_s: 3.0,
            level_dbfs: -6.0,
        }
    }
}

/// The concrete configuration of one stream.
pub struct StreamSpec {
    pub device_name: String,
    pub config: StreamConfig,
    pub sample_format: SampleFormat,
}

impl StreamSpec {
    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate.0
    }

    pub fn channels(&self) -> u16 {
        self.config.channels
    }
}

/// The result of playing and recording simultaneously.
pub struct Recording {
    /// Mono capture (channel 0 of the input device).
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    /// Index of the sample at which playback started.
    pub play_start: usize,
    /// Length of the played signal in seconds.
    pub play_secs: f32,
}

impl Recording {
    /// The steady-state part of the capture: drops `guard_s` seconds from both
    /// ends so that output latency and room decay stay out of the analysis.
    pub fn steady_state(&self, guard_s: f32) -> Result<&[f32]> {
        let fs = self.sample_rate as f32;
        let start = self.play_start + (guard_s * fs) as usize;
        let end = self.play_start + ((self.play_secs - guard_s) * fs) as usize;
        let end = end.min(self.samples.len());
        if end <= start || start >= self.samples.len() {
            bail!(
                "no usable segment left in the capture ({} samples recorded, expected about {})",
                self.samples.len(),
                (self.play_secs * fs) as usize
            );
        }
        Ok(&self.samples[start..end])
    }
}

pub struct AudioContext {
    input_device: Device,
    output_device: Device,
    pub input: StreamSpec,
    pub output: StreamSpec,
    pub buffer_size: Option<u32>,
    pub params: RunParams,
}

impl AudioContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        input_device: Device,
        input_name: String,
        input_config: SupportedStreamConfig,
        output_device: Device,
        output_name: String,
        output_config: SupportedStreamConfig,
        buffer_size: Option<u32>,
        params: RunParams,
    ) -> Self {
        let apply = |c: &SupportedStreamConfig| -> StreamConfig {
            let mut cfg = c.config();
            if let Some(frames) = buffer_size {
                cfg.buffer_size = BufferSize::Fixed(frames);
            }
            cfg
        };

        Self {
            input: StreamSpec {
                device_name: input_name,
                config: apply(&input_config),
                sample_format: input_config.sample_format(),
            },
            output: StreamSpec {
                device_name: output_name,
                config: apply(&output_config),
                sample_format: output_config.sample_format(),
            },
            input_device,
            output_device,
            buffer_size,
            params,
        }
    }

    pub fn info(&self) -> ContextInfo {
        ContextInfo {
            input_device: self.input.device_name.clone(),
            output_device: self.output.device_name.clone(),
            input_sample_rate: self.input.sample_rate(),
            output_sample_rate: self.output.sample_rate(),
            input_channels: self.input.channels(),
            output_channels: self.output.channels(),
            buffer_size: self.buffer_size,
        }
    }

    pub fn describe(&self) {
        println!(
            "Input:  {} | {} Hz | {} ch | {:?}",
            self.input.device_name,
            self.input.sample_rate(),
            self.input.channels(),
            self.input.sample_format
        );
        println!(
            "Output: {} | {} Hz | {} ch | {:?}",
            self.output.device_name,
            self.output.sample_rate(),
            self.output.channels(),
            self.output.sample_format
        );
        if self.input.sample_rate() != self.output.sample_rate() {
            println!("Note: input and output run at different sample rates.");
            println!("      Harmless for noise measurements; analysis uses the input rate.");
        }
        match self.buffer_size {
            Some(n) => println!("Buffer: {n} samples (fixed)"),
            None => println!("Buffer: driver default"),
        }
    }

    /// Spreads a mono signal into one specific output channel, leaving the
    /// other channels silent.
    pub fn mono_to_channel(&self, mono: &[f32], channel: usize) -> Result<Vec<f32>> {
        let channels = self.output.channels() as usize;
        if channel >= channels {
            bail!("output device has {channels} channels, channel {channel} does not exist");
        }
        let mut out = vec![0.0f32; mono.len() * channels];
        for (i, &s) in mono.iter().enumerate() {
            out[i * channels + channel] = s;
        }
        Ok(out)
    }

    /// Spreads a mono signal into every output channel.
    #[allow(dead_code)] // primitive reserved for the planned modules
    pub fn mono_to_all(&self, mono: &[f32]) -> Vec<f32> {
        let channels = self.output.channels() as usize;
        let mut out = Vec::with_capacity(mono.len() * channels);
        for &s in mono {
            for _ in 0..channels {
                out.push(s);
            }
        }
        out
    }

    /// Primitive 1: play an interleaved buffer and wait until it finishes.
    #[allow(dead_code)] // primitive reserved for the planned modules
    pub fn play(&self, interleaved: &[f32]) -> Result<()> {
        let playback = Arc::new(Playback::new(interleaved.to_vec()));
        let stream = build_output_stream(
            &self.output_device,
            &self.output.config,
            self.output.sample_format,
            Arc::clone(&playback),
        )?;
        stream.play()?;
        wait_for_playback(&playback, self.play_seconds(interleaved.len()))?;
        thread::sleep(Duration::from_millis(150));
        let _ = stream.pause();
        Ok(())
    }

    /// Primitive 2: record N seconds from the microphone (mono, channel 0).
    #[allow(dead_code)] // primitive reserved for the planned modules
    pub fn record(&self, seconds: f32) -> Result<Vec<f32>> {
        let recorder = Arc::new(Recorder::new(self.input.channels() as usize));
        let stream = build_input_stream(
            &self.input_device,
            &self.input.config,
            self.input.sample_format,
            Arc::clone(&recorder),
        )?;
        stream.play()?;
        thread::sleep(Duration::from_secs_f32(seconds.max(0.0)));
        let _ = stream.pause();
        drop(stream);
        Ok(recorder.take())
    }

    /// Primitive 3: play a buffer while recording.
    ///
    /// Recording starts `PRE_ROLL` early so the input is certainly running, and
    /// keeps going for `tail_s` afterwards. The returned [`Recording`] knows
    /// where playback began.
    pub fn play_and_record(&self, interleaved: &[f32], tail_s: f32) -> Result<Recording> {
        const PRE_ROLL: Duration = Duration::from_millis(400);

        let recorder = Arc::new(Recorder::new(self.input.channels() as usize));
        let input_stream = build_input_stream(
            &self.input_device,
            &self.input.config,
            self.input.sample_format,
            Arc::clone(&recorder),
        )?;
        input_stream.play()?;
        thread::sleep(PRE_ROLL);

        let playback = Arc::new(Playback::new(interleaved.to_vec()));
        let output_stream = build_output_stream(
            &self.output_device,
            &self.output.config,
            self.output.sample_format,
            Arc::clone(&playback),
        )
        .map_err(|e| {
            if self
                .input
                .device_name
                .eq_ignore_ascii_case(&self.output.device_name)
            {
                anyhow!(
                    "{e}\n  Input and output are the same device. Some drivers (typically \
                     ASIO on Windows) refuse that - pick a different output device."
                )
            } else {
                e
            }
        })?;

        let play_secs = self.play_seconds(interleaved.len());
        let play_start = recorder.len();
        output_stream.play()?;

        wait_for_playback(&playback, play_secs)?;
        thread::sleep(Duration::from_secs_f32(tail_s.max(0.0)));

        let _ = output_stream.pause();
        let _ = input_stream.pause();
        drop(output_stream);
        drop(input_stream);

        Ok(Recording {
            samples: recorder.take(),
            sample_rate: self.input.sample_rate(),
            play_start,
            play_secs,
        })
    }

    fn play_seconds(&self, interleaved_len: usize) -> f32 {
        let channels = self.output.channels().max(1) as f32;
        interleaved_len as f32 / channels / self.output.sample_rate() as f32
    }
}

/// Waits until the output callback has consumed the whole buffer.
fn wait_for_playback(playback: &Arc<Playback>, play_secs: f32) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs_f32(play_secs + 10.0);
    while playback.pos.load(Ordering::Acquire) < playback.data.len() {
        if Instant::now() > deadline {
            bail!("playback never started or stalled (timeout)");
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

struct Playback {
    data: Vec<f32>,
    pos: AtomicUsize,
}

impl Playback {
    fn new(data: Vec<f32>) -> Self {
        Self {
            data,
            pos: AtomicUsize::new(0),
        }
    }
}

struct Recorder {
    buf: Mutex<Vec<f32>>,
    channels: usize,
}

impl Recorder {
    fn new(channels: usize) -> Self {
        Self {
            buf: Mutex::new(Vec::new()),
            channels: channels.max(1),
        }
    }

    fn len(&self) -> usize {
        self.buf.lock().map(|b| b.len()).unwrap_or(0)
    }

    fn take(&self) -> Vec<f32> {
        self.buf.lock().map(|b| b.clone()).unwrap_or_default()
    }
}

fn build_output_stream(
    device: &Device,
    config: &StreamConfig,
    format: SampleFormat,
    playback: Arc<Playback>,
) -> Result<Stream> {
    match format {
        SampleFormat::F32 => output_stream::<f32>(device, config, playback),
        SampleFormat::I16 => output_stream::<i16>(device, config, playback),
        SampleFormat::I32 => output_stream::<i32>(device, config, playback),
        SampleFormat::U16 => output_stream::<u16>(device, config, playback),
        SampleFormat::I8 => output_stream::<i8>(device, config, playback),
        SampleFormat::U8 => output_stream::<u8>(device, config, playback),
        other => bail!("unsupported output sample format: {other:?}"),
    }
}

fn output_stream<T>(
    device: &Device,
    config: &StreamConfig,
    playback: Arc<Playback>,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let stream = device.build_output_stream(
        config,
        move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
            let pos = playback.pos.load(Ordering::Acquire);
            let available = playback.data.len().saturating_sub(pos);
            let n = out.len().min(available);
            let (head, tail) = out.split_at_mut(n);
            for (dst, &src) in head.iter_mut().zip(&playback.data[pos..pos + n]) {
                *dst = T::from_sample(src);
            }
            for s in tail.iter_mut() {
                *s = T::EQUILIBRIUM;
            }
            playback.pos.store(pos + n, Ordering::Release);
        },
        |err| eprintln!("  [audio] output stream error: {err}"),
        None,
    )?;
    Ok(stream)
}

fn build_input_stream(
    device: &Device,
    config: &StreamConfig,
    format: SampleFormat,
    recorder: Arc<Recorder>,
) -> Result<Stream> {
    match format {
        SampleFormat::F32 => input_stream::<f32>(device, config, recorder),
        SampleFormat::I16 => input_stream::<i16>(device, config, recorder),
        SampleFormat::I32 => input_stream::<i32>(device, config, recorder),
        SampleFormat::U16 => input_stream::<u16>(device, config, recorder),
        SampleFormat::I8 => input_stream::<i8>(device, config, recorder),
        SampleFormat::U8 => input_stream::<u8>(device, config, recorder),
        other => bail!("unsupported input sample format: {other:?}"),
    }
}

fn input_stream<T>(
    device: &Device,
    config: &StreamConfig,
    recorder: Arc<Recorder>,
) -> Result<Stream>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let channels = recorder.channels;
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            if let Ok(mut buf) = recorder.buf.lock() {
                for frame in data.chunks(channels) {
                    if let Some(&first) = frame.first() {
                        buf.push(f32::from_sample(first));
                    }
                }
            }
        },
        |err| eprintln!("  [audio] input stream error: {err}"),
        None,
    )?;
    Ok(stream)
}
