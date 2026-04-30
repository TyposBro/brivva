use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::features::broadcast::data::workers_api::WorkersApi;

#[derive(Clone)]
pub struct SessionLogEmitter {
    enabled: bool,
    verbose: bool,
    workers_api: Arc<WorkersApi>,
    session_id: Option<String>,
    live_session_id: String,
}

#[derive(Serialize)]
struct SessionLogPayload {
    events: Vec<SessionLogEvent>,
}

#[derive(Serialize)]
struct SessionLogEvent {
    session_id: String,
    live_session_id: String,
    source: &'static str,
    level: &'static str,
    event: &'static str,
    message: Option<String>,
    fields: Value,
    ts_ms: i64,
}

impl SessionLogEmitter {
    pub fn new(
        enabled: bool,
        verbose: bool,
        workers_api: Arc<WorkersApi>,
        session_id: Option<String>,
        live_session_id: String,
    ) -> Self {
        Self {
            enabled,
            verbose,
            workers_api,
            session_id,
            live_session_id,
        }
    }

    pub fn verbose(&self) -> bool {
        self.verbose
    }

    pub fn info(&self, event: &'static str, fields: Value) {
        self.emit("info", event, None, fields);
    }

    pub fn warn(&self, event: &'static str, message: impl Into<String>, fields: Value) {
        self.emit("warn", event, Some(message.into()), fields);
    }

    pub fn debug(&self, event: &'static str, fields: Value) {
        if self.verbose {
            self.emit("debug", event, None, fields);
        }
    }

    fn emit(
        &self,
        level: &'static str,
        event: &'static str,
        message: Option<String>,
        fields: Value,
    ) {
        let Some(session_id) = self.session_id.clone() else {
            return;
        };
        if !self.enabled {
            return;
        }
        let payload = SessionLogPayload {
            events: vec![SessionLogEvent {
                session_id,
                live_session_id: self.live_session_id.clone(),
                source: "server-rs",
                level,
                event,
                message,
                fields: strip_secrets(fields),
                ts_ms: now_ms(),
            }],
        };
        let workers_api = self.workers_api.clone();
        tokio::spawn(async move {
            if let Err(error) = workers_api.report_session_logs(&payload).await {
                tracing::debug!(error = %error, "server-rs session log upload failed");
            }
        });
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

fn strip_secrets(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(strip_secret_map(map)),
        Value::Array(items) => Value::Array(items.into_iter().map(strip_secrets).collect()),
        other => other,
    }
}

fn strip_secret_map(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .map(|(key, value)| {
            let redacted = is_secret_key(&key);
            let next = if redacted {
                Value::String("[redacted]".to_string())
            } else {
                strip_secrets(value)
            };
            (key, next)
        })
        .collect()
}

fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    [
        "token",
        "secret",
        "key",
        "authorization",
        "password",
        "rtmp",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strip_secrets_redacts_nested_secret_fields() {
        let redacted = strip_secrets(json!({
            "stream_key": "abc",
            "stats": { "fps": 30, "token": "jwt" },
            "items": [{ "password": "pw" }]
        }));

        assert_eq!(redacted["stream_key"], "[redacted]");
        assert_eq!(redacted["stats"]["token"], "[redacted]");
        assert_eq!(redacted["items"][0]["password"], "[redacted]");
        assert_eq!(redacted["stats"]["fps"], 30);
    }
}
