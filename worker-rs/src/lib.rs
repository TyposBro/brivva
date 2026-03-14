use worker::*;

mod types;
mod room;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    Router::new()
        .get("/live", |_req, _ctx| {
            Response::ok(r#"{"status":"ok"}"#)
        })
        .get_async("/api/room", room::routes::handle_room)
        .options("/api/room", |_req, _ctx| {
            let headers = Headers::new();
            let _ = headers.set("Access-Control-Allow-Origin", "*");
            let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
            let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Upgrade");
            Ok(Response::empty()?.with_headers(headers))
        })
        .run(req, env)
        .await
}
