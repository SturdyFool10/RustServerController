use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use axum_extra::response::*;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tower_http::services::ServeDir;
use tracing::*;
use crate::websocket::*;

use crate::{
    configuration::Config, AppState::AppState, ControlledProgram::ControlledProgramDescriptor,
};

async fn js_serve(State(_state): State<AppState>) -> JavaScript<String> {
    JavaScript::from(include_str!("html_src/index.js").to_owned())
}

async fn get_router(_state: AppState) -> Router<AppState> {
    let router: Router<AppState> = Router::new()
        .nest_service("/html", ServeDir::new("html_src"))
        .route("/", get(main_serve))
        .route("/index.js", get(js_serve))
        .route("/ws", get(handle_ws_upgrade))
        .route("/favicon.ico", get(handle_icon));
    router
}
async fn handle_icon(State(state): State<AppState>) -> impl IntoResponse {
    let ico_bytes: &'static [u8] = include_bytes!("html_src/icon.ico");
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "image/x-icon")
        .body(Body::from(ico_bytes))
        .unwrap();

    response
}
#[no_mangle]
pub async fn start_web_server(_state: AppState) {
    let router = get_router(_state.clone()).await; //.route("/ws", get(handle_socket))
    let config = _state.config.lock().await;
    let mut address = config.interface.clone();
    address += (":".to_owned() + config.port.clone().as_str()).as_str();
    drop(config);
    info!("Starting server on {}", address.replace("0.0.0.0", "*"));

    let stateful_router = router.with_state(_state);
    axum::Server::bind(&address.parse().unwrap())
        .serve(stateful_router.into_make_service())
        .await
        .unwrap();
}
#[no_mangle]
async fn main_serve(State(_state): State<AppState>) -> Html<String> {
    Html::from(include_str!("html_src/index.html").to_owned().replace("styles!();", include_str!("html_src/style.css")))
}

