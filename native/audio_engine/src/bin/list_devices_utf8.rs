use cpal::traits::{DeviceTrait, HostTrait};
use std::fs::File;
use std::io::Write;

fn main() {
    let host = cpal::default_host();
    let mut file = File::create("devices.txt").expect("Failed to create file");
    
    writeln!(file, "=== INPUT DEVICES ===").unwrap();
    if let Ok(devices) = host.input_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                writeln!(file, "  - {}", name).unwrap();
                println!("  - {}", name);
            }
        }
    }
    
    writeln!(file, "\n=== OUTPUT DEVICES ===").unwrap();
    if let Ok(devices) = host.output_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                writeln!(file, "  - {}", name).unwrap();
                println!("  - {}", name);
            }
        }
    }
    
    println!("\nDevices saved to devices.txt");
}
