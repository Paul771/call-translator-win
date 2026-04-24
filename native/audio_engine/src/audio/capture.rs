use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig};
use crossbeam_channel::Sender;
use log::{debug, error, info};

pub struct AudioChunk {
    pub samples: Vec<f32>,
}

/// Captures audio from a named device and sends chunks to a channel.
pub struct AudioCapture {
    stream: Stream,
    device_name: String,
    sample_rate: u32,
}

impl AudioCapture {
    /// Create capture from a specific device name.
    ///
    /// Uses the device's default configuration (sample rate + channels) to guarantee
    /// compatibility across different devices (built-in mic, headphones, etc.).
    /// Audio is downmixed to mono before sending.
    // ... existing code ...
    pub fn new(device_name: &str, sender: Sender<AudioChunk>) -> Result<Self> {
        let device = find_input_device(device_name)?;
        let actual_name = device.name().unwrap_or_else(|_| "unknown".into());

        let default_cfg = device
            .default_input_config()
            .context("Failed to get default input config")?;

        let channels = default_cfg.channels();
        let sample_rate = default_cfg.sample_rate().0;

        let config = StreamConfig {
            channels,
            sample_rate: default_cfg.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        info!(
            "Opening capture device '{}': rate={}, channels={}",
            actual_name, sample_rate, channels
        );

        // On Windows, we use the provided config. Some devices might require 
        // specific sample rates or buffer sizes.
        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                    // Downmix to mono by averaging channels if needed.
                    let mono: Vec<f32> = if channels == 1 {
                        data.to_vec()
                    } else {
                        data.chunks(channels as usize)
                            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                            .collect()
                    };
                    let chunk = AudioChunk { samples: mono };
                    if let Err(e) = sender.try_send(chunk) {
                        debug!("Capture channel full or disconnected: {}", e);
                    }
                },
                move |err| error!("Capture stream error: {}", err),
                None,
            )
            .context("Failed to build input stream")?;

        Ok(Self { stream, device_name: actual_name, sample_rate })
    }
    // ... existing code ...

    pub fn start(&self) -> Result<()> {
        self.stream.play().context("Failed to start capture stream")?;
        info!("Capture started on '{}'", self.device_name);
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        self.stream.pause().context("Failed to pause capture stream")?;
        info!("Capture stopped on '{}'", self.device_name);
        Ok(())
    }

    /// Actual sample rate the device is running at.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

/// Find an input device by name. `"default"` returns the default input device.
fn find_input_device(name: &str) -> Result<Device> {
    let host = cpal::default_host();

    if name == "default" {
        return host
            .default_input_device()
            .context("No default input device available");
    }

    let devices = host
        .input_devices()
        .context("Failed to enumerate input devices")?;

    let mut available = Vec::new();
    for device in devices {
        let dev_name = device.name().unwrap_or_else(|_| "unknown".into());
        if dev_name == name {
            return Ok(device);
        }
        available.push(dev_name);
    }

    anyhow::bail!(
        "Input device '{}' not found. Available input devices: {:?}",
        name,
        available
    )
}
