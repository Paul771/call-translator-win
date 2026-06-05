use cpal::traits::{DeviceTrait, HostTrait};

fn main() {
    let host = cpal::default_host();
    
    println!("=== INPUT DEVICES (ASCII only) ===");
    if let Ok(devices) = host.input_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                // Only show devices with English names
                if name.chars().all(|c| c.is_ascii() || c.is_whitespace() || c == '(' || c == ')' || c == '-' || c == ',') {
                    println!("  - {}", name);
                } else {
                    // Show ASCII part only
                    let ascii_name: String = name.chars().filter(|c| c.is_ascii()).collect();
                    println!("  - {} (non-ASCII chars removed)", ascii_name);
                }
            }
        }
    }
}
