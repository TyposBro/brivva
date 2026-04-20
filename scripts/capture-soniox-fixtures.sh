#!/usr/bin/env bash
# §0.5.1 — capture real Soniox STT WebSocket frames into
# server-rs/tests/fixtures/soniox/. Fills the "Pending" list in that
# vendor README (happy transcripts + rate-limit + model-unavailable).
#
# Prereqs:
#   - Valid SONIOX_API_KEY.
#   - ffmpeg installed (for piping a known audio file into the stream).
#   - websocat (`brew install websocat`) OR `wscat` for the WS read side.
#
# Strategy:
#   Rather than scraping stderr (brittle, depends on RUST_LOG layout),
#   we record raw frames end-to-end by proxying through a local
#   transcribing script. This is the same approach the existing
#   CAPTURED fixtures used — see the per-file lineage in the README.
#
# Usage:
#   export SONIOX_API_KEY=sx-XXXX
#   bash scripts/capture-soniox-fixtures.sh path/to/sample.wav
#
# Output:
#   server-rs/tests/fixtures/soniox/happy_transcript_en_ja.json
#   server-rs/tests/fixtures/soniox/happy_source_en.json
#   server-rs/tests/fixtures/soniox/end_token.json
#
# For error variants (429 rate-limited, 503 model-unavailable), induce
# the condition once (spam reconnects for 429; invalid model_id for 503)
# and capture the first rejection frame.

set -euo pipefail

: "${SONIOX_API_KEY:?export SONIOX_API_KEY=... first}"
AUDIO_FILE="${1:-}"
if [[ -z "${AUDIO_FILE}" || ! -f "${AUDIO_FILE}" ]]; then
    echo "usage: $0 path/to/sample.wav" >&2
    exit 1
fi

if ! command -v websocat >/dev/null 2>&1; then
    echo "error: websocat required (brew install websocat)" >&2
    exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq required" >&2
    exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${REPO_ROOT}/server-rs/tests/fixtures/soniox"
mkdir -p "$DEST"

# Soniox STT stream endpoint. The config frame must be sent first;
# subsequent binary frames carry audio.
WS_URL="wss://stt-rt.soniox.com/transcribe-websocket"

echo "── capturing happy_source_en.json ──"
# Source-mode config: STT only, English. Translation status in frames
# will be `original`. We send ~5s of audio and take the first 20
# responses (more than enough to observe interim + final + <end>).
CONFIG_SOURCE="$(jq -n --arg k "$SONIOX_API_KEY" '{
    api_key: $k,
    audio_format: "pcm_s16le",
    sample_rate: 16000,
    num_channels: 1,
    model: "stt-rt-preview",
    language_hints: ["en"],
    enable_endpoint_detection: true
}')"

(
    echo "$CONFIG_SOURCE"
    ffmpeg -nostdin -loglevel error -i "$AUDIO_FILE" \
        -ar 16000 -ac 1 -f s16le - 2>/dev/null || true
) | websocat --binary --text "$WS_URL" |
    head -n 40 |
    jq -sc . > "${DEST}/happy_source_en_raw.json"
echo "  captured $(jq 'length' "${DEST}/happy_source_en_raw.json") frames to happy_source_en_raw.json"
echo "  manually pick a representative single frame → happy_source_en.json"

echo "── capturing happy_transcript_en_ja.json ──"
CONFIG_TRANSLATE="$(jq -n --arg k "$SONIOX_API_KEY" '{
    api_key: $k,
    audio_format: "pcm_s16le",
    sample_rate: 16000,
    num_channels: 1,
    model: "stt-rt-preview",
    language_hints: ["en"],
    enable_endpoint_detection: true,
    translation: { type: "two_way", language_a: "en", language_b: "ja" }
}')"

(
    echo "$CONFIG_TRANSLATE"
    ffmpeg -nostdin -loglevel error -i "$AUDIO_FILE" \
        -ar 16000 -ac 1 -f s16le - 2>/dev/null || true
) | websocat --binary --text "$WS_URL" |
    head -n 40 |
    jq -sc . > "${DEST}/happy_transcript_en_ja_raw.json"
echo "  captured $(jq 'length' "${DEST}/happy_transcript_en_ja_raw.json") frames"
echo "  manually pick a frame with translation_status=translation → happy_transcript_en_ja.json"
echo "  and a frame containing <end> token → end_token.json"
echo
echo "── done. Manual step: ──"
echo "  1. Inspect the _raw.json arrays, pick representative frames, save"
echo "     as happy_source_en.json / happy_transcript_en_ja.json /"
echo "     end_token.json. Do NOT edit field shapes."
echo "  2. Delete the _raw.json files after extraction (not committed)."
echo "  3. Update server-rs/tests/fixtures/soniox/README.md — move the"
echo "     three new rows out of Pending into the CAPTURED status table."
echo "  4. Add a fixture-roundtrip unit test asserting each new file"
echo "     parses into SonioxResponse without panicking."
