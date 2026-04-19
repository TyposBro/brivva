// ElevenLabs voice clone proxy.
//
// FE sends base64 audio (host recorded ~30s of their voice); Worker forwards
// to ElevenLabs /v1/voices/add as multipart and returns the new voice_id.

const ADD_VOICE_URL = "https://api.elevenlabs.io/v1/voices/add";
const DELETE_VOICE_URL = "https://api.elevenlabs.io/v1/voices"; // + /{voice_id}

export async function cloneVoice(
  apiKey: string,
  name: string,
  audioBase64: string,
): Promise<{ voice_id: string }> {
  const bin = Uint8Array.from(atob(audioBase64), (c) => c.charCodeAt(0));
  const form = new FormData();
  form.set("name", name);
  form.set("description", `Brivva cloned voice: ${name}`);
  form.set("files", new Blob([bin], { type: "audio/wav" }), "host.wav");

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
