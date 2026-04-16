#[tokio::main]
async fn main() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("bind server");
    axum::serve(listener, brivva_server::app::app())
        .await
        .expect("run server");
}
