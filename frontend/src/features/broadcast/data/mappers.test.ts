import { describe, it, expect } from "vitest";
import { toTranscriptEntry, applyTranslation, applyChunkTranslation } from "./mappers";
import type { FinalMsg, TranslationMsg, ChunkTranslationMsg } from "./dtos";
import type { TranscriptEntry } from "../domain/broadcast-types";

describe("toTranscriptEntry", () => {
  it("should_map_final_msg_to_transcript_entry", () => {
    const msg: FinalMsg = { type: "final", transcript: "hello world", utteranceId: 42 };

    const entry = toTranscriptEntry(msg);

    expect(entry).toEqual({ id: 42, text: "hello world", translations: {} });
  });

  it("should_create_empty_translations_map", () => {
    const msg: FinalMsg = { type: "final", transcript: "test", utteranceId: 1 };

    const entry = toTranscriptEntry(msg);

    expect(entry.translations).toEqual({});
  });
});

describe("applyTranslation", () => {
  it("should_add_translation_to_empty_entry", () => {
    const entry: TranscriptEntry = { id: 1, text: "hello", translations: {} };
    const msg: TranslationMsg = { type: "translation", lang: "ko", text: "annyeong", utteranceId: 1, translateMs: 100 };

    const updated = applyTranslation(entry, msg);

    expect(updated.translations).toEqual({ ko: "annyeong" });
  });

  it("should_preserve_existing_translations", () => {
    const entry: TranscriptEntry = { id: 1, text: "hello", translations: { ko: "annyeong" } };
    const msg: TranslationMsg = { type: "translation", lang: "ja", text: "konnichiwa", utteranceId: 1, translateMs: 150 };

    const updated = applyTranslation(entry, msg);

    expect(updated.translations).toEqual({ ko: "annyeong", ja: "konnichiwa" });
  });

  it("should_overwrite_existing_translation_for_same_lang", () => {
    const entry: TranscriptEntry = { id: 1, text: "hello", translations: { ko: "old" } };
    const msg: TranslationMsg = { type: "translation", lang: "ko", text: "new", utteranceId: 1, translateMs: 200 };

    const updated = applyTranslation(entry, msg);

    expect(updated.translations.ko).toBe("new");
  });

  it("should_not_mutate_original_entry", () => {
    const entry: TranscriptEntry = { id: 1, text: "hello", translations: {} };
    const msg: TranslationMsg = { type: "translation", lang: "ko", text: "annyeong", utteranceId: 1, translateMs: 100 };

    applyTranslation(entry, msg);

    expect(entry.translations).toEqual({});
  });
});

describe("applyChunkTranslation", () => {
  it("should_set_first_chunk_without_separator", () => {
    const entry: TranscriptEntry = { id: 1, text: "hello", translations: {} };
    const msg: ChunkTranslationMsg = {
      type: "chunk_translation", lang: "ko", text: "first", utteranceId: 1, chunkIndex: 0, translateMs: 50,
    };

    const updated = applyChunkTranslation(entry, msg);

    expect(updated.translations.ko).toBe("first");
  });

  it("should_append_subsequent_chunks_with_space", () => {
    const entry: TranscriptEntry = { id: 1, text: "hello", translations: { ko: "first" } };
    const msg: ChunkTranslationMsg = {
      type: "chunk_translation", lang: "ko", text: "second", utteranceId: 1, chunkIndex: 1, translateMs: 80,
    };

    const updated = applyChunkTranslation(entry, msg);

    expect(updated.translations.ko).toBe("first second");
  });

  it("should_not_add_separator_for_chunk_zero_on_empty_existing", () => {
    const entry: TranscriptEntry = { id: 1, text: "hello", translations: {} };
    const msg: ChunkTranslationMsg = {
      type: "chunk_translation", lang: "ja", text: "chunk", utteranceId: 1, chunkIndex: 0, translateMs: 30,
    };

    const updated = applyChunkTranslation(entry, msg);

    expect(updated.translations.ja).toBe("chunk");
  });

  it("should_not_mutate_original_entry", () => {
    const entry: TranscriptEntry = { id: 1, text: "hello", translations: { ko: "existing" } };
    const msg: ChunkTranslationMsg = {
      type: "chunk_translation", lang: "ko", text: "more", utteranceId: 1, chunkIndex: 1, translateMs: 60,
    };

    applyChunkTranslation(entry, msg);

    expect(entry.translations.ko).toBe("existing");
  });
});
