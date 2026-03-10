# Brivva Real-Time Translation Prototype

## Purpose

Demo prototype for Brivva interview — showing a real-time voice translation pipeline.
This is to impress them at the paid technical test stage. I'm the top candidate out of 15 and they want to proceed.

## Current Status (Mar 10 2026)

**Working and deployed.** Full real-time pipeline is live:
- **Frontend:** https://brivva.pages.dev (Cloudflare Pages)
- **Worker:** https://brivva-translation.milliytechnology.workers.dev
- **Repo:** https://github.com/TyposBro/brivva (private)

Real-time WebSocket streaming is implemented but pending CF AI Gateway secrets before it goes live.
The silence-detection batch pipeline works in the meantime.

## What This Is

A working MVP of Brivva's core product pipeline: **Speak → STT → Translate → TTS → Playback**
Hardcoded: English → Spanish. Audio-only (no video/lip-sync).

## Architecture (current)

```
Browser (React/TS)
├── Web Audio API → PCM linear16 @ 16kHz
├── WebSocket → Cloudflare Worker
├── Receive: interim words (live) + final + translation + TTS audio
└── Play TTS audio via Audio API

Cloudflare Worker (Hono/TS)
├── /api/realtime   ← WebSocket endpoint
│   ├── Proxy PCM audio → Nova-3 via CF AI Gateway WebSocket
│   ├── Stream interim transcripts back immediately
│   ├── On speech_final → Llama 3.1 8B translate → aura-2-es TTS
│   └── Stream TTS audio back through same WebSocket
├── /api/translate  ← legacy batch HTTP (kept as fallback)
└── /api/tts        ← standalone TTS endpoint
```

## Tech Stack

- **Frontend:** React + TypeScript + Vite, deployed on Cloudflare Pages
- **Backend:** Cloudflare Workers (Hono framework)
- **STT:** `@cf/deepgram/nova-3` via CF AI Gateway WebSocket (real-time streaming)
- **Translation:** `@cf/meta/llama-3.1-8b-instruct` (English → Spanish)
- **TTS:** `@cf/deepgram/aura-2-es` (Spanish, speaker: aquila)

## Real-Time Pipeline Detail

```
PCM audio (250-byte chunks, 16kHz)
    ↓ WebSocket
Nova-3 (CF AI Gateway WS)
    ├── is_final=false  → interim transcript → show live in UI
    └── speech_final=true → final transcript
                              ↓
                    Llama 3.1 8B → Spanish text
                              ↓
                    aura-2-es → MPEG audio stream
                              ↓
                    WebSocket binary → browser Audio API
```

**Message protocol (Worker ↔ Browser):**
```
{ type: "interim",     transcript, utteranceId }   ← live words
{ type: "final",       transcript, utteranceId }   ← sentence done
{ type: "translation", text,       utteranceId }   ← Spanish text
{ type: "tts_start",               utteranceId }   ← audio coming
[ArrayBuffer...]                                   ← MPEG chunks
{ type: "tts_end",                 utteranceId }   ← audio done
```

## Worker Secrets Required

```bash
cd worker
wrangler secret put CF_ACCOUNT_ID     # Cloudflare account ID
wrangler secret put CF_API_TOKEN      # CF API token (AI Gateway: Run permission)
wrangler secret put CF_AI_GATEWAY_ID  # Gateway ID from CF dashboard > AI Gateway
```

## Key Technical Decisions

- Cloudflare edge for entire pipeline — no external services, minimal latency
- PCM linear16 @ 16kHz → standard for STT APIs, no codec overhead
- `utteranceId = Date.now()` — prevents out-of-order translations updating wrong card
- TTS streamed through same WebSocket — avoids second HTTP round-trip
- ScriptProcessorNode for PCM (deprecated but universal; AudioWorklet is the upgrade path)

## Why This Matters for the Interview

Brivva's current listed pipeline: Whisper STT → Context NMT → Emotive TTS → Wav2Lip
My proposed improvements (now demonstrated):

1. **Deepgram Nova-3** over Whisper — real-time WebSocket streaming, better accuracy
2. **Llama 3.1 on CF edge** over Context NMT — no external API, runs at the edge
3. **Deepgram Aura-2** over Emotive TTS — natural voice, CF-native, streaming
4. **Cloudflare edge** for entire STT+translate+TTS pipeline — only GPU needed for lip-sync
5. **InfiniteTalk** over Wav2Lip (future) — full body + expression sync, Apache 2.0

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
│   │   ├── App.tsx                    ← main UI (live transcript + utterances)
│   │   ├── App.css
│   │   ├── components/
│   │   │   ├── AudioRecorder.tsx      ← waveform canvas + record button
│   │   │   ├── AudioPlayer.tsx        ← standalone TTS player (legacy)
│   │   │   └── TranscriptionDisplay.tsx
│   │   └── hooks/
│   │       ├── useRealtimeTranslation.ts  ← WebSocket + PCM capture + TTS playback
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
    │       │   └── realtime.routes.ts ← /api/realtime WebSocket proxy
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
