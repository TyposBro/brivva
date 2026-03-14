use worker::*;

fn generate_room_id() -> String {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).unwrap_or(());
    let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    bytes
        .iter()
        .flat_map(|b| {
            vec![
                chars[(b >> 4) as usize % 36] as char,
                chars[(b & 0x0F) as usize % 36] as char,
            ]
        })
        .take(6)
        .collect()
}

/// Thin proxy — resolves roomId, forwards WS upgrade to the Durable Object.
pub async fn handle_room(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let url = req.url()?;
    let params: std::collections::HashMap<String, String> =
        url.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();

    let role = params.get("role").map(|s| s.as_str());

    let room_id = match role {
        Some("host") => generate_room_id(),
        Some("guest") => match params.get("roomId") {
            Some(id) if !id.is_empty() => id.clone(),
            _ => return Response::error("Missing roomId", 400),
        },
        Some(_) => return Response::error("Invalid role", 400),
        None => return Response::error("Missing role param", 400),
    };

    let namespace = ctx.env.durable_object("ROOMS")?;
    let stub = namespace.id_from_name(&room_id)?.get_stub()?;

    // Rebuild URL with roomId injected
    let mut forward_url = url.clone();
    forward_url.query_pairs_mut().append_pair("roomId", &room_id);

    // Forward the original request to the DO
    let forward_req = Request::new(forward_url.as_str(), Method::Get)?;
    stub.fetch_with_request(forward_req).await
}
