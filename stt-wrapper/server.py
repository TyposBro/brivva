"""STT Wrapper: clean WebSocket proxy between Rust server and WhisperLiveKit.

Accepts raw audio from the Rust server, forwards it to WhisperLiveKit,
and emits clean interim/final events based on sentence boundary detection.

When silence is detected (no new transcription for SILENCE_TIMEOUT seconds),
the WhisperLiveKit connection is reset to clear its internal state. This
prevents the "frozen transcript" bug where Whisper fills its context with
[BLANK_AUDIO] markers and stops recognizing new speech.

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
import time

import websockets

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("stt-wrapper")

STT_HOST = os.environ.get("STT_HOST", "localhost")
STT_URL = f"ws://{STT_HOST}:8765/asr"
WRAPPER_PORT = int(os.environ.get("WRAPPER_PORT", "8766"))
SILENCE_TIMEOUT = float(os.environ.get("SILENCE_TIMEOUT", "3.0"))

# Regex to strip markers from Whisper output:
# (silence), (chuckles), (sil — parenthesized, possibly incomplete
# [BLANK_AUDIO], [BLANK_ — square-bracketed, possibly incomplete
PAREN_RE = re.compile(r"\([^)]*\)?")
BRACKET_RE = re.compile(r"\[[^\]]*\]?")


def clean_stt_text(text: str) -> str:
    """Strip parenthesized/bracketed markers and collapse whitespace."""
    cleaned = PAREN_RE.sub("", text)
    cleaned = BRACKET_RE.sub("", cleaned)
    return " ".join(cleaned.split())


class ParserState:
    """Tracks STT state and emits clean interim/final events."""

    def __init__(self):
        self.emitted_up_to = 0
        self.last_clean_text = ""
        self.last_new_text_time = time.monotonic()

    def reset(self):
        """Reset state for a fresh STT connection."""
        self.emitted_up_to = 0
        self.last_clean_text = ""
        self.last_new_text_time = time.monotonic()

    @property
    def silence_duration(self) -> float:
        return time.monotonic() - self.last_new_text_time

    def process(self, resp: dict) -> list[dict]:
        """Process one WhisperLiveKit message, return 0+ clean events."""
        msg_type = resp.get("type", "")
        if msg_type in ("config", "ready_to_stop"):
            return []

        parts = [line.get("text", "") for line in resp.get("lines", [])]
        buf = resp.get("buffer_transcription", "")
        if buf:
            parts.append(buf)
        full_text = " ".join(parts)

        clean = clean_stt_text(full_text)
        if clean == self.last_clean_text or not clean:
            return []

        self.last_clean_text = clean
        self.last_new_text_time = time.monotonic()

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
    for attempt in range(1, 31):
        try:
            ws = await websockets.connect(STT_URL, max_size=10 * 1024 * 1024)
            log.info("Connected to STT (attempt %d)", attempt)
            return ws
        except Exception as e:
            log.warning("STT connect attempt %d/30 failed: %s", attempt, e)
            await asyncio.sleep(5)
    raise ConnectionError("Failed to connect to STT after 30 attempts")


async def handle_client(client_ws):
    """Handle one client connection with automatic STT reset on silence."""
    log.info("Client connected")

    state = ParserState()
    stt_ws = None
    audio_queue: asyncio.Queue[bytes] = asyncio.Queue()
    running = True

    async def open_stt():
        """Open a fresh STT connection."""
        nonlocal stt_ws
        try:
            stt_ws = await connect_to_stt()
            await asyncio.sleep(0.2)  # let WhisperLiveKit send config
            return True
        except ConnectionError as e:
            log.error("%s", e)
            await client_ws.send(json.dumps({"type": "error", "message": str(e)}))
            return False

    async def close_stt():
        """Close current STT connection if open."""
        nonlocal stt_ws
        if stt_ws:
            try:
                await stt_ws.close()
            except Exception:
                pass
            stt_ws = None

    async def read_client():
        """Read audio from the Rust server into the queue."""
        nonlocal running
        try:
            async for msg in client_ws:
                if isinstance(msg, bytes):
                    audio_queue.put_nowait(msg)
        except websockets.ConnectionClosed:
            pass
        finally:
            running = False

    async def forward_audio():
        """Forward queued audio to the current STT websocket."""
        while running:
            try:
                data = await asyncio.wait_for(audio_queue.get(), timeout=0.1)
            except asyncio.TimeoutError:
                continue
            if stt_ws:
                try:
                    await stt_ws.send(data)
                except websockets.ConnectionClosed:
                    pass

    async def read_stt():
        """Read STT responses and emit events. Returns on disconnect."""
        while running and stt_ws:
            try:
                msg = await asyncio.wait_for(stt_ws.recv(), timeout=0.5)
            except asyncio.TimeoutError:
                continue
            except websockets.ConnectionClosed:
                return

            if isinstance(msg, bytes):
                continue
            try:
                resp = json.loads(msg)
            except json.JSONDecodeError:
                continue

            events = state.process(resp)
            for event in events:
                log.info("[%s] %s", event["type"].upper(), event.get("text", ""))
                try:
                    await client_ws.send(json.dumps(event))
                except websockets.ConnectionClosed:
                    return

    async def silence_watchdog():
        """Monitor for prolonged silence and reset STT connection."""
        while running:
            await asyncio.sleep(1.0)
            if state.silence_duration >= SILENCE_TIMEOUT and stt_ws:
                # Only reset if we've actually received some text before
                if state.last_clean_text:
                    log.info(
                        "Silence for %.1fs — resetting STT connection",
                        state.silence_duration,
                    )
                    await close_stt()
                    state.reset()
                    if not await open_stt():
                        return

    # Initial STT connection
    if not await open_stt():
        await client_ws.close()
        return

    # Start all tasks
    client_task = asyncio.create_task(read_client())
    audio_task = asyncio.create_task(forward_audio())
    stt_task = asyncio.create_task(read_stt())
    watchdog_task = asyncio.create_task(silence_watchdog())

    try:
        # Wait until client disconnects
        await client_task
    finally:
        running = False
        # Cancel all background tasks
        for task in [audio_task, stt_task, watchdog_task]:
            task.cancel()
            try:
                await task
            except (asyncio.CancelledError, Exception):
                pass
        await close_stt()
        log.info("Client disconnected")


async def main():
    log.info("STT Wrapper listening on ws://0.0.0.0:%d/asr", WRAPPER_PORT)
    async with websockets.serve(handle_client, "0.0.0.0", WRAPPER_PORT):
        await asyncio.Future()


if __name__ == "__main__":
    asyncio.run(main())
