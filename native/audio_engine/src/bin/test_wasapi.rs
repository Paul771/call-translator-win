use wasapi::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    initialize_mta();
    
    let enumerator = DeviceEnumerator::new()?;
    let collection = enumerator.get_device_collection(&Direction::Capture)?;
    let n = collection.get_nbr_devices()?;
    eprintln!("Testing {} capture devices...", n);
    
    for i in 0..n {
        let dev = collection.get_device_at_index(i)?;
        let name = dev.get_friendlyname()?;
        
        let mut client = dev.get_iaudioclient()?;
        let mix = client.get_mixformat()?;
        let sr = mix.get_samplespersec();
        let ch = mix.get_nchannels();
        let bits = mix.get_bitspersample();
        
        let wf = mix.to_waveformatex().unwrap_or_else(|_| {
            WaveFormat::new(bits as usize, bits as usize, &SampleType::Float, sr as usize, ch as usize, None)
        });
        
        let result = client.initialize_client(&wf, &Direction::Capture, &StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: 200_000,
        });
        
        match result {
            Ok(()) => {
                eprintln!("[{}] '{}': SUCCESS! {}Hz {}ch {}bit", i, name, sr, ch, bits);
                let capture = client.get_audiocaptureclient()?;
                client.start_stream()?;
                std::thread::sleep(std::time::Duration::from_millis(100));
                if let Ok(Some(size)) = capture.get_next_packet_size() {
                    eprintln!("  -> First packet: {} frames", size);
                }
                client.stop_stream()?;
                return Ok(());
            }
            Err(e) => {
                eprintln!("[{}] '{}': FAILED - {}", i, name, e);
            }
        }
    }
    
    eprintln!("No device worked!");
    deinitialize();
    Ok(())
}
