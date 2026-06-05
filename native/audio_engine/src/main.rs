mod audio;
mod engine;
mod protocol;
mod stt;
mod tracelog;
mod translation;
mod tts;

use std::io::{BufReader, BufWriter, Write};
use std::thread;

use anyhow::Result;
use crossbeam_channel::bounded;
use log::{debug, error, info};

use crate::engine::{Engine, EngineConfig};
use crate::protocol::{read_command, write_event, Event};

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

    // ONNX Runtime init is deferred to the TTS preload thread (engine.rs)
    // to avoid COM conflicts with WASAPI audio capture.
    // ort::init().commit() is called inside the TTS init worker.
    info!("ONNX Runtime init deferred to TTS preload thread");
    
    // Channel for events from pipeline threads -> stdout writer

    // Channel for events from pipeline threads -> stdout writer
    let (event_tx, event_rx) = bounded::<Event>(256);

    // Event writer thread: reads events from the channel, writes to stdout.
    // This is the only thread that writes to stdout, avoiding lock contention.
    let writer_handle = thread::Builder::new()
        .name("event-writer".into())
        .spawn(move || {
            let stdout = std::io::stdout().lock();
            let mut writer = BufWriter::new(stdout);

            while let Ok(event) = event_rx.recv() {
                debug!("Sending event: {:?}", event);
                if let Err(e) = write_event(&mut writer, &event) {
                    error!("Failed to write event: {:#}", e);
                    break;
                }
                if let Err(e) = writer.flush() {
                    error!("Failed to flush stdout: {:#}", e);
                    break;
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
