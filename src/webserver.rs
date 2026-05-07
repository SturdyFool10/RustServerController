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

use crate::{
    app_state::AppState,
    configuration::{effective_certificate_targets, WebTransportConfig},
};

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
js_asset!(admin_serve, "html_src/admin.js");
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
        .route("/html/admin.js", get(admin_serve))
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
    use tokio::net::TcpListener;

    let router = get_router(_state.clone()).await;
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

    if transport.enable_http3 {
        let http3_state = _state.clone();
        let http3_router = router.clone();
        let http3_transport = transport.clone();
        let http3_targets = cert_targets.clone();
        tokio::spawn(async move {
            start_http3_server(http3_state, http3_router, http3_transport, http3_targets).await;
        });
    }

    info!("Starting server on {}", address.replace("0.0.0.0", "*"));

    let stateful_router = router.with_state(_state);
    let listener = match TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            error!("Failed to bind web server to {}: {}", address, error);
            return;
        }
    };
    if let Err(error) = axum::serve(listener, stateful_router.into_make_service()).await {
        error!("Web server stopped with error: {}", error);
    }
}

async fn start_http3_server(
    state: AppState,
    router: Router<AppState>,
    transport: WebTransportConfig,
    cert_targets: Vec<String>,
) {
    use futures_util::StreamExt;
    use quinn::crypto::rustls::QuicServerConfig;
    use rustls_acme::{caches::DirCache, rustls, AcmeConfig};
    use std::{net::SocketAddr, sync::Arc};

    if !transport.acme.enabled {
        warn!("HTTP/3 requested, but ACME is disabled; HTTP/3 server not started");
        return;
    }
    if cert_targets.is_empty() {
        warn!("HTTP/3 requested, but no certificate_targets are configured; HTTP/3 server not started");
        return;
    }

    let config = state.config.lock().await;
    let port = transport
        .http3_port
        .as_deref()
        .or(transport.https_port.as_deref())
        .unwrap_or("443")
        .parse::<u16>()
        .unwrap_or(443);
    let bind_address = format!("{}:{}", config.interface, port);
    drop(config);

    let address: SocketAddr = match bind_address.parse() {
        Ok(address) => address,
        Err(error) => {
            error!("Invalid HTTP/3 bind address '{}': {}", bind_address, error);
            return;
        }
    };

    let mut acme = AcmeConfig::new(cert_targets.iter().map(String::as_str))
        .cache(DirCache::new(
            transport
                .acme
                .cache_dir
                .clone()
                .unwrap_or_else(|| "controller_data/acme".to_string()),
        ))
        .directory_lets_encrypt(transport.acme.production);
    if let Some(contact_email) = transport.acme.contact_email.as_deref() {
        if !contact_email.trim().is_empty() {
            acme = acme.contact_push(format!("mailto:{}", contact_email.trim()));
        }
    }

    let mut acme_state = acme.state();
    let resolver = acme_state.resolver();
    tokio::spawn(async move {
        while let Some(event) = acme_state.next().await {
            match event {
                Ok(event) => debug!("ACME HTTP/3 event: {:?}", event),
                Err(error) => warn!("ACME HTTP/3 event error: {:?}", error),
            }
        }
    });

    let mut rustls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    rustls_config.alpn_protocols = vec![b"h3".to_vec()];

    let quic_config = match QuicServerConfig::try_from(rustls_config) {
        Ok(config) => config,
        Err(error) => {
            error!("Failed to build HTTP/3 QUIC TLS config: {}", error);
            return;
        }
    };
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_config));
    let endpoint = match quinn::Endpoint::server(server_config, address) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            error!("Failed to bind HTTP/3 endpoint on {}: {}", address, error);
            return;
        }
    };

    info!(
        "Starting HTTP/3 server on {} with certificate targets: {}",
        address,
        cert_targets.join(", ")
    );
    while let Some(connecting) = endpoint.accept().await {
        let app = router.clone().with_state(state.clone());
        tokio::spawn(async move {
            let connection = match connecting.await {
                Ok(connection) => connection,
                Err(error) => {
                    debug!("HTTP/3 connection failed: {}", error);
                    return;
                }
            };
            let h3_connection = h3_quinn::Connection::new(connection);
            let mut h3_connection = match h3::server::Connection::new(h3_connection).await {
                Ok(connection) => connection,
                Err(error) => {
                    debug!("HTTP/3 handshake failed: {:?}", error);
                    return;
                }
            };

            loop {
                match h3_connection.accept().await {
                    Ok(Some(resolver)) => {
                        let app = app.clone();
                        tokio::spawn(async move {
                            if let Err(error) = h3_axum::serve_h3_with_axum(app, resolver).await {
                                debug!("HTTP/3 request failed: {}", error);
                            }
                        });
                    }
                    Ok(None) => break,
                    Err(error) => {
                        if !h3_axum::is_graceful_h3_close(&error) {
                            debug!("HTTP/3 accept failed: {:?}", error);
                        }
                        break;
                    }
                }
            }
        });
    }
}

async fn start_https_server(
    state: AppState,
    router: Router<AppState>,
    transport: WebTransportConfig,
    cert_targets: Vec<String>,
) {
    use rustls_acme::{caches::DirCache, AcmeConfig};
    use std::{net::SocketAddr, sync::Arc};

    if !transport.acme.enabled {
        warn!("HTTPS requested, but ACME is disabled; HTTPS server not started");
        return;
    }
    if cert_targets.is_empty() {
        warn!(
            "HTTPS requested, but no certificate_targets are configured; HTTPS server not started"
        );
        return;
    }

    let config = state.config.lock().await;
    let port = transport
        .https_port
        .as_deref()
        .unwrap_or("443")
        .parse::<u16>()
        .unwrap_or(443);
    let bind_address = format!("{}:{}", config.interface, port);
    drop(config);

    let address: SocketAddr = match bind_address.parse() {
        Ok(address) => address,
        Err(error) => {
            error!("Invalid HTTPS bind address '{}': {}", bind_address, error);
            return;
        }
    };

    let mut acme = AcmeConfig::new(cert_targets.iter().map(String::as_str))
        .cache(DirCache::new(
            transport
                .acme
                .cache_dir
                .clone()
                .unwrap_or_else(|| "controller_data/acme".to_string()),
        ))
        .directory_lets_encrypt(transport.acme.production);
    if let Some(contact_email) = transport.acme.contact_email.as_deref() {
        if !contact_email.trim().is_empty() {
            acme = acme.contact_push(format!("mailto:{}", contact_email.trim()));
        }
    }
    let acme_state = acme.state();
    let rustls_config = acme_state.default_rustls_config();
    let acceptor = acme_state.axum_acceptor(Arc::clone(&rustls_config));

    info!(
        "Starting HTTPS server on {} with certificate targets: {}",
        address,
        cert_targets.join(", ")
    );
    if let Err(error) = axum_server::bind(address)
        .acceptor(acceptor)
        .serve(router.with_state(state).into_make_service())
        .await
    {
        error!("HTTPS server stopped with error: {}", error);
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
