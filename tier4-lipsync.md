# Tier 4 — Post-Processed Lipsync via Sync Labs

## Overview

After a live broadcast session ends, the host can trigger lipsync post-processing for any target language. The system records video and translated audio during the live session, then submits them to Sync Labs' API to generate a lip-synced video where the host's mouth matches the translated speech. The result is a downloadable MP4 per language — suitable for VOD replay on Coupang, Rakuten, YouTube, etc.

## Why Tier 4 (Post-Processed) Before Tier 3 (Live)

- No GPU required on the host machine
- No additional latency during live broadcast
- Sync Labs handles all compute (~5-15min for 1hr video)
- ~$0.02-0.05/sec of video ($72-180 for a 1hr session)
- Live lipsync (Tier 3) needs a dedicated A100/H100 GPU server + adds 100-200ms latency

## Architecture

```
LIVE SESSION (existing + recording)
┌──────────────────────────────────────────────────────────────────┐
│                                                                  │
│  Host Video (fMP4 chunks) ──┬──→ FFmpeg stdin → RTMP push       │
│                             └──→ SessionRecorder → video.fmp4   │
│                                                                  │
│  Translated Audio (PCM)  ──┬──→ StreamingPcm → RTMP push        │
│  (per target language)     └──→ SessionRecorder → audio_ru.pcm  │
│                                                                  │
│  Host Audio (PCM)        ────→ SessionRecorder → host_audio.pcm │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
        │
        ▼
  /tmp/brivva/recordings/{session_id}/
  ├── video.fmp4          (raw fMP4 from MediaRecorder)
  ├── audio_ru.pcm        (translated Russian audio, 44.1kHz 16-bit mono)
  ├── audio_ja.pcm        (translated Japanese audio)
  └── host_audio.pcm      (original host audio, for reference/debugging)


POST-SESSION (triggered by user)
┌──────────────────────────────────────────────────────────────────┐
│                                                                  │
│  1. Mux video.fmp4 → video.mp4 (FFmpeg, ~10s)                  │
│  2. Mux audio_ru.pcm → audio_ru.wav (PCM header, instant)      │
│  3. Upload video.mp4 + audio_ru.wav → Sync Labs API             │
│  4. Poll job status every 30s                                    │
│  5. Download result → ru_lipsync.mp4                            │
│  6. Repeat steps 2-5 for each target language                   │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
        │
        ▼
  /tmp/brivva/lipsync/{session_id}/
  ├── video.mp4             (muxed source video, uploaded once)
  ├── audio_ru.wav          (muxed translated audio)
  ├── ru_lipsync.mp4        (final lipsync'd video — Russian)
  ├── audio_ja.wav
  ├── ja_lipsync.mp4        (final lipsync'd video — Japanese)
  └── ...
```

## Components

### 1. Session Recorder (`shared/recording/`)

Records raw video and audio to disk during a live session. Runs alongside existing RTMP pipeline — no impact on live stream performance.

**Files:**
- `shared/recording/recorder.rs` — SessionRecorder struct, manages file handles
- `shared/recording/mod.rs` — module declaration

**SessionRecorder struct:**
```
SessionRecorder {
    session_id: String,
    output_dir: PathBuf,                              // /tmp/brivva/recordings/{session_id}
    video_file: Option<Mutex<BufWriter<File>>>,       // video.fmp4
    audio_files: Mutex<HashMap<String, BufWriter<File>>>,  // audio_{lang}.pcm per language
    host_audio_file: Option<Mutex<BufWriter<File>>>,  // host_audio.pcm
    enabled: bool,                                     // controlled by session tier
}
```

**Methods:**
- `new(session_id, tier) -> Self` — creates output dir, opens files if tier >= 4
- `write_video_chunk(data: &[u8])` — appends to video.fmp4 (called from push_video_chunk)
- `write_translated_audio(lang: &str, pcm: &[u8])` — appends to audio_{lang}.pcm
- `write_host_audio(pcm: &[u8])` — appends to host_audio.pcm
- `finalize()` — flushes and closes all file handles

**Integration points:**
- `manager.rs::push_video_chunk()` — add `recorder.write_video_chunk(data)`
- `final_handler.rs` after TTS completes — add `recorder.write_translated_audio(lang, pcm)`
- `handler.rs::drain_host_audio()` — add `recorder.write_host_audio(audio)`
- `cleanup.rs::cleanup_session()` — add `recorder.finalize()`

**Storage on Session:**
- Add `recorder: Option<Arc<SessionRecorder>>` to Session struct
- Created during session setup when tier >= 4
- Passed through via SttContext (similar to how pipeline_counters is passed)

### 2. Post-Processor (`features/lipsync/`)

Converts raw recordings into uploadable formats. Vertical feature slice.

**Files:**
- `features/lipsync/domain/types.rs` — LipsyncJob, LipsyncStatus types
- `features/lipsync/data/muxer.rs` — FFmpeg muxing (fMP4→MP4, PCM→WAV)
- `features/lipsync/data/sync_labs.rs` — Sync Labs API client
- `features/lipsync/data/job_runner.rs` — orchestrates mux → upload → poll → download

**LipsyncJob:**
```
LipsyncJob {
    id: String,                    // UUID
    session_id: String,
    lang: String,                  // target language
    status: LipsyncStatus,
    sync_labs_job_id: Option<String>,
    created_at: Instant,
    output_path: Option<PathBuf>,  // final lipsync'd video
    error: Option<String>,
}

enum LipsyncStatus {
    Pending,          // queued
    Muxing,           // FFmpeg converting raw → MP4/WAV
    Uploading,        // uploading to Sync Labs
    Processing,       // Sync Labs working
    Downloading,      // downloading result
    Complete,         // done, output_path set
    Failed,           // error set
}
```

**Muxer (muxer.rs):**
```
// fMP4 → MP4 (required because Sync Labs needs a proper MP4, not raw fMP4 fragments)
async fn mux_video(input: &Path, output: &Path) -> Result<(), String>
  // ffmpeg -i video.fmp4 -c copy video.mp4

// PCM → WAV (add WAV header to raw PCM)
fn mux_audio(input: &Path, output: &Path) -> Result<(), String>
  // Uses existing core::wav::pcm_to_wav() — already in codebase
```

**Sync Labs Client (sync_labs.rs):**

API: `https://api.synclabs.so/v2`

```
struct SyncLabsClient {
    api_key: String,
    http: reqwest::Client,
}
```

**Methods:**
```
// Step 1: Create lipsync job
async fn create_job(video_url: &str, audio_url: &str) -> Result<String, String>
  // POST /v2/lipsync
  // Body: { "videoUrl": "...", "audioUrl": "...", "model": "sync-2.0.0" }
  // Returns: { "id": "job-uuid" }

// Step 2: Poll job status
async fn get_job_status(job_id: &str) -> Result<SyncLabsJobStatus, String>
  // GET /v2/lipsync/{job_id}
  // Returns: { "status": "PENDING|PROCESSING|COMPLETED|FAILED", "outputUrl": "..." }

// Step 3: Download result
async fn download_result(url: &str, output: &Path) -> Result<(), String>
  // GET outputUrl → write to file
```

**Note on file uploads:** Sync Labs accepts URLs, not direct file uploads. Options:
- **Option A:** Use Sync Labs' upload endpoint if available
- **Option B:** Upload to a temporary S3/GCS presigned URL, pass URL to Sync Labs
- **Option C:** Serve files locally via the existing Brivva HTTP server (127.0.0.1:3000) and expose them temporarily — works only if Sync Labs can reach the URL (they can't for localhost)

**Recommended: Option B** — upload recordings to a cloud storage bucket with a presigned URL. Simple, reliable, no networking issues. Requires one env var: `LIPSYNC_UPLOAD_BUCKET` or use Sync Labs' own upload mechanism.

**Job Runner (job_runner.rs):**
```
async fn run_lipsync_job(
    session_id: &str,
    lang: &str,
    recording_dir: &Path,
    sync_labs: &SyncLabsClient,
) -> Result<PathBuf, String> {
    // 1. Mux video.fmp4 → video.mp4 (skip if already done for another lang)
    // 2. Mux audio_{lang}.pcm → audio_{lang}.wav
    // 3. Upload video.mp4 + audio_{lang}.wav (get URLs)
    // 4. Create Sync Labs job (video_url, audio_url)
    // 5. Poll every 30s until COMPLETED or FAILED
    // 6. Download result → {lang}_lipsync.mp4
    // 7. Return output path
}
```

### 3. API Endpoint (`features/lipsync/data/handlers.rs`)

Expose lipsync operations via the existing Axum router.

**Endpoints:**
```
POST /api/lipsync/start
  Body: { "session_id": "...", "lang": "ru" }
  Response: { "job_id": "...", "status": "pending" }

GET /api/lipsync/status/{job_id}
  Response: { "job_id": "...", "status": "processing", "progress": 45 }

GET /api/lipsync/jobs/{session_id}
  Response: [{ "job_id": "...", "lang": "ru", "status": "complete", "output_path": "..." }, ...]

GET /api/lipsync/download/{job_id}
  Response: (binary MP4 file stream)
```

**State:** Jobs stored in `Arc<Mutex<HashMap<String, LipsyncJob>>>` on the app context. Not persisted — lipsync jobs are ephemeral (restart = re-trigger). For persistence later, could add SQLite.

### 4. Frontend (`features/lipsync/`)

Minimal UI added to the broadcast page after a session ends.

**Files:**
- `features/lipsync/domain/types.ts` — LipsyncJob, LipsyncStatus types
- `features/lipsync/data/api.ts` — fetch wrappers for /api/lipsync/*
- `features/lipsync/presentation/hooks/use-lipsync-jobs.ts` — poll job status
- `features/lipsync/presentation/components/lipsync-panel.tsx` — job list + trigger button

**UI:**
```
┌─────────────────────────────────────────────────────────┐
│  Post-Processing — Lipsync                              │
│                                                         │
│  Russian   [Generate Lipsync]   ● Processing (45%)      │
│  Japanese  [Generate Lipsync]   ○ Not started            │
│                                                         │
│  Completed:                                             │
│  Russian   ru_lipsync.mp4   [Download]                   │
└─────────────────────────────────────────────────────────┘
```

- "Generate Lipsync" button per language (disabled during live session, enabled after stop)
- Status updates via polling GET /api/lipsync/status every 5s
- Download button when complete

## Configuration

**Environment variables (orchestration only):**
```
SYNC_LABS_API_KEY=...          # Sync Labs API key
LIPSYNC_UPLOAD_BUCKET=...     # S3/GCS bucket for temporary file uploads (optional — can use Sync Labs' upload)
BRIVVA_RECORDING_DIR=/tmp/brivva/recordings  # Override recording directory (optional)
```

**Session config (from frontend):**
- Tier 4 selection in tier-selector.tsx enables recording
- Recording starts automatically when tier >= 4

## File Size Estimates

For a 1-hour session:
- `video.fmp4`: ~500MB-1.5GB (H.264, depends on resolution/bitrate)
- `audio_{lang}.pcm`: ~50-100MB per language (44.1kHz 16-bit mono, but only translated portions — not full hour)
- `video.mp4` (muxed): same as fmp4 (codec copy)
- `audio_{lang}.wav`: same as PCM + 44 byte header
- Total disk: ~1-2GB per session

## Cost Estimate

Sync Labs pricing (~$0.02-0.05/sec):
- 1hr session × $0.03/sec = ~$108 per language
- 3 languages = ~$324 per session
- 30 sessions/month = ~$9,720/month

This is premium pricing for premium output. The business case: lipsync'd VOD content has higher engagement than subtitles-only, and can be repurposed across multiple platforms.

## Implementation Order

1. **Session Recorder** — recording during live session (no external deps)
2. **Muxer** — FFmpeg post-processing (no external deps)
3. **Sync Labs Client** — API integration (needs API key)
4. **Job Runner** — orchestration
5. **API Endpoints** — REST handlers
6. **Frontend** — lipsync panel UI

Steps 1-2 can be tested independently. Steps 3-6 need a Sync Labs API key.

## Failure Points & Reliability

### Recording Phase (during live session)

| Failure | Impact | Mitigation |
|---------|--------|------------|
| **Disk full** | Recording stops mid-session. Video/audio files truncated. | Check available disk before session start. Require 3GB free minimum. Monitor during session — warn at 500MB remaining, stop recording (not stream) at 100MB. |
| **Disk I/O slow** | `write_video_chunk()` blocks the video drain thread → RTMP stream stutters. | Use `BufWriter` with large buffer (1MB). Write on a **separate thread** — recorder owns a dedicated writer thread with an mpsc channel. Video drain sends chunks via channel, never blocks. |
| **App crash mid-session** | Partial recordings on disk. fMP4 may be unplayable (missing final moov atom). | fMP4 fragments are self-contained — each moof/mdat pair is valid. FFmpeg can read partial fMP4 files. PCM is headerless raw bytes — always valid at any truncation point. Partial recordings are usable. |
| **Session cleanup deletes files** | `cleanup_session()` runs before user triggers lipsync. | Recorder `finalize()` moves files from `/tmp/brivva/recordings/` to a **completed** directory. Cleanup only deletes active session state, not finalized recordings. |
| **Recording impacts live stream** | Disk writes compete with FFmpeg I/O → degraded RTMP quality. | Dedicated writer thread (see above). Use `O_DIRECT` or `O_SYNC` only if needed. Profile: at 8Mbps video + 128kbps audio, disk write is ~1MB/s — trivial for any SSD. |

### Muxing Phase (post-session)

| Failure | Impact | Mitigation |
|---------|--------|------------|
| **FFmpeg mux fails** | Can't convert fMP4 → MP4. Sync Labs can't process. | Validate recording files exist and have non-zero size before muxing. If FFmpeg fails, retry once. If still fails, report error with FFmpeg stderr to user. |
| **Corrupt fMP4** | FFmpeg can't parse input. | fMP4 fragments are resilient — FFmpeg skips corrupt fragments. Test: truncate a recording mid-stream and verify FFmpeg still produces output (it will, just shorter). |
| **PCM → WAV fails** | Unlikely — just prepending a 44-byte header. | The `pcm_to_wav()` function already exists in `core::wav`. Only fails if PCM file is empty (0 bytes). Check file size > 0 before muxing. |

### Upload Phase

| Failure | Impact | Mitigation |
|---------|--------|------------|
| **Upload timeout / network error** | Files not delivered to Sync Labs. | Retry with exponential backoff (3 attempts, 5s/15s/45s). For large files (>500MB video), use multipart upload with resume capability. |
| **S3/GCS credentials expired** | Upload 403. | Validate credentials at app startup. Use long-lived service account key or refresh tokens. Log clear error: "Upload credentials expired — check LIPSYNC_UPLOAD_BUCKET config." |
| **Presigned URL expires before Sync Labs fetches** | Sync Labs gets 403 when downloading from our URL. | Set presigned URL expiry to 24 hours. Sync Labs typically fetches within minutes. |
| **Upload succeeds but Sync Labs can't access** | Job created but immediately fails with "invalid URL." | Verify URL is accessible with a HEAD request before submitting to Sync Labs. If HEAD fails, re-upload. |

### Sync Labs API Phase

| Failure | Impact | Mitigation |
|---------|--------|------------|
| **API key invalid / rate limited** | All jobs fail. | Validate API key at startup with a lightweight API call (GET /v2/account or similar). Circuit breaker: after 3 consecutive API failures, stop submitting and alert user. |
| **Job stuck in PROCESSING** | User waits indefinitely. | Timeout: if job hasn't completed after 60 minutes, mark as failed. Sync Labs typical processing: 5-15min for 1hr video. 60min timeout is generous. |
| **Job fails (FAILED status)** | No lipsync output. | Capture Sync Labs error message. Allow user to retry. Log: session_id, lang, job_id, error. Common causes: audio/video duration mismatch, unsupported codec, face not detected. |
| **Sync Labs outage** | All jobs fail for extended period. | Circuit breaker (reuse existing `core::circuit_breaker`). After 5 failures → open circuit for 5 minutes → half-open probe. Show "Sync Labs unavailable" in UI. |
| **Result URL expires** | Download fails after job completes. | Download result immediately when status changes to COMPLETED. Don't rely on polling later — the output URL may have a TTL. Store locally as `{lang}_lipsync.mp4`. |
| **Audio/video duration mismatch** | Sync Labs rejects or produces glitchy output. The translated audio doesn't cover the full video duration — there are gaps where no translation was spoken. | Pad audio to match video duration before upload. After muxing PCM → WAV, check: if audio_duration < video_duration, append silence to fill the gap. This ensures Sync Labs gets matching durations. |
| **Face not detected** | Sync Labs can't find a face to lipsync. Fails or produces original video unchanged. | Pre-check: log a warning if video resolution < 480p or if the host isn't visible. In practice, Brivva's studio setup (fixed camera, well-lit host) should always have a detectable face. |
| **Sync Labs API changes** | Breaking changes to /v2 endpoints. | Pin to specific API version. Wrap all API calls in `SyncLabsClient` — single place to update. Version check at startup if Sync Labs provides one. |

### Download Phase

| Failure | Impact | Mitigation |
|---------|--------|------------|
| **Download timeout** | Large result file (1-2GB) times out. | Stream download with chunked transfer. Resume support: if download fails mid-way, retry with `Range` header from last byte received. |
| **Disk full during download** | Partial output file. | Check available disk before download (need ~1-2GB per language). Fail early with clear error. |
| **Network interruption** | Partial download. | Retry 3x with resume. Verify file integrity: check file size matches `Content-Length` header. |

### System-Level Failures

| Failure | Impact | Mitigation |
|---------|--------|------------|
| **App restart during lipsync job** | In-memory job state lost. Active Sync Labs jobs become orphaned. | On startup, scan `/tmp/brivva/lipsync/` for incomplete jobs. If a `{lang}_lipsync.mp4` doesn't exist but `audio_{lang}.wav` does, the job was interrupted. Allow user to re-trigger. Sync Labs jobs are idempotent — re-submitting is safe. |
| **Multiple sessions recorded, disk fills** | Old recordings consume disk. | **Retention policy:** auto-delete recordings older than 7 days. On startup, scan recording directory and delete expired sessions. Configurable via `BRIVVA_RECORDING_TTL_DAYS`. |
| **Concurrent lipsync jobs overwhelm disk/network** | Thrashing. | Limit: max 2 concurrent Sync Labs jobs per session. Queue additional requests. Max 1 active upload at a time (sequential uploads, parallel Sync Labs processing). |
| **/tmp partition too small** | macOS /tmp is on the boot volume, typically has space. But Docker/CI environments may have tiny /tmp. | Allow override via `BRIVVA_RECORDING_DIR` env var. Default to `/tmp/brivva/recordings` but support any path. Validate directory is writable at startup. |

### Data Integrity

| Concern | Mitigation |
|---------|------------|
| **Audio/video sync in recording** | Video chunks carry timestamps from `Instant::now()`. Audio PCM is appended as TTS completes. There's no shared timeline — the recording is raw data, not muxed A/V. Sync is reconstructed during muxing: video keeps original timing, audio is aligned to video start. Potential issue: if TTS was slow, audio gaps exist. Pad silence in those gaps during mux. |
| **Init segment not in recording** | The fMP4 init segment (moov/trex) must be the first bytes in `video.fmp4`. The recorder hooks into `push_video_chunk()` which is called for every chunk including the init segment (it's the first call). Verify: first write to video file should be the init segment. |
| **Recording file corruption** | Use `fsync` on `finalize()` to ensure all data is flushed to disk. BufWriter handles batching, `finalize()` calls `flush()` + `sync_all()`. |
| **Translated audio has gaps** | During counting/continuous speech, some force-chunks produce no TTS audio. The PCM file will have gaps. This is fine — Sync Labs processes audio as-is. The lipsync'd video will show original lip movement during gaps (natural behavior). |

### Recovery Playbook

```
SCENARIO: Recording exists but mux failed
  → User clicks "Generate Lipsync"
  → System retries mux
  → If still fails: show FFmpeg error, suggest re-recording

SCENARIO: Upload succeeded but Sync Labs job failed
  → Show Sync Labs error message
  → User clicks "Retry"
  → Re-submit same files (no re-upload if URLs still valid, re-upload if expired)

SCENARIO: App crashed mid-session, partial recording
  → On restart, finalized recordings are preserved
  → User can still trigger lipsync on partial recording
  → Result will be shorter but valid

SCENARIO: Sync Labs is down
  → Circuit breaker opens after 5 failures
  → UI shows "Sync Labs unavailable — try again later"
  → Recordings are preserved on disk indefinitely (within TTL)
  → User retries when service is back

SCENARIO: Disk full during recording
  → Recording stops, live stream continues unaffected
  → Warning shown in health dashboard
  → Partial recording is usable for lipsync (shorter output)
```

## Open Questions

1. **File upload strategy:** Does Sync Labs have a direct upload endpoint, or do we need S3/GCS? Need to check their API docs.
2. **Recording retention:** Default 7 days TTL. Auto-delete on startup. Configurable via `BRIVVA_RECORDING_TTL_DAYS`. Acceptable?
3. **Multiple languages:** Sequential uploads, parallel Sync Labs processing. Max 2 concurrent jobs per session.
4. **Video resolution:** Record at original resolution. Sync Labs handles any resolution. No downscale needed.
5. **Cost approval:** ~$108/language/session — is this within budget? Should there be a confirmation step before triggering?
