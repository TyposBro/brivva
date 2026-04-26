use crate::features::broadcast::data::{BroadcastState, session_ws_handler};
use axum::{Router, routing::get};
use tower_http::cors::{Any, CorsLayer};

pub fn build_app(state: BroadcastState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route(
            "/",
            get(|| async { "Brivva Translation Server (media-only)" }),
        )
        .route("/health", get(|| async { "ok" }))
        .route("/api/session", get(session_ws_handler))
        .route("/api/room", get(session_ws_handler))
        .layer(cors)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_route_returns_ok_literal() {
        let app = build_app(BroadcastState::new());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"ok");
    }

    #[tokio::test]
    async fn root_route_returns_media_only_banner() {
        let app = build_app(BroadcastState::new());
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("media-only"));
    }

    #[tokio::test]
    async fn unknown_path_yields_404() {
        let app = build_app(BroadcastState::new());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/does-not-exist")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
