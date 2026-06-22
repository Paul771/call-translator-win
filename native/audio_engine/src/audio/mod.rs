pub mod capture;
pub mod loopback;
pub mod playback;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};


/// List all available audio devices (useful for debugging).
/// Returns (input_names, output_names).
pub fn list_devices() -> Result<(Vec<String>, Vec<String>)> {
    let host = cpal::default_host();

    let mut input_names = Vec::new();
    let mut output_names = Vec::new();
    let mut lines = Vec::new();

    if let Some(dev) = host.default_input_device() {
        let name = dev.name().unwrap_or_else(|_| "unknown".into());
        lines.push(format!("Default INPUT device: {}", name));
    } else {
        lines.push("No default input device".into());
    }

    if let Some(dev) = host.default_output_device() {
        let name = dev.name().unwrap_or_else(|_| "unknown".into());
        lines.push(format!("Default OUTPUT device: {}", name));
    } else {
        lines.push("No default output device".into());
    }

    let inputs = host
        .input_devices()
        .context("Failed to enumerate input devices")?;

    lines.push("Available INPUT devices:".into());
    for device in inputs {
        let name = device.name().unwrap_or_else(|_| "unknown".into());
        lines.push(format!("  '{}'", name));
        input_names.push(name);
    }

    let outputs = host
        .output_devices()
        .context("Failed to enumerate output devices")?;

    lines.push("Available OUTPUT devices:".into());
    for device in outputs {
        let name = device.name().unwrap_or_else(|_| "unknown".into());
        lines.push(format!("  '{}'", name));
        output_names.push(name);
    }

    let msg = lines.join("\n");
    // Print to stderr and log file
    eprintln!("{}", msg);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true).open("deepgram_debug.log")
    {
        use std::io::Write;
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_secs();
        let _ = writeln!(f, "[{}] AUDIO DEVICES:\n{}", secs, msg);
    }

    Ok((input_names, output_names))
}
