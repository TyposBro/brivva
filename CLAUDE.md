# Brivva Real-Time Translation Prototype

## Purpose

Demo prototype for Brivva interview — showing a real-time voice translation pipeline.
This is to impress them at the paid technical test stage. I'm the top candidate out of 15 and they want to proceed.

## What This Is

A working MVP of Brivva's core product pipeline: **Record voice → STT → Translate → TTS → Playback**
No video/lip-sync for now — audio-only pipeline first.

## Architecture

```
Browser (React/TS)
├── Record audio from mic
├── Send audio to Cloudflare Worker
├── Display transcription + translation
└── Play back translated audio

Cloudflare Worker (JS — will port to Rust WASM later)
├── Receive audio from frontend
├── Deepgram STT (Cloudflare has Deepgram integrated)
├── Translation (Cloudflare AI or external API)
├── Kokoro TTS (generate speech in target language)
└── Return translated audio + text to frontend
```

## Tech Stack

- **Frontend:** React + TypeScript + Vite
- **Backend:** Cloudflare Workers (JavaScript first, Rust WASM later)
- **STT:** Deepgram via Cloudflare (real-time transcription)
- **Translation:** Cloudflare Workers AI (or DeepL API as fallback)
- **TTS:** Kokoro TTS (small, fast, good quality — replaces expensive 3B param "Emotive TTS")

## Key Technical Decisions

- Start with JS Workers to prove the pipeline works fast. Port to Rust WASM as a second step.
- Cloudflare edge handles STT + translation + orchestration (cheap, fast, no server management)
- Only GPU-heavy tasks (lip-sync in future) would need AWS/GCP
- Kokoro TTS chosen over larger models because it's fast enough for real-time and sounds natural

## Why This Matters for the Interview

Brivva's current listed pipeline: Whisper STT → Context NMT → Emotive TTS → Wav2Lip
My proposed improvements:

1. **Deepgram** over Whisper — Cloudflare has it built in, real-time streaming support
2. **Kokoro TTS** over Emotive TTS — much smaller, near-instant, good quality
3. **InfiniteTalk** over Wav2Lip (future) — syncs lips + head + body + expressions, Apache 2.0
4. **Cloudflare edge** for orchestration — only use GPU servers for lip-sync, massive cost reduction

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
brivva-prototype/
├── CLAUDE.md          ← you are here
├── frontend/          ← React + TypeScript + Vite
│   ├── src/
│   │   ├── App.tsx
│   │   ├── components/
│   │   │   ├── AudioRecorder.tsx
│   │   │   ├── TranscriptionDisplay.tsx
│   │   │   └── AudioPlayer.tsx
│   │   └── hooks/
│   │       └── useAudioRecorder.ts
│   └── package.json
└── worker/            ← Cloudflare Worker
    ├── src/
    │   └── index.ts
    ├── wrangler.toml
    └── package.json
```

## Implementation Order

1. Set up project structure (frontend + worker)
2. Build audio recording in React (MediaRecorder API)
3. Set up Cloudflare Worker with Deepgram STT
4. Add translation (Cloudflare AI)
5. Add Kokoro TTS
6. Wire everything together — record → transcribe → translate → speak
7. Polish UI — language selector, waveform visualization, loading states

## Rules

- Ship fast. This is a demo, not production code.
- Use existing Cloudflare account (I run Spiko on it)
- Test with Korean → English first (matches Brivva's K-Beauty use case)
- Keep it simple — no auth, no database, no deployment pipeline
- If something is hard to integrate, mock it and move on
