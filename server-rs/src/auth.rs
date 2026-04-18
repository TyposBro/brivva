//! JWT verification for the Workers → Fargate auth bridge.
//!
//! Workers signs HS256 JWTs with `JWT_SECRET`. Fargate verifies them on WS
//! upgrade. Claims identify the user; session context (streams, voice id) is
//! fetched separately from the Workers /internal API so claims stay small.

use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::sync::LazyLock;

const JWT_ISSUER: &str = "brivva-api";
const JWT_AUDIENCE: &str = "brivva-fargate";

static JWT_SECRET: LazyLock<String> =
    LazyLock::new(|| std::env::var("JWT_SECRET").unwrap_or_default());

#[derive(Debug, Deserialize)]
pub struct Claims {
    pub sub: String, // user_id
    #[allow(dead_code)]
    pub exp: i64,
}

pub fn verify(token: &str) -> Result<Claims, String> {
    if JWT_SECRET.is_empty() {
        return Err("JWT_SECRET not configured".into());
    }
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[JWT_ISSUER]);
    validation.set_audience(&[JWT_AUDIENCE]);
    let key = DecodingKey::from_secret(JWT_SECRET.as_bytes());
    decode::<Claims>(token, &key, &validation)
        .map(|data| data.claims)
        .map_err(|e| format!("invalid jwt: {e}"))
}
