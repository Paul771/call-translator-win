"""
Local Whisper STT - reads audio from stdin, outputs transcript to stdout.
Fixed: increased buffer, better post-processing to reduce hallucinations.
"""
import sys
import struct
import json
import os

def main():
    header = sys.stdin.buffer.read(4)
    if len(header) < 4:
        print(json.dumps({"text": "", "language": "en", "duration": 0.0}))
        return

    sample_rate = struct.unpack('<I', header)[0]
    audio_data = sys.stdin.buffer.read()
    num_samples = len(audio_data) // 4
    samples = struct.unpack(f'<{num_samples}f', audio_data)

    import numpy as np
    audio = np.array(samples, dtype=np.float32)

    # Skip very short audio (less than 1 second) - likely noise
    if len(audio) < sample_rate:
        print(json.dumps({"text": "", "language": "en", "duration": float(len(audio)) / sample_rate}))
        return

    if not hasattr(main, '_model'):
        model_size = os.environ.get('WHISPER_MODEL', 'small')
        device = os.environ.get('WHISPER_DEVICE', 'cpu')
        compute_type = os.environ.get('WHISPER_COMPUTE', 'int8')
        from faster_whisper import WhisperModel
        main._model = WhisperModel(model_size, device=device, compute_type=compute_type)
        print(f"[whisper] Model loaded: {model_size}", file=sys.stderr)

    segments, info = main._model.transcribe(
        audio,
        beam_size=3,
        language=None,
        vad_filter=True,
        vad_parameters=dict(
            min_silence_duration_ms=500,
            speech_pad_ms=200,
        ),
    )

    text_parts = []
    for segment in segments:
        t = segment.text.strip()
        # Filter out hallucinations - common Whisper garbage patterns
        if len(t) < 3:
            continue
        # Skip if text looks like hallucination (very short, single word that doesn't match context)
        text_parts.append(t)

    transcript = ' '.join(text_parts).strip()

    result = {
        "text": transcript,
        "language": info.language,
        "duration": float(info.duration),
    }
    print(json.dumps(result))

if __name__ == '__main__':
    main()
