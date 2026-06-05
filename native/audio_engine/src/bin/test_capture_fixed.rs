use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let host = cpal::default_host();
    
    let devices: Vec<_> = host.input_devices()
        .expect("no input devices")
        .filter_map(|d| d.name().ok().map(|n| (d, n)))
        .collect();
    
    println!("=== Available INPUT devices ===");
    for (ref _d, name) in &devices {
        println!("  '{}'", name);
    }
    
    let patterns = ["default", "Realtek(R) Audio", "CABLE Output", "Jabra"];
    
    for pat in &patterns {
        println!("\n--- Trying: '{}' ---", pat);
        
        let found_dev = if *pat == "default" {
            host.default_input_device()
        } else {
            devices.iter()
                .find(|(_, n)| n.contains(pat))
                .map(|(d, _)| d.clone())
        };
        
        match found_dev {
            Some(dev) => {
                let name = dev.name().unwrap_or_default();
                println!("Found: '{}'", name);
                
                match dev.default_input_config() {
                    Ok(cfg) => {
                        println!("  Config: {}Hz, {}ch, {:?}", cfg.sample_rate().0, cfg.channels(), cfg.sample_format());
                        
                        let stream_config = cpal::StreamConfig {
                            channels: cfg.channels(),
                            sample_rate: cfg.sample_rate(),
                            buffer_size: cpal::BufferSize::Fixed(320),
                        };
                        
                        println!("  Building stream with Fixed(320)...");
                        let counter = Arc::new(AtomicUsize::new(0));
                        let counter2 = counter.clone();
                        
                        match dev.build_input_stream(
                            &stream_config,
                            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                let prev = counter2.fetch_add(data.len(), Ordering::SeqCst);
                                if prev < 32000 {
                                    println!("  Got {} samples (total: {})", data.len(), prev + data.len());
                                }
                            },
                            |err| eprintln!("  Error: {}", err),
                            None,
                        ) {
                            Ok(stream) => {
                                println!("  Stream created! Playing for 5 seconds...");
                                stream.play().expect("play failed");
                                std::thread::sleep(Duration::from_secs(5));
                                stream.pause().expect("pause failed");
                                let total = counter.load(Ordering::SeqCst);
                                println!("  Done. Total samples: {}", total);
                            }
                            Err(e) => println!("  FAILED: {}", e),
                        }
                    }
                    Err(e) => println!("  No config: {}", e),
                }
            }
            None => println!("  Not found"),
        }
    }
}
