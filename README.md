# Brivva Real-Time Translation

Real-time multilingual voice translation for live commerce. Host speaks → guests hear translated audio in their language.

**Live:** https://brivva.pages.dev

## What It Does

- Host creates a room, speaks into mic (English or Korean)
- Guests join with a room code, pick their language (EN / JA / ZH)
- Each guest sees live subtitles + hears TTS audio in their language
- Translation runs once per language group — 50 JA guests = 1 translation call

## Architecture (v3 — self-hosted)

```
Host Browser → PCM 16kHz → WebSocket → server-rs (Rust/axum :3000)
  → WhisperLiveKit STT (:8765) — streaming transcription
  → NLLB Translation (:8000) — per active language, parallel
  → Kokoro TTS (:8880) — streaming MP3 chunks
  → WebSocket → Guest Browser (Blob URL playback)
```

All ML services co-located on one machine. No cloud API dependencies.

| Service | Port | Model | Purpose |
|---------|------|-------|---------|
| server-rs | 3000 | — | WebSocket rooms, pipeline orchestration |
| WhisperLiveKit | 8765 | base.en (mlx/faster-whisper) | Streaming STT |
| NLLB | 8000 | nllb-200-distilled-600M | Translation (200 languages) |
| Kokoro | 8880 | kokoro-v1_0 82M | TTS (EN/JA/ZH voices) |

---

## Prerequisites

- **Mac (local dev):** Python 3.10, Rust, cloudflared, uv
- **Linux (Docker):** NVIDIA GPU, Docker, nvidia-container-toolkit, cloudflared
- **NixOS:** see [NixOS Setup](#nixos-setup) below

---

## Option 1: Local Dev (Mac M1/M2 — MPS acceleration)

```bash
git clone git@github.com:TyposBro/brivva.git
cd brivva

# Clone Kokoro TTS (separate repo, not included in brivva)
git clone https://github.com/remsky/Kokoro-FastAPI kokoro

# One-time: set up Python venvs for each ML service
# NLLB
cd nllb
python3.10 -m venv .venv
.venv/bin/pip install fastapi uvicorn transformers torch sentencepiece protobuf
cd ..

# WhisperLiveKit
cd whisper-stt
python3.10 -m venv .venv
.venv/bin/pip install whisperlivekit faster-whisper torch torchaudio
cd ..

# Kokoro (uses uv)
cd kokoro
uv sync
.venv/bin/python -m unidic download   # 526MB, required for Japanese TTS
cd ..

# Build Rust server
cargo build --release --manifest-path server-rs/Cargo.toml

# Start everything (cloudflared + NLLB + WhisperLiveKit + server-rs + Kokoro)
bash init.sh
```

This starts all 5 services. `Ctrl+C` stops everything.

**Verify:**
```bash
curl http://localhost:3000       # → "Brivva Translation Server"
curl http://localhost:8000/health # → {"status":"healthy","model":"...","device":"mps"}
curl http://localhost:8880/health # → {"status":"healthy"}
```

---

## Option 2: Docker (Linux with NVIDIA GPU)

```bash
git clone git@github.com:TyposBro/brivva.git
cd brivva

# Clone Kokoro TTS (separate repo, referenced by docker-compose)
git clone https://github.com/remsky/Kokoro-FastAPI kokoro

# Build and start all services with GPU
docker compose up --build
```

First build is slow — downloads CUDA base images, Python deps, and pre-downloads ML models into the images. Subsequent builds use cached layers.

**Verify GPU access:**
```bash
docker run --rm --gpus all nvidia/cuda:12.4.1-runtime-ubuntu22.04 nvidia-smi
```

**Services in Docker:**

| Container | Base Image | GPU |
|-----------|-----------|-----|
| server | rust:1.85 → debian:bookworm-slim | No |
| nllb | nvidia/cuda:12.4.1-runtime | Yes |
| whisper-stt | nvidia/cuda:12.4.1-runtime | Yes |
| kokoro | Kokoro GPU Dockerfile | Yes |

---

## Cloudflared Tunnel

The frontend at `brivva.pages.dev` connects to `brivva-server.milliytechnology.org`, which is a cloudflared tunnel pointing to `localhost:3000`. This means you can run the backend anywhere and the frontend works without changes.

### Tunnel credentials

You need two files in `~/.cloudflared/`:

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
  - hostname: kokoro.milliytechnology.org
    service: http://localhost:8880
  - hostname: brivva-server.milliytechnology.org
    service: http://localhost:3000
  - service: http_status:404
```

> **Note:** Update `credentials-file` path to match your home directory.

### Copy credentials from Mac to another machine

```bash
# From Mac
scp ~/.cloudflared/brivva-kokoro.yml <user>@<host>:~/.cloudflared/
scp ~/.cloudflared/b6e5239e-*.json <user>@<host>:~/.cloudflared/
```

### Start the tunnel

```bash
cloudflared tunnel --config ~/.cloudflared/brivva-kokoro.yml run
```

Once running, `brivva-server.milliytechnology.org` points to your machine. The frontend at `brivva.pages.dev` works immediately.

---

## NixOS Setup

### System config

Add to your NixOS configuration:

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
git clone https://github.com/remsky/Kokoro-FastAPI kokoro
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
├── WhisperLiveKit     :8765  (GPU, ~2GB VRAM)
├── NLLB               :8000  (GPU, ~2GB VRAM)
├── Kokoro             :8880  (GPU, ~1GB VRAM)
└── Total VRAM: ~5GB / 24GB available
```

```bash
# On EC2 with NVIDIA GPU + Docker + nvidia-container-toolkit
git clone git@github.com:TyposBro/brivva.git
cd brivva
git clone https://github.com/remsky/Kokoro-FastAPI kokoro
docker compose up --build -d
```

### Phase 3 — ECS (production)

```
ALB → ECS Service: server-rs (CPU task, auto-scale)
        ├→ ECS Service: WhisperLiveKit (GPU task)
        ├→ ECS Service: NLLB (GPU task)
        └→ ECS Service: Kokoro (GPU task)
```

Push images to ECR, create task definitions with GPU resource reservations.

---

## Project Structure

```
brivva/
├── docker-compose.yml             All 4 services (NVIDIA GPU)
├── init.sh                        Local dev: starts everything (Mac MPS)
├── server-rs/                     Rust axum WebSocket server
│   ├── Dockerfile
│   └── src/
│       ├── main.rs                Entry, router, state
│       ├── types.rs               Lang, Room, Guest, ServerMsg
│       ├── pipeline.rs            STT → Translate → TTS pipeline
│       └── room/handler.rs        WebSocket host/guest handlers
├── nllb/                          NLLB translation server
│   ├── Dockerfile
│   └── server.py                  FastAPI, POST /translate
├── whisper-stt/                   WhisperLiveKit STT
│   └── Dockerfile
├── kokoro/                        Kokoro TTS (third-party)
│   └── docker/gpu/Dockerfile
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
| WhisperLiveKit not transcribing | Check `--no-vac` flag is set (VAC blocks audio) |
| STT lag >1s | Use `base.en` model, not `large-v3-turbo` (too slow for real-time on M1) |
| Guest doesn't hear audio | Check browser console for autoplay errors; click language picker to unlock AudioContext |
| Duplicate translations firing | The `(silence)` bug — ensure pipeline.rs strips markers and emits once per line index |
| NLLB first request very slow | Model loading on first inference (~5-15s). Subsequent requests are fast. |
| `docker compose up` fails on Mac | GPU Docker doesn't work on Mac (no MPS in containers). Use `bash init.sh` instead. |
