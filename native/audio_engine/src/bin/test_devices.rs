use cpal::traits::{DeviceTrait, HostTrait};

fn main() {
    let host = cpal::default_host();
    
    println!("=== INPUT DEVICES ===");
    if let Ok(devices) = host.input_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                println!("  - {}", name);
            }
        }
    }
    
    println!("\n=== OUTPUT DEVICES ===");
    if let Ok(devices) = host.output_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                println!("  - {}", name);
            }
        }
    }
}
