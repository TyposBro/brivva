// Stub for Soniox Realtime STT WS.
// server-rs opens WS, sends JSON config, then PCM s16le audio frames.
// We ignore audio content and on a fixed interval emit synthetic tokens
// that look like Soniox translation output so server-rs runs the TTS path.
//
// Token schema (subset server-rs consumes in soniox.rs:SonioxResponse):
//   { tokens: [{ text, is_final, translation_status }, ...] }
// <end> marks utterance boundary → triggers TTS broadcast.

const PORT = Number(process.env.PORT ?? 9001);
const UTTERANCE_EVERY_MS = Number(process.env.SONIOX_UTTERANCE_MS ?? 5_000);

const phrases = [
  "hello from the smoke test",
  "this is a synthetic translation",
  "server pipeline is alive",
];

Bun.serve({
  port: PORT,
  fetch(req, server) {
    if (server.upgrade(req)) return;
    return new Response("soniox-stub", { status: 200 });
  },
  websocket: {
    open(ws) {
      console.log("[soniox-stub] client connected");
      let utterance = 0;
      const timer = setInterval(() => {
        const phrase = phrases[utterance % phrases.length]!;
        const payload = {
          tokens: [
            ...phrase.split(" ").map((word) => ({
              text: `${word} `,
              is_final: true,
              translation_status: "translation",
            })),
            { text: "<end>", is_final: true, translation_status: "translation" },
          ],
        };
        ws.send(JSON.stringify(payload));
        utterance += 1;
      }, UTTERANCE_EVERY_MS);
      (ws as unknown as { __timer?: NodeJS.Timeout }).__timer = timer;
    },
    message(_ws, _data) {
      // swallow config + PCM audio
    },
    close(ws) {
      const t = (ws as unknown as { __timer?: NodeJS.Timeout }).__timer;
      if (t) clearInterval(t);
      console.log("[soniox-stub] client disconnected");
    },
  },
});

console.log(`[soniox-stub] WS listening on :${PORT}`);
