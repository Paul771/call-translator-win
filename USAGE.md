# Usage Guide

Open **http://127.0.0.1:5050** after running `./run.sh`.

## First Launch

Settings open automatically on first launch. Fill in:

1. **API Keys** — enter your [Deepgram](https://console.deepgram.com) and [Groq](https://console.groq.com) keys, click **Test** to verify
2. **Languages** — set "My Language" (what you speak) and "Their Language" (what the other person speaks)
3. **Voice** — pick a TTS voice for each language. One default voice is pre-installed; download more from the dropdown
4. **Audio Devices** — select your mic and speakers
5. Click **Save & Restart Engine**

   - On Windows: no special configuration needed — WASAPI handles routing automatically

## Controls

| Button | What it does |
|--------|-------------|
| **Start / Stop** | Start or stop the translation engine. A call session is recorded between Start and Stop |
| **Mic Out** | Mute/unmute your microphone (outgoing translation) |
| **Mic In** | Mute/unmute incoming audio (their speech) |
| **Tab Audio** | Capture audio from a browser tab directly — no external drivers needed |
| **Monitor** | Play translated audio in the browser so you can hear what the other person will hear |
| **Compact** | Toggle compact chat mode — smaller bubbles, less spacing |
| **Saved** | Filter to show only bookmarked messages |
| **History** | Open call history — view past sessions, generate AI summaries |
| **Export** | Download the current transcript as a text file |
| **Clear** | Clear all messages from the current view |
| **Settings** | Open the settings panel |

## Live Chat

The main area shows a real-time chat with translations:

- **Blue bubbles** (right) — your speech, translated to their language
- **Purple bubbles** (left) — their speech, translated to your language
- **Italic text** below each bubble — the original (untranslated) text
- **Timing** — STT, translation, and TTS latency shown per message
- **Click a bubble** to copy the translated text
- **Star icon** on hover — bookmark a message for later

## Voice Management

In Settings > Voice:

- The dropdown shows **Downloaded** voices at the top, **Available** voices below
- Available voices show their size in MB
- Select an available voice and click the **download button** (arrow icon) to download it
- Click the **play button** to preview any downloaded voice
- When switching to a new language with no downloaded voices, you'll be prompted to download the default voice

## Call History

Click **History** to see past call sessions:

- Each session shows start/end time, languages, and message count
- Click a session to see the full transcript
- **Summary** — generate an AI-powered summary of the call using Groq
- **Delete** — permanently remove a call from history

## Audio Setup for Calls

**macOS:**
1. Install [BlackHole](https://existential.audio/blackhole/) (both 2ch and 16ch)
2. In your call app, set **BlackHole 2ch** as microphone
3. In your call app, set **BlackHole 16ch** as speakers
4. Use **Google Chrome** for the call app — Safari has audio limitations

The translator automatically routes audio through BlackHole: your translated speech goes to the call, their speech comes back for translation.

**Windows:**
No additional drivers needed! The project uses WASAPI directly:
1. Set your desired microphone as the default input device
2. Set your speakers/headphones as the default output device
3. No virtual routing required — audio flows directly between devices

## Keyboard Shortcuts

- Hover over any button to see a tooltip explaining what it does (in your selected language)
