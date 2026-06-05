use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn main() {
    eprintln!("Minimal test - no Device calls before build");
    let host = cpal::default_host();
    
    match host.default_input_device() {
        Some(dev) => {
            // NO calls to dev at all - go straight to build
            let scfg = cpal::StreamConfig {
                channels: 1,
                sample_rate: cpal::SampleRate(16000),
                buffer_size: cpal::BufferSize::Fixed(320),
            };
            
            eprintln!("Building stream immediately...");
            let counter = Arc::new(AtomicUsize::new(0));
            let counter2 = counter.clone();
            
            match dev.build_input_stream(
                &scfg,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let prev = counter2.fetch_add(data.len(), Ordering::SeqCst);
                    if prev < 32000 {
                        eprintln!("Got {} samples (total: {})", data.len(), prev + data.len());
                    }
                },
                |err| eprintln!("Stream err: {}", err),
                None,
            ) {
                Ok(stream) => {
                    eprintln!("Stream created! Playing 8s...");
                    stream.play().expect("play failed");
                    std::thread::sleep(Duration::from_secs(8));
                    stream.pause().expect("pause failed");
                    let total = counter.load(Ordering::SeqCst);
                    eprintln!("Done. Total samples: {}", total);
                }
                Err(e) => eprintln!("Stream build error: {}", e),
            }
        }
        None => eprintln!("No default input device"),
    }
    eprintln!("Complete.");
}
