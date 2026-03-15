# Brivva Real-Time Translation

Real-time multilingual voice translation for live commerce. Host speaks → guests hear translated audio in their language.

**Live:** https://brivva.pages.dev

## What It Does

- Host creates a room, speaks into mic (English or Korean)
- Guests join with a room code, pick their language (EN / JA / ZH)
- Each guest sees live subtitles + hears TTS audio in their language
- Translation runs once per language group — 50 JA guests = 1 translation call

## Architecture (v3)

```
Host Browser → PCM 16kHz → WebSocket → server-rs (Rust/axum :3000)
  → STT Wrapper (:8766) → CF Nova-3 (streaming transcription)
  → NLLB Translation (:8000) — per active language, parallel
  → ElevenLabs TTS (API) — streaming MP3
  → WebSocket → Guest Browser (Blob URL playback)
```

| Service | Port | Tech | Purpose |
|---------|------|------|---------|
| server-rs | 3000 | Rust/axum | WebSocket rooms, pipeline orchestration |
| stt-wrapper | 8766 | CF Nova-3 (API) | STT proxy (clean events) |
| NLLB | 8000 | nllb-200-distilled-600M | Translation (82–164ms on GPU) |
| ElevenLabs | API | eleven_flash_v2_5 | TTS (548–1,440ms, 32 languages) |

Only NLLB runs locally with GPU. STT and TTS are API-based — no local GPU needed for those.

---

## Quick Start (Docker — works on macOS and Linux)

```bash
git clone git@github.com:TyposBro/brivva.git
cd brivva

# Create .env with API keys
cat > .env << 'EOF'
CF_ACCOUNT_ID=80a55132ae169d5b282ccf505bc66bf7
CF_API_TOKEN=your-cf-api-token
ELEVENLABS_API_KEY=your-elevenlabs-api-key
EOF

# Build and start all services
docker compose up --build
```

That's it. Three containers: server-rs, stt-wrapper, nllb.

**Without GPU (Mac/any):** NLLB runs on CPU. Translation works, just slower.
**With NVIDIA GPU (Linux):** Use the GPU override for fast translation (82–164ms):
```bash
docker compose -f docker-compose.yml -f docker-compose.gpu.yml up --build
```

**Verify:**
```bash
curl http://localhost:3000       # → "Brivva Translation Server"
curl http://localhost:8000/health # → {"status":"healthy","model":"...","device":"cuda/cpu"}
```

### GPU on Linux

If you have an NVIDIA GPU and want fast translation:

```bash
# Install nvidia-container-toolkit
# Then verify:
docker run --rm --gpus all nvidia/cuda:12.4.1-runtime-ubuntu22.04 nvidia-smi
```

---

## Cloudflared Tunnel

The frontend at `brivva.pages.dev` connects to `brivva-server.milliytechnology.org`, which is a cloudflared tunnel pointing to `localhost:3000`. Run the backend anywhere and the frontend works.

### Tunnel credentials

```
~/.cloudflared/
├── brivva-kokoro.yml                              ← tunnel config
└── b6e5239e-b304-4087-8879-97fb649e6ba1.json      ← tunnel credentials
```

**brivva-kokoro.yml:**
```yaml
tunnel: b6e5239e-b304-4087-8879-97fb649e6ba1
credentials-file: /home/typosbro/.cloudflared/b6e5239e-b304-4087-8879-97fb649e6ba1.json
ingress:
  - hostname: brivva-server.milliytechnology.org
    service: http://localhost:3000
  - service: http_status:404
```

> **Note:** Update `credentials-file` path to match your home directory.

### Copy credentials to another machine

```bash
scp ~/.cloudflared/brivva-kokoro.yml <user>@<host>:~/.cloudflared/
scp ~/.cloudflared/b6e5239e-*.json <user>@<host>:~/.cloudflared/
```

### Start the tunnel

```bash
cloudflared tunnel --config ~/.cloudflared/brivva-kokoro.yml run
```

---

## NixOS Setup

### System config

```nix
# Docker with NVIDIA GPU passthrough
virtualisation.docker.enable = true;
hardware.nvidia-container-toolkit.enable = true;

# Add your user to the docker group
users.users.<your-user>.extraGroups = [ "docker" ];
```

### Packages needed

```nix
home.packages = with pkgs; [
  awscli2
  cloudflared
];
```

### Full steps

```bash
# 1. Rebuild NixOS
sudo nixos-rebuild switch --flake .#nixos

# 2. Log out and back in (docker group needs new session)

# 3. Verify GPU in Docker
docker run --rm --gpus all nvidia/cuda:12.4.1-runtime-ubuntu22.04 nvidia-smi

# 4. Copy tunnel credentials from Mac
mkdir -p ~/.cloudflared
scp mac:~/.cloudflared/brivva-kokoro.yml ~/.cloudflared/
scp mac:~/.cloudflared/b6e5239e-*.json ~/.cloudflared/
# Edit brivva-kokoro.yml: update credentials-file path to /home/<user>/...

# 5. Clone and start
git clone git@github.com:TyposBro/brivva.git
cd brivva
docker compose up --build -d

# 6. Start tunnel
cloudflared tunnel --config ~/.cloudflared/brivva-kokoro.yml run

# 7. Open https://brivva.pages.dev — it now hits your NixOS machine
```

---

## Frontend

Deployed on Cloudflare Pages. Points to `brivva-server.milliytechnology.org` (set in `frontend/.env`).

```bash
cd frontend
npm install
npm run dev        # local dev at http://localhost:5173
npm run deploy     # deploy to Cloudflare Pages
```

---

## AWS Deployment (ECS)

### Phase 2 — Single EC2

```
EC2 g5.xlarge (A10G 24GB VRAM, ~$1/hr)
├── server-rs          :3000  (CPU, ~10MB RAM)
├── stt-wrapper        :8766  (CPU, CF Nova-3 API)
├── NLLB               :8000  (GPU, ~2GB VRAM)
├── ElevenLabs         API    (external, no local resources)
└── Total VRAM: ~2GB / 24GB available
```

```bash
# On EC2 with Docker (GPU optional but recommended for fast translation)
git clone git@github.com:TyposBro/brivva.git
cd brivva
docker compose up --build -d
```

### Phase 3 — ECS (production)

```
ALB → ECS Service: server-rs (CPU task, auto-scale)
        ├→ ECS Service: stt-wrapper (CPU task, CF Nova-3 API)
        ├→ ECS Service: NLLB (GPU task, g5.xlarge or CPU with quantization)
        └→ ElevenLabs TTS (external API, no ECS task needed)
```

Push images to ECR, create task definitions with GPU resource reservations.

---

## Project Structure

```
brivva/
├── docker-compose.yml             3 services (server, stt-wrapper, nllb)
├── .env                           API keys (CF_ACCOUNT_ID, CF_API_TOKEN, ELEVENLABS_API_KEY)
├── server-rs/                     Rust axum WebSocket server
│   ├── Dockerfile
│   └── src/
│       ├── main.rs                Entry, router, state
│       ├── types.rs               Lang, Room, Guest, ServerMsg
│       ├── pipeline.rs            STT → Translate → TTS pipeline
│       └── room/handler.rs        WebSocket host/guest handlers
├── stt-wrapper/                   STT proxy (CF Nova-3)
│   ├── Dockerfile
│   └── server.py                  asyncio WebSocket proxy
├── nllb/                          NLLB translation server
│   ├── Dockerfile
│   └── server.py                  FastAPI, POST /translate
├── frontend/                      React + TypeScript + Vite
│   └── src/
│       ├── pages/                 HostPage, GuestPage, HomePage
│       ├── components/            LatencyDashboard, PipelineAnalysis
│       ├── hooks/                 useHostRoom, useGuestRoom, useTimings
│       ├── lib/                   RoomSocket, TtsPlayer
│       └── state/                 Host/guest reducers + message handlers
├── worker/                        CF Worker (v1/v2 legacy)
├── CLAUDE.md                      Project context
├── BENCHMARK.md                   Benchmark dashboard spec
└── docs.md                        Full technical documentation
```

---

## Troubleshooting

| Problem | Fix |
|---------|-----|
| `nvidia-smi` works but Docker can't see GPU | Install `nvidia-container-toolkit`, restart Docker |
| stt-wrapper not transcribing | Check CF_ACCOUNT_ID and CF_API_TOKEN env vars in .env |
| Guest doesn't hear audio | Check browser console for autoplay errors; click language picker to unlock AudioContext |
| Duplicate translations firing | stt-wrapper handles dedup; check stt-wrapper logs |
| NLLB first request very slow | Model loading on first inference (~5-15s). Subsequent requests are fast. |
| No GPU on Mac | That's fine — NLLB runs on CPU (slower translation but works) |
