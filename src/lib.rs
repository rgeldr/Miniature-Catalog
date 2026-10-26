mod auth;
mod db;
mod desktop;
mod error;
pub mod prep;
mod routes;
mod state;
mod thumb;

#[cfg(test)]
mod api_tests;

pub use desktop::{
    app_data_dir, listen_for_ui, load_or_create_admin_token, load_settings, resolve_data_dir,
    spawn_ui, ApiHost, Settings, DEFAULT_API_BIND,
};
pub use routes::{router, ui_router};
pub use state::AppState;
