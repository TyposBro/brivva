# Launch Browser + Media Runbook

Last updated: 2026-05-05

## Supported host browser

For launch testing and paid live shows, hosts must use **desktop Chrome, Brave, or another Chromium-based browser**.

Do **not** host from Firefox or Safari for launch tests.

Why:

- BRIVVA ingests browser WebRTC video as H.264 and pipes it to FFmpeg for RTMP outputs.
- RTMP platforms, especially YouTube/Grip, require stable H.264 + AAC FLV output.
- Firefox on macOS connected successfully but produced H.264 access units that FFmpeg logged as corrupt (`decode_slice_header error`, `Invalid data found when processing input`). FFmpeg connected to YouTube and marked outputs live, but YouTube did not show usable video.
- Safari behavior is not launch-validated and should be treated as unsupported until proven.

Frontend now blocks Go Live when the host browser is not Chromium-based and shows: use Chrome/Brave.

## Known May 5 production incidents and fixes

### 1. NVENC unavailable inside ECS GPU task

Symptom:

- UI: `Media connection problem`
- provider health: `ffmpeg exit_code=-1`, `max ffmpeg restart attempts reached`
- CloudWatch:
  - `Cannot load libnvidia-encode.so.1`
  - `minimum required Nvidia driver for nvenc is 550.54.14 or newer`

Cause:

- ECS assigned a GPU, but NVIDIA container runtime did not mount NVENC libraries because `NVIDIA_DRIVER_CAPABILITIES` lacked `video`.

Fix:

- Set `NVIDIA_DRIVER_CAPABILITIES=video,compute,utility` in Terraform and deploy script.
- Verified startup self-check: `RS007_RUNTIME_SELF_CHECK_OK`, `encoders_has_h264_nvenc: true`.

### 2. Live FFmpeg killed by browser FPS/profile correction

Symptom:

- Outputs started then quickly entered reconnect/fail.
- CloudWatch:
  - `ffmpeg restarting to apply host video profile`
  - then `ffmpeg rtmp process crashed`
  - then H.264 decode errors / missing parameter sets.

Cause:

- Server observed WebRTC FPS/dimensions after Go Live and killed active FFmpeg processes to apply profile changes.
- The monitor treated those intentional kills as crashes and burned restart budget.
- Restarts happened mid-GOP, causing missing SPS/PPS or corrupted H.264 decode.

Fix:

- Do not kill active FFmpeg children for observed video profile updates.
- Current streams keep launch profile; future restarts use corrected profile.

### 3. Soniox `<fin>` leaked into captions/transcripts

Symptom:

- UI transcript showed repeated endpoint markers: `<fin><fin><fin>...`.

Cause:

- Soniox emitted `<fin>` as an endpoint marker. Code only filtered `<end>`.

Fix:

- Treat both `<end>` and `<fin>` as endpoint tokens and strip them from source/translated text.

### 4. Slow YouTube output at ~0.5x speed

Symptom:

- UI provider health: `speed=0.514 consecutive_ticks=51`
- Browser showed capture `30fps`, but WebRTC outbound was ~`15fps`.

Cause:

- FFmpeg raw H.264 input used `-r 30`, trusting the browser-reported camera FPS.
- Browser actually sent ~15fps. FFmpeg timestamped incoming frames as 30fps, so output advanced at ~0.5x realtime.

Fix:

- Use `-use_wallclock_as_timestamps 1` for raw H.264 pipe input.
- Output FPS filter still owns RTMP cadence.

### 5. Firefox/macOS YouTube not showing video

Symptom:

- UI said outputs were live, but YouTube did not show the stream.
- CloudWatch showed all YouTube RTMP outputs started and `output.live`.
- No YouTube auth/reject errors (`401/403/404`) were present.
- Host user agent: Firefox on macOS.
- FFmpeg stderr:
  - `decode_slice_header error`
  - `Error submitting packet to decoder: Invalid data found when processing input`

Cause:

- Firefox H.264 WebRTC output was not compatible enough with current H.264→FFmpeg pipe path for launch reliability.

Fix / policy:

- Chrome/Brave-only host policy for launch.
- Frontend blocks Firefox/Safari before Go Live.

## Operator checklist for “YouTube is not streaming”

Check CloudWatch `/ecs/brivva`:

1. Find latest `ffmpeg rtmp stream group started` and `v2 output health event` lines.
2. If outputs are `output.live` and there are no `401/403/404`/RTMP auth errors, the stream key is probably not the issue.
3. Check host browser in `client media quality contract warning` user agent.
   - Firefox/Safari: stop test and rerun from desktop Chrome/Brave.
4. Check FFmpeg stderr:
   - `Cannot load libnvidia-encode.so.1` → GPU/NVENC capability regression.
   - `speed=0.5x` → input clock/FPS mismatch regression.
   - `decode_slice_header error` / `Invalid data found when processing input` → corrupt/unsupported H.264 ingest, often browser-specific.
5. Check browser stats:
   - `outbound below 30fps floor` means host/browser is sending low FPS.
   - `outbound below 1080p floor` means source quality is below launch target, but not necessarily a stream-killer.

## Launch host rules

- Use desktop Chrome or Brave.
- Keep BRIVVA tab visible.
- Do not switch networks during a live show.
- Prefer wired or stable Wi-Fi.
- Do not use mobile browser hosting for production.
- For Grip, paste only a fresh current-session one-shot key and confirm the checkbox.
