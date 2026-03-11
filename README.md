# Brivva Real-Time Translation Demo

Real-time voice translation pipeline: **Speak (EN) → STT → Translate → TTS → Play (FR)**

Live: https://brivva.pages.dev

## Prerequisites

- Node.js + npm
- Python (via `uv`)
- Cloudflare account (already configured)
- Homebrew (for cloudflared)

---

## Running End-to-End

### 1. Start Kokoro + Tunnel (one command)

```bash
cd ~/Documents/private/brivva
bash kokoro-start.sh
```

Starts Kokoro on `:8880` and the cloudflared tunnel together. `Ctrl+C` stops both.

Verify: `curl http://localhost:8880/health` → `{"status":"healthy"}`

---

### 3. Deploy Worker (only needed after code changes)

```bash
cd ~/Documents/private/brivva/worker
npx wrangler deploy --env=""
```

Worker is already live at `https://brivva-translation.milliytechnology.workers.dev`.

---

### 4. Open the App

https://brivva.pages.dev

Or run locally:

```bash
cd ~/Documents/private/brivva/frontend
npm run dev
# open http://localhost:5173
```

---

## Architecture

```
Browser mic
  → PCM 16kHz → WebSocket → Cloudflare Worker
  → Nova-3 (Deepgram, streaming STT via CF AI Gateway)
  → M2M100-1.2B (CF Workers AI, EN→FR translation)
  → Kokoro-FastAPI (self-hosted M1 Pro, MPS, ff_siwis voice)
  → WebSocket → Browser MSE streaming playback
```

## Measured Latency (self-hosted Kokoro, M1 Pro MPS)

| Phase | Typical |
|-------|---------|
| STT (first interim → final) | 1–2s (includes speech duration) |
| Translation (M2M100 CF) | 394–870ms |
| TTS generation (Kokoro MPS) | 584ms–2300ms |
| Time-to-first-audio (MSE streaming) | ~200ms after TTS starts |
| **Total from stop speaking** | **~1.5–3s** |

## Project Structure

```
brivva/
├── frontend/          React + TypeScript + Vite (Cloudflare Pages)
│   └── src/
│       ├── App.tsx                          main UI + latency panel
│       └── hooks/useRealtimeTranslation.ts  WebSocket + MSE TTS streaming
├── worker/            Cloudflare Worker (Hono)
│   └── src/features/realtime/api/
│       └── realtime.routes.ts               Nova-3 proxy + M2M100 + Kokoro
├── kokoro/            Kokoro-FastAPI (cloned, self-hosted)
├── docs.md            Full technical analysis + decisions
└── CLAUDE.md          Project context for Claude Code
```

## Worker Secrets

```
CF_ACCOUNT_ID    = 80a55132ae169d5b282ccf505bc66bf7
CF_AI_GATEWAY_ID = default
CF_API_TOKEN     = (CF AI Gateway auth — wrangler secret)
KOKORO_URL       = https://kokoro.milliytechnology.org
```

Update a secret:
```bash
cd worker && echo "value" | npx wrangler secret put SECRET_NAME --env=""
```

## Deploying Frontend

```bash
cd frontend && npm run build
# Cloudflare Pages auto-deploys on git push to main
```

Or trigger manually from the Cloudflare dashboard.
