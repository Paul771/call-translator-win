//! Length-prefixed JSON protocol for communication with Elixir over stdin/stdout.
//!
//! Wire format: 4-byte big-endian length prefix followed by JSON payload.

use std::io::{Read, Write};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Commands sent from Elixir to Rust.
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Ping,
    Start {
        pipelines: Vec<String>,
    },
    Stop,
    SetConfig {
        key: String,
        value: serde_json::Value,
    },
    Shutdown,
    ListDevices,
    TtsPreview {
        lang: String,
        voice: String,
    },
}

/// Events sent from Rust to Elixir.
#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Pong,
    Started {
        pipelines: Vec<String>,
    },
    Stopped,
    Transcript {
        direction: String,
        text: String,
        lang: String,
        stt_ms: u64,
    },
    PartialTranscript {
        direction: String,
        text: String,
        lang: String,
        stt_ms: u64,
    },
    StablePartialTranscript {
        direction: String,
        text: String,
        lang: String,
        stt_ms: u64,
    },
    Translation {
        direction: String,
        text: String,
        translate_ms: u64,
    },
    TranslationRevised {
        direction: String,
        text: String,
        translate_ms: u64,
    },
    Metrics {
        direction: String,
        stt_ms: u64,
        translate_ms: u64,
        tts_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_used: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        time_to_first_audio_ms: Option<u64>,
    },
    Error {
        message: String,
    },
    Log {
        level: String,
        message: String,
    },
    DeviceList {
        input: Vec<String>,
        output: Vec<String>,
    },
    TtsPreviewDone,
    TtsStatus {
        direction: String,
        status: String,
        message: String,
    },
    TtsAudio {
        direction: String,
        sample_rate: u32,
        audio_b64: String,
    },
}

/// Reads a single command from the given reader.
///
/// Expects 4-byte big-endian length prefix followed by a JSON payload.
/// Returns `Ok(None)` on clean EOF (peer closed connection).
pub fn read_command(reader: &mut impl Read) -> Result<Option<Command>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e).context("failed to read message length"),
    }

    let len = u32::from_be_bytes(len_buf) as usize;

    let mut payload = vec![0u8; len];
    reader
        .read_exact(&mut payload)
        .context("failed to read message payload")?;

    let cmd: Command =
        serde_json::from_slice(&payload).context("failed to deserialize command")?;

    Ok(Some(cmd))
}

/// Writes a single event to the given writer.
///
/// Serializes to JSON, writes 4-byte big-endian length prefix, then the payload.
pub fn write_event(writer: &mut impl Write, event: &Event) -> Result<()> {
    let payload = serde_json::to_vec(event).context("failed to serialize event")?;
    let len = payload.len() as u32;

    writer
        .write_all(&len.to_be_bytes())
        .context("failed to write event length")?;
    writer
        .write_all(&payload)
        .context("failed to write event payload")?;

    Ok(())
}
