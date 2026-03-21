//! SQLite database: users, voices, sessions, streams.

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;

// ── Types ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub youtube_channel_id: Option<String>,
    pub youtube_channel_name: Option<String>,
    pub youtube_access_token: Option<String>,
    pub youtube_refresh_token: Option<String>,
    pub youtube_token_expires_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voice {
    pub id: String,
    pub user_id: String,
    pub elevenlabs_voice_id: String,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub voice_id: Option<String>,
    pub title: String,
    pub source_lang: String,
    pub target_langs: String, // JSON array
    pub status: String,       // "setup" | "live" | "ended"
    pub room_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamRecord {
    pub id: String,
    pub session_id: String,
    pub lang: String,
    pub platform: String, // "youtube", "instagram", "coupang", "custom"
    pub platform_broadcast_id: Option<String>,
    pub platform_stream_id: Option<String>,
    pub stream_key: Option<String>,
    pub rtmp_url: Option<String>,
    pub status: String,
    pub created_at: i64,
}

// ── Init ──────────────────────────────────────────────

pub async fn init_db() -> SqlitePool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:brivva.db?mode=rwc".into());
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("Failed to connect to SQLite");

    // Run migrations
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            youtube_channel_id TEXT,
            youtube_channel_name TEXT,
            youtube_access_token TEXT,
            youtube_refresh_token TEXT,
            youtube_token_expires_at INTEGER,
            created_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .expect("Failed to create users table");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS voices (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            elevenlabs_voice_id TEXT NOT NULL,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .expect("Failed to create voices table");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            voice_id TEXT REFERENCES voices(id),
            title TEXT NOT NULL,
            source_lang TEXT NOT NULL,
            target_langs TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'setup',
            room_id TEXT,
            created_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .expect("Failed to create sessions table");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS streams (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES sessions(id),
            lang TEXT NOT NULL,
            platform TEXT NOT NULL DEFAULT 'youtube',
            platform_broadcast_id TEXT,
            platform_stream_id TEXT,
            stream_key TEXT,
            rtmp_url TEXT,
            status TEXT NOT NULL DEFAULT 'created',
            created_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .expect("Failed to create streams table");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_credentials (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            platform TEXT NOT NULL,
            rtmp_url TEXT,
            stream_key TEXT,
            display_name TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            UNIQUE(user_id, platform)
        )",
    )
    .execute(&pool)
    .await
    .expect("Failed to create platform_credentials table");

    println!("[DB] SQLite initialized");
    pool
}

// ── User CRUD ──────────────────────────────────────────

pub async fn get_or_create_user(pool: &SqlitePool, id: &str) -> User {
    let now = chrono::Utc::now().timestamp();

    sqlx::query("INSERT OR IGNORE INTO users (id, created_at) VALUES (?, ?)")
        .bind(id)
        .bind(now)
        .execute(pool)
        .await
        .ok();

    let row = sqlx::query("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("User must exist after insert");

    User {
        id: row.get("id"),
        youtube_channel_id: row.get("youtube_channel_id"),
        youtube_channel_name: row.get("youtube_channel_name"),
        youtube_access_token: row.get("youtube_access_token"),
        youtube_refresh_token: row.get("youtube_refresh_token"),
        youtube_token_expires_at: row.get("youtube_token_expires_at"),
        created_at: row.get("created_at"),
    }
}

pub async fn update_youtube_tokens(
    pool: &SqlitePool,
    user_id: &str,
    access_token: &str,
    refresh_token: &str,
    expires_at: i64,
    channel_id: &str,
    channel_name: &str,
) {
    sqlx::query(
        "UPDATE users SET youtube_access_token=?, youtube_refresh_token=?,
         youtube_token_expires_at=?, youtube_channel_id=?, youtube_channel_name=?
         WHERE id=?",
    )
    .bind(access_token)
    .bind(refresh_token)
    .bind(expires_at)
    .bind(channel_id)
    .bind(channel_name)
    .bind(user_id)
    .execute(pool)
    .await
    .ok();
}

pub async fn update_access_token(pool: &SqlitePool, user_id: &str, access_token: &str, expires_at: i64) {
    sqlx::query("UPDATE users SET youtube_access_token=?, youtube_token_expires_at=? WHERE id=?")
        .bind(access_token)
        .bind(expires_at)
        .bind(user_id)
        .execute(pool)
        .await
        .ok();
}

// ── Voice CRUD ─────────────────────────────────────────

pub async fn create_voice(pool: &SqlitePool, user_id: &str, elevenlabs_voice_id: &str, name: &str) -> Voice {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query("INSERT INTO voices (id, user_id, elevenlabs_voice_id, name, created_at) VALUES (?,?,?,?,?)")
        .bind(&id)
        .bind(user_id)
        .bind(elevenlabs_voice_id)
        .bind(name)
        .bind(now)
        .execute(pool)
        .await
        .expect("Failed to insert voice");

    Voice { id, user_id: user_id.to_string(), elevenlabs_voice_id: elevenlabs_voice_id.to_string(), name: name.to_string(), created_at: now }
}

pub async fn list_voices(pool: &SqlitePool, user_id: &str) -> Vec<Voice> {
    sqlx::query("SELECT * FROM voices WHERE user_id=? ORDER BY created_at DESC")
        .bind(user_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| Voice {
            id: r.get("id"),
            user_id: r.get("user_id"),
            elevenlabs_voice_id: r.get("elevenlabs_voice_id"),
            name: r.get("name"),
            created_at: r.get("created_at"),
        })
        .collect()
}

pub async fn delete_voice_db(pool: &SqlitePool, voice_id: &str) {
    sqlx::query("DELETE FROM voices WHERE id=?")
        .bind(voice_id)
        .execute(pool)
        .await
        .ok();
}

pub async fn get_voice(pool: &SqlitePool, voice_id: &str) -> Option<Voice> {
    sqlx::query("SELECT * FROM voices WHERE id=?")
        .bind(voice_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|r| Voice {
            id: r.get("id"),
            user_id: r.get("user_id"),
            elevenlabs_voice_id: r.get("elevenlabs_voice_id"),
            name: r.get("name"),
            created_at: r.get("created_at"),
        })
}

// ── Session CRUD ───────────────────────────────────────

pub async fn create_session(
    pool: &SqlitePool,
    user_id: &str,
    voice_id: Option<&str>,
    title: &str,
    source_lang: &str,
    target_langs: &str,
) -> Result<Session, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO sessions (id, user_id, voice_id, title, source_lang, target_langs, status, created_at)
         VALUES (?,?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(voice_id)
    .bind(title)
    .bind(source_lang)
    .bind(target_langs)
    .bind("setup")
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create session: {}", e))?;

    Ok(Session {
        id,
        user_id: user_id.to_string(),
        voice_id: voice_id.map(String::from),
        title: title.to_string(),
        source_lang: source_lang.to_string(),
        target_langs: target_langs.to_string(),
        status: "setup".to_string(),
        room_id: None,
        created_at: now,
    })
}

pub async fn list_sessions(pool: &SqlitePool, user_id: &str) -> Vec<Session> {
    sqlx::query("SELECT * FROM sessions WHERE user_id=? ORDER BY created_at DESC")
        .bind(user_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| Session {
            id: r.get("id"),
            user_id: r.get("user_id"),
            voice_id: r.get("voice_id"),
            title: r.get("title"),
            source_lang: r.get("source_lang"),
            target_langs: r.get("target_langs"),
            status: r.get("status"),
            room_id: r.get("room_id"),
            created_at: r.get("created_at"),
        })
        .collect()
}

pub async fn get_session(pool: &SqlitePool, id: &str) -> Option<Session> {
    sqlx::query("SELECT * FROM sessions WHERE id=?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|r| Session {
            id: r.get("id"),
            user_id: r.get("user_id"),
            voice_id: r.get("voice_id"),
            title: r.get("title"),
            source_lang: r.get("source_lang"),
            target_langs: r.get("target_langs"),
            status: r.get("status"),
            room_id: r.get("room_id"),
            created_at: r.get("created_at"),
        })
}

pub async fn update_session_status(pool: &SqlitePool, id: &str, status: &str, room_id: Option<&str>) {
    sqlx::query("UPDATE sessions SET status=?, room_id=? WHERE id=?")
        .bind(status)
        .bind(room_id)
        .bind(id)
        .execute(pool)
        .await
        .ok();
}

// ── Stream CRUD ────────────────────────────────────────

pub async fn create_stream(pool: &SqlitePool, session_id: &str, lang: &str, platform: &str) -> StreamRecord {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query("INSERT INTO streams (id, session_id, lang, platform, status, created_at) VALUES (?,?,?,?,?,?)")
        .bind(&id)
        .bind(session_id)
        .bind(lang)
        .bind(platform)
        .bind("created")
        .bind(now)
        .execute(pool)
        .await
        .expect("Failed to insert stream");

    StreamRecord {
        id,
        session_id: session_id.to_string(),
        lang: lang.to_string(),
        platform: platform.to_string(),
        platform_broadcast_id: None,
        platform_stream_id: None,
        stream_key: None,
        rtmp_url: None,
        status: "created".to_string(),
        created_at: now,
    }
}

/// Create a stream with manual RTMP URL + key (for non-YouTube platforms)
pub async fn create_stream_manual(
    pool: &SqlitePool,
    session_id: &str,
    lang: &str,
    platform: &str,
    rtmp_url: &str,
    stream_key: &str,
) -> StreamRecord {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO streams (id, session_id, lang, platform, rtmp_url, stream_key, status, created_at)
         VALUES (?,?,?,?,?,?,?,?)"
    )
        .bind(&id)
        .bind(session_id)
        .bind(lang)
        .bind(platform)
        .bind(rtmp_url)
        .bind(stream_key)
        .bind("ready")
        .bind(now)
        .execute(pool)
        .await
        .expect("Failed to insert stream");

    StreamRecord {
        id,
        session_id: session_id.to_string(),
        lang: lang.to_string(),
        platform: platform.to_string(),
        platform_broadcast_id: None,
        platform_stream_id: None,
        stream_key: Some(stream_key.to_string()),
        rtmp_url: Some(rtmp_url.to_string()),
        status: "ready".to_string(),
        created_at: now,
    }
}

pub async fn update_stream_platform(
    pool: &SqlitePool,
    stream_id: &str,
    broadcast_id: &str,
    platform_stream_id: &str,
    stream_key: &str,
    rtmp_url: &str,
) {
    sqlx::query(
        "UPDATE streams SET platform_broadcast_id=?, platform_stream_id=?, stream_key=?, rtmp_url=?, status='ready'
         WHERE id=?",
    )
    .bind(broadcast_id)
    .bind(platform_stream_id)
    .bind(stream_key)
    .bind(rtmp_url)
    .bind(stream_id)
    .execute(pool)
    .await
    .ok();
}

pub async fn delete_stream(pool: &SqlitePool, stream_id: &str) {
    sqlx::query("DELETE FROM streams WHERE id=?")
        .bind(stream_id)
        .execute(pool)
        .await
        .ok();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformCredential {
    pub id: String,
    pub user_id: String,
    pub platform: String,
    pub rtmp_url: Option<String>,
    pub stream_key: Option<String>,
    pub display_name: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub async fn list_streams(pool: &SqlitePool, session_id: &str) -> Vec<StreamRecord> {
    sqlx::query("SELECT * FROM streams WHERE session_id=? ORDER BY platform, lang")
        .bind(session_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| StreamRecord {
            id: r.get("id"),
            session_id: r.get("session_id"),
            lang: r.get("lang"),
            platform: r.get("platform"),
            platform_broadcast_id: r.get("platform_broadcast_id"),
            platform_stream_id: r.get("platform_stream_id"),
            stream_key: r.get("stream_key"),
            rtmp_url: r.get("rtmp_url"),
            status: r.get("status"),
            created_at: r.get("created_at"),
        })
        .collect()
}

// ── Platform Credentials CRUD ─────────────────────────

pub async fn upsert_platform_credential(
    pool: &SqlitePool,
    user_id: &str,
    platform: &str,
    rtmp_url: Option<&str>,
    stream_key: Option<&str>,
    display_name: Option<&str>,
) -> Result<PlatformCredential, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO platform_credentials (id, user_id, platform, rtmp_url, stream_key, display_name, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(user_id, platform) DO UPDATE SET
           rtmp_url = excluded.rtmp_url,
           stream_key = excluded.stream_key,
           display_name = COALESCE(excluded.display_name, platform_credentials.display_name),
           updated_at = excluded.updated_at",
    )
    .bind(&id)
    .bind(user_id)
    .bind(platform)
    .bind(rtmp_url)
    .bind(stream_key)
    .bind(display_name)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    // Fetch the actual record (might have existing id if it was an update)
    get_platform_credential(pool, user_id, platform)
        .await
        .map(|opt| opt.unwrap())
}

pub async fn get_platform_credential(
    pool: &SqlitePool,
    user_id: &str,
    platform: &str,
) -> Result<Option<PlatformCredential>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, user_id, platform, rtmp_url, stream_key, display_name, created_at, updated_at
         FROM platform_credentials WHERE user_id = ? AND platform = ?",
    )
    .bind(user_id)
    .bind(platform)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| PlatformCredential {
        id: r.get("id"),
        user_id: r.get("user_id"),
        platform: r.get("platform"),
        rtmp_url: r.get("rtmp_url"),
        stream_key: r.get("stream_key"),
        display_name: r.get("display_name"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }))
}

pub async fn list_platform_credentials(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Vec<PlatformCredential>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, user_id, platform, rtmp_url, stream_key, display_name, created_at, updated_at
         FROM platform_credentials WHERE user_id = ? ORDER BY platform",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PlatformCredential {
            id: r.get("id"),
            user_id: r.get("user_id"),
            platform: r.get("platform"),
            rtmp_url: r.get("rtmp_url"),
            stream_key: r.get("stream_key"),
            display_name: r.get("display_name"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        })
        .collect())
}

pub async fn delete_platform_credential(
    pool: &SqlitePool,
    user_id: &str,
    platform: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM platform_credentials WHERE user_id = ? AND platform = ?")
        .bind(user_id)
        .bind(platform)
        .execute(pool)
        .await?;
    Ok(())
}
