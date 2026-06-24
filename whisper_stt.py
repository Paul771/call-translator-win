"""
Whisper STT - reads audio from stdin, outputs transcript to stdout.
Debug version: logs audio stats.
"""
import sys
import struct
import json
import os
import numpy as np

def main():
    model_size = os.environ.get('WHISPER_MODEL', 'tiny')
    device = os.environ.get('WHISPER_DEVICE', 'cpu')
    compute_type = os.environ.get('WHISPER_COMPUTE', 'int8')

    from faster_whisper import WhisperModel
    model = WhisperModel(model_size, device=device, compute_type=compute_type)
    print(f"[whisper] Model loaded: {model_size}", file=sys.stderr)
    sys.stderr.flush()

    while True:
        header = sys.stdin.buffer.read(4)
        if len(header) < 4:
            break

        sample_rate = struct.unpack('<I', header)[0]
        audio_data = sys.stdin.buffer.read()
        num_samples = len(audio_data) // 4
        if num_samples == 0:
            print(json.dumps({"text": "", "language": "en", "duration": 0.0}))
            sys.stdout.flush()
            continue

        samples = struct.unpack(f'<{num_samples}f', audio_data)
        audio = np.array(samples, dtype=np.float32)

        duration = float(len(audio)) / sample_rate
        rms = float(np.sqrt(np.mean(audio**2)))
        peak = float(np.max(np.abs(audio)))
        print(f"[whisper] Audio: dur={duration:.2f}s rms={rms:.4f} peak={peak:.4f} samples={num_samples}", file=sys.stderr)
        sys.stderr.flush()

        if duration < 1.0:
            print(json.dumps({"text": "", "language": "en", "duration": duration}))
            sys.stdout.flush()
            continue

        # Amplify very quiet audio
        if rms > 0 and rms < 0.1:
            gain = min(0.3 / rms, 10.0)
            audio = audio * gain
            print(f"[whisper] Amplified by {gain:.1f}x, new rms={float(np.sqrt(np.mean(audio**2))):.4f}", file=sys.stderr)
            sys.stderr.flush()

        segments, info = model.transcribe(
            audio,
            beam_size=1,
            language=None,
            vad_filter=True,
            vad_parameters=dict(
                min_silence_duration_ms=300,
                speech_pad_ms=300,
            ),
            no_speech_threshold=0.3,
            log_prob_threshold=-2.0,
            condition_on_previous_text=False,
        )

        text_parts = []
        for segment in segments:
            t = segment.text.strip()
            if len(t) >= 2:
                text_parts.append(t)

        transcript = ' '.join(text_parts).strip()
        print(f"[whisper] Result: text='{transcript}' lang={info.language}", file=sys.stderr)
        sys.stderr.flush()

        result = {
            "text": transcript,
            "language": info.language,
            "duration": duration,
        }
        print(json.dumps(result))
        sys.stdout.flush()

if __name__ == '__main__':
    main()
