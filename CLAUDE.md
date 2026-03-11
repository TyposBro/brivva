# Brivva Real-Time Translation Prototype

## Purpose

Demo prototype for Brivva interview — showing a real-time voice translation pipeline.
This is to impress them at the paid technical test stage. I'm the top candidate out of 15 and they want to proceed.

## Current Status (Mar 11 2026)

**Fully working and deployed.** Complete real-time pipeline is live and verified:
- **Frontend:** https://brivva.pages.dev (Cloudflare Pages)
- **Worker:** https://brivva-translation.milliytechnology.workers.dev
- **Repo:** https://github.com/TyposBro/brivva (private)

Verified latency from session logs (English → Japanese, Kokoro `jf_alpha`, self-hosted M1 Pro):
- Translation (M2M100): 394–870ms CF-side
- TTS (Kokoro self-hosted MPS): ~430ms short text (Japanese is compact)
- Total from FINAL to audio done: ~887ms short utterance
- No cold starts, no rate limits
- Continuous speech correctly segmented — multiple translation cards appear while speaking

See `docs.md` for full technical analysis including STT/TTS provider comparisons.

## What This Is

A working MVP of Brivva's core product pipeline: **Speak → STT → Translate → TTS → Playback**
Hardcoded: English → Japanese. Audio-only (no video/lip-sync).

## Architecture (current)

```
Browser (React/TS)
├── Web Audio API → PCM linear16 @ 16kHz (ScriptProcessorNode)
├── WebSocket → Cloudflare Worker
├── Receive: interim words (live) + final + translation + TTS audio
└── Play TTS audio via Blob URL + new Audio() (FIFO queue, no overlap)

Cloudflare Worker (Hono/TS)
├── /api/realtime   ← WebSocket endpoint
│   ├── Proxy PCM audio → Nova-3 via CF AI Gateway WebSocket
│   ├── Stream interim transcripts back immediately
│   ├── On is_final OR speech_final → M2M100 translate → Kokoro TTS
│   └── Stream TTS audio back through same WebSocket
├── /api/translate  ← legacy batch HTTP (kept as fallback)
└── /api/tts        ← standalone TTS endpoint
```

## Tech Stack

- **Frontend:** React + TypeScript + Vite, deployed on Cloudflare Pages
- **Backend:** Cloudflare Workers (Hono framework)
- **STT:** `@cf/deepgram/nova-3` via CF AI Gateway WebSocket (real-time streaming)
- **Translation:** `@cf/meta/m2m100-1.2b` (dedicated seq2seq, ~500ms CF)
- **TTS:** Kokoro 82M self-hosted on M1 Pro MPS via Kokoro-FastAPI (Japanese voice: `jf_alpha`, ~430ms+)

## Real-Time Pipeline Detail

```
PCM audio (4096-sample chunks, 16kHz)
    ↓ WebSocket binary
Nova-3 (CF AI Gateway WS)
    ├── is_final=false  → interim transcript → show live in UI
    ├── is_final=true   → finalized chunk → translate + TTS immediately
    └── speech_final=true → utterance endpoint → translate + TTS (uses
                             last interim if transcript is empty)
                              ↓
                    M2M100 1.2B → Japanese text (~500ms CF)
                              ↓
                    Kokoro jf_alpha self-hosted MPS → MP3 audio (~430ms+)
                              ↓
                    WebSocket binary → browser Blob URL playback
```

**Message protocol (Worker ↔ Browser):**
```
{ type: "interim",     transcript, utteranceId }              ← live words
{ type: "final",       transcript, utteranceId }              ← chunk done
{ type: "translation", text, utteranceId, translateMs }       ← Japanese text + CF timing
{ type: "tts_start",   utteranceId }                          ← audio coming
[ArrayBuffer...]                                              ← raw audio bytes
{ type: "tts_end",     utteranceId, ttsMs }                   ← audio done + CF timing
```

**TTS playback queue (frontend):**
- Chunks buffered into `entry.chunks[]` while streaming
- On `tts_end`: `new Blob(chunks, {type:"audio/mpeg"})` → `new Audio(blobUrl).play()`
- `isTtsPlayingRef` gates playback — no overlap, FIFO order
- MSE (`audio/mpeg`) was tried first but throws on Safari — Blob URL works everywhere

**Per-utterance timing display:**
- Each card shows: `translate: Xms (CF: Yms) · audio: Xms (CF: Yms)`
- "Copy log" button copies full JSON session log for analysis

## Worker Secrets (already set)

```
CF_ACCOUNT_ID    = 80a55132ae169d5b282ccf505bc66bf7
CF_API_TOKEN     = (set via wrangler secret)
CF_AI_GATEWAY_ID = default
KOKORO_URL       = https://kokoro.milliytechnology.org
```

To update: `cd worker && npx wrangler secret put <NAME> --env=""`

## Key Technical Decisions & Bug Fixes

- **M2M100 over LLM for translation** — dedicated seq2seq, ~3x faster than llama-3.2-1b for EN→FR
- **Trigger on `is_final` not just `speech_final`** — speech_final only fires on silence; is_final fires for each Deepgram chunk during continuous speech
- **`state.pending` fallback** — speech_final sometimes arrives with empty transcript (endpoint signal only); we use last non-empty interim
- **TTS queue** — `isTtsPlayingRef` prevents overlap; each clip plays after previous ends
- **TTS playback — Blob URL** — `MediaSource.addSourceBuffer("audio/mpeg")` throws on Safari; fixed by buffering all chunks, combining into `Blob`, playing via `new Audio(blobUrl)`. Works on all browsers.
- **`clientWs.send(value)` not `value.buffer`** — Uint8Array subview bug; full underlying buffer contained garbage bytes
- **ScriptProcessorNode + Int16 PCM** — MediaRecorder gives WebM chunks; Nova-3 needs raw PCM linear16; Web Audio captures Float32 and converts
- CF secrets needed for AI Gateway: `CF_ACCOUNT_ID`, `CF_API_TOKEN` (AI Gateway Run + Workers AI Run), `CF_AI_GATEWAY_ID=default`
- **Kokoro self-hosted** — Kokoro-FastAPI on M1 Pro MPS, voice `jf_alpha` (Japanese female); exposed via cloudflared named tunnel at `kokoro.milliytechnology.org`
- **UniDic download required** — `misaki[ja]` needs MeCab + UniDic dictionary; `unidic` pip package installs without data — must run `python -m unidic download` once (526MB). `kokoro-start.sh` checks and downloads automatically.

## Why This Matters for the Interview

Brivva's current listed pipeline: Whisper STT → Context NMT → Emotive TTS → Wav2Lip
My proposed improvements (now demonstrated):

1. **Deepgram Nova-3** over Whisper — real-time WebSocket streaming (interims while speaking); Whisper is batch-only, cannot stream
2. **M2M100 on CF edge** over Context NMT — no external API, dedicated translation model, ~500ms
3. **Kokoro TTS** over Emotive TTS — open-source (Apache 2.0), 587ms warm, self-hostable on M1 Pro
4. **Cloudflare edge** for STT+translate pipeline — only TTS needs external compute
5. **InfiniteTalk** over Wav2Lip (future) — full body + expression sync, Apache 2.0

**STT decision (settled):** Nova-3 is non-negotiable for real-time. Whisper Large v3 Turbo is faster/cheaper/more accurate in batch (Groq: 375.9x, 4.8% WER, $0.67/1000min vs Nova-3: 222.6x, 6.5% WER, $4.30/1000min) but cannot stream. Different tools for different jobs.

**TTS decision (done):** Kokoro self-hosted on M1 Pro MPS. No cold starts, no rate limits, ~430ms for short Japanese text.

## About Brivva (from interview Mar 10)

- Real-time multilingual live commerce platform
- Voice translation + lip-sync for live streams
- Distribute to TikTok, Naver, Instagram, Rakuten simultaneously
- Founders have a previous exit
- Seed round closing April 2025
- 10 paying customers ($30-80K contracts each)
- Stack: Rust backend, React/TS frontend, AWS
- Target latency: <300ms end-to-end
- They acknowledged I need ~3 months Rust ramp-up and are fine with it

## My Background (relevant context)

- Built Spiko solo — 50K+ MAU AI English coaching app, profitable
- Android (Kotlin) + serverless backend (Cloudflare Workers, Hono, D1)
- 140+ API routes, 80+ DB tables, 15 cron jobs
- Real-time audio experience: chunked upload, ExoPlayer, WebSocket
- Open source: Hono.js contributor, uzpay npm package
- DON'T know Rust yet (on chapter 3 of Rust book) but understand ownership/borrowing concepts
- Main stack: TypeScript, React, Next.js, Kotlin

## Project Structure

```
brivva/
├── CLAUDE.md
├── frontend/                          ← React + TypeScript + Vite
│   ├── src/
│   │   ├── App.tsx                    ← main UI (live transcript + utterances + timing)
│   │   ├── App.css
│   │   ├── components/
│   │   │   ├── AudioRecorder.tsx      ← waveform canvas + record button
│   │   │   ├── AudioPlayer.tsx        ← standalone TTS player (legacy)
│   │   │   └── TranscriptionDisplay.tsx
│   │   └── hooks/
│   │       ├── useRealtimeTranslation.ts  ← WebSocket + PCM capture + TTS queue + session log
│   │       └── useAudioRecorder.ts        ← legacy batch recorder
│   └── .env                           ← VITE_WORKER_URL=https://...workers.dev
└── worker/                            ← Cloudflare Worker (Hono)
    ├── src/
    │   ├── index.ts                   ← app entry, routes mounted
    │   ├── core/
    │   │   ├── types.ts               ← Bindings (AI, CF secrets), Variables
    │   │   └── middleware/
    │   │       └── error.middleware.ts
    │   └── features/
    │       ├── realtime/api/
    │       │   └── realtime.routes.ts ← /api/realtime WebSocket proxy + timing
    │       ├── translation/
    │       │   ├── api/translation.routes.ts
    │       │   └── core/translation.service.ts
    │       └── tts/api/
    │           └── tts.routes.ts
    ├── wrangler.toml
    └── package.json

## Rules

- Ship fast. This is a demo, not production code.
- Use existing Cloudflare account (Spiko runs on same account)
- Keep it simple — no auth, no database
- If something is hard to integrate, mock it and move on
