import type { TranscriptEntry } from "../domain/broadcast-types";
import type { FinalMsg, TranslationMsg, ChunkTranslationMsg } from "./dtos";

export function toTranscriptEntry(msg: FinalMsg): TranscriptEntry {
  return { id: msg.utteranceId, text: msg.transcript, translations: {} };
}

export function applyTranslation(entry: TranscriptEntry, msg: TranslationMsg): TranscriptEntry {
  return { ...entry, translations: { ...entry.translations, [msg.lang]: msg.text } };
}

export function applyChunkTranslation(entry: TranscriptEntry, msg: ChunkTranslationMsg): TranscriptEntry {
  const existing = entry.translations[msg.lang] || "";
  const separator = existing && msg.chunkIndex > 0 ? " " : "";

  return { ...entry, translations: { ...entry.translations, [msg.lang]: existing + separator + msg.text } };
}
