//! YouTube OAuth2 + Data API v3 broadcast management.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::LazyLock;

use crate::db;

// ── Config ──────────────────────────────────────────────

static GOOGLE_CLIENT_ID: LazyLock<String> =
    LazyLock::new(|| std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default());
static GOOGLE_CLIENT_SECRET: LazyLock<String> =
    LazyLock::new(|| std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default());
static GOOGLE_REDIRECT_URI: LazyLock<String> = LazyLock::new(|| {
    std::env::var("GOOGLE_REDIRECT_URI")
        .unwrap_or_else(|_| "https://brivva-server.milliytechnology.org/auth/youtube/callback".into())
});

const SCOPES: &str = "https://www.googleapis.com/auth/youtube.force-ssl https://www.googleapis.com/auth/youtube.readonly";

// ── Types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BroadcastInfo {
    pub broadcast_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamInfo {
    pub stream_id: String,
    pub stream_key: String,
    pub rtmp_url: String,
}

// ── OAuth2 ─────────────────────────────────────────────

/// Build the Google OAuth consent URL
pub fn oauth_url(user_id: &str) -> String {
    format!(
        "https://accounts.google.com/o/oauth2/v2/auth\
         ?client_id={}\
         &redirect_uri={}\
         &response_type=code\
         &scope={}\
         &access_type=offline\
         &prompt=consent\
         &state={}",
        urlencoded(&GOOGLE_CLIENT_ID),
        urlencoded(&GOOGLE_REDIRECT_URI),
        urlencoded(SCOPES),
        urlencoded(user_id),
    )
}

/// Exchange authorization code for tokens
pub async fn exchange_code(code: &str) -> Result<TokenResponse, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code),
            ("client_id", &GOOGLE_CLIENT_ID),
            ("client_secret", &GOOGLE_CLIENT_SECRET),
            ("redirect_uri", &GOOGLE_REDIRECT_URI),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|e| format!("Token exchange request failed: {}", e))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token exchange failed: {}", body));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| format!("Token parse error: {}", e))
}

/// Refresh an expired access token
pub async fn refresh_access_token(refresh_token: &str) -> Result<TokenResponse, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("refresh_token", refresh_token),
            ("client_id", &*GOOGLE_CLIENT_ID),
            ("client_secret", &*GOOGLE_CLIENT_SECRET),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| format!("Refresh request failed: {}", e))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token refresh failed: {}", body));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| format!("Refresh parse error: {}", e))
}

/// Get a valid access token, refreshing if needed
pub async fn ensure_valid_token(pool: &SqlitePool, user_id: &str) -> Result<String, String> {
    let user = db::get_or_create_user(pool, user_id).await;

    let access_token = user.youtube_access_token.ok_or("No YouTube token")?;
    let refresh_token = user.youtube_refresh_token.ok_or("No refresh token")?;
    let expires_at = user.youtube_token_expires_at.unwrap_or(0);

    let now = chrono::Utc::now().timestamp();
    if now < expires_at - 60 {
        // Token still valid (with 60s buffer)
        return Ok(access_token);
    }

    // Refresh
    println!("[YOUTUBE] Refreshing access token for user {}", user_id);
    let tokens = refresh_access_token(&refresh_token).await?;
    let new_expires = now + tokens.expires_in;
    db::update_access_token(pool, user_id, &tokens.access_token, new_expires).await;
    Ok(tokens.access_token)
}

/// Get channel info (id, name) from YouTube
pub async fn get_channel_info(access_token: &str) -> Result<(String, String), String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://www.googleapis.com/youtube/v3/channels?part=snippet&mine=true")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Channel info request failed: {}", e))?;

    let body: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;

    let items = body["items"].as_array().ok_or("No channel items")?;
    if items.is_empty() {
        return Err("No YouTube channel found".into());
    }

    let channel = &items[0];
    let id = channel["id"].as_str().unwrap_or("").to_string();
    let name = channel["snippet"]["title"].as_str().unwrap_or("").to_string();
    Ok((id, name))
}

// ── Broadcast Management ───────────────────────────────

/// Create a YouTube live broadcast
pub async fn create_broadcast(
    access_token: &str,
    title: &str,
    scheduled_start: &str, // ISO 8601
    privacy_status: &str,  // "public", "unlisted", or "private"
) -> Result<BroadcastInfo, String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "snippet": {
            "title": title,
            "scheduledStartTime": scheduled_start,
        },
        "status": {
            "privacyStatus": privacy_status,
            "selfDeclaredMadeForKids": false,
        },
        "contentDetails": {
            "enableAutoStart": true,
            "enableAutoStop": true,
            "latencyPreference": "ultraLow",
        }
    });

    let resp = client
        .post("https://www.googleapis.com/youtube/v3/liveBroadcasts?part=snippet,status,contentDetails")
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Create broadcast failed: {}", e))?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        return Err(format!("Create broadcast error: {}", err));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    let broadcast_id = data["id"].as_str().unwrap_or("").to_string();
    println!("[YOUTUBE] Created broadcast: {}", broadcast_id);
    Ok(BroadcastInfo { broadcast_id })
}

/// Create a YouTube live stream (gets RTMP ingest info)
pub async fn create_live_stream(access_token: &str, title: &str) -> Result<StreamInfo, String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "snippet": {
            "title": title,
        },
        "cdn": {
            "frameRate": "30fps",
            "ingestionType": "rtmp",
            "resolution": "720p",
        }
    });

    let resp = client
        .post("https://www.googleapis.com/youtube/v3/liveStreams?part=snippet,cdn")
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Create stream failed: {}", e))?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        return Err(format!("Create stream error: {}", err));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    let stream_id = data["id"].as_str().unwrap_or("").to_string();
    let ingestion = &data["cdn"]["ingestionInfo"];
    let stream_key = ingestion["streamName"].as_str().unwrap_or("").to_string();
    let rtmp_url = ingestion["ingestionAddress"].as_str().unwrap_or("").to_string();

    println!("[YOUTUBE] Created stream: {} (key={}...)", stream_id, &stream_key[..8.min(stream_key.len())]);
    Ok(StreamInfo { stream_id, stream_key, rtmp_url })
}

/// Bind a broadcast to a stream
pub async fn bind_broadcast(
    access_token: &str,
    broadcast_id: &str,
    stream_id: &str,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://www.googleapis.com/youtube/v3/liveBroadcasts/bind?id={}&part=id&streamId={}",
        broadcast_id, stream_id
    );

    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Bind failed: {}", e))?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        return Err(format!("Bind error: {}", err));
    }

    println!("[YOUTUBE] Bound broadcast {} to stream {}", broadcast_id, stream_id);
    Ok(())
}

/// Transition broadcast status (testing → live → complete)
pub async fn transition_broadcast(
    access_token: &str,
    broadcast_id: &str,
    status: &str,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://www.googleapis.com/youtube/v3/liveBroadcasts/transition?broadcastStatus={}&id={}&part=id,status",
        status, broadcast_id
    );

    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Transition failed: {}", e))?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        return Err(format!("Transition error: {}", err));
    }

    println!("[YOUTUBE] Broadcast {} → {}", broadcast_id, status);
    Ok(())
}

// ── Helpers ────────────────────────────────────────────

fn urlencoded(s: &str) -> String {
    s.replace(' ', "%20")
        .replace(':', "%3A")
        .replace('/', "%2F")
        .replace('?', "%3F")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('@', "%40")
}
