/// Web server and HTTP API for the Rust Server Controller.
///
/// Provides routes for serving the web UI, static assets, and websocket upgrades.
/// Uses [`AppState`] for shared application state.
use crate::websocket::*;
use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use axum_extra::response::JavaScript;

use tower_http::services::ServeDir;
use tracing::*;

use crate::app_state::AppState;

/// Serves the main JavaScript file for the web UI.
///
/// # Arguments
/// * `_state` - The shared application state (unused).
async fn js_serve(State(_state): State<AppState>) -> JavaScript<String> {
    JavaScript::from(include_str!("html_src/index.js").to_owned())
}

/// Builds the main Axum router for the web server.
///
/// Registers routes for the web UI, static assets, websocket, and favicon.
///
/// # Arguments
/// * `_state` - The shared application state.
///
/// # Returns
/// * `Router<AppState>` with all routes registered.
async fn get_router(_state: AppState) -> Router<AppState> {
    let router: Router<AppState> = Router::new()
        .nest_service("/html", ServeDir::new("html_src"))
        .route("/", get(main_serve))
        .route("/index.js", get(js_serve))
        .route("/msgpack.min.js", get(msgpack_serve))
        .route("/ws", get(handle_ws_upgrade))
        .route("/favicon.ico", get(handle_icon));
    router
}
/// Serves the favicon for the web UI.
///
/// # Arguments
/// * `_state` - The shared application state (unused).
async fn handle_icon(State(_state): State<AppState>) -> impl IntoResponse {
    let ico_bytes: &'static [u8] = include_bytes!("html_src/icon.ico");
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "image/x-icon")
        .body(Body::from(ico_bytes))
        .unwrap()
}

/// Serves the msgpack.min.js file for the web UI.
async fn msgpack_serve(State(_state): State<AppState>) -> Response {
    let js_bytes: &'static [u8] = include_bytes!("html_src/msgpack.min.js");
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/javascript")
        .body(Body::from(js_bytes))
        .unwrap()
}
/// Starts the Axum web server for the controller.
///
/// Binds to the configured address and serves the web UI and API.
///
/// # Arguments
/// * `_state` - The shared application state.
#[no_mangle]
pub async fn start_web_server(_state: AppState) {
    use axum::serve;
    use tokio::net::TcpListener;

    let router = get_router(_state.clone()).await;
    let config = _state.config.lock().await;
    let mut address = config.interface.clone();
    address += (":".to_owned() + config.port.clone().as_str()).as_str();
    drop(config);
    info!("Starting server on {}", address.replace("0.0.0.0", "*"));

    let stateful_router = router.with_state(_state);
    let listener = TcpListener::bind(&address).await.unwrap();
    serve(listener, stateful_router.into_make_service())
        .await
        .unwrap();
}
/// Serves the main HTML page for the web UI, inlining the CSS.
///
/// # Arguments
/// * `_state` - The shared application state (unused).
#[no_mangle]
async fn main_serve(State(_state): State<AppState>) -> Html<String> {
    Html(
        include_str!("html_src/index.html")
            .to_owned()
            .replace("styles!();", include_str!("html_src/style.css")),
    )
}
