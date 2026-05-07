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

macro_rules! js_asset {
    ($name:ident, $path:literal) => {
        async fn $name(State(_state): State<AppState>) -> JavaScript<String> {
            JavaScript::from(include_str!($path).to_owned())
        }
    };
}

js_asset!(msgpack_serve, "html_src/msgpack.min.js");
js_asset!(constants_serve, "html_src/constants.js");
js_asset!(core_serve, "html_src/core.js");
js_asset!(menu_serve, "html_src/menu.js");
js_asset!(server_ui_serve, "html_src/server_ui.js");
js_asset!(stats_serve, "html_src/stats.js");
js_asset!(themes_serve, "html_src/themes.js");
js_asset!(editor_serve, "html_src/editor.js");
js_asset!(webgl_background_serve, "html_src/webgl_background.js");
js_asset!(index_js_serve, "html_src/index.js");

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
        .route("/html/msgpack.min.js", get(msgpack_serve))
        .route("/html/constants.js", get(constants_serve))
        .route("/html/core.js", get(core_serve))
        .route("/html/menu.js", get(menu_serve))
        .route("/html/server_ui.js", get(server_ui_serve))
        .route("/html/stats.js", get(stats_serve))
        .route("/html/themes.js", get(themes_serve))
        .route("/html/editor.js", get(editor_serve))
        .route("/html/webgl_background.js", get(webgl_background_serve))
        .route("/html/index.js", get(index_js_serve))
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
    match Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "image/x-icon")
        .body(Body::from(ico_bytes))
    {
        Ok(response) => response,
        Err(error) => {
            error!("Failed to build favicon response: {}", error);
            Response::new(Body::empty())
        }
    }
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
    let listener = match TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            error!("Failed to bind web server to {}: {}", address, error);
            return;
        }
    };
    if let Err(error) = serve(listener, stateful_router.into_make_service()).await {
        error!("Web server stopped with error: {}", error);
    }
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
