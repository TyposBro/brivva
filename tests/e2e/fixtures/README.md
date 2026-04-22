# Fake cam + mic fixtures for E2E

Chromium reads these via `--use-file-for-fake-video-capture` +
`--use-file-for-fake-audio-capture`, piping the bytes through
`getUserMedia()` as if they came from a real device. Frontend +
backend see a normal MediaStream and can't tell the difference.

Not checked in (see `.gitignore`) — both files are large and derived
from whatever source video you picked. Regenerate with:

```bash
ffmpeg -y -i SOURCE.mp4 -pix_fmt yuv420p fake-cam.y4m
ffmpeg -y -i SOURCE.mp4 -vn -acodec pcm_s16le -ar 44100 -ac 1 fake-mic.wav
```

Chromium constraints:
- Video must be Y4M (raw YUV). MP4/H.264 is rejected.
- Audio must be WAV PCM. MP3/AAC is rejected.
- 44.1 kHz mono matches the server-rs Soniox forwarder; stereo works
  too but adds pointless bytes.
- Files loop automatically when they end — a 13 s clip is fine for
  long-script simulation.

Launch helper:

```bash
./scripts/dev-fake-media.sh            # opens :5173 with fake devices
```

Playwright: see `playwright.config.ts` — the launch options add the
same flags so `test:e2e` can also run against fake media. Gate behind
an env var because the file paths are host-specific.
