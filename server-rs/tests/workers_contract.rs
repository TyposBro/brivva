use serde_json::Value;

fn openapi() -> Value {
    let path = format!(
        "{}/../contracts/openapi/brivva-workers.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(path).expect("read workers openapi export");
    serde_json::from_str(&raw).expect("parse workers openapi export")
}

fn required_names(schema: &Value) -> Vec<&str> {
    schema["required"]
        .as_array()
        .expect("required array")
        .iter()
        .map(|value| value.as_str().expect("required string"))
        .collect()
}

#[test]
fn openapi_exposes_internal_session_routes_for_server_rs() {
    let spec = openapi();
    let paths = spec["paths"].as_object().expect("paths object");
    let internal = paths
        .get("/internal/sessions/{id}")
        .expect("internal session path");

    assert!(
        internal.get("get").is_some(),
        "missing GET /internal/sessions/{{id}}"
    );
    assert!(
        internal.get("patch").is_some(),
        "missing PATCH /internal/sessions/{{id}}"
    );
}

#[test]
fn internal_session_bundle_schema_contains_fields_server_rs_deserializes() {
    let spec = openapi();
    let schemas = spec["components"]["schemas"]
        .as_object()
        .expect("components.schemas object");

    let bundle = &schemas["InternalSessionBundle"];
    let bundle_required = required_names(bundle);
    assert!(bundle_required.contains(&"session"));
    assert!(bundle_required.contains(&"streams"));
    assert!(bundle_required.contains(&"voice"));

    let session = &schemas["Session"];
    let session_required = required_names(session);
    assert!(session_required.contains(&"id"));
    assert!(session_required.contains(&"user_id"));
    assert!(session_required.contains(&"source_lang"));
    assert!(session_required.contains(&"target_langs"));
    assert!(session_required.contains(&"live_session_id"));

    let stream = &schemas["Stream"];
    let stream_required = required_names(stream);
    assert!(stream_required.contains(&"id"));
    assert!(stream_required.contains(&"lang"));
    assert!(stream_required.contains(&"rtmp_url"));
    assert!(stream_required.contains(&"stream_key"));
    assert!(stream_required.contains(&"delay_ms"));
    assert!(stream_required.contains(&"host_gain"));

    let voice = &schemas["Voice"];
    let voice_required = required_names(voice);
    assert!(voice_required.contains(&"id"));
    assert!(voice_required.contains(&"elevenlabs_voice_id"));
    assert!(voice_required.contains(&"name"));
}

#[test]
fn internal_session_status_patch_schema_matches_server_rs_payload() {
    let spec = openapi();
    let schemas = spec["components"]["schemas"]
        .as_object()
        .expect("components.schemas object");

    let update = &schemas["InternalSessionStatusUpdate"];
    let required = required_names(update);
    assert_eq!(required, vec!["status"]);
    assert!(update["properties"]["status"].is_object());
    assert!(update["properties"]["live_session_id"].is_object());

    let patch = &spec["paths"]["/internal/sessions/{id}"]["patch"];
    let request_schema = &patch["requestBody"]["content"]["application/json"]["schema"]["$ref"];
    assert_eq!(
        request_schema.as_str(),
        Some("#/components/schemas/InternalSessionStatusUpdate")
    );
}
