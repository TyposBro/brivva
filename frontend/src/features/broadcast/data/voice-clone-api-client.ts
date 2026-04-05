type VoiceStatusResponse = {
  active: boolean;
};

export async function checkVoiceStatus(apiBaseUrl: string): Promise<boolean> {
  const response = await fetch(`${apiBaseUrl}/api/voice`);
  const body: VoiceStatusResponse = await response.json();

  return body.active;
}

export async function uploadVoiceClone(apiBaseUrl: string, pcm: Int16Array): Promise<void> {
  const response = await fetch(`${apiBaseUrl}/api/voice/clone`, {
    method: "POST",
    headers: { "Content-Type": "application/octet-stream" },
    body: new Uint8Array(pcm.buffer) as unknown as BodyInit,
  });

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`Voice clone failed: ${errorText}`);
  }
}
