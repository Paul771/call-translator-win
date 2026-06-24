import json, sys
from faster_whisper import WhisperModel

model_size = sys.argv[1] if len(sys.argv) > 1 else 'tiny'
wav_path = sys.argv[2] if len(sys.argv) > 2 else ''
result_path = sys.argv[3] if len(sys.argv) > 3 else ''

if not wav_path or not result_path:
    sys.exit(1)

m = WhisperModel(model_size, device='cpu', compute_type='int8')
segs, info = m.transcribe(wav_path, beam_size=1, language=None, vad_filter=True,
    vad_parameters=dict(min_silence_duration_ms=500, speech_pad_ms=400),
    no_speech_threshold=0.5, log_prob_threshold=-1.5, condition_on_previous_text=False)
t = ' '.join(s.text.strip() for s in segs if len(s.text.strip()) >= 2)
r = json.dumps({'text': t, 'language': info.language, 'duration': 0})
with open(result_path, 'w') as f:
    f.write(r)
