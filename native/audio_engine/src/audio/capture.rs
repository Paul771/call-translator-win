use std::sync::atomic::{AtomicU64, Ordering};
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig};
use crossbeam_channel::Sender;
use log::{debug, error, info};
use std::time::Duration;

pub struct AudioChunk {
    pub samples: Vec<f32>,
}

fn log_file_cap(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true).open("deepgram_debug.log")
    {
        use std::io::Write;
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_secs();
        let _ = writeln!(f, "[{}] {}", secs, msg);
    }
}

pub static CAPTURE_CHUNK_COUNT: AtomicU64 = AtomicU64::new(0);

pub struct AudioCapture {
    stream: Option<Stream>,
    device_name: String,
    sample_rate: u32,
}

impl AudioCapture {
    pub fn new(device_name: &str, sender: Sender<AudioChunk>) -> Result<Self> {
        let device = find_input_device(device_name)?;
        let actual_name = device.name().unwrap_or_else(|_| "unknown".into());

        let default_cfg = device
            .default_input_config()
            .context("Failed to get default input config")?;

        let channels = default_cfg.channels();
        let sample_rate = default_cfg.sample_rate().0;

        let cfg_msg = format!(
            "Device '{}' default config: rate={}Hz, channels={}, sample_format={:?}",
            actual_name, sample_rate, channels, default_cfg.sample_format()
        );
        info!("{}", cfg_msg);
        log_file_cap(&cfg_msg);

        let config = StreamConfig {
            channels,
            sample_rate: default_cfg.sample_rate(),
            buffer_size: cpal::BufferSize::Fixed(320),
        };

        info!(
            "Opening capture device '{}': rate={}, channels={}",
            actual_name, sample_rate, channels
        );

        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                    let prev = CAPTURE_CHUNK_COUNT.fetch_add(1, Ordering::Relaxed);
                    let first = data.first().copied().unwrap_or(0.0);
                    let rms_raw = (data.iter().map(|s| s * s).sum::<f32>() / data.len() as f32).sqrt();
                    if prev == 0 || prev % 50 == 0 || rms_raw > 0.001 {
                        let msg = format!("Capture chunk #{} ({} samples, first={:.4}, rms_raw={:.6})", prev + 1, data.len(), first, rms_raw);
                        info!("{}", msg);
                        log_file_cap(&msg);
                    }
                    // Dynamic gain normalization: scale audio to use full dynamic range
                    let rms = (data.iter().map(|s| s * s).sum::<f32>() / data.len() as f32).sqrt();
                    let target_rms = 0.25; // Target RMS for speech (25% of full scale)
                    let adaptive_gain = if rms > 0.00001 { (target_rms / rms).min(100.0).max(1.0) } else { 50.0 };
                    let effective_gain = adaptive_gain;
                    let mono: Vec<f32> = if channels == 1 {
                        data.iter().map(|&s| (s * effective_gain).clamp(-1.0, 1.0)).collect()
                    } else {
                        data.chunks(channels as usize)
                            .map(|frame| (frame.iter().sum::<f32>() / channels as f32 * effective_gain).clamp(-1.0, 1.0))
                            .collect()
                    };
                    if prev % 200 == 0 || rms > 0.001 {
                        info!("[{}] gain={:.1}, rms={:.6}, adaptive_gain={:.1}, first_sample={:.4}", prev+1, effective_gain, rms, adaptive_gain, data.first().copied().unwrap_or(0.0));
                    }
                    let chunk = AudioChunk { samples: mono };
                    if let Err(e) = sender.try_send(chunk) {
                        debug!("Capture channel full or disconnected: {}", e);
                    }
                },
                move |err| error!("Capture stream error: {}", err),
                None,
            )
            .context("Failed to build input stream")?;

        Ok(Self { stream: Some(stream), device_name: actual_name, sample_rate })
    }

    pub fn start(&self) -> Result<()> {
        if let Some(ref stream) = self.stream {
            stream.play().context("Failed to start capture stream")?;
        }
        info!("Capture started on '{}'", self.device_name);
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        if let Some(ref stream) = self.stream {
            stream.pause().context("Failed to pause capture stream")?;
        }
        info!("Capture stopped on '{}'", self.device_name);
        Ok(())
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

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
