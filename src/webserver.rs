/// Web server and HTTP API for the Rust Server Controller.
///
/// Provides routes for serving the web UI, static assets, and websocket upgrades.
/// Uses [`AppState`] for shared application state.
use crate::websocket::*;
use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use axum_extra::response::JavaScript;

use tower_http::services::ServeDir;
use tracing::*;

use crate::{
    app_state::AppState,
    configuration::{effective_certificate_targets, LocalCertificateConfig, WebTransportConfig},
};
use serde::Deserialize;

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
        .route("/themes", get(themes_list))
        .route("/themes/css", get(theme_css))
        .route("/plugins", get(plugins_list))
        .route("/plugins/{plugin_id}/{*asset}", get(plugin_asset))
        .route("/auth/status", get(crate::auth::auth_status))
        .route(
            "/auth/webauthn/settings",
            get(crate::auth::webauthn_settings),
        )
        .route("/auth/challenge", post(crate::auth::challenge))
        .route("/auth/login", post(crate::auth::login))
        .route("/auth/setup", post(crate::auth::setup))
        .route("/auth/set-password", post(crate::auth::set_password))
        .route("/auth/request-account", post(crate::auth::request_account))
        .route(
            "/auth/webauthn/register/start",
            post(crate::auth::start_webauthn_registration),
        )
        .route(
            "/auth/webauthn/register/finish",
            post(crate::auth::finish_webauthn_registration),
        )
        .route(
            "/auth/webauthn/authenticate/start",
            post(crate::auth::start_webauthn_authentication),
        )
        .route(
            "/auth/webauthn/authenticate/finish",
            post(crate::auth::finish_webauthn_authentication),
        )
        .route(
            "/auth/webauthn/credentials",
            get(crate::auth::list_webauthn_credentials),
        )
        .route(
            "/auth/webauthn/credentials/delete",
            post(crate::auth::delete_webauthn_credential),
        )
        .route("/auth/accounts", get(crate::auth::list_accounts))
        .route("/auth/admin/create-user", post(crate::auth::create_user))
        .route(
            "/auth/admin/approve-request",
            post(crate::auth::approve_account_request),
        )
        .route(
            "/auth/admin/reject-request",
            post(crate::auth::reject_account_request),
        )
        .route(
            "/auth/admin/update-user-permissions",
            post(crate::auth::update_user_permissions),
        )
        .route(
            "/auth/admin/update-permission-model",
            post(crate::auth::update_permission_model),
        )
        .route(
            "/auth/admin/reset-credential-stores",
            post(crate::auth::reset_credential_stores),
        )
        .route("/auth/logout", post(crate::auth::logout))
        .route("/oauth/token", post(crate::auth::oauth_token))
        .route("/ws", get(handle_ws_upgrade))
        .route("/favicon.ico", get(handle_icon));
    router
}

async fn load_plugin_catalog(state: &AppState) -> crate::controller_plugins::PluginCatalog {
    let plugins_folder = {
        let config = state.config.lock().await;
        config.plugins_folder.clone()
    };
    crate::controller_plugins::load_plugin_catalog(plugins_folder.as_deref()).await
}

async fn plugins_list(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if crate::auth::auth_session_from_headers(&headers, &state)
        .await
        .is_none()
    {
        return crate::auth::error_response(StatusCode::UNAUTHORIZED, "authentication required");
    }
    let catalog = load_plugin_catalog(&state).await;
    Json(crate::controller_plugins::public_catalog(&catalog)).into_response()
}

async fn plugin_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((plugin_id, asset)): AxumPath<(String, String)>,
) -> impl IntoResponse {
    if crate::auth::auth_session_from_headers(&headers, &state)
        .await
        .is_none()
    {
        return crate::auth::error_response(StatusCode::UNAUTHORIZED, "authentication required");
    }
    let catalog = load_plugin_catalog(&state).await;
    let Some((_plugin, path)) =
        crate::controller_plugins::find_declared_asset(&catalog, &plugin_id, &asset)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = if asset.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if asset.ends_with(".css") {
        "text/css; charset=utf-8"
    } else {
        "application/octet-stream"
    };
    match Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", content_type)
        .body(Body::from(bytes))
    {
        Ok(response) => response,
        Err(error) => {
            error!("Failed to build plugin asset response: {}", error);
            Response::new(Body::empty())
        }
    }
}

#[derive(Deserialize)]
struct ThemeCssQuery {
    theme_name: String,
}

async fn load_theme_collection(state: &AppState) -> crate::theme::ThemeCollection {
    let config = state.config.lock().await;
    let themes_folder = config
        .themes_folder
        .clone()
        .unwrap_or_else(|| "themes".to_string());
    drop(config);

    crate::theme::ThemeCollection::load_from_directory_async(&themes_folder)
        .await
        .unwrap_or_default()
}

async fn themes_list(State(state): State<AppState>) -> impl IntoResponse {
    let collection = load_theme_collection(&state).await;
    Json(crate::messages::ThemesList {
        r#type: "themesList".to_string(),
        themes: collection
            .themes
            .iter()
            .map(|theme| theme.name.clone())
            .collect(),
    })
}

async fn theme_css(
    State(state): State<AppState>,
    Query(query): Query<ThemeCssQuery>,
) -> impl IntoResponse {
    let collection = load_theme_collection(&state).await;
    let css = collection
        .themes
        .iter()
        .find(|theme| theme.name == query.theme_name)
        .or_else(|| collection.themes.first())
        .map(|theme| theme.to_css())
        .unwrap_or_default();
    Json(crate::messages::ThemeCSS {
        r#type: "themeCSS".to_string(),
        theme_name: query.theme_name,
        css,
    })
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

    if cert_targets.is_empty() {
        warn!("HTTP/3 requested, but no certificate_targets are configured; HTTP/3 server not started");
        return;
    }
    if !transport.acme.enabled {
        info!("HTTP/3 requested; enabling ACME for HTTP/3 certificate management");
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

pub(crate) async fn start_https_server(
    state: AppState,
    router: Router<AppState>,
    transport: WebTransportConfig,
    cert_targets: Vec<String>,
) {
    use axum_server::tls_rustls::RustlsConfig;
    use rustls_acme::{caches::DirCache, AcmeConfig};
    use std::sync::Arc;

    let address = match https_bind_address(&state, &transport).await {
        Some(address) => address,
        None => return,
    };

    if transport.acme.enabled {
        if cert_targets.is_empty() {
            warn!(
                "HTTPS requested, but no certificate_targets are configured; HTTPS server not started"
            );
            return;
        }

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
        return;
    }

    let certificate = if transport.local_certificate.enabled {
        Some(transport.local_certificate.clone())
    } else if transport.self_signed.enabled || !transport.acme.enabled {
        match ensure_self_signed_certificate(&transport, &cert_targets, address).await {
            Some(certificate) => Some(certificate),
            None => return,
        }
    } else {
        None
    };

    let Some(certificate) = certificate else {
        warn!("HTTPS requested, but no certificate mode is available; HTTPS server not started");
        return;
    };

    let Some(cert_path) = certificate
        .cert_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    else {
        warn!("HTTPS local certificate is enabled, but cert_path is missing");
        return;
    };
    let Some(key_path) = certificate
        .key_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    else {
        warn!("HTTPS local certificate is enabled, but key_path is missing");
        return;
    };

    let rustls_config = match RustlsConfig::from_pem_file(cert_path, key_path).await {
        Ok(config) => config,
        Err(error) => {
            error!(
                "Failed to load HTTPS certificate '{}' and key '{}': {}",
                cert_path, key_path, error
            );
            return;
        }
    };

    info!(
        "Starting HTTPS server on {} with certificate file '{}'",
        address, cert_path
    );
    log_certificate_fingerprint(cert_path).await;
    if let Err(error) = axum_server::bind_rustls(address, rustls_config)
        .serve(router.with_state(state).into_make_service())
        .await
    {
        error!("HTTPS server stopped with error: {}", error);
    }
}

async fn https_bind_address(
    state: &AppState,
    transport: &WebTransportConfig,
) -> Option<std::net::SocketAddr> {
    let config = state.config.lock().await;
    let port = transport
        .https_port
        .as_deref()
        .unwrap_or("443")
        .parse::<u16>()
        .unwrap_or(443);
    let bind_address = format!("{}:{}", config.interface, port);
    drop(config);

    match bind_address.parse::<std::net::SocketAddr>() {
        Ok(address) => Some(address),
        Err(error) => {
            error!("Invalid HTTPS bind address '{}': {}", bind_address, error);
            None
        }
    }
}

async fn ensure_self_signed_certificate(
    transport: &WebTransportConfig,
    cert_targets: &[String],
    bind_address: std::net::SocketAddr,
) -> Option<LocalCertificateConfig> {
    use rcgen::{generate_simple_self_signed, CertifiedKey};
    use std::path::Path;

    let cert_path = transport
        .self_signed
        .cert_path
        .clone()
        .unwrap_or_else(|| "controller_data/tls/self_signed_cert.pem".to_string());
    let key_path = transport
        .self_signed
        .key_path
        .clone()
        .unwrap_or_else(|| "controller_data/tls/self_signed_key.pem".to_string());

    if Path::new(&cert_path).exists() && Path::new(&key_path).exists() {
        return Some(LocalCertificateConfig {
            enabled: true,
            cert_path: Some(cert_path),
            key_path: Some(key_path),
        });
    }

    let names = self_signed_subject_alt_names(transport, cert_targets, bind_address);

    let CertifiedKey { cert, key_pair } = match generate_simple_self_signed(names.clone()) {
        Ok(certificate) => certificate,
        Err(error) => {
            error!(
                "Failed to generate self-signed HTTPS certificate for {}: {}",
                names.join(", "),
                error
            );
            return None;
        }
    };

    if let Some(parent) = Path::new(&cert_path).parent() {
        if let Err(error) = tokio::fs::create_dir_all(parent).await {
            error!(
                "Failed to create self-signed certificate directory '{}': {}",
                parent.display(),
                error
            );
            return None;
        }
    }
    if let Some(parent) = Path::new(&key_path).parent() {
        if let Err(error) = tokio::fs::create_dir_all(parent).await {
            error!(
                "Failed to create self-signed key directory '{}': {}",
                parent.display(),
                error
            );
            return None;
        }
    }

    if let Err(error) = tokio::fs::write(&cert_path, cert.pem()).await {
        error!(
            "Failed to write self-signed certificate '{}': {}",
            cert_path, error
        );
        return None;
    }
    if let Err(error) = tokio::fs::write(&key_path, key_pair.serialize_pem()).await {
        error!("Failed to write self-signed key '{}': {}", key_path, error);
        return None;
    }

    info!(
        "Generated self-signed HTTPS certificate '{}' for {}",
        cert_path,
        names.join(", ")
    );
    Some(LocalCertificateConfig {
        enabled: true,
        cert_path: Some(cert_path),
        key_path: Some(key_path),
    })
}

fn self_signed_subject_alt_names(
    transport: &WebTransportConfig,
    cert_targets: &[String],
    bind_address: std::net::SocketAddr,
) -> Vec<String> {
    let mut names: Vec<String> = transport
        .self_signed
        .subject_alt_names
        .iter()
        .chain(cert_targets.iter())
        .map(|name| name.trim().trim_end_matches('.').to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();

    names.extend([
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ]);
    if !bind_address.ip().is_unspecified() {
        names.push(bind_address.ip().to_string());
    }

    names.sort();
    names.dedup();
    names
}

async fn log_certificate_fingerprint(cert_path: &str) {
    let Ok(cert_pem) = tokio::fs::read_to_string(cert_path).await else {
        return;
    };
    let Ok(cert) = pem::parse(cert_pem) else {
        return;
    };
    let digest = ring::digest::digest(&ring::digest::SHA256, cert.contents());
    let fingerprint = digest
        .as_ref()
        .iter()
        .map(|byte| format!("{:02X}", byte))
        .collect::<Vec<_>>()
        .join(":");
    info!(
        "HTTPS certificate SHA-256 fingerprint for pinning: {}",
        fingerprint
    );
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
