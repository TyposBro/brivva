pub mod core;
pub mod features;
pub mod orchestration;
pub mod shared;

pub use features::broadcast::data::BroadcastState as AppState;
pub use orchestration::router::build_app;
pub use orchestration::state::broadcast_state as app_state;
