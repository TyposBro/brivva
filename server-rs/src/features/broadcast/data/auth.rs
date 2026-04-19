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

pub fn verify(token: &str, secret: &str) -> Result<Claims, String> {
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
        let err = verify("ignored", "").expect_err("verify should fail without secret");
        assert_eq!(err, "JWT_SECRET not configured");
    }

    #[test]
    fn verify_accepts_valid_token() {
        let claims = verify(
            &make_token("test-secret", JWT_ISSUER, JWT_AUDIENCE),
            "test-secret",
        )
        .expect("valid jwt should verify");

        assert_eq!(claims.sub, "user-123");
    }

    #[test]
    fn verify_rejects_wrong_audience() {
        let err = verify(
            &make_token("test-secret", JWT_ISSUER, "wrong-audience"),
            "test-secret",
        )
        .expect_err("verify should fail for wrong aud");

        assert!(err.contains("invalid jwt:"), "unexpected error: {err}");
    }

    #[test]
    fn verify_rejects_wrong_issuer() {
        let err = verify(
            &make_token("test-secret", "wrong-issuer", JWT_AUDIENCE),
            "test-secret",
        )
        .expect_err("verify should fail for wrong iss");
        assert!(err.contains("invalid jwt:"), "unexpected error: {err}");
    }

    #[test]
    fn verify_rejects_signature_mismatch() {
        let token = make_token("a-secret", JWT_ISSUER, JWT_AUDIENCE);
        let err = verify(&token, "different-secret")
            .expect_err("verify should fail when signature does not match");
        assert!(err.contains("invalid jwt:"), "unexpected error: {err}");
    }

    #[test]
    fn verify_rejects_garbage_token() {
        let err =
            verify("not.a.token", "test-secret").expect_err("verify should fail on garbage input");
        assert!(err.contains("invalid jwt:"), "unexpected error: {err}");
    }

    #[test]
    fn verify_rejects_expired_token() {
        let expired = encode(
            &Header::default(),
            &TestClaims {
                sub: "user-123",
                exp: 0,
                iss: JWT_ISSUER,
                aud: JWT_AUDIENCE,
            },
            &EncodingKey::from_secret(b"test-secret"),
        )
        .expect("encode expired token");
        let err = verify(&expired, "test-secret").expect_err("expired tokens must fail");
        assert!(err.contains("invalid jwt:"), "unexpected error: {err}");
    }
}
