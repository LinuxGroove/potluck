//! Potluck's share service and web market, as a library so the tests can
//! run it whole: see main.rs for the binary.

pub mod api;
pub mod avatar;
pub mod checks;
pub mod codes;
pub mod config;
pub mod db;
pub mod devices;
pub mod forgejo;
pub mod highlight;
pub mod identity;
pub mod market;
pub mod servers;
pub mod store;
pub mod web;

#[cfg(test)]
mod tests;

/// Every route: the game's API, files by hash, and the web pages.
pub fn router(app: std::sync::Arc<market::App>) -> axum::Router {
    api::routes().merge(web::routes()).layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024)).with_state(app)
}

/// Serves the market on a listener until the process stops (the game's
/// tests run one this way).
pub async fn serve(listener: tokio::net::TcpListener, app: std::sync::Arc<market::App>) -> std::io::Result<()> {
    axum::serve(listener, router(app)).await
}
