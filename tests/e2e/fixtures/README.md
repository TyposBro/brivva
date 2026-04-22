# Fake cam + mic fixtures for E2E

Chromium reads these via `--use-file-for-fake-video-capture` +
`--use-file-for-fake-audio-capture`, piping the bytes through
`getUserMedia()` as if they came from a real device. Frontend +
backend see a normal MediaStream and can't tell the difference.

Not checked in (see `.gitignore`) — both files are large and derived
from whatever source video you picked. Regenerate with:

```bash
# Keep both files the SAME duration and from the SAME source clip,
# otherwise the shorter one loops while the longer keeps playing
# and A/V drifts mid-test (the original 60s video vs 1259s audio
# loop caused exactly this). 180s covers the default 90s --duration
# with a 2× safety margin; bump -t if you run longer recordings.
ffmpeg -y -t 180 -i SOURCE.mp4 -vf "scale=480:-2,fps=15" -pix_fmt yuv420p fake-cam.y4m
ffmpeg -y -t 180 -i SOURCE.mp4 -vn -acodec pcm_s16le -ar 44100 -ac 1 fake-mic.wav
```

Chromium constraints:
- Video must be Y4M (raw YUV). MP4/H.264 is rejected.
- Audio must be WAV PCM. MP3/AAC is rejected.
- 44.1 kHz mono matches the server-rs Soniox forwarder; stereo works
  too but adds pointless bytes.
- Files loop automatically when they end — if you MUST use a short
  clip, ensure cam + mic are the exact same duration so loops stay
  phase-aligned.

Launch helper:

```bash
./scripts/dev-fake-media.sh            # opens :5173 with fake devices
```

Playwright: see `playwright.config.ts` — the launch options add the
same flags so `test:e2e` can also run against fake media. Gate behind
an env var because the file paths are host-specific.
