export const SAMPLE_RATE = 44_100;
export const AUDIO_FRAME_DURATION_MS = 20;
export const BYTES_PER_SAMPLE = 2;
export const AUDIO_CHANNELS = 1;
export const AUDIO_SAMPLES_PER_FRAME = (SAMPLE_RATE * AUDIO_FRAME_DURATION_MS) / 1000;
export const AUDIO_BYTES_PER_FRAME =
  AUDIO_SAMPLES_PER_FRAME * AUDIO_CHANNELS * BYTES_PER_SAMPLE;

export function float32ToInt16Pcm(samples: Float32Array): Int16Array {
  const pcm = new Int16Array(samples.length);
  for (let i = 0; i < samples.length; i += 1) {
    const sample = Math.max(-1, Math.min(1, samples[i] ?? 0));
    pcm[i] = sample < 0 ? sample * 0x8000 : sample * 0x7fff;
  }
  return pcm;
}
