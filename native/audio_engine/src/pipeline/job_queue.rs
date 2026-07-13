use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Manages a bounded set of active pipeline jobs so stale work can be canceled.
///
/// Each new job gets a unique ID. When a new job arrives, the previous active ID
/// is bumped. Completed jobs check whether their ID is still current before
/// committing results (e.g. TTS playback).
pub struct JobQueue {
    current_id: AtomicU64,
    last_commit_id: Mutex<u64>,
}

impl JobQueue {
    pub fn new() -> Self {
        Self {
            current_id: AtomicU64::new(1),
            last_commit_id: Mutex::new(0),
        }
    }

    /// Bump to the next job ID and return it.
    pub fn next_job(&self) -> u64 {
        self.current_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Try to commit a result for the given job ID.
    /// Returns `true` if this job is still the most recent one.
    pub fn try_commit(&self, job_id: u64) -> bool {
        let mut last = self.last_commit_id.lock().unwrap_or_else(|e| e.into_inner());
        if job_id > *last {
            *last = job_id;
            true
        } else {
            false
        }
    }

    /// Inspect whether the given job has been superseded.
    pub fn is_stale(&self, job_id: u64) -> bool {
        let last = self.last_commit_id.lock().unwrap_or_else(|e| e.into_inner());
        job_id < *last
    }
}
