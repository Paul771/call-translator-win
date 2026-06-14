mod audio;
mod engine;
mod protocol;
mod stt;
mod tracelog;
mod translation;
mod tts;

use std::io::{BufReader, BufWriter, Write};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossbeam_channel::bounded;
use cpal::traits::{DeviceTrait, HostTrait};
use log::{debug, error, info};

use crate::engine::{Engine, EngineConfig};
use crate::protocol::{read_command, write_event, Event};

fn log_audio_devices() {
    let host = cpal::default_host();
    tracelog::trace("engine", "DEVICE", &format!("Available audio devices on host '{}':", host.id().name()));
    if let Ok(devices) = host.devices() {
        for device in devices {
            let name = device.name().unwrap_or_else(|_| "?".into());
            let is_input = device.supported_input_configs().map(|mut c| c.next().is_some()).unwrap_or(false);
            let is_output = device.supported_output_configs().map(|mut c| c.next().is_some()).unwrap_or(false);
            tracelog::trace("engine", "DEVICE", &format!("  {} (input={}, output={})", name, is_input, is_output));
        }
    }
    tracelog::trace("engine", "DEVICE", "--- end device list ---");
    info!("Audio device enumeration complete");
}

fn main() -> Result<()> {
    // Logger writes to stderr — stdout is reserved for the protocol channel.
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stderr)
        .init();

    info!("audio_engine starting");

    // Initialize pipeline trace logger (clears old log, prunes >24h logs)
    tracelog::init_log();
    tracelog::prune_old_logs();
    tracelog::trace("engine", "INIT", "audio_engine started, trace logging active");

    // Log key env vars for device routing diagnostics
    for var in &["TRANSLATOR_MIC_DEVICE", "TRANSLATOR_SPEAKER_DEVICE",
                 "TRANSLATOR_MEET_INPUT", "TRANSLATOR_MEET_OUTPUT",
                 "DEEPGRAM_API_KEY", "GROQ_API_KEY",
                 "TRANSLATOR_MY_LANG", "TRANSLATOR_THEIR_LANG"] {
        let val = std::env::var(var).unwrap_or_default();
        let display = if var.contains("API_KEY") && val.len() > 8 {
            format!("{}...", &val[..8])
        } else {
            val
        };
        tracelog::trace("engine", "ENV", &format!("{}='{}'", var, display));
    }
    log_audio_devices();

    // ONNX Runtime init is deferred to the TTS preload thread (engine.rs)
    // to avoid COM conflicts with WASAPI audio capture.
    // ort::init().commit() is called inside the TTS init worker.
    info!("ONNX Runtime init deferred to TTS preload thread");
    
    // Channel for events from pipeline threads -> stdout writer

    // Channel for events from pipeline threads -> stdout writer
    let (event_tx, event_rx) = bounded::<Event>(256);

    // Event writer thread: reads events from the channel, writes to stdout.
    // Uses recv_timeout to avoid blocking indefinitely on a full pipe.
    // If flush fails (pipe full), we keep the writer but drop events to
    // prevent cascading backpressure into pipeline threads.
    let writer_handle = thread::Builder::new()
        .name("event-writer".into())
        .spawn(move || {
            let stdout = std::io::stdout().lock();
            let mut writer = BufWriter::new(stdout);
            let flush_interval = Duration::from_millis(100);
            let mut last_flush = std::time::Instant::now();

            loop {
                match event_rx.recv_timeout(flush_interval) {
                    Ok(event) => {
                        debug!("Sending event: {:?}", event);
                        if let Err(e) = write_event(&mut writer, &event) {
                            error!("Failed to write event: {:#}", e);
                            continue;
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        // Timeout is normal — periodic flush keeps pipe clear
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        debug!("Event channel closed, writer exiting");
                        break;
                    }
                }
                // Flush periodically (not on every event to reduce syscalls)
                if last_flush.elapsed() >= flush_interval {
                    if let Err(e) = writer.flush() {
                        error!("Failed to flush stdout: {:#}", e);
                        // Don't break — keep trying. Reset writer to clear buffered data.
                        let _ = std::mem::replace(&mut writer, BufWriter::new(std::io::stdout().lock()));
                    }
                    last_flush = std::time::Instant::now();
                }
            }
            debug!("Event writer thread exiting");
        })?;

    let config = EngineConfig::from_env();
    let mut engine = Engine::new(config, event_tx.clone());

    // Command reader: reads from stdin
    let stdin = std::io::stdin().lock();
    let mut reader = BufReader::new(stdin);

    loop {
        let cmd = match read_command(&mut reader) {
            Ok(Some(cmd)) => cmd,
            Ok(None) => {
                info!("EOF on stdin, shutting down");
                break;
            }
            Err(e) => {
                error!("Failed to read command: {:#}", e);
                let err_event = Event::Error {
                    message: format!("{:#}", e),
                };
                let _ = event_tx.send(err_event);
                continue;
            }
        };

        debug!("Received command: {:?}", cmd);

        // handle_command returns immediate events (Pong, Started, etc.)
        let immediate_events = engine.handle_command(cmd);

        // Send immediate events through the same channel
        for event in immediate_events {
            if let Err(e) = event_tx.send(event) {
                error!("Failed to send immediate event: {:#}", e);
            }
        }

        if engine.is_shutting_down() {
            info!("Shutdown requested, exiting");
            break;
        }
    }

    // Drop the sender so the writer thread can finish
    drop(event_tx);

    // Wait for the writer thread
    if let Err(e) = writer_handle.join() {
        error!("Event writer thread panicked: {:?}", e);
    }

    info!("audio_engine stopped");
    Ok(())
}
