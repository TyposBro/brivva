//! JWT verification for the Workers → Fargate auth bridge.
//!
//! Workers signs HS256 JWTs with `JWT_SECRET`. Fargate verifies them on WS
//! upgrade. Claims identify the user; session context (streams, voice id) is
//! fetched separately from the Workers /internal API so claims stay small.

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::Deserialize;

const JWT_ISSUER: &str = "brivva-api";
const JWT_AUDIENCE: &str = "brivva-fargate";

#[derive(Debug, Deserialize)]
pub struct Claims {
    pub sub: String, // user_id
    #[allow(dead_code)]
    pub exp: i64,
}

pub fn verify(token: &str) -> Result<Claims, String> {
    let secret = std::env::var("JWT_SECRET").unwrap_or_default();
    if secret.is_empty() {
        return Err("JWT_SECRET not configured".into());
    }
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[JWT_ISSUER]);
    validation.set_audience(&[JWT_AUDIENCE]);
    let key = DecodingKey::from_secret(secret.as_bytes());
    decode::<Claims>(token, &key, &validation)
        .map(|data| data.claims)
        .map_err(|e| format!("invalid jwt: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde::Serialize;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[derive(Serialize)]
    struct TestClaims<'a> {
        sub: &'a str,
        exp: i64,
        iss: &'a str,
        aud: &'a str,
    }

    fn make_token(secret: &str, iss: &str, aud: &str) -> String {
        encode(
            &Header::default(),
            &TestClaims {
                sub: "user-123",
                exp: 4_102_444_800,
                iss,
                aud,
            },
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("encode test jwt")
    }

    #[test]
    fn verify_rejects_when_secret_missing() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::remove_var("JWT_SECRET");
        }

        let err = verify("ignored").expect_err("verify should fail without env");
        assert_eq!(err, "JWT_SECRET not configured");
    }

    #[test]
    fn verify_accepts_valid_token() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("JWT_SECRET", "test-secret");
        }

        let claims = verify(&make_token("test-secret", JWT_ISSUER, JWT_AUDIENCE))
            .expect("valid jwt should verify");

        assert_eq!(claims.sub, "user-123");
    }

    #[test]
    fn verify_rejects_wrong_audience() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("JWT_SECRET", "test-secret");
        }

        let err = verify(&make_token("test-secret", JWT_ISSUER, "wrong-audience"))
            .expect_err("verify should fail for wrong aud");

        assert!(err.contains("invalid jwt:"), "unexpected error: {err}");
    }
}
