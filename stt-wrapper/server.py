"""STT Service: Cloudflare Workers AI (Deepgram Nova-3) WebSocket proxy.

Accepts raw PCM audio (16kHz, 16-bit, mono) from the Rust server,
streams it to CF Workers AI Deepgram Nova-3 via WebSocket, and emits
clean interim/final events back.

Protocol (server → client):
  {"type": "interim", "text": "Hello world"}
  {"type": "final",   "text": "Hello world."}
  {"type": "error",   "message": "..."}
"""

import asyncio
import json
import os
import logging

import websockets

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("stt")

STT_PORT = int(os.environ.get("STT_PORT", "8766"))
CF_ACCOUNT_ID = os.environ.get("CF_ACCOUNT_ID", "")
CF_API_TOKEN = os.environ.get("CF_API_TOKEN", "")
STT_LANGUAGE = os.environ.get("STT_LANGUAGE", "en")

# Build Deepgram Nova-3 WebSocket URL via CF Workers AI
DG_WS_URL = (
    f"wss://api.cloudflare.com/client/v4/accounts/{CF_ACCOUNT_ID}"
    f"/ai/run/@cf/deepgram/nova-3"
    f"?encoding=linear16"
    f"&sample_rate=16000"
    f"&channels=1"
    f"&language={STT_LANGUAGE}"
    f"&punctuate=true"
    f"&smart_format=true"
    f"&interim_results=true"
    f"&endpointing=400"
    f"&vad_events=true"
    f"&utterance_end_ms=1500"
)


async def connect_to_deepgram():
    """Connect to CF Workers AI Deepgram Nova-3 WebSocket."""
    headers = {"Authorization": f"Bearer {CF_API_TOKEN}"}
    for attempt in range(1, 11):
        try:
            ws = await websockets.connect(
                DG_WS_URL,
                additional_headers=headers,
                max_size=10 * 1024 * 1024,
            )
            log.info("Connected to Deepgram Nova-3 (attempt %d)", attempt)
            return ws
        except Exception as e:
            log.warning("Deepgram connect attempt %d/10 failed: %s", attempt, e)
            await asyncio.sleep(2)
    raise ConnectionError("Failed to connect to Deepgram after 10 attempts")


async def handle_client(client_ws):
    """Handle one client: proxy audio to Deepgram, emit STT events."""
    log.info("Client connected")

    if not CF_ACCOUNT_ID or not CF_API_TOKEN:
        log.error("CF_ACCOUNT_ID and CF_API_TOKEN must be set")
        await client_ws.send(json.dumps({
            "type": "error",
            "message": "STT not configured: missing CF credentials"
        }))
        await client_ws.close()
        return

    try:
        dg_ws = await connect_to_deepgram()
    except ConnectionError as e:
        log.error("%s", e)
        await client_ws.send(json.dumps({"type": "error", "message": str(e)}))
        await client_ws.close()
        return

    last_interim = ""

    async def forward_audio():
        """Forward binary audio from client to Deepgram."""
        try:
            async for msg in client_ws:
                if isinstance(msg, bytes):
                    await dg_ws.send(msg)
        except websockets.ConnectionClosed:
            pass
        finally:
            # Signal Deepgram to finalize
            try:
                await dg_ws.send(json.dumps({"type": "CloseStream"}))
            except Exception:
                pass

    async def parse_responses():
        """Parse Deepgram responses and emit clean events."""
        nonlocal last_interim
        try:
            async for msg in dg_ws:
                if isinstance(msg, bytes):
                    continue
                try:
                    data = json.loads(msg)
                except json.JSONDecodeError:
                    continue

                msg_type = data.get("type", "")

                if msg_type == "Results":
                    channel = data.get("channel", {})
                    alts = channel.get("alternatives", [])
                    if not alts:
                        continue

                    transcript = alts[0].get("transcript", "").strip()
                    if not transcript:
                        continue

                    is_final = data.get("is_final", False)
                    speech_final = data.get("speech_final", False)

                    if speech_final or is_final:
                        last_interim = ""
                        log.info("[FINAL] %s", transcript)
                        await client_ws.send(json.dumps({
                            "type": "final",
                            "text": transcript,
                        }))
                    else:
                        if transcript != last_interim:
                            last_interim = transcript
                            log.info("[INTERIM] %s", transcript)
                            await client_ws.send(json.dumps({
                                "type": "interim",
                                "text": transcript,
                            }))

                elif msg_type == "SpeechStarted":
                    log.info("VAD: speech started")

                elif msg_type == "UtteranceEnd":
                    log.info("VAD: utterance end")

                elif msg_type == "Metadata":
                    log.info("Deepgram session started (request_id=%s)",
                             data.get("request_id", "?"))

                elif msg_type == "Error":
                    log.error("Deepgram error: %s", data.get("message", ""))

        except websockets.ConnectionClosed:
            pass

    audio_task = asyncio.create_task(forward_audio())
    parse_task = asyncio.create_task(parse_responses())

    try:
        done, pending = await asyncio.wait(
            [audio_task, parse_task], return_when=asyncio.FIRST_COMPLETED
        )
        for task in pending:
            task.cancel()
    finally:
        try:
            await dg_ws.close()
        except Exception:
            pass
        log.info("Client disconnected")


async def main():
    log.info("STT server listening on ws://0.0.0.0:%d/asr", STT_PORT)
    log.info("Using CF Workers AI Deepgram Nova-3 (lang=%s)", STT_LANGUAGE)
    async with websockets.serve(handle_client, "0.0.0.0", STT_PORT):
        await asyncio.Future()


if __name__ == "__main__":
    asyncio.run(main())
