//! Send interim (partial) transcripts to the frontend host.

use crate::core::types::ServerMsg;

use super::state::SttContext;

pub(super) fn send_interim_to_host(ctx: &SttContext, transcript: &str) {
    tracing::debug!("[INTERIM] {}", transcript);
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.send_to_host(
            crate::features::broadcast::data::pipeline_helpers::to_ws(
                &ServerMsg::Interim { transcript: transcript.to_string() },
            ),
        );
    }
}
