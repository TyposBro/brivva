pub mod core;
pub mod features;
pub mod orchestration;
pub mod shared;

pub use orchestration::router::build_app;
pub use orchestration::state::{AppState, app_state};
