# Brivva Real-Time Translation Demo

Real-time multilingual live commerce prototype: **Host speaks → guests hear translated audio in their language.**

Live: https://brivva.pages.dev

## What It Does (v2)

- Host creates a room, speaks into mic (English or Korean)
- Guests join with a room code, pick their language (EN / JA / ZH)
- Each guest sees live subtitles + hears Kokoro TTS audio in their language
- Translation runs once per language group — 50 JA guests = 1 translation call, not 50

## Prerequisites

- Node.js + npm
- Python (via `uv`) — for Kokoro TTS
- Cloudflare account (already configured)
- Homebrew + cloudflared

---

## Running End-to-End

### 1. Start Kokoro + Tunnel

```bash
cd ~/Documents/private/brivva
bash kokoro-start.sh
```

Starts Kokoro on `:8880` and the cloudflared tunnel. `Ctrl+C` stops both.

Verify: `curl http://localhost:8880/health` → `{"status":"healthy"}`

### 2. Deploy Worker (after code changes)

```bash
cd ~/Documents/private/brivva/worker
npm run deploy
```

### 3. Deploy Frontend (after code changes)

```bash
cd ~/Documents/private/brivva/frontend
npm run deploy
```

### 4. Open the App

https://brivva.pages.dev

Or run locally:

```bash
cd ~/Documents/private/brivva/frontend
npm run dev
# open http://localhost:5173
```

---

## Architecture (v2)

```
Host Browser (/host)
  → mic → PCM linear16 @ 16kHz (ScriptProcessorNode)
  → WebSocket /api/room?role=host&sourceLang=en
  → Worker generates roomId → RoomDO (Durable Object)
  → room:created{roomId} → host sees room code + guest counts

RoomDO (single actor per room, pinned to host's DC)
  → Nova-3 STT (Deepgram direct API or CF AI Gateway fallback)
  → On is_final / speech_final:
      Promise.all([M2M100 →en, M2M100 →ja, M2M100 →zh])  (active langs only)
  → Per language: Kokoro TTS → stream audio chunks to all guests in group

Guest Browser (/room/:id)
  → pick language → WebSocket /api/room?role=guest&roomId=...&lang=ja
  → receives: interim, final, translation, tts_start, [MP3 chunks], tts_end
  → Blob URL playback queue, FIFO, no overlap
```

## Measured Latency

| Phase | Typical |
|-------|---------|
| STT (Nova-3, first interim → final) | 1–2s (includes speech) |
| Translation (M2M100, CF Workers AI) | 394–870ms |
| TTS generation (Kokoro, M1 Pro MPS) | 584ms–2300ms |
| **Total from stop speaking** | **~1.5–3s** |

## Project Structure

```
brivva/
├── frontend/                   React + TypeScript + Vite (Cloudflare Pages)
│   └── src/
│       ├── App.tsx             router wrapper
│       ├── pages/
│       │   ├── HomePage.tsx    create/join room
│       │   ├── HostPage.tsx    room code + guest counts + mic
│       │   └── GuestPage.tsx   language picker + subtitles + audio
│       └── hooks/
│           ├── useHostRoom.ts  WebSocket + PCM capture + STT display
│           └── useGuestRoom.ts WebSocket + TTS Blob URL queue
├── worker/                     Cloudflare Worker (Hono + Durable Objects)
│   └── src/
│       ├── index.ts            routes + RoomDO export
│       ├── core/types.ts       Bindings
│       └── features/rooms/
│           ├── api/room.routes.ts   thin DO proxy
│           └── room.do.ts           all room state + STT/translate/TTS fan-out
├── kokoro/                     Kokoro-FastAPI (self-hosted)
├── kokoro-start.sh             starts Kokoro + cloudflared tunnel
├── docs.md                     full technical analysis
└── CLAUDE.md                   project context for Claude Code
```

## Worker Secrets

```
CF_ACCOUNT_ID    = 80a55132ae169d5b282ccf505bc66bf7
CF_AI_GATEWAY_ID = default
CF_API_TOKEN     = (CF AI Gateway auth)
KOKORO_URL       = https://kokoro.milliytechnology.org
DEEPGRAM_API_KEY = (optional — enables Korean STT via direct Deepgram API)
```

Set a secret:
```bash
cd worker && echo "value" | npx wrangler secret put SECRET_NAME --env=""
```

## STT: Language Support

| Config | STT path | Languages |
|--------|----------|-----------|
| No `DEEPGRAM_API_KEY` | CF AI Gateway (`@cf/deepgram/nova-3`) | English only |
| `DEEPGRAM_API_KEY` set | Direct Deepgram API | All languages incl. Korean |

Host passes `sourceLang=en` (or `ko`) in the WebSocket URL. Default is `ko`.
