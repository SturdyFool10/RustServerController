use axum::{routing::get, Router};
use tracing::info;

use crate::{app_state::AppState, websocket::handle_ws_upgrade};

async fn get_router(_state: AppState) -> Router<AppState> {
    let router: Router<AppState> = Router::new().route("/ws", get(handle_ws_upgrade));
    router
}

pub async fn start_slave(_state: AppState) {
    let router = get_router(_state.clone()).await; //.route("/ws", get(handle_socket))
    let config = _state.config.lock().await;
    let mut address = config.interface.clone();
    address += (":".to_owned() + config.port.clone().as_str()).as_str();
    drop(config);
    info!("Starting server on {}", address.replace("0.0.0.0", "*"));

    let stateful_router = router.with_state(_state);
    use axum::serve;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(&address).await.unwrap();
    serve(listener, stateful_router.into_make_service())
        .await
        .unwrap();
}
