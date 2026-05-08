/// Slave node entry point and web server for the Rust Server Controller.
///
/// Provides the HTTP and websocket interface for slave nodes, allowing the master
/// to communicate and control servers running on this node.
use axum::{routing::get, Router};
use tracing::{error, info};

use crate::{
    app_state::AppState, configuration::effective_certificate_targets,
    webserver::start_https_server, websocket::handle_ws_upgrade,
};

/// Builds the Axum router for the slave node web server.
///
/// Registers the websocket route for communication with the master node.
///
/// # Arguments
/// * `_state` - The shared application state.
///
/// # Returns
/// * `Router<AppState>` with the websocket route registered.
async fn get_router(_state: AppState) -> Router<AppState> {
    let router: Router<AppState> = Router::new().route("/ws", get(handle_ws_upgrade));
    router
}

/// Starts the Axum web server for the slave node.
///
/// Binds to the configured address and serves the websocket API for master-slave communication.
///
/// # Arguments
/// * `_state` - The shared application state.
pub async fn start_slave(_state: AppState) {
    let router = get_router(_state.clone()).await; //.route("/ws", get(handle_socket))
    let config = _state.config.lock().await;
    let mut address = config.interface.clone();
    address += (":".to_owned() + config.port.clone().as_str()).as_str();
    let transport = config.web_transport.clone();
    let cert_targets = effective_certificate_targets(&config);
    drop(config);

    if transport.enable_https {
        let https_state = _state.clone();
        let https_router = router.clone();
        let https_transport = transport.clone();
        let https_targets = cert_targets.clone();
        tokio::spawn(async move {
            start_https_server(https_state, https_router, https_transport, https_targets).await;
        });
    }

    info!("Starting server on {}", address.replace("0.0.0.0", "*"));

    let stateful_router = router.with_state(_state);
    use axum::serve;
    use tokio::net::TcpListener;

    let listener = match TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            error!("Failed to bind slave server to {}: {}", address, error);
            return;
        }
    };
    if let Err(error) = serve(listener, stateful_router.into_make_service()).await {
        error!("Slave server stopped with error: {}", error);
    }
}
