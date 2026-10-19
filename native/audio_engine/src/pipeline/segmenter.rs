use std::time::{Duration, Instant};

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

// START_BLOCK_FRAGMENT_JOINER
/// Fewest words a fragment needs to be translated and spoken on its own.
pub const MIN_SEGMENT_WORDS: usize = 2;
/// A held fragment is released alone after this long without any sign of further speech.
pub const QUIET_BEFORE_RELEASE: Duration = Duration::from_millis(700);
/// Upper bound for the whole wait, however long the speaker keeps talking.
pub const MAX_HOLD: Duration = Duration::from_secs(4);

/// What to do with a fragment offered to the [`FragmentJoiner`].
#[derive(Debug, PartialEq)]
pub enum Joined {
    /// Translate and speak this text now.
    Ready(String),
    /// Too short on its own: kept back, waiting for the next fragment.
    Held,
    /// Nothing to translate.
    Nothing,
}

/// Merges very short fragments (typically a single word such as "hi") into the
/// next one. Spoken alone they come out as a clipped ~150 ms blip that neither a
/// listener nor a recogniser can make out, and the translation loses its context.
///
/// Deepgram's endpointing finalizes a result at every short pause ("Hi," | "my name
/// is Pavel"), so a *final* one-word result says nothing about the sentence being over:
/// it is held too, until more speech arrives or the speaker stays quiet.
pub struct FragmentJoiner {
    held: Option<(String, Instant)>,
    min_words: usize,
    quiet: Duration,
    max_hold: Duration,
}

impl FragmentJoiner {
    pub fn new(min_words: usize, quiet: Duration, max_hold: Duration) -> Self {
        Self { held: None, min_words, quiet, max_hold }
    }

    /// `segment` is the delta from [`TextSegmenter::next_segment`].
    pub fn offer(&mut self, segment: Option<String>, now: Instant) -> Joined {
        match segment {
            Some(delta) => {
                let (text, since) = match self.held.take() {
                    Some((held, since)) => (format!("{} {}", held, delta), since),
                    None => (delta, now),
                };
                if text.split_whitespace().count() < self.min_words {
                    self.held = Some((text, since));
                    Joined::Held
                } else {
                    Joined::Ready(text)
                }
            }
            None if self.held.is_some() => Joined::Held,
            None => Joined::Nothing,
        }
    }

    /// Time left before the held fragment must be released; `None` when nothing is held.
    /// `last_activity` is when the recogniser last reported anything (even a partial
    /// result): speech that is still coming in postpones the release.
    pub fn remaining(&self, now: Instant, last_activity: Option<Instant>) -> Option<Duration> {
        let (_, since) = self.held.as_ref()?;
        let quiet_from = last_activity.filter(|a| a > since).unwrap_or(*since);
        let deadline = (*since + self.max_hold).min(quiet_from + self.quiet);
        Some(deadline.saturating_duration_since(now))
    }

    /// Release the held fragment (its wait ran out).
    pub fn flush(&mut self) -> Option<String> {
        self.held.take().map(|(text, _)| text)
    }
}
// END_BLOCK_FRAGMENT_JOINER

#[cfg(test)]
mod tests {
    use super::*;

    fn joiner() -> FragmentJoiner {
        FragmentJoiner::new(MIN_SEGMENT_WORDS, QUIET_BEFORE_RELEASE, MAX_HOLD)
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn single_word_is_held_then_joined_with_the_next_fragment() {
        let (mut j, t0) = (joiner(), Instant::now());
        assert_eq!(j.offer(Some("hi".into()), t0), Joined::Held);
        assert_eq!(
            j.offer(Some("my name is Pavel".into()), t0 + ms(1200)),
            Joined::Ready("hi my name is Pavel".into())
        );
        assert_eq!(j.remaining(t0 + ms(1200), None), None);
    }

    #[test]
    fn two_words_are_ready_at_once() {
        let mut j = joiner();
        assert_eq!(j.offer(Some("my name".into()), Instant::now()), Joined::Ready("my name".into()));
    }

    #[test]
    fn a_repeat_of_the_held_text_changes_nothing() {
        // A stable partial "hi" is followed by the final "hi": no new segment, still held.
        let (mut j, t0) = (joiner(), Instant::now());
        assert_eq!(j.offer(Some("hi".into()), t0), Joined::Held);
        assert_eq!(j.offer(None, t0 + ms(100)), Joined::Held);
        assert_eq!(j.flush(), Some("hi".into()));
    }

    #[test]
    fn released_after_quiet_when_no_more_speech_comes() {
        let (mut j, t0) = (joiner(), Instant::now());
        j.offer(Some("thanks".into()), t0);
        assert_eq!(j.remaining(t0, None), Some(QUIET_BEFORE_RELEASE));
        assert_eq!(j.remaining(t0 + ms(700), None), Some(Duration::ZERO));
        assert_eq!(j.flush(), Some("thanks".into()));
        assert_eq!(j.remaining(t0, None), None);
    }

    #[test]
    fn ongoing_speech_postpones_the_release() {
        let (mut j, t0) = (joiner(), Instant::now());
        j.offer(Some("hi".into()), t0 + ms(300));
        // A partial result for the next words arrived 500 ms after the fragment was held.
        let activity = Some(t0 + ms(800));
        assert_eq!(j.remaining(t0 + ms(800), activity), Some(QUIET_BEFORE_RELEASE));
        // Activity from before the fragment was held does not count.
        assert_eq!(j.remaining(t0 + ms(300), Some(t0)), Some(QUIET_BEFORE_RELEASE));
    }

    #[test]
    fn the_wait_is_capped_even_while_speech_continues() {
        let (mut j, t0) = (joiner(), Instant::now());
        j.offer(Some("well".into()), t0);
        let activity = Some(t0 + ms(3900));
        assert_eq!(j.remaining(t0 + ms(3900), activity), Some(ms(100)));
        assert_eq!(j.remaining(t0 + MAX_HOLD, activity), Some(Duration::ZERO));
    }

    #[test]
    fn holding_again_keeps_the_original_start() {
        let (mut j, t0) = (FragmentJoiner::new(3, QUIET_BEFORE_RELEASE, MAX_HOLD), Instant::now());
        assert_eq!(j.offer(Some("a".into()), t0), Joined::Held);
        assert_eq!(j.offer(Some("b".into()), t0 + ms(900)), Joined::Held);
        assert_eq!(j.remaining(t0 + ms(900), Some(t0 + ms(900))), Some(QUIET_BEFORE_RELEASE));
        assert_eq!(j.remaining(t0 + MAX_HOLD, Some(t0 + MAX_HOLD)), Some(Duration::ZERO));
        assert_eq!(j.flush(), Some("a b".into()));
    }

    #[test]
    fn nothing_offered_means_nothing_held() {
        let mut j = joiner();
        assert_eq!(j.offer(None, Instant::now()), Joined::Nothing);
        assert_eq!(j.remaining(Instant::now(), None), None);
    }
}
