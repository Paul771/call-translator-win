pub mod capture;
pub mod playback;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use log::info;

/// List all available audio devices (useful for debugging).
/// Returns (input_names, output_names).
pub fn list_devices() -> Result<(Vec<String>, Vec<String>)> {
    let host = cpal::default_host();

    let mut input_names = Vec::new();
    let mut output_names = Vec::new();

    if let Some(dev) = host.default_input_device() {
        let name = dev.name().unwrap_or_else(|_| "unknown".into());
        // Filter out devices with invalid UTF-8 sequences or control characters
        if name.chars().all(|c| !c.is_control() || c.is_whitespace()) {
            info!("Default input device: {}", name);
        } else {
            info!("Default input device: {}", name);
        }
    }

    if let Some(dev) = host.default_output_device() {
        let name = dev.name().unwrap_or_else(|_| "unknown".into());
        // Filter out devices with invalid UTF-8 sequences or control characters
        if name.chars().all(|c| !c.is_control() || c.is_whitespace()) {
            info!("Default output device: {}", name);
        } else {
            info!("Default output device: {}", name);
        }
    }

    let inputs = host
        .input_devices()
        .context("Failed to enumerate input devices")?;

    info!("Available input devices:");
    for device in inputs {
        let name = device.name().unwrap_or_else(|_| "unknown".into());
        // Filter out devices with invalid UTF-8 sequences or control characters
        if name.chars().all(|c| !c.is_control() || c.is_whitespace()) {
            info!("  - {}", name);
            input_names.push(name);
        } else {
            let clean_name = name
                .chars()
                .filter(|c| !c.is_control() || c.is_whitespace())
                .collect::<String>();
            info!("  - {} (cleaned)", clean_name);
            input_names.push(clean_name);
        }
    }

    let outputs = host
        .output_devices()
        .context("Failed to enumerate output devices")?;

    info!("Available output devices:");
    for device in outputs {
        let name = device.name().unwrap_or_else(|_| "unknown".into());
        // Filter out devices with invalid UTF-8 sequences or control characters
        if name.chars().all(|c| !c.is_control() || c.is_whitespace()) {
            info!("  - {}", name);
            output_names.push(name);
        } else {
            let clean_name = name
                .chars()
                .filter(|c| !c.is_control() || c.is_whitespace())
                .collect::<String>();
            info!("  - {} (cleaned)", clean_name);
            output_names.push(clean_name);
        }
    }

    Ok((input_names, output_names))
}
