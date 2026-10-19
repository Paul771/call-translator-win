//! Virtual audio endpoints for automated end-to-end tests.
//!
//! MODULE_CONTRACT
//!   PURPOSE: Let a test harness drive a pipeline without a microphone or speakers.
//!   SCOPE:   `inject` capture device (audio pushed from WAV files via the
//!            `inject_audio` command) and `null` playback device (audio discarded;
//!            the TTS result still leaves the engine as `Event::TtsAudio`).
//!   INPUTS:  16 kHz PCM WAV files, pipeline name, tail silence length.
//!   OUTPUTS: `AudioChunk`s on the pipeline's capture channel, paced in real time and
//!            continuous: silence is fed between phrases, like a live microphone.
//!
//! Selected by setting TRANSLATOR_MIC_DEVICE / TRANSLATOR_MEET_INPUT to `inject`
//! and TRANSLATOR_SPEAKER_DEVICE / TRANSLATOR_MEET_OUTPUT to `null`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use log::{info, warn};

use crate::audio::capture::AudioChunk;
use crate::audio::playback::AudioPlayback;
use crate::protocol::Event;

pub const INJECT_DEVICE: &str = "inject";
pub const NULL_DEVICE: &str = "null";
/// Sample rate of the virtual capture device. Files must already be 16 kHz.
pub const INJECT_RATE: u32 = 16_000;
/// Silence appended after the speech so the STT endpointing closes the utterance.
pub const DEFAULT_TAIL_MS: u32 = 2_500;
/// Playback rate reported by the null device (Piper's native rate, no resampling).
const NULL_PLAYBACK_RATE: u32 = 22_050;
const CHUNK_SAMPLES: usize = 320; // 20 ms at 16 kHz
const CHUNK_MS: u64 = 20;

// START_BLOCK_REGISTRY
/// One phrase to speak into a pipeline.
struct Job {
    samples: Vec<f32>,
    tail_ms: u32,
    direction: String,
    event_tx: Sender<Event>,
}

fn sources() -> &'static Mutex<HashMap<String, Sender<Job>>> {
    static SOURCES: OnceLock<Mutex<HashMap<String, Sender<Job>>>> = OnceLock::new();
    SOURCES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Called by a pipeline that opened the `inject` device. Starts the feeder thread, which
/// streams audio to the pipeline for as long as the pipeline is registered.
pub fn register(direction: &str, tx: Sender<AudioChunk>) {
    let (job_tx, job_rx) = crossbeam_channel::unbounded::<Job>();
    let spawned = std::thread::Builder::new()
        .name(format!("inject-feed-{}", direction))
        .spawn(move || run_feeder(tx, job_rx));
    if spawned.is_err() {
        warn!("[inject-{}] failed to spawn feeder thread", direction);
        return;
    }
    if let Ok(mut map) = sources().lock() {
        map.insert(direction.to_string(), job_tx);
    }
}

/// Called when the pipeline stops. Dropping the job sender ends the feeder thread.
pub fn unregister(direction: &str) {
    if let Ok(mut map) = sources().lock() {
        map.remove(direction);
    }
}
// END_BLOCK_REGISTRY

// START_BLOCK_WAV
/// Read a WAV file into mono f32 samples. Only 16 kHz is accepted: resampling is
/// the harness's job (ffmpeg), the engine must not hide a wrong-rate fixture.
pub fn read_wav_mono_16k(path: &str) -> Result<Vec<f32>> {
    let data = std::fs::read(path).with_context(|| format!("cannot read '{}'", path))?;
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        bail!("'{}' is not a RIFF/WAVE file", path);
    }

    let mut pos = 12usize;
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // tag, channels, rate, bits
    let mut pcm: Option<&[u8]> = None;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
        let start = pos + 8;
        let end = (start + size).min(data.len());
        if id == b"fmt " && end - start >= 16 {
            let b = &data[start..end];
            fmt = Some((
                u16::from_le_bytes([b[0], b[1]]),
                u16::from_le_bytes([b[2], b[3]]),
                u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                u16::from_le_bytes([b[14], b[15]]),
            ));
        } else if id == b"data" {
            pcm = Some(&data[start..end]);
        }
        pos = start + size + (size & 1);
    }

    let (tag, channels, rate, bits) = fmt.context("WAV has no fmt chunk")?;
    let pcm = pcm.context("WAV has no data chunk")?;
    if rate != INJECT_RATE {
        bail!(
            "'{}' is {} Hz, expected {} Hz (convert: ffmpeg -i in -ar 16000 -ac 1 -c:a pcm_s16le out.wav)",
            path, rate, INJECT_RATE
        );
    }
    let channels = channels.max(1) as usize;

    let interleaved: Vec<f32> = match (tag, bits) {
        (1 | 0xFFFE, 16) => pcm
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
            .collect(),
        (3, 32) => pcm
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
        _ => bail!("unsupported WAV format tag={} bits={} (need PCM16 or float32)", tag, bits),
    };

    if channels == 1 {
        return Ok(interleaved);
    }
    Ok(interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect())
}
// END_BLOCK_WAV

// START_BLOCK_FEEDER
/// Queue a WAV file for the named pipeline. The feeder speaks it in real time followed by
/// `tail_ms` of silence and reports `inject_speech_end` / `inject_done` log events.
/// Returns the speech duration in seconds.
pub fn inject_wav(direction: &str, path: &str, tail_ms: u32, event_tx: Sender<Event>) -> Result<f32> {
    let job_tx = sources()
        .lock()
        .ok()
        .and_then(|map| map.get(direction).cloned())
        .with_context(|| {
            format!(
                "no inject source for pipeline '{}' (not running, or its capture device is not '{}')",
                direction, INJECT_DEVICE
            )
        })?;
    let samples = read_wav_mono_16k(path)?;
    let seconds = samples.len() as f32 / INJECT_RATE as f32;
    job_tx
        .send(Job { samples, tail_ms, direction: direction.to_string(), event_tx })
        .map_err(|_| anyhow::anyhow!("inject feeder for '{}' has stopped", direction))?;
    Ok(seconds)
}

/// Real-time pacing on one continuous timeline, so idle silence and speech line up.
struct Pacer {
    t0: Instant,
    index: u64,
}

impl Pacer {
    /// Wait for this chunk's slot, then deliver it. `false` once the pipeline is gone.
    fn push(&mut self, tx: &Sender<AudioChunk>, chunk: Vec<f32>) -> bool {
        let deadline = self.t0 + Duration::from_millis(self.index * CHUNK_MS);
        if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        self.index += 1;
        let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len().max(1) as f32).sqrt();
        tx.send_timeout(AudioChunk { samples: chunk, raw_rms: rms }, Duration::from_secs(1)).is_ok()
    }
}

/// A live microphone never stops streaming. Between phrases the feeder keeps sending
/// silence, otherwise the recogniser sees a stream that stalls and restarts and
/// finalizes utterances differently from the first one.
fn run_feeder(tx: Sender<AudioChunk>, jobs: Receiver<Job>) {
    let mut pacer = Pacer { t0: Instant::now(), index: 0 };
    let silence = || vec![0.0f32; CHUNK_SAMPLES];
    loop {
        match jobs.try_recv() {
            Ok(job) => {
                for piece in job.samples.chunks(CHUNK_SAMPLES) {
                    let mut chunk = piece.to_vec();
                    chunk.resize(CHUNK_SAMPLES, 0.0);
                    if !pacer.push(&tx, chunk) {
                        warn!("[inject-{}] pipeline closed while feeding", job.direction);
                        return;
                    }
                }
                let _ = job.event_tx.try_send(Event::Log {
                    level: "info".into(),
                    message: format!("inject_speech_end {}", job.direction),
                });
                for _ in 0..(job.tail_ms as u64 / CHUNK_MS).max(1) {
                    if !pacer.push(&tx, silence()) {
                        return;
                    }
                }
                let _ = job.event_tx.try_send(Event::Log {
                    level: "info".into(),
                    message: format!("inject_done {}", job.direction),
                });
                info!("[inject-{}] done", job.direction);
            }
            Err(TryRecvError::Empty) => {
                if !pacer.push(&tx, silence()) {
                    return;
                }
            }
            Err(TryRecvError::Disconnected) => return,
        }
    }
}
// END_BLOCK_FEEDER

// START_BLOCK_PLAYBACK
/// Playback sink that drops everything. Mono at Piper's native rate.
#[allow(dead_code)]
pub struct NullPlayback {
    feeder: JoinHandle<()>,
}

impl NullPlayback {
    fn new(receiver: Receiver<Vec<f32>>) -> Result<Self> {
        let feeder = std::thread::Builder::new()
            .name("null-playback".into())
            .spawn(move || while receiver.recv().is_ok() {})
            .context("failed to spawn null playback thread")?;
        Ok(Self { feeder })
    }
}

/// Either a real output device or the null sink.
pub enum Playback {
    Device(AudioPlayback),
    Null(NullPlayback),
}

impl Playback {
    pub fn open(device_name: &str, sample_rate: u32, receiver: Receiver<Vec<f32>>) -> Result<Self> {
        if device_name == NULL_DEVICE {
            info!("Playback: null sink (test mode)");
            return Ok(Playback::Null(NullPlayback::new(receiver)?));
        }
        Ok(Playback::Device(AudioPlayback::new(device_name, sample_rate, receiver)?))
    }

    pub fn sample_rate(&self) -> u32 {
        match self {
            Playback::Device(p) => p.sample_rate(),
            Playback::Null(_) => NULL_PLAYBACK_RATE,
        }
    }

    pub fn channels(&self) -> u16 {
        match self {
            Playback::Device(p) => p.channels(),
            Playback::Null(_) => 1,
        }
    }

    pub fn start(&self) -> Result<()> {
        match self {
            Playback::Device(p) => p.start(),
            Playback::Null(_) => Ok(()),
        }
    }

    pub fn stop(&self) -> Result<()> {
        match self {
            Playback::Device(p) => p.stop(),
            Playback::Null(_) => Ok(()),
        }
    }
}
// END_BLOCK_PLAYBACK

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_bytes(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        v.extend_from_slice(&(channels * 2).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            v.extend_from_slice(&s.to_le_bytes());
        }
        v
    }

    fn write_tmp(name: &str, bytes: &[u8]) -> String {
        let path = std::env::temp_dir().join(format!("inject_test_{}_{}.wav", name, std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn reads_mono_16k_pcm16() {
        let path = write_tmp("mono", &wav_bytes(16_000, 1, &[0, 16384, -16384, 32767]));
        let out = read_wav_mono_16k(&path).unwrap();
        assert_eq!(out.len(), 4);
        assert!((out[1] - 0.5).abs() < 1e-3);
        assert!((out[2] + 0.5).abs() < 1e-3);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn downmixes_stereo() {
        let path = write_tmp("stereo", &wav_bytes(16_000, 2, &[16384, -16384, 8192, 8192]));
        let out = read_wav_mono_16k(&path).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out[0].abs() < 1e-3);
        assert!((out[1] - 0.25).abs() < 1e-3);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_wrong_sample_rate() {
        let path = write_tmp("rate", &wav_bytes(44_100, 1, &[0, 1, 2]));
        let err = read_wav_mono_16k(&path).unwrap_err().to_string();
        assert!(err.contains("44100"), "unexpected error: {}", err);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_non_wav() {
        let path = write_tmp("junk", b"not a wav file at all");
        assert!(read_wav_mono_16k(&path).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn feeder_streams_silence_when_idle_and_speech_when_injected() {
        let (audio_tx, audio_rx) = crossbeam_channel::bounded::<AudioChunk>(256);
        let (event_tx, event_rx) = crossbeam_channel::bounded::<Event>(16);
        register("feeder-test", audio_tx);

        // Idle: the very first chunk is silence, not nothing.
        let first = audio_rx.recv_timeout(Duration::from_millis(500)).expect("idle silence");
        assert!(first.samples.iter().all(|s| *s == 0.0));

        // 0.1 s of a constant tone, then a short tail.
        let path = write_tmp("feeder", &wav_bytes(16_000, 1, &vec![16384i16; 1600]));
        let seconds = inject_wav("feeder-test", &path, 60, event_tx).unwrap();
        assert!((seconds - 0.1).abs() < 1e-3);

        let mut speech_chunks = 0;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut done = false;
        while Instant::now() < deadline && !done {
            while let Ok(chunk) = audio_rx.try_recv() {
                if chunk.samples.iter().any(|s| (*s - 0.5).abs() < 1e-3) {
                    speech_chunks += 1;
                }
            }
            while let Ok(ev) = event_rx.try_recv() {
                if let Event::Log { message, .. } = ev {
                    done |= message.starts_with("inject_done");
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(done, "inject_done was never reported");
        assert_eq!(speech_chunks, 5, "0.1 s at 20 ms chunks");

        // Silence keeps flowing afterwards.
        while audio_rx.try_recv().is_ok() {}
        assert!(audio_rx.recv_timeout(Duration::from_millis(500)).is_ok());
        unregister("feeder-test");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn inject_without_pipeline_fails_clearly() {
        let (tx, _rx) = crossbeam_channel::bounded::<Event>(4);
        let err = inject_wav("nonexistent", "x.wav", 100, tx).unwrap_err().to_string();
        assert!(err.contains("no inject source"), "unexpected error: {}", err);
    }
}
