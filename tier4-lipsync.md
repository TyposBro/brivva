# Tier 4 — Post-Processed Dubbing via ElevenLabs

## Overview

After a live broadcast session ends, the host triggers dubbing post-processing for any target language. The system records video and host audio during the live session, then submits them to ElevenLabs' Dubbing API to generate a re-dubbed video where the host's voice is replaced with the translated speech. The result is a downloadable MP4 per language — suitable for VOD replay on Coupang, Rakuten, YouTube, etc.

## Why ElevenLabs Dubbing

- Handles transcript + translation + TTS synthesis internally
- No GPU required on the host machine
- No additional latency during live broadcast
- Processing: 5–15 min per language
- Pricing: ~$0.03–0.05/sec of video

## Architecture

```
LIVE SESSION (existing + recording)
┌──────────────────────────────────────────────────────────────────┐
│                                                                  │
│  Host Video (fMP4 chunks) ──┬──→ FFmpeg stdin → RTMP push       │
│                             └──→ SessionRecorder → video.fmp4   │
│                                                                  │
│  Host Audio (PCM)        ────→ SessionRecorder → host_audio.pcm │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
        │
        ▼
  /tmp/brivva/recordings/{session_id}/
  ├── video.fmp4          (raw fMP4 from MediaRecorder)
  └── host_audio.pcm      (original host audio, 44.1kHz 16-bit mono)


POST-SESSION (triggered by user)
┌──────────────────────────────────────────────────────────────────┐
│                                                                  │
│  1. Mux video.fmp4 + host_audio.pcm → video.mp4 (FFmpeg, ~10s) │
│  2. Upload video.mp4 → ElevenLabs Dubbing API                   │
│  3. Poll job status every 15s (ElevenLabs transcribes,          │
│     translates, voices, and syncs internally)                   │
│  4. Download dubbed audio → {lang}.mp3                          │
│  5. Mux original video + dubbed audio → {lang}_dubbed.mp4       │
│  6. Repeat steps 2–5 for each target language                   │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
        │
        ▼
  /tmp/brivva/dubbing/{session_id}_{lang}/
  ├── {lang}.mp3             (dubbed audio from ElevenLabs)
  └── {lang}_dubbed.mp4      (final output — original video + dubbed audio)
```

## Implementation Status

All components implemented and shipped.

### Session Recorder (`shared/recording/`)

Records raw video and host audio to disk during a live session. Enabled for tier >= 4.

- `recorder.rs` — `SessionRecorder` struct; `write_video_chunk()`, `write_host_audio()`, `finalize()`
- Integrated in `manager.rs::push_video_chunk()`, `handler.rs::drain_host_audio()`, `cleanup.rs::cleanup_session()`

### Muxer (`features/dubbing/data/muxer.rs`)

- `mux_fmp4_with_audio()` — FFmpeg: fMP4 + host PCM → MP4
- `mux_video_with_dubbed_audio()` — FFmpeg: original fMP4 + dubbed MP3 → final MP4

### ElevenLabs Dubbing Client (`shared/dubbing/`)

- `client.rs` — `create_dubbing()`, `poll_status()`, `download_audio()`
- API endpoint: `https://api.elevenlabs.io/v1/dubbing`
- Auth: `xi-api-key` header
- Upload: multipart form with video file, source_lang, target_lang
- ElevenLabs handles: transcription → translation → TTS → timing internally

### Job Runner (`features/dubbing/data/job_runner.rs`)

Orchestrates: mux → upload → poll → download → final mux. Status updates written to `DubbingJobs` map at each step.

### API Endpoints (`features/dubbing/data/handlers.rs`)

```
POST /api/dubbing/start         body: { session_id, lang, source_lang }
GET  /api/dubbing/status/:id    → DubbingJob JSON
GET  /api/dubbing/jobs/:sid     → DubbingJob[] for session
GET  /api/dubbing/download/:id  → MP4 stream
```

### Frontend (`features/dubbing/`)

- `domain/types.ts` — `DubbingJob`, `DubbingJobStatus`
- `data/api.ts` — fetch wrappers
- `presentation/hooks/use-dubbing-jobs.ts` — polls every 5s while active jobs exist
- `presentation/components/dubbing-panel.tsx` — step progress UI, Dub All button, retry on failure, download

## Pipeline Steps (UI)

```
Preparing → Uploading → Dubbing → Downloading → Finalizing
  (mux)     (EL upload)  (EL proc)  (dl audio)   (final mux)
```

## Configuration

```
ELEVENLABS_API_KEY=...   # same key used for live TTS
```

No additional env vars. Recording dir defaults to `/tmp/brivva/recordings`.

## File Size Estimates (1hr session)

- `video.fmp4`: ~500MB–1.5GB
- `host_audio.pcm`: ~300MB (44.1kHz 16-bit mono, full session)
- `video.mp4` (muxed): same as fmp4
- `{lang}.mp3`: ~30–60MB (dubbed audio)
- `{lang}_dubbed.mp4`: same as video.mp4 + audio track

## Cost Estimate

ElevenLabs Dubbing pricing (~$0.03–0.05/sec of source video):
- 30min session × $0.04/sec = ~$72 per language
- 3 languages = ~$216 per session

## Open Questions

1. **ElevenLabs voice consistency:** Dubbing uses ElevenLabs' internal voice selection — not the cloned voice from live TTS. For voice consistency, explore ElevenLabs' `voice_id` param in the dubbing API if available.
2. **Partial recordings:** If session crashes mid-way, partial fmp4 is still usable — ElevenLabs processes what it gets.
3. **Audio/video duration:** Dubbed audio from ElevenLabs matches the source video duration internally — no manual padding needed.
