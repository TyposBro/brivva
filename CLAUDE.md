# Brivva Real-Time Translation Prototype

## Purpose

Demo prototype for Brivva interview — showing a real-time voice translation pipeline with rooms.
I'm the top candidate out of 15 and they want to proceed to a paid technical test.

## Current Status (Mar 11 2026)

**v2 — WORKING and deployed.** Rooms + multi-language pipeline is live:

- **Frontend:** https://brivva.pages.dev (Cloudflare Pages)
- **Worker:** https://brivva-translation.milliytechnology.workers.dev
- **Repo:** https://github.com/TyposBro/brivva (private)
- Host speaks (EN or KO) → guests pick EN/JA/ZH → each gets translated Kokoro audio
- Rooms backed by Durable Objects (single actor per room, pinned to host's DC)
- STT: CF AI Gateway Nova-3 (English), direct Deepgram API (Korean, requires `DEEPGRAM_API_KEY`)
- Deploy: `npm run deploy` in both `worker/` and `frontend/`

**v1** still deployed at `/api/realtime` (EN→JA, single-user, untouched).

---

## v2 Goal — Rooms + Multi-Language

Add rooms so it matches Brivva's live commerce use case:

- Host creates room, speaks Korean (or any source language)
- Guests join room with a code, pick their listening language (EN/JA/ZH)
- Each guest hears translated audio in their language
- Translate ONCE per language group, broadcast to all guests in that group
- This directly mirrors Brivva's product: Korean beauty host → multilingual audience

### Architecture (v2)

```
Host Browser (React/TS)
├── Record audio from mic (ScriptProcessorNode, PCM linear16 @ 16kHz)
├── WebSocket → send binary audio frames to Worker
└── See room code + shareable link + connected guest count per language

Guest Browser (React/TS)
├── Join room with code or direct link (/room/:id?lang=ja)
├── Select listening language (EN/JA/ZH)
├── WebSocket → receive translated audio + subtitles
└── Auto-play translated TTS audio (Blob URL queue, same as v1)

Cloudflare Worker (Hono + Durable Objects)
├── Room management via RoomDO (one DO instance per room, keyed by roomId)
├── Per-room state: host WS, guest WSs grouped by language
├── On host audio:
│   1. Deepgram Nova-3 STT (streaming WebSocket, same as v1)
│   2. On is_final or speech_final:
│      ├── M2M100 translate → EN (if EN guests exist)
│      ├── M2M100 translate → JA (if JA guests exist)
│      └── M2M100 translate → ZH (if ZH guests exist)
│      (Run all translations in parallel via Promise.all)
│   3. Per language: Kokoro TTS → generate audio
│   4. Broadcast: send audio + transcript to all guests in that language group
└── Cleanup on host disconnect → notify all guests
```

### WebSocket Protocol (v2)

**Host messages:**

- `{ type: "host:create", sourceLang: "ko" }` → server responds `{ type: "room:created", roomId: "ABC123" }`
- Binary frames → PCM audio from mic (same format as v1)
- `{ type: "host:end" }` → close room

**Guest messages:**

- `{ type: "guest:join", roomId: "ABC123", lang: "en" }` → server responds `{ type: "room:joined" }`
- Server pushes same message types as v1: interim, final, translation, tts_start, [ArrayBuffer...], tts_end

**Server broadcasts:**

- `{ type: "room:guest_count", counts: { en: 5, ja: 3, zh: 12 } }` → to host
- `{ type: "room:closed" }` → to all guests when host disconnects

### Room State

```typescript
interface Room {
  id: string;
  sourceLang: string; // host's speaking language
  hostWs: WebSocket;
  guests: Map<string, { ws: WebSocket; lang: "en" | "ja" | "zh" }>;
  langGroups: Map<string, Set<string>>; // lang → set of guest IDs
}

// In-memory — rooms die when worker restarts, fine for demo
const rooms = new Map<string, Room>();
```

### Key Optimization: Translate Once, Broadcast Many

If 50 guests listen in Japanese, run ONE JA translation + ONE JA TTS, then send the same audio blob to all 50. This is critical for Brivva's scale — live commerce streams have thousands of viewers.

### UI Design (v2)

**Home page (/):**

- "Create Room" button (becomes host)
- "Join Room" input + button (becomes guest)

**Host View (/host):**

- Room code displayed prominently (e.g., "ABC123")
- Shareable link: `brivva.pages.dev/room/ABC123`
- Live mic waveform + mute button (reuse AudioRecorder from v1)
- Guest count per language: EN: 5 | JA: 3 | ZH: 12
- Live transcript of what host is saying (same as v1)

**Guest View (/room/:id):**

- Language picker: English / 日本語 / 中文
- Live subtitles: original text + translated text
- Auto-playing translated audio (Blob URL queue from v1)
- Connection status indicator

### Implementation Order (v2)

v1 pipeline is done. For v2, build in this order:

1. Add room state management to Worker (in-memory Map, create/join/leave)
2. New WebSocket endpoint: /api/room (or refactor /api/realtime to support rooms)
3. Host flow: create room → get room ID → start streaming audio (reuse existing STT pipeline)
4. Guest flow: join room → receive translated audio for their language
5. Fan-out logic: on utterance complete, check which languages have guests, translate in parallel, TTS per language, broadcast
6. Frontend routing: / (home) → /host (create) → /room/:id (guest join)
7. Frontend host view: room code + guest counts + existing mic/transcript UI
8. Frontend guest view: language picker + subtitles + audio playback
9. Test with 3 browser tabs: 1 host (Korean), 1 EN guest, 1 JA guest

### Latency Optimization Ideas

- **Parallel translation:** Run EN/JA/ZH translations simultaneously (Promise.all)
- **Utterance chunking:** Translate shorter utterances for faster turnaround
- **Warm connections:** Keep Deepgram WebSocket alive between utterances
- **Edge caching:** Cache repeated TTS for common phrases

---

## v1 Architecture (current, working)

```
Browser mic
  → PCM linear16 @ 16kHz (ScriptProcessorNode)
  → WebSocket → Cloudflare Worker
  → Nova-3 (Deepgram, streaming STT via CF AI Gateway WS)
  → M2M100-1.2B (CF Workers AI, translation EN→JA)
  → Kokoro 82M (self-hosted on M1 Pro MPS, voice: jf_alpha)
  → WebSocket binary → Browser Blob URL playback → speaker
```

### Measured Latency (self-hosted Kokoro, M1 Pro MPS)

| Phase                        | Typical                         |
| ---------------------------- | ------------------------------- |
| STT (first interim → final)  | 1–2s (includes speech duration) |
| Translation (M2M100 CF)      | 394–870ms                       |
| TTS generation (Kokoro MPS)  | 584ms–2300ms                    |
| **Total from stop speaking** | **~1.5–3s**                     |

### Message Protocol (v1, Worker ↔ Browser)

```
{ type: "interim",     transcript, utteranceId }          ← live words while speaking
{ type: "final",       transcript, utteranceId }          ← utterance finalized
{ type: "translation", text, utteranceId, translateMs }   ← translated text + timing
{ type: "tts_start",   utteranceId }                      ← audio stream starting
[ArrayBuffer...]                                          ← raw MP3 audio bytes
{ type: "tts_end",     utteranceId, ttsMs }               ← audio done + timing
{ type: "error",       message }                          ← pipeline error
```

## Tech Stack

- **Frontend:** React + TypeScript + Vite, deployed on Cloudflare Pages
- **Backend:** Cloudflare Workers (Hono framework)
- **STT:** `@cf/deepgram/nova-3` via CF AI Gateway WebSocket (real-time streaming)
- **Translation:** `@cf/meta/m2m100-1.2b` (dedicated seq2seq, ~500ms CF)
- **TTS:** Kokoro 82M self-hosted on M1 Pro MPS via Kokoro-FastAPI
  - Japanese voice: `jf_alpha` (~430ms+ short text)
  - English voice: `af_bella` (Grade A-)
  - Chinese voice: `zf_xiaobei` (experimental)
  - French voice: `ff_siwis`

## Key Technical Decisions & Bug Fixes

- **Nova-3 over Whisper** — streaming (interims while speaking) vs batch-only. Non-negotiable for real-time.
- **M2M100 over LLM** — dedicated seq2seq, ~3x faster than llama-3.2-1b
- **Trigger on `is_final` not just `speech_final`** — speech_final only fires on silence
- **`state.pending` fallback** — speech_final sometimes has empty transcript, use last interim
- **TTS playback — Blob URL** — MSE addSourceBuffer("audio/mpeg") throws on Safari; Blob URL works everywhere
- **TTS queue freeze** — `audio.play()` rejection must call `onDone()` or `isTtsPlayingRef` stays true forever
- **AudioContext unlock** — call `ctx.resume()` on user gesture (language picker click) before first `audio.play()`
- **Host transcript** — `handleNovaMessage` must send interim/final to both `hostWs` and all guests
- **CF AI Gateway language support** — Nova-3 only works with `language=en`; Korean requires direct Deepgram API
- **Durable Objects over in-memory Map** — CF Workers can run in multiple isolates; in-memory Map causes "Room not found" for guests on a different isolate
- **`clientWs.send(value)` not `value.buffer`** — Uint8Array subview bug
- **UniDic required for Japanese** — `python -m unidic download` (526MB), checked by kokoro-start.sh

## Why This Matters for the Interview

Brivva's listed pipeline: Whisper STT → Context NMT → Emotive TTS → Wav2Lip
My improvements (demonstrated in v1, extended in v2):

1. **Deepgram Nova-3** over Whisper — real-time streaming, not batch
2. **M2M100 on CF edge** — no external API, ~500ms
3. **Kokoro TTS** over Emotive TTS (3B params) — 82M params, open-source, self-hostable, fast
4. **Cloudflare edge** for STT+translate — only TTS needs external compute
5. **Rooms + multi-language** (v2) — directly maps to their live commerce product
6. **InfiniteTalk** over Wav2Lip (future) — full body + expression sync, Apache 2.0

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

## Worker Secrets (already set)

```
CF_ACCOUNT_ID    = 80a55132ae169d5b282ccf505bc66bf7
CF_AI_GATEWAY_ID = default
CF_API_TOKEN     = (wrangler secret) — CF AI Gateway auth
KOKORO_URL       = https://kokoro.milliytechnology.org
```

Update: `cd worker && echo "value" | npx wrangler secret put SECRET_NAME --env=""`

## Running Locally

### 1. Start Kokoro + Tunnel

```bash
cd ~/Documents/private/brivva
bash kokoro-start.sh
```

Verify: `curl http://localhost:8880/health` → `{"status":"healthy"}`

### 2. Deploy Worker (after code changes)

```bash
cd ~/Documents/private/brivva/worker
npx wrangler deploy --env=""
```

### 3. Open the App

https://brivva.pages.dev or `cd frontend && npm run dev`

## Project Structure

```
brivva/
├── CLAUDE.md
├── frontend/                          ← React + TypeScript + Vite
│   ├── src/
│   │   ├── App.tsx                    ← main UI (live transcript + utterances + timing)
│   │   ├── components/
│   │   │   ├── AudioRecorder.tsx      ← waveform canvas + record button
│   │   │   ├── AudioPlayer.tsx        ← standalone TTS player (legacy)
│   │   │   └── TranscriptionDisplay.tsx
│   │   └── hooks/
│   │       ├── useRealtimeTranslation.ts  ← WebSocket + PCM capture + TTS queue
│   │       └── useAudioRecorder.ts        ← legacy batch recorder
│   └── .env                           ← VITE_WORKER_URL
├── worker/                            ← Cloudflare Worker (Hono)
│   ├── src/
│   │   ├── index.ts                   ← app entry, routes mounted
│   │   ├── core/types.ts             ← Bindings (AI, CF secrets)
│   │   └── features/
│   │       ├── realtime/api/
│   │       │   └── realtime.routes.ts ← /api/realtime WebSocket (v1 pipeline)
│   │       ├── translation/
│   │       └── tts/
│   ├── wrangler.toml
│   └── package.json
├── kokoro/                            ← Kokoro-FastAPI (self-hosted)
├── kokoro-start.sh                    ← starts Kokoro + cloudflared tunnel
└── docs.md                            ← full technical analysis
```

## Rules

- Ship fast. This is a demo, not production code.
- Use existing Cloudflare account (Spiko runs on same account)
- Host language: Korean (matches Brivva's K-Beauty use case)
- Guest languages: EN, JA, ZH (Brivva's priority markets)
- No auth, no persistent storage — in-memory rooms are fine
- Room system should work with at least 3 simultaneous connections for demo
- Reuse as much v1 code as possible — the STT/translate/TTS pipeline doesn't change, just add room routing around it
