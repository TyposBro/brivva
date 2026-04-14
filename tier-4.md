# Tier 4 — Post-Processed Dubbing (ElevenLabs Dubbing API)

## What This Is

Tier 4 = post-processed dubbing. After a live stream ends, the recorded session (video + host audio) is sent to ElevenLabs Dubbing API for high-quality translation + voice dubbing. Output = studio-quality dubbed video per target language. Not real-time — takes minutes, meant for VOD/replay clips.

## Why It Matters

Real-time Tier 2 (live TTS) sounds acceptable but not perfect. Tier 4 produces broadcast-quality dubbed video for:
- Coupang/Taobao VOD replays
- Social media clips
- Marketing materials
- Archive of live shows

Chinese market (Brivva's primary) cares about VOD quality. Tier 4 is the premium product.

## ElevenLabs Dubbing API

### Create Dubbing

```
POST https://api.elevenlabs.io/v1/dubbing
Content-Type: multipart/form-data
Header: xi-api-key: <API_KEY>
```

| Field | Type | Required | Description |
|---|---|---|---|
| `file` | file | Yes* | Video/audio file to dub |
| `source_url` | string | Yes* | URL instead of file upload |
| `source_lang` | string | No | Source language (auto-detect if omitted) |
| `target_lang` | string | Yes | Target language code (`zh`, `ja`, `ko`, `en`, etc.) |
| `num_speakers` | integer | No | Number of speakers (0 = auto-detect) |
| `watermark` | boolean | No | Add watermark (free tier) |
| `name` | string | No | Project name |
| `highest_resolution` | boolean | No | Use highest resolution |
| `start_time` | integer | No | Start time in seconds |
| `end_time` | integer | No | End time in seconds |

*One of `file` or `source_url` required.

**Response:**
```json
{
  "dubbing_id": "abc-123-uuid",
  "expected_duration_sec": 120.5
}
```

### Poll Status

```
GET https://api.elevenlabs.io/v1/dubbing/{dubbing_id}
Header: xi-api-key: <API_KEY>
```

**Response:**
```json
{
  "dubbing_id": "abc-123-uuid",
  "name": "session_xyz",
  "status": "dubbing",
  "target_languages": ["zh"],
  "error": null
}
```

Status values: `pending` → `dubbing` → `dubbed` | `error`

### Download Output

```
GET https://api.elevenlabs.io/v1/dubbing/{dubbing_id}/audio/{language_code}
Header: xi-api-key: <API_KEY>
```

Returns binary audio/video stream. Save to file.

## Current Implementation

### Backend (Rust)

**Files:**
- `server-rs/src/shared/dubbing/client.rs` — HTTP client: `create_dubbing()`, `poll_status()`, `download_audio()`
- `server-rs/src/shared/dubbing/types.rs` — `DubbingStatus` enum, response types
- `server-rs/src/shared/recording/recorder.rs` — `SessionRecorder`: writes video.fmp4 + host_audio.pcm during live session

**Recording flow:**
1. Session starts with tier >= 4 → `SessionRecorder::new(session_id, tier)` enables recording
2. During stream: `write_video_chunk(data)` and `write_host_audio(pcm)` accumulate to disk
3. Session ends → `finalize()` flushes buffers
4. Output: `{RECORDING_DIR}/{session_id}/video.fmp4` + `host_audio.pcm`

**Dubbing flow (post-session):**
1. Frontend calls `POST /api/dubbing/start` with `session_id`, `lang`, `source_lang`
2. Backend muxes video.fmp4 + host_audio.pcm into single MP4 (FFmpeg)
3. Uploads MP4 to ElevenLabs `POST /v1/dubbing`
4. Polls `GET /v1/dubbing/{id}` until `dubbed` or `error`
5. Downloads dubbed audio via `GET /v1/dubbing/{id}/audio/{lang}`
6. Muxes original video + dubbed audio → final MP4
7. Serves download via `GET /api/dubbing/download/{job_id}`

### Frontend (React)

**Files:**
- `frontend/src/features/dubbing/data/api.ts` — API calls: `startDubbing()`, `getJobStatus()`, `getSessionJobs()`, `getDownloadUrl()`
- `frontend/src/features/dubbing/domain/types.ts` — `DubbingJob` type, `DubbingJobStatus` union
- `frontend/src/features/dubbing/presentation/hooks/use-dubbing-jobs.ts` — React hook for polling job status
- `frontend/src/features/dubbing/presentation/components/dubbing-panel.tsx` — UI panel: start dubbing, show progress, download button

**Job statuses in frontend:**
`pending` → `muxing` → `uploading` → `dubbing` → `downloading` → `muxing_final` → `complete` | `failed`

### API Routes (Backend → Frontend)

| Method | Route | Description |
|---|---|---|
| POST | `/api/dubbing/start` | Start dubbing job for session+lang |
| GET | `/api/dubbing/status/{job_id}` | Poll single job status |
| GET | `/api/dubbing/jobs/{session_id}` | List all jobs for session |
| GET | `/api/dubbing/download/{job_id}` | Download final dubbed video |

### Request body for `/api/dubbing/start`:
```json
{
  "session_id": "b0242036",
  "lang": "zh",
  "source_lang": "ko"
}
```

## What Needs Work

### Must Fix
1. **Muxing step** — video.fmp4 + host_audio.pcm → MP4 before upload. Verify FFmpeg command handles fMP4 container correctly.
2. **Final mux** — original video + ElevenLabs dubbed audio → playable MP4 with correct A/V sync.
3. **Error recovery** — if ElevenLabs returns error, surface message to frontend clearly.

### Should Improve
4. **Multi-language** — support dubbing same session into multiple languages simultaneously (parallel jobs).
5. **Progress feedback** — ElevenLabs doesn't give percentage. Estimate from `expected_duration_sec`.
6. **Storage cleanup** — delete raw recordings after dubbing complete + downloaded.
7. **Cost tracking** — log dubbing API usage per session for billing.

### Nice to Have
8. **Partial dubbing** — use `start_time`/`end_time` for clips instead of full session.
9. **Dubbing Studio link** — deep-link to ElevenLabs web editor for manual transcript correction.
10. **Batch dubbing** — auto-trigger Tier 4 for all target languages when session ends.

## Architecture Rules

Follow `claude.md` in repo root:
- Dubbing client lives in `shared/dubbing/` (Shared layer)
- Recording lives in `shared/recording/` (Shared layer)
- Dubbing orchestration (mux + upload + poll + download + serve) lives in `features/` layer
- Frontend dubbing feature in `frontend/src/features/dubbing/`
- No backward dependencies (Features cannot import from Orchestration)

## Testing

```bash
# Unit tests (types + status parsing)
cargo test -p server-rs dubbing

# Unit tests (recorder)
cargo test -p server-rs recorder

# E2E: start a tier-4 session, stream for 30s, stop, trigger dubbing, wait for complete
# Manual test — no automated E2E yet
```
