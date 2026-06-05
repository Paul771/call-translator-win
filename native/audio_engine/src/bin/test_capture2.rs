use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let deepgram_key = std::env::var("DEEPGRAM_API_KEY").unwrap_or_else(|_| "NOT SET".to_string());
    println!("DEEPGRAM_API_KEY: {}", if deepgram_key.len() > 10 { format!("{}...", &deepgram_key[..10]) } else { deepgram_key.clone() });
    
    let device_name = "Микрофон (Jabra EVOLVE 20)";
    println!("Finding device: {}", device_name);
    
    let host = cpal::default_host();
    let mut device = None;
    
    if let Ok(devices) = host.input_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                if name == device_name {
                    device = Some(dev);
                    break;
                }
            }
        }
    }
    
    let device = match device {
        Some(d) => d,
        None => {
            println!("ERROR: Device not found!");
            return;
        }
    };
    
    let config = device.default_input_config().unwrap();
    println!("Sample rate: {}, Channels: {}", config.sample_rate().0, config.channels());
    
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    
    let stream_config = cpal::StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };
    
    println!("Building stream...");
    let stream = device.build_input_stream(
        &stream_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let count = counter_clone.fetch_add(data.len(), Ordering::SeqCst);
            if count < 1000 {
                println!("Received {} samples, total: {}", data.len(), count + data.len());
            }
        },
        |err| eprintln!("Stream error: {}", err),
        None,
    );
    
    match stream {
        Ok(s) => {
            println!("Stream created successfully!");
            println!("Starting stream for 10 seconds, SPEAK NOW...");
            s.play().unwrap();
            std::thread::sleep(Duration::from_secs(10));
            s.pause().unwrap();
            let total = counter.load(Ordering::SeqCst);
            println!("Stopped. Total samples received: {}", total);
        }
        Err(e) => {
            println!("ERROR creating stream: {}", e);
        }
    }
}
