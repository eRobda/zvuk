//! `AudioContext` holds the selected input device and gives modules two ways
//! to capture: for a fixed number of seconds, or for as long as some piece of
//! work takes.
//!
//! There is deliberately no playback here. The test signal is written to a file
//! by `zvuk generate` and played by the user through the system under test, so
//! the measurement includes the head unit, its processing and the amplifier
//! rather than bypassing them.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{
    BufferSize, Device, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
    SupportedStreamConfig,
};

use crate::measurement::ContextInfo;

/// The configuration of the capture stream.
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

/// A mono recording (channel 0 of the input device).
pub struct Capture {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Capture {
    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate as f32
    }
}

pub struct AudioContext {
    input_device: Device,
    pub input: StreamSpec,
    pub buffer_size: Option<u32>,
}

impl AudioContext {
    pub fn new(
        input_device: Device,
        input_name: String,
        input_config: SupportedStreamConfig,
        buffer_size: Option<u32>,
    ) -> Self {
        let mut config = input_config.config();
        if let Some(frames) = buffer_size {
            config.buffer_size = BufferSize::Fixed(frames);
        }

        Self {
            input: StreamSpec {
                device_name: input_name,
                config,
                sample_format: input_config.sample_format(),
            },
            input_device,
            buffer_size,
        }
    }

    pub fn info(&self) -> ContextInfo {
        ContextInfo {
            input_device: self.input.device_name.clone(),
            input_sample_rate: self.input.sample_rate(),
            input_channels: self.input.channels(),
            buffer_size: self.buffer_size,
        }
    }

    pub fn describe(&self) {
        println!(
            "Input: {} | {} Hz | {} ch | {:?}",
            self.input.device_name,
            self.input.sample_rate(),
            self.input.channels(),
            self.input.sample_format
        );
        match self.buffer_size {
            Some(n) => println!("Buffer: {n} samples (fixed)"),
            None => println!("Buffer: driver default"),
        }
    }

    /// Records for a fixed number of seconds.
    #[allow(dead_code)] // primitive reserved for the planned modules
    pub fn record(&self, seconds: f32) -> Result<Capture> {
        self.record_while(|| {
            thread::sleep(Duration::from_secs_f32(seconds.max(0.0)));
            Ok(())
        })
    }

    /// Records for as long as `work` takes.
    ///
    /// This is how a module waits for the user: `work` prints the instructions
    /// and blocks on Enter, and the capture covers exactly that window.
    pub fn record_while<F>(&self, work: F) -> Result<Capture>
    where
        F: FnOnce() -> Result<()>,
    {
        let recorder = Arc::new(Recorder::new(self.input.channels() as usize));
        let stream = build_input_stream(
            &self.input_device,
            &self.input.config,
            self.input.sample_format,
            Arc::clone(&recorder),
        )?;
        stream.play()?;

        let outcome = work();

        // Let the last callback land before tearing the stream down.
        thread::sleep(Duration::from_millis(100));
        let _ = stream.pause();
        drop(stream);

        outcome?;
        Ok(Capture {
            samples: recorder.take(),
            sample_rate: self.input.sample_rate(),
        })
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

    fn take(&self) -> Vec<f32> {
        self.buf.lock().map(|b| b.clone()).unwrap_or_default()
    }
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
