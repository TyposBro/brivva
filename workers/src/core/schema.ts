// Drizzle schema — source of truth for table shapes + the typed query builder.
//
// Property names are deliberately snake_case so selects return rows whose
// JSON shape matches what the frontend + Fargate internal clients already
// expect (we return Drizzle rows straight from the Hono handlers). The string
// passed to each column helper is the physical SQL column name, which also
// happens to be snake_case — so the JS property and SQL column are aligned.

import { sql } from "drizzle-orm";
import {
  index,
  integer,
  real,
  sqliteTable,
  text,
  uniqueIndex,
} from "drizzle-orm/sqlite-core";

export const users = sqliteTable("users", {
  id: text("id").primaryKey(),
  youtube_channel_id: text("youtube_channel_id"),
  youtube_channel_name: text("youtube_channel_name"),
  youtube_access_token: text("youtube_access_token"),
  youtube_refresh_token: text("youtube_refresh_token"),
  youtube_token_expires_at: integer("youtube_token_expires_at"),
  email: text("email"),
  name: text("name"),
  picture: text("picture"),
  created_at: integer("created_at").notNull(),
});

export const voices = sqliteTable(
  "voices",
  {
    id: text("id").primaryKey(),
    user_id: text("user_id")
      .notNull()
      .references(() => users.id),
    elevenlabs_voice_id: text("elevenlabs_voice_id").notNull(),
    name: text("name").notNull(),
    // Host-sample language hint sent to ElevenLabs at clone time. Nullable for
    // historical rows cloned before language-hint support shipped.
    source_lang: text("source_lang"),
    created_at: integer("created_at").notNull(),
  },
  (t) => [index("voices_user_id_idx").on(t.user_id)],
);

export const sessions = sqliteTable(
  "sessions",
  {
    id: text("id").primaryKey(),
    user_id: text("user_id")
      .notNull()
      .references(() => users.id),
    voice_id: text("voice_id").references(() => voices.id),
    title: text("title").notNull(),
    source_lang: text("source_lang").notNull(),
    // JSON-encoded array of target-language codes (e.g. '["ja","ko"]').
    target_langs: text("target_langs").notNull(),
    status: text("status").notNull().default("setup"),
    live_session_id: text("live_session_id"),
    created_at: integer("created_at").notNull(),
  },
  (t) => [index("sessions_user_id_idx").on(t.user_id)],
);

export const streams = sqliteTable(
  "streams",
  {
    id: text("id").primaryKey(),
    session_id: text("session_id")
      .notNull()
      .references(() => sessions.id),
    lang: text("lang").notNull(),
    platform: text("platform").notNull().default("youtube"),
    platform_broadcast_id: text("platform_broadcast_id"),
    platform_stream_id: text("platform_stream_id"),
    stream_key: text("stream_key"),
    rtmp_url: text("rtmp_url"),
    status: text("status").notNull().default("created"),
    /** Fargate output delay. Covers STT+translate+TTS budget per target. */
    delay_ms: integer("delay_ms").notNull().default(2000),
    /** Host-audio gain under translated TTS. 1.0 = passthrough, 0.2 = duck. */
    host_gain: real("host_gain").notNull().default(0.2),
    created_at: integer("created_at").notNull(),
  },
  (t) => [index("streams_session_id_idx").on(t.session_id)],
);

export const platform_credentials = sqliteTable(
  "platform_credentials",
  {
    id: text("id").primaryKey(),
    user_id: text("user_id").notNull(),
    platform: text("platform").notNull(),
    rtmp_url: text("rtmp_url"),
    stream_key: text("stream_key"),
    display_name: text("display_name"),
    created_at: integer("created_at").notNull(),
    updated_at: integer("updated_at").notNull(),
  },
  (t) => [
    uniqueIndex("platform_credentials_user_platform_unique").on(
      t.user_id,
      t.platform,
    ),
    index("platform_creds_user_id_idx").on(t.user_id),
  ],
);

// Per-session usage rollup. Fargate PATCHes /internal/sessions/:id/metrics as
// host seconds + translated-output seconds per target language accumulate; the
// billing + usage endpoints read from here. Row is lazily upserted on the
// first PATCH after a session starts.
export const session_metrics = sqliteTable("session_metrics", {
  session_id: text("session_id").primaryKey(),
  source_seconds: real("source_seconds").notNull().default(0),
  // JSON-encoded { [lang]: seconds }. Stored as text so we don't need a
  // separate (session_id, lang) table for what is effectively a small map.
  output_seconds_json: text("output_seconds_json").notNull().default("{}"),
  updated_at: integer("updated_at").notNull(),
});

// Row types inferred directly from schema. Use these in handler signatures
// instead of hand-written mirror types.
export type User = typeof users.$inferSelect;
export type Voice = typeof voices.$inferSelect;
export type Session = typeof sessions.$inferSelect;
export type StreamRecord = typeof streams.$inferSelect;
export type PlatformCredential = typeof platform_credentials.$inferSelect;
export type SessionMetrics = typeof session_metrics.$inferSelect;

// Used below if/when we need a raw-SQL escape hatch from inside Drizzle land.
export const _sqlEscape = sql;
