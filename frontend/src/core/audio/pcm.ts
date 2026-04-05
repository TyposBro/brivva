const INT16_MAX = 32767;
const INT16_MIN = -32768;
const INT16_SCALE = 32768;

export function float32ToInt16(float32: Float32Array): Int16Array {
  const int16 = new Int16Array(float32.length);
  for (let i = 0; i < float32.length; i++) {
    int16[i] = Math.max(INT16_MIN, Math.min(INT16_MAX, Math.round(float32[i] * INT16_SCALE)));
  }
  return int16;
}

export function mergePcmChunks(chunks: Int16Array[]): Int16Array {
  const totalLen = chunks.reduce((sum, c) => sum + c.length, 0);
  const merged = new Int16Array(totalLen);
  let offset = 0;
  for (const chunk of chunks) {
    merged.set(chunk, offset);
    offset += chunk.length;
  }
  return merged;
}

