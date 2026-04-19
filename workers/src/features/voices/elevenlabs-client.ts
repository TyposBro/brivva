// ElevenLabs voice clone proxy.
//
// FE sends base64 audio (host recorded ~30s of their voice); Worker forwards
// to ElevenLabs /v1/voices/add as multipart and returns the new voice_id.
//
// `source_lang` is optional — when present it rides along as a label on the
// clone. ElevenLabs exposes `labels` as a free-form JSON map on
// /v1/voices/add; the Instant Voice Clone model doesn't take language as a
// first-class parameter, but the label is surfaced back in /v1/voices and
// helps downstream TTS pick a language-appropriate model at synthesis time.
// See docs/voice-clone-notes.md for the fuller story.

const ADD_VOICE_URL = "https://api.elevenlabs.io/v1/voices/add";
const DELETE_VOICE_URL = "https://api.elevenlabs.io/v1/voices"; // + /{voice_id}

export type CloneVoiceArgs = {
  name: string;
  audioBase64: string;
  sourceLang: string | null;
};

export async function cloneVoice(
  apiKey: string,
  args: CloneVoiceArgs,
): Promise<{ voice_id: string }> {
  const bin = Uint8Array.from(atob(args.audioBase64), (c) => c.charCodeAt(0));
  const form = new FormData();
  form.set("name", args.name);
  form.set("description", `Brivva cloned voice: ${args.name}`);
  form.set("files", new Blob([bin], { type: "audio/wav" }), "host.wav");
  if (args.sourceLang) {
    form.set("labels", JSON.stringify({ language: args.sourceLang }));
  }

  const resp = await fetch(ADD_VOICE_URL, {
    method: "POST",
    headers: { "xi-api-key": apiKey },
    body: form,
  });
  if (!resp.ok) {
    const body = await resp.text();
    throw new Error(`ElevenLabs clone failed: ${resp.status} ${body}`);
  }
  return await resp.json<{ voice_id: string }>();
}

export async function deleteRemoteVoice(apiKey: string, voiceId: string): Promise<void> {
  await fetch(`${DELETE_VOICE_URL}/${encodeURIComponent(voiceId)}`, {
    method: "DELETE",
    headers: { "xi-api-key": apiKey },
  });
}
