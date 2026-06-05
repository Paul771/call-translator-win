use cpal::traits::{DeviceTrait, HostTrait};

fn main() {
    let host = cpal::default_host();
    let device_name = "Микрофон (Jabra EVOLVE 20)";
    
    println!("Trying to find device: {}", device_name);
    
    let mut found = false;
    if let Ok(devices) = host.input_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                println!("Found device: {}", name);
                if name == device_name {
                    found = true;
                    println!("SUCCESS: Found matching device!");
                    
                    // Try to get default config
                    match dev.default_input_config() {
                        Ok(config) => {
                            println!("  Sample rate: {}", config.sample_rate().0);
                            println!("  Channels: {}", config.channels());
                        }
                        Err(e) => println!("  Error getting config: {}", e),
                    }
                    
                    // Try to build input stream
                    let config = dev.default_input_config().unwrap();
                    let stream_config = cpal::StreamConfig {
                        channels: config.channels(),
                        sample_rate: config.sample_rate(),
                        buffer_size: cpal::BufferSize::Default,
                    };
                    
                    println!("  Trying to build input stream...");
                    match dev.build_input_stream(
                        &stream_config,
                        |_data: &[f32], _: &cpal::InputCallbackInfo| {
                            // Silent callback
                        },
                        |err| eprintln!("Stream error: {}", err),
                        None,
                    ) {
                        Ok(_stream) => {
                            println!("  SUCCESS: Stream built!");
                            // Keep stream alive
                            std::thread::sleep(std::time::Duration::from_secs(2));
                        }
                        Err(e) => println!("  ERROR building stream: {}", e),
                    }
                    break;
                }
            }
        }
    }
    
    if !found {
        println!("ERROR: Device '{}' not found!", device_name);
    }
}
