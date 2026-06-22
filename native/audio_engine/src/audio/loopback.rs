use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender as StdSender;
use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use windows::Win32::Media::Audio::*;
use windows::Win32::Devices::FunctionDiscovery::*;
use windows::Win32::System::Com::*;

use super::capture::AudioChunk;

const REFTIMES_PER_SEC: i64 = 10_000_000;

pub struct LoopbackCapture {
    handle: Option<std::thread::JoinHandle<()>>,
    device_name: String,
    sample_rate: u32,
}

impl LoopbackCapture {
    pub fn new(
        device_name: &str,
        sender: StdSender<AudioChunk>,
        stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<Self> {
        let name = device_name.to_string();
        info!("[loopback] Initializing WASAPI loopback capture for '{}'", device_name);

        let handle = std::thread::Builder::new()
            .name(format!("wasapi-loopback-{}", device_name))
            .spawn(move || {
                if let Err(e) = run_loopback_inner(&name, &sender, &stop_flag) {
                    error!("[loopback] Loopback capture failed: {:#}", e);
                }
                info!("[loopback] Loopback capture thread exiting");
            })
            .context("Failed to spawn loopback capture thread")?;

        Ok(Self {
            handle: Some(handle),
            device_name: device_name.to_string(),
            sample_rate: 48000,
        })
    }

    pub fn start(&self) -> Result<()> {
        info!("[loopback] Capture started on '{}'", self.device_name);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        info!("[loopback] Stopping capture on '{}'", self.device_name);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        Ok(())
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

fn run_loopback_inner(
    device_name: &str,
    sender: &StdSender<AudioChunk>,
    stop_flag: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if hr.is_err() {
            anyhow::bail!("[loopback] CoInitializeEx failed: {:?}", hr);
        }

        let enumerator: IMMDeviceEnumerator = CoCreateInstance(
            &MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        ).context("[loopback] Failed to create IMMDeviceEnumerator")?;

        let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .context("[loopback] Failed to enumerate render endpoints")?;

        let count = devices.GetCount().context("[loopback] Failed to get device count")?;
        info!("[loopback] Found {} output devices", count);

        let mut target_device: Option<IMMDevice> = None;

        for i in 0..count {
            let device = devices.Item(i).context("[loopback] Failed to get device")?;
            let props = device.OpenPropertyStore(STGM_READ)
                .context("[loopback] Failed to open property store")?;
            let name_val = props.GetValue(&PKEY_Device_FriendlyName)
                .context("[loopback] Failed to get device name")?;
            let friendly_name = name_val.to_string();

            debug!("[loopback]   Output device {}: '{}'", i, friendly_name);

            if friendly_name.contains(device_name) || device_name.contains(&friendly_name) {
                info!("[loopback] Found target output device for loopback: '{}'", friendly_name);
                target_device = Some(device);
                break;
            }
        }

        let device = target_device
            .ok_or_else(|| anyhow::anyhow!("[loopback] Output device '{}' not found", device_name))?;

        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .context("[loopback] Failed to activate IAudioClient")?;

        let mix_format = client.GetMixFormat()
            .context("[loopback] Failed to get mix format")?;
        let channels = (*mix_format).nChannels;
        let sample_rate = (*mix_format).nSamplesPerSec;
        let bits_per_sample = (*mix_format).wBitsPerSample;

        info!("[loopback] Mix format: rate={}Hz, channels={}, bits={}", sample_rate, channels, bits_per_sample);

        let stream_flags: u32 = AUDCLNT_STREAMFLAGS_LOOPBACK;

        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            stream_flags,
            REFTIMES_PER_SEC,
            0,
            mix_format,
            Some(std::ptr::null()),
        ).context("[loopback] Failed to initialize loopback AudioClient")?;

        let capture_client: IAudioCaptureClient = client
            .GetService()
            .context("[loopback] Failed to get IAudioCaptureClient")?;

        client.Start().context("[loopback] Failed to start loopback stream")?;
        info!("[loopback] WASAPI loopback stream started on '{}'", device_name);

        let frame_size = channels as usize * bits_per_sample as usize / 8;

        loop {
            if stop_flag.load(Ordering::SeqCst) {
                info!("[loopback] Stop flag set, exiting");
                break;
            }

            let packet_length = capture_client.GetNextPacketSize()
                .context("[loopback] Failed to get packet size")?;
            if packet_length == 0 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }

            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut num_frames = 0u32;
            let mut flags: u32 = 0;

            capture_client.GetBuffer(
                &mut data_ptr,
                &mut num_frames,
                &mut flags,
                Some(std::ptr::null_mut()),
                Some(std::ptr::null_mut()),
            ).context("[loopback] Failed to get capture buffer")?;

            if num_frames > 0 && !data_ptr.is_null() {
                let byte_count = num_frames as usize * frame_size;
                let data_slice = std::slice::from_raw_parts(data_ptr, byte_count);

                let mono_samples = convert_to_mono_f32(data_slice, channels, bits_per_sample);

                static LOOPBACK_COUNT: AtomicU64 = AtomicU64::new(0);
                let count = LOOPBACK_COUNT.fetch_add(1, Ordering::Relaxed);
                let rms = (mono_samples.iter().map(|s| s * s).sum::<f32>() / mono_samples.len().max(1) as f32).sqrt();
                if count % 200 == 0 || rms > 0.001 {
                    debug!("[loopback] chunk #{} ({} samples, rms={:.6})", count + 1, mono_samples.len(), rms);
                }

                let chunk = AudioChunk { samples: mono_samples };
                if sender.send(chunk).is_err() {
                    debug!("[loopback] Receiver dropped, stopping");
                    break;
                }
            }

            capture_client.ReleaseBuffer(num_frames)
                .context("[loopback] Failed to release capture buffer")?;
        }

        client.Stop().context("[loopback] Failed to stop loopback stream")?;
        info!("[loopback] WASAPI loopback stream stopped");
    }

    Ok(())
}

fn convert_to_mono_f32(data: &[u8], channels: u16, bits_per_sample: u16) -> Vec<f32> {
    match bits_per_sample {
        16 => {
            let samples: Vec<i16> = data.chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            if channels == 1 {
                samples.iter().map(|&s| s as f32 / 32768.0).collect()
            } else {
                samples.chunks(channels as usize)
                    .map(|frame| frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / channels as f32)
                    .collect()
            }
        }
        24 => {
            let samples: Vec<f32> = data.chunks_exact(3)
                .map(|b| {
                    let val = (b[0] as i32) | ((b[1] as i32) << 8) | ((b[2] as i32) << 16);
                    let val = if val & 0x800000 != 0 { val | !0xFFFFFF } else { val };
                    val as f32 / 8388608.0
                })
                .collect();
            if channels == 1 {
                samples
            } else {
                samples.chunks(channels as usize)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect()
            }
        }
        32 => {
            let samples: Vec<f32> = data.chunks_exact(4)
                .map(|b| {
                    let val = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                    val as f32 / 2147483648.0
                })
                .collect();
            if channels == 1 {
                samples
            } else {
                samples.chunks(channels as usize)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect()
            }
        }
        _ => {
            warn!("[loopback] Unsupported bits_per_sample: {}, treating as silence", bits_per_sample);
            vec![0.0; data.len() / (bits_per_sample as usize / 8).max(1)]
        }
    }
}
