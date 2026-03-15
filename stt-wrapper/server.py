"""STT Service: WebSocket server using RealtimeSTT.

Accepts raw PCM audio (16kHz, 16-bit, mono) from the Rust server,
feeds it to RealtimeSTT which handles VAD, silence detection, and
transcription internally. Emits clean interim/final events.

Protocol (server → client):
  {"type": "interim", "text": "Hello world"}
  {"type": "final",   "text": "Hello world."}
  {"type": "error",   "message": "..."}
"""

import asyncio
import json
import os
import logging
import threading

import websockets

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("stt")

STT_PORT = int(os.environ.get("STT_PORT", "8766"))
STT_MODEL = os.environ.get("STT_MODEL", "base.en")
STT_LANGUAGE = os.environ.get("STT_LANGUAGE", "en")


class STTSession:
    """Bridges RealtimeSTT (threaded/multiprocessing) with asyncio websocket."""

    def __init__(self, loop: asyncio.AbstractEventLoop):
        from RealtimeSTT import AudioToTextRecorder

        self.loop = loop
        self.events: asyncio.Queue = asyncio.Queue()
        self._running = True
        self._last_interim = ""

        self.recorder = AudioToTextRecorder(
            model=STT_MODEL,
            language=STT_LANGUAGE,
            use_microphone=False,
            spinner=False,

            # Realtime transcription for interim results
            enable_realtime_transcription=True,
            realtime_model_type=STT_MODEL,
            on_realtime_transcription_stabilized=self._on_interim,

            # VAD: use Silero for end-of-speech detection (more robust than WebRTC)
            silero_sensitivity=0.4,
            silero_deactivity_detection=True,
            webrtc_sensitivity=3,
            post_speech_silence_duration=0.4,
            min_length_of_recording=0.3,
            pre_recording_buffer_duration=0.2,

            # Performance
            beam_size=5,
            beam_size_realtime=3,
            no_log_file=True,
            level=logging.INFO,

            # Callbacks for debugging
            on_recording_start=lambda: log.info("VAD: speech started"),
            on_recording_stop=lambda: log.info("VAD: speech ended"),
        )

        self._thread = threading.Thread(target=self._transcription_loop, daemon=True)
        self._thread.start()
        log.info("STT session started (model=%s, lang=%s)", STT_MODEL, STT_LANGUAGE)

    def _emit(self, event: dict):
        """Thread-safe emit to asyncio queue."""
        self.loop.call_soon_threadsafe(self.events.put_nowait, event)

    def _on_interim(self, text: str):
        text = text.strip()
        if text and text != self._last_interim:
            self._last_interim = text
            log.info("[INTERIM] %s", text)
            self._emit({"type": "interim", "text": text})

    def _on_final(self, text: str):
        text = text.strip()
        if text:
            self._last_interim = ""  # reset dedup on final
            log.info("[FINAL] %s", text)
            self._emit({"type": "final", "text": text})

    def _transcription_loop(self):
        """Blocking loop: waits for complete utterances via VAD."""
        while self._running:
            try:
                self.recorder.text(self._on_final)
            except Exception as e:
                if self._running:
                    log.error("Transcription error: %s", e)
                break

    def feed(self, audio_bytes: bytes):
        """Feed raw PCM audio data to the recorder."""
        self.recorder.feed_audio(audio_bytes)

    def shutdown(self):
        self._running = False
        try:
            self.recorder.shutdown()
        except Exception:
            pass


async def handle_client(ws):
    """Handle one client: receive audio, emit STT events."""
    log.info("Client connected")
    loop = asyncio.get_event_loop()
    session = STTSession(loop)

    async def send_events():
        """Forward STT events from queue to websocket."""
        try:
            while True:
                event = await session.events.get()
                await ws.send(json.dumps(event))
        except (websockets.ConnectionClosed, asyncio.CancelledError):
            pass

    async def receive_audio():
        """Receive audio chunks from client and feed to STT."""
        try:
            async for msg in ws:
                if isinstance(msg, bytes):
                    session.feed(msg)
        except websockets.ConnectionClosed:
            pass

    sender = asyncio.create_task(send_events())
    try:
        await receive_audio()
    except Exception as e:
        log.error("Client error: %s", e)
    finally:
        sender.cancel()
        session.shutdown()
        log.info("Client disconnected")


async def main():
    log.info("STT server listening on ws://0.0.0.0:%d/asr", STT_PORT)
    log.info("Model: %s, Language: %s", STT_MODEL, STT_LANGUAGE)
    async with websockets.serve(handle_client, "0.0.0.0", STT_PORT):
        await asyncio.Future()


if __name__ == "__main__":
    main_loop = asyncio.new_event_loop()
    main_loop.run_until_complete(main())
