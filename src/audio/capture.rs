//! Microphone capture via cpal. Only compiled with the `audio` feature
//! (requires ALSA headers on Linux).

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{ensure, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::FromSample;

use super::wav;

/// Captures the default input device into a growing mono `f32` buffer.
pub struct Recorder {
    device: cpal::Device,
    config: cpal::SupportedStreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    stream: Option<cpal::Stream>,
    recording: bool,
}

impl Recorder {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("no default audio input device found")?;
        let config = device
            .default_input_config()
            .context("failed to read default input config")?;
        Ok(Self {
            device,
            config,
            buffer: Arc::new(Mutex::new(Vec::new())),
            stream: None,
            recording: false,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        // cpal 0.18: SampleRate is a plain u32 alias.
        self.config.sample_rate()
    }

    /// Start capturing. Previous buffer contents are discarded.
    pub fn start(&mut self) -> Result<()> {
        ensure!(!self.recording, "already recording");
        self.buffer
            .lock()
            .expect("capture buffer poisoned")
            .clear();

        let buffer = Arc::clone(&self.buffer);
        let channels = self.config.channels() as usize;
        let err_fn = |err| eprintln!("capture stream error: {err}");
        let stream = match self.config.sample_format() {
            cpal::SampleFormat::F32 => {
                self.build_stream::<f32>(buffer, channels, err_fn)?
            }
            cpal::SampleFormat::I16 => {
                self.build_stream::<i16>(buffer, channels, err_fn)?
            }
            cpal::SampleFormat::U16 => {
                self.build_stream::<u16>(buffer, channels, err_fn)?
            }
            other => anyhow::bail!("unsupported capture sample format: {other:?}"),
        };
        stream.play().context("failed to start capture stream")?;
        self.stream = Some(stream);
        self.recording = true;
        Ok(())
    }

    /// Stop capturing and write the buffer as 16-bit PCM WAV. Returns the
    /// sample rate the clip was captured at.
    pub fn stop(&mut self, out: &Path) -> Result<u32> {
        ensure!(self.recording, "not recording");
        self.recording = false;
        drop(self.stream.take());
        let samples = self
            .buffer
            .lock()
            .expect("capture buffer poisoned")
            .clone();
        ensure!(!samples.is_empty(), "no audio captured");
        let rate = self.sample_rate();
        wav::write_wav_i16_mono(out, &samples, rate)?;
        Ok(rate)
    }

    fn build_stream<T>(
        &self,
        buffer: Arc<Mutex<Vec<f32>>>,
        channels: usize,
        err_fn: impl FnMut(cpal::Error) + Send + 'static,
    ) -> Result<cpal::Stream>
    where
        T: cpal::SizedSample,
        f32: FromSample<T>,
    {
        let config: cpal::StreamConfig = self.config.clone().into();
        self.device
            .build_input_stream(
                config,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    let mut buf = buffer.lock().expect("capture buffer poisoned");
                    for frame in data.chunks(channels) {
                        if frame.is_empty() {
                            continue;
                        }
                        // cpal 0.18 wraps dasp_sample, whose conversion method
                        // is `from_sample_` (trailing underscore).
                        let mono = frame
                            .iter()
                            .map(|s| f32::from_sample_(*s))
                            .sum::<f32>()
                            / frame.len() as f32;
                        buf.push(mono);
                    }
                },
                err_fn,
                None,
            )
            .context("failed to build input stream")
    }
}
