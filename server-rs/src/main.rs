use server_rs::{
    app_state, build_app,
    features::broadcast::data::ffmpeg,
};

#[tokio::main]
async fn main() {
    // Kill any orphan FFmpeg processes from a previous crash.
    ffmpeg::kill_orphan_ffmpeg();

    let app = build_app(app_state());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    println!("Listening on http://localhost:3000");
    println!("WebSocket at ws://localhost:3000/api/session");
    println!("Compatibility WebSocket at ws://localhost:3000/api/room");
    axum::serve(listener, app).await.unwrap();
}
