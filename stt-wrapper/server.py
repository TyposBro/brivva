"""STT Wrapper: clean WebSocket proxy between Rust server and WhisperLiveKit.

Accepts raw audio from the Rust server, forwards it to WhisperLiveKit,
and emits clean interim/final events based on sentence boundary detection.

Protocol (wrapper → client):
  {"type": "interim", "text": "Hello world"}
  {"type": "final",   "text": "Hello world."}
  {"type": "error",   "message": "STT disconnected"}
"""

import asyncio
import json
import os
import re
import logging

import websockets

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("stt-wrapper")

STT_HOST = os.environ.get("STT_HOST", "localhost")
STT_URL = f"ws://{STT_HOST}:8765/asr"
WRAPPER_PORT = int(os.environ.get("WRAPPER_PORT", "8766"))

# Regex to strip parenthesized markers: (silence), (chuckles), (sil, etc.
# Handles both complete (silence) and incomplete (sil at end of text
PAREN_RE = re.compile(r"\([^)]*\)?")


def clean_stt_text(text: str) -> str:
    """Strip parenthesized markers and collapse whitespace."""
    cleaned = PAREN_RE.sub("", text)
    return " ".join(cleaned.split())


class ParserState:
    """Tracks STT state and emits clean interim/final events."""

    def __init__(self):
        self.emitted_up_to = 0
        self.last_clean_text = ""

    def process(self, resp: dict) -> list[dict]:
        """Process one WhisperLiveKit message, return 0+ clean events."""
        # Skip config/control messages
        msg_type = resp.get("type", "")
        if msg_type in ("config", "ready_to_stop"):
            return []

        # Collect all text: lines[] + buffer_transcription
        parts = [line.get("text", "") for line in resp.get("lines", [])]
        buf = resp.get("buffer_transcription", "")
        if buf:
            parts.append(buf)
        full_text = " ".join(parts)

        # Clean markers and deduplicate
        clean = clean_stt_text(full_text)
        if clean == self.last_clean_text or not clean:
            return []
        self.last_clean_text = clean

        events = []
        unemitted = clean[self.emitted_up_to :]

        # Find last sentence-ending punctuation
        last_end = -1
        for i, ch in enumerate(unemitted):
            if ch in ".?!":
                last_end = i

        if last_end >= 0:
            final_text = unemitted[: last_end + 1].strip()
            if final_text:
                events.append({"type": "final", "text": final_text})
                self.emitted_up_to += last_end + 1

            remaining = unemitted[last_end + 1 :].strip()
            if remaining:
                events.append({"type": "interim", "text": remaining})
        elif unemitted.strip():
            events.append({"type": "interim", "text": unemitted.strip()})

        return events


async def connect_to_stt() -> websockets.WebSocketClientProtocol:
    """Connect to WhisperLiveKit with retries."""
    for attempt in range(1, 11):
        try:
            ws = await websockets.connect(STT_URL, max_size=10 * 1024 * 1024)
            log.info("Connected to STT (attempt %d)", attempt)
            return ws
        except Exception as e:
            log.warning("STT connect attempt %d/10 failed: %s", attempt, e)
            await asyncio.sleep(3)
    raise ConnectionError("Failed to connect to STT after 10 attempts")


async def forward_audio(client_ws, stt_ws):
    """Forward binary audio frames from client to WhisperLiveKit."""
    try:
        async for msg in client_ws:
            if isinstance(msg, bytes):
                await stt_ws.send(msg)
    except websockets.ConnectionClosed:
        pass


async def parse_responses(stt_ws, client_ws):
    """Parse WhisperLiveKit responses and emit clean events."""
    state = ParserState()
    try:
        async for msg in stt_ws:
            if isinstance(msg, bytes):
                continue
            try:
                resp = json.loads(msg)
            except json.JSONDecodeError:
                continue

            events = state.process(resp)
            for event in events:
                await client_ws.send(json.dumps(event))
    except websockets.ConnectionClosed:
        pass


async def handle_client(client_ws):
    """Handle one client connection: proxy to WhisperLiveKit with clean output."""
    log.info("Client connected")
    try:
        stt_ws = await connect_to_stt()
    except ConnectionError as e:
        log.error("%s", e)
        await client_ws.send(json.dumps({"type": "error", "message": str(e)}))
        await client_ws.close()
        return

    # Let WhisperLiveKit settle (sends config message)
    await asyncio.sleep(0.2)

    audio_task = asyncio.create_task(forward_audio(client_ws, stt_ws))
    parse_task = asyncio.create_task(parse_responses(stt_ws, client_ws))

    try:
        done, pending = await asyncio.wait(
            [audio_task, parse_task], return_when=asyncio.FIRST_COMPLETED
        )
        for task in pending:
            task.cancel()
    finally:
        await stt_ws.close()
        log.info("Client disconnected")


async def main():
    log.info("STT Wrapper listening on ws://0.0.0.0:%d/asr", WRAPPER_PORT)
    async with websockets.serve(handle_client, "0.0.0.0", WRAPPER_PORT):
        await asyncio.Future()  # run forever


if __name__ == "__main__":
    asyncio.run(main())
