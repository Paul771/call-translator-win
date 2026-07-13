/// Holds the last-translated stable text and computes the incremental delta
/// when a new stable partial arrives.
pub struct TextSegmenter {
    last_translated: String,
}

impl TextSegmenter {
    pub fn new() -> Self {
        Self { last_translated: String::new() }
    }

    /// Returns the text segment that should be translated now as the delta
    /// between `stable_text` and the last text already processed.
    /// Returns `None` when nothing new needs translation.
    pub fn next_segment(&mut self, stable_text: &str) -> Option<String> {
        // New utterance detected — reset and return the full text.
        if !stable_text.starts_with(&self.last_translated) {
            self.last_translated = stable_text.to_string();
            let trimmed = stable_text.trim();
            return if trimmed.is_empty() { None } else { Some(trimmed.to_string()) };
        }
        if stable_text.len() <= self.last_translated.len() {
            return None;
        }
        let delta = &stable_text[self.last_translated.len()..];
        let trimmed = delta.trim_start();
        if trimmed.is_empty() {
            return None;
        }
        self.last_translated = stable_text.to_string();
        Some(trimmed.to_string())
    }

    /// Reset state for a new utterance (e.g. when a final transcript resets the pipeline).
    pub fn reset(&mut self) {
        self.last_translated.clear();
    }
}
