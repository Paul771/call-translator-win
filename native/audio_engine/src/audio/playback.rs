use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig};
use crossbeam_channel::Receiver;
use log::{debug, error, info};
use ringbuf::{
    traits::{Consumer, Producer, Split},
    HeapRb,
};

use crate::tracelog;

/// Plays audio received from a channel to a named output device.
#[allow(dead_code)]
pub struct AudioPlayback {
    stream: Stream,
    device_name: String,
    config: StreamConfig,
    feeder: std::thread::JoinHandle<()>,
}

impl AudioPlayback {
    /// Create playback to a specific device name.
    ///
    /// `device_name`: `"default"` for default output, or a specific name like `"BlackHole 2ch"`.
    /// `sample_rate`: desired sample rate (e.g. 48000).
    /// `receiver`: channel providing audio sample buffers to play.
    pub fn new(device_name: &str, _sample_rate: u32, receiver: Receiver<Vec<f32>>) -> Result<Self> {
        let device = find_output_device(device_name)?;
        let actual_name = device
            .name()
            .unwrap_or_else(|_| "unknown".into());

        // Use the device's DEFAULT config — this is the rate the hardware actually uses.
        // Trying to force a different rate via WASAPI often silently fails and the device
        // uses its native rate, causing audio to play at wrong speed.
        let default_config = device
            .default_output_config()
            .context("Failed to get default output config")?;

        let native_rate = default_config.sample_rate().0;
        let channels = default_config.channels();

        let config = StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(native_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let actual_sample_rate = native_rate;

        tracelog::trace("playback", "DEVICE", &format!(
            "Playback device '{}' native_rate={}Hz (requested {}Hz)",
            actual_name, actual_sample_rate, _sample_rate
        ));
        let ring_size = actual_sample_rate as usize * 10;
        let ring = HeapRb::<f32>::new(ring_size);
        let (mut producer, mut consumer) = ring.split();

        let feeder_name = actual_name.clone();
        let feeder = std::thread::Builder::new()
            .name(format!("playback-feeder-{}", feeder_name))
            .spawn(move || {
                debug!("Playback feeder thread started for '{}'", feeder_name);
                loop {
                    match receiver.recv() {
                        Ok(samples) => {
                            let _ = producer.push_slice(&samples);
                        }
                        Err(_) => {
                            debug!("Playback feeder: channel disconnected, stopping");
                            break;
                        }
                    }
                }
            })
            .context("Failed to spawn playback feeder thread")?;

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                    let filled = consumer.pop_slice(data);
                    if filled < data.len() {
                        for sample in &mut data[filled..] {
                            *sample = 0.0;
                        }
                    }
                },
                move |err| {
                    error!("Playback stream error: {}", err);
                },
                None,
            )
            .context("Failed to build output stream")?;

        Ok(Self {
            stream,
            device_name: actual_name,
            config,
            feeder,
        })
    }

    pub fn start(&self) -> Result<()> {
        self.stream
            .play()
            .context("Failed to start playback stream")?;
        info!("Playback started on '{}'", self.device_name);
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        self.stream
            .pause()
            .context("Failed to pause playback stream")?;
        info!("Playback stopped on '{}'", self.device_name);
        Ok(())
    }

    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate.0
    }
}

/// Find an output device by name. `"default"` returns the default output device.
fn find_output_device(name: &str) -> Result<Device> {
    let host = cpal::default_host();

    if name == "default" {
        return host
            .default_output_device()
            .context("No default output device available");
    }

    let devices = host
        .output_devices()
        .context("Failed to enumerate output devices")?;

    let mut available = Vec::new();
    for device in devices {
        let dev_name = device.name().unwrap_or_else(|_| "unknown".into());
        if dev_name == name {
            return Ok(device);
        }
        available.push(dev_name);
    }

    anyhow::bail!(
        "Output device '{}' not found. Available output devices: {:?}",
        name,
        available
    )
}