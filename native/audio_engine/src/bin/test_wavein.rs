use windows::Win32::Media::Audio::*;
use windows::core::PSTR;
use std::mem;
use std::ptr;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("=== waveIn capture test ===");
    
    let n_devices = unsafe { waveInGetNumDevs() };
    eprintln!("Number of waveIn input devices: {}", n_devices);
    
    for i in 0..n_devices {
        let mut caps = WAVEINCAPSW::default();
        let rc = unsafe { waveInGetDevCapsW(i as usize, &mut caps, mem::size_of::<WAVEINCAPSW>() as u32) };
        if rc == 0 {
            let arr = caps.szPname;
            let name = String::from_utf16_lossy(&arr);
            eprintln!("  [{}] '{}'", i, name.trim_end_matches('\0').trim());
        }
    }
    
    // Test each format individually to avoid packed struct alignment issues
    let formats: Vec<(&str, u32, u16, u32, u32, u16, u16)> = vec![
        ("16kHz 16-bit mono", 1, 1, 16000, 32000, 2, 16),
        ("44.1kHz 16-bit mono", 1, 1, 44100, 88200, 2, 16),
        ("48kHz 16-bit mono", 1, 1, 48000, 96000, 2, 16),
        ("44.1kHz 16-bit stereo", 1, 2, 44100, 176400, 4, 16),
    ];
    
    for (desc, tag, ch, sr, avg, align, bits) in &formats {
        let wf = Box::new(WAVEFORMATEX {
            wFormatTag: *tag as u16,
            nChannels: *ch,
            nSamplesPerSec: *sr,
            nAvgBytesPerSec: *avg,
            nBlockAlign: *align,
            wBitsPerSample: *bits,
            cbSize: 0,
        });
        
        for dev_id in 0..n_devices {
            let mut h = HWAVEIN(ptr::null_mut());
            let rc = unsafe { waveInOpen(Some(&mut h), dev_id, &*wf, None, None, CALLBACK_NULL) };
            if rc == 0 {
                eprintln!("SUCCESS: Device {} with '{}'", dev_id, desc);
                run_capture(&mut h)?;
                unsafe { waveInClose(h); }
                eprintln!("Test complete.");
                return Ok(());
            }
        }
        
        // Also try WAVE_MAPPER
        let mut h = HWAVEIN(ptr::null_mut());
        let rc = unsafe { waveInOpen(Some(&mut h), WAVE_MAPPER, &*wf, None, None, CALLBACK_NULL) };
        if rc == 0 {
            eprintln!("SUCCESS: WAVE_MAPPER with '{}'", desc);
            run_capture(&mut h)?;
            unsafe { waveInClose(h); }
            eprintln!("Test complete.");
            return Ok(());
        }
    }
    
    eprintln!("All formats failed on all devices!");
    Ok(())
}

fn run_capture(handle: &mut HWAVEIN) -> Result<(), Box<dyn std::error::Error>> {
    let buf_frames = 320u32;
    let buf_bytes = (buf_frames * 2) as usize;
    
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    let mut headers: Vec<WAVEHDR> = Vec::new();
    
    for i in 0..4 {
        let mut buf = vec![0u8; buf_bytes as usize];
        let hdr = WAVEHDR {
            lpData: PSTR(buf.as_mut_ptr()),
            dwBufferLength: buf_bytes as u32,
            dwBytesRecorded: 0,
            dwUser: i,
            dwFlags: 0,
            dwLoops: 0,
            lpNext: ptr::null_mut(),
            reserved: 0,
        };
        buffers.push(buf);
        headers.push(hdr);
        
        unsafe {
            waveInPrepareHeader(*handle, &mut headers[i], mem::size_of::<WAVEHDR>() as u32);
            waveInAddBuffer(*handle, &mut headers[i], mem::size_of::<WAVEHDR>() as u32);
        }
    }
    
    eprintln!("Starting capture...");
    unsafe { waveInStart(*handle); }
    eprintln!("Recording 5s...");
    
    let start = std::time::Instant::now();
    let mut total_bufs = 0u64;
    
    while start.elapsed() < Duration::from_secs(5) {
        for i in 0..4 {
            let mut hdr = headers[i];
            if (hdr.dwFlags & WHDR_DONE) != 0 {
                total_bufs += 1;
                let recorded = hdr.dwBytesRecorded;
                let rms = compute_rms(&buffers[i]);
                if total_bufs <= 10 || total_bufs % 100 == 0 {
                    eprintln!("Buf #{}: {} bytes, rms={:.6}", total_bufs, recorded, rms);
                }
                hdr.dwFlags = 0;
                hdr.dwBytesRecorded = 0;
                headers[i] = hdr;
                unsafe {
                    waveInAddBuffer(*handle, &mut headers[i], mem::size_of::<WAVEHDR>() as u32);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    
    unsafe { waveInStop(*handle); }
    eprintln!("Done! Total buffers: {}", total_bufs);
    
    for i in 0..4 {
        unsafe {
            waveInUnprepareHeader(*handle, &mut headers[i], mem::size_of::<WAVEHDR>() as u32);
        }
    }
    unsafe { waveInClose(*handle); }
    eprintln!("Closed.");
    Ok(())
}

fn compute_rms(buf: &[u8]) -> f64 {
    let samples: Vec<i16> = buf.chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| i16::from_ne_bytes([c[0], c[1]]))
        .collect();
    if samples.is_empty() { return 0.0; }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
    (sum_sq / samples.len() as f64).sqrt()
}
