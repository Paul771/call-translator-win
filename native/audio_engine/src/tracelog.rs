//! Pipeline trace logger — writes timestamped trace events to both stderr and a log file.
//!
//! Format: [HH:MM:SS.mmm] [pipeline] [STAGE] message
//! Stages: CAPTURE | STT | TRANSLATE | TTS | PLAYBACK | EVENT | ERROR
//!
//! On startup, existing pipeline_trace.log is truncated (cleared).
//! Old .log files (>24h) in the working directory are deleted.

use std::io::Write;
use std::fs::OpenOptions;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Initialize the trace log. Call once at engine startup.
pub fn init_log() {
    let mut guard = LOG.lock().unwrap();
    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("pipeline_trace.log")
    {
        Ok(f) => {
            *guard = Some(f);
            eprintln!("[TRACE_LOG] initialized pipeline_trace.log");
        }
        Err(e) => {
            eprintln!("[TRACE_LOG] failed to open pipeline_trace.log: {}", e);
        }
    }
}

/// Delete .log files older than 24 hours in the current directory.
pub fn prune_old_logs() {
    let cutoff = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(86400);

    if let Ok(entries) = std::fs::read_dir(".") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let s = name.to_string_lossy().to_string();
            if s.ends_with(".log") && s != "pipeline_trace.log" {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        if modified.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) < cutoff {
                            if std::fs::remove_file(entry.path()).is_ok() {
                                eprintln!("[TRACE_LOG] pruned: {}", s);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Emit a trace event visible in both stderr and the log file.
///
/// `pipeline` — e.g. "outgoing", "incoming"
/// `stage`   — e.g. "CAPTURE", "STT", "TRANSLATE", "TTS", "PLAYBACK", "EVENT", "ERROR"
/// `msg`     — human-readable message
pub fn trace(pipeline: &str, stage: &str, msg: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_ms = now.as_millis();
    let hours = ((total_ms / 3_600_000) % 24) as u32;
    let mins = ((total_ms / 60_000) % 60) as u32;
    let secs = ((total_ms / 1_000) % 60) as u32;
    let ms = (total_ms % 1_000) as u32;

    let ts = format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, ms);
    let line = format!("[{}] [{:>10}] [{:>12}] {}", ts, pipeline, stage, msg);

    // stderr always
    eprintln!("{}", line);

    // log file
    if let Ok(mut guard) = LOG.lock() {
        if let Some(ref mut f) = *guard {
            let _ = writeln!(f, "{}", line);
            let _ = f.flush();
        }
    }
}
