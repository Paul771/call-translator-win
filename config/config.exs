import Config

config :translator,
  audio_engine_path:
    Path.join([__DIR__, "..", "native", "audio_engine", "target", "release", "audio_engine.exe"])
    |> Path.expand(),
  vad_threshold: 0.5,
  min_silence_ms: 500,
  transcript_timeout_ms: 2000,
  sample_rate: 48000

config :logger, :console,
  format: "$time $metadata[$level] $message\n",
  metadata: [:direction, :pipeline]
