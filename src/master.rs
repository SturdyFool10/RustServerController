//! Multi-node support.
//!
//! A *master* controller declares every server in its own config; a server
//! with `"node": "<name>"` runs on the matching entry of `slave_connections`
//! instead of locally. The master connects out to each node (OAuth
//! client-credentials, then an authenticated websocket), pushes it the server
//! descriptors assigned to it, and mirrors the node's console output, stats
//! and status into its own state, so the web UI treats remote servers exactly
//! like local ones. Commands for a remote server are forwarded to its node.
use crate::{
    app_state::AppState,
    controlled_program::ControlledProgramDescriptor,
    messages::*,
    servers::broadcast_json,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, error::Error, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async, connect_async_tls_with_config,
    tungstenite::{client::IntoClientRequest, Message},
    Connector,
};
use tracing::{info, warn};

/// How many console lines a node's mirrored output keeps per server (matches
/// what a local server keeps).
const MIRRORED_OUTPUT_LINES: usize = 150;
/// How often the master re-requests a node's full snapshot (aggregate stats,
/// player activity) beyond the live updates the node pushes on its own.
const NODE_REFRESH_INTERVAL: Duration = Duration::from_secs(15);
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);

/// A remote node the master controls.
///
/// `client_id`/`client_secret` are an OAuth client configured on the node
/// (`auth.oauth_clients`, with the `admin` scope and the SHA-256 of the
/// secret as `client_secret_hash`).
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct SlaveConnectionDescriptor {
    /// Name servers use in their `node` field. Defaults to `address:port`.
    #[serde(default)]
    pub name: String,
    pub address: String,
    pub port: String,
    #[serde(default = "default_slave_use_https")]
    pub use_https: bool,
    #[serde(default)]
    pub pinned_certificate_sha256: Option<String>,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
}

fn default_slave_use_https() -> bool {
    true
}

impl SlaveConnectionDescriptor {
    pub fn node_name(&self) -> String {
        if self.name.trim().is_empty() {
            format!("{}:{}", self.address, self.port)
        } else {
            self.name.trim().to_string()
        }
    }

    /// A copy that's safe to show to web clients (no credentials).
    pub fn public(&self) -> Self {
        Self {
            client_id: String::new(),
            client_secret: String::new(),
            ..self.clone()
        }
    }
}

/// What the master currently knows about one node.
#[derive(Default)]
pub struct NodeState {
    pub connected: bool,
    pub last_error: Option<String>,
    /// The node's servers, as last reported / updated live.
    pub servers: Vec<ServerInfo>,
    pub archived_server_stats: Value,
    pub global_player_activity: Value,
    /// JSON text queued to be sent to the node.
    pub sender: Option<mpsc::UnboundedSender<String>>,
    pub descriptor: Option<SlaveConnectionDescriptor>,
}

pub type NodeRegistry = Arc<tokio::sync::Mutex<BTreeMap<String, NodeState>>>;

#[derive(Debug)]
struct PinnedCertificateVerifier {
    expected_sha256: String,
}

impl rustls::client::danger::ServerCertVerifier for PinnedCertificateVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let actual = certificate_sha256_hex(end_entity.as_ref());
        if actual == self.expected_sha256 {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::UnknownIssuer,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

fn certificate_sha256_hex(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{:02X}", byte))
        .collect::<String>()
}

fn normalize_fingerprint(fingerprint: &str) -> String {
    fingerprint
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .flat_map(|ch| ch.to_uppercase())
        .collect()
}

/// A rustls client config that trusts exactly the certificate with this
/// SHA-256 fingerprint, or `None` when no pin is configured.
fn pinned_client_config(
    pinned_certificate_sha256: Option<&str>,
) -> Result<Option<rustls::ClientConfig>, Box<dyn Error + Send + Sync>> {
    let Some(pin) = pinned_certificate_sha256
        .map(normalize_fingerprint)
        .filter(|pin| !pin.is_empty())
    else {
        return Ok(None);
    };
    if pin.len() != 64 {
        return Err("pinned_certificate_sha256 must be a SHA-256 hex fingerprint".into());
    }

    Ok(Some(
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedCertificateVerifier {
                expected_sha256: pin,
            }))
            .with_no_client_auth(),
    ))
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

/// Exchanges the node's OAuth client credentials for an access token.
async fn fetch_access_token(
    desc: &SlaveConnectionDescriptor,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let scheme = if desc.use_https { "https" } else { "http" };
    let url = format!("{}://{}:{}/oauth/token", scheme, desc.address, desc.port);
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(10));
    if desc.use_https {
        if let Some(config) = pinned_client_config(desc.pinned_certificate_sha256.as_deref())? {
            builder = builder.use_preconfigured_tls(config);
        }
    }
    let response = builder
        .build()?
        .post(url)
        .json(&json!({
            "grant_type": "client_credentials",
            "client_id": desc.client_id,
            "client_secret": desc.client_secret,
            "scope": "admin",
        }))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("token request rejected ({})", response.status()).into());
    }
    Ok(response.json::<TokenResponse>().await?.access_token)
}


async fn set_node_status(state: &AppState, name: &str, connected: bool, error: Option<String>) {
    let mut nodes = state.nodes.lock().await;
    let node = nodes.entry(name.to_string()).or_default();
    node.connected = connected;
    node.last_error = error;
    if !connected {
        node.sender = None;
    }
}

async fn broadcast_merged_info(state: &AppState) {
    let info = crate::websocket_protocol::build_server_info_message(state, true).await;
    broadcast_json(state, &info);
}

/// The message that tells a node which servers (and account filter groups) it
/// should be running: everything in the master's config assigned to it.
async fn node_sync_message(state: &AppState, node_name: &str) -> String {
    let config = state.config.lock().await;
    let servers: Vec<ControlledProgramDescriptor> = config
        .servers
        .iter()
        .filter(|server| server.node.as_deref() == Some(node_name))
        .cloned()
        .map(|mut server| {
            server.node = None;
            server
        })
        .collect();
    json!({
        "type": "nodeConfigSync",
        "servers": servers,
        "minecraft_account_filter_detail_groups": config.minecraft_account_filter_detail_groups,
    })
    .to_string()
}

/// Re-sends every connected node its server assignment. Call after the
/// master's config changes.
pub async fn sync_nodes(state: &AppState) {
    let names: Vec<String> = state.nodes.lock().await.keys().cloned().collect();
    for name in names {
        let message = node_sync_message(state, &name).await;
        if let Some(sender) = state
            .nodes
            .lock()
            .await
            .get(&name)
            .and_then(|node| node.sender.clone())
        {
            let _ = sender.send(message);
        }
    }
}

/// Spawns a connection task for every configured node (master mode only).
pub async fn start_nodes(state: AppState) {
    let descriptors = state.config.lock().await.slave_connections.clone();
    for desc in descriptors {
        let name = desc.node_name();
        {
            let mut nodes = state.nodes.lock().await;
            nodes.entry(name.clone()).or_default().descriptor = Some(desc.public());
        }
        tokio::spawn(run_node(state.clone(), desc, name));
    }
}

async fn run_node(state: AppState, desc: SlaveConnectionDescriptor, name: String) {
    let mut delay = Duration::from_secs(1);
    loop {
        let result = serve_node(&state, &desc, &name).await;
        let message = match &result {
            Ok(()) => "connection closed".to_string(),
            Err(error) => error.to_string(),
        };
        warn!("Node '{}' disconnected: {}", name, message);
        set_node_status(&state, &name, false, Some(message)).await;
        broadcast_merged_info(&state).await;
        if result.is_ok() {
            delay = Duration::from_secs(1);
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(MAX_RECONNECT_DELAY);
    }
}

async fn serve_node(
    state: &AppState,
    desc: &SlaveConnectionDescriptor,
    name: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let token = fetch_access_token(desc).await?;

    let scheme = if desc.use_https { "wss" } else { "ws" };
    let mut request = format!("{}://{}:{}/ws", scheme, desc.address, desc.port)
        .into_client_request()?;
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {}", token).parse()?);
    let connector = if desc.use_https {
        pinned_client_config(desc.pinned_certificate_sha256.as_deref())?
            .map(|config| Connector::Rustls(Arc::new(config)))
    } else {
        None
    };
    let (stream, _) = match connector {
        Some(connector) => {
            connect_async_tls_with_config(request, None, false, Some(connector)).await?
        }
        None => connect_async(request).await?,
    };
    let (mut write, mut read) = stream.split();

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    {
        let mut nodes = state.nodes.lock().await;
        let node = nodes.entry(name.to_string()).or_default();
        node.connected = true;
        node.last_error = None;
        node.sender = Some(tx.clone());
    }
    info!("Connected to node '{}'", name);

    // Tell the node what to run first, then ask for its snapshot.
    let _ = tx.send(node_sync_message(state, name).await);
    let _ = tx.send(json!({ "type": "requestInfo", "arguments": [true] }).to_string());

    let mut refresh = tokio::time::interval(NODE_REFRESH_INTERVAL);
    refresh.tick().await;
    loop {
        tokio::select! {
            incoming = read.next() => {
                let Some(incoming) = incoming else { return Ok(()); };
                match incoming? {
                    Message::Binary(bin) => {
                        if let Ok(value) = rmp_serde::from_slice::<Value>(&bin) {
                            handle_node_message(state, name, value).await;
                        }
                    }
                    Message::Text(text) => {
                        if let Ok(value) = serde_json::from_str::<Value>(&text) {
                            handle_node_message(state, name, value).await;
                        }
                    }
                    Message::Close(_) => return Ok(()),
                    _ => {}
                }
            }
            outgoing = rx.recv() => {
                let Some(outgoing) = outgoing else { return Ok(()); };
                write.send(Message::Text(outgoing.into())).await?;
            }
            _ = refresh.tick() => {
                // Console output already arrives live; only the aggregates need re-pulling.
                let _ = tx.send(json!({ "type": "requestInfo", "arguments": [false] }).to_string());
            }
        }
    }
}

fn same_server(info: &ServerInfo, uuid: Option<&str>, name: &str) -> bool {
    match (info.server_uuid.as_deref(), uuid) {
        (Some(left), Some(right)) => left == right,
        _ => info.name == name,
    }
}

fn append_output(existing: &mut String, new_lines: &str) {
    existing.push_str(new_lines);
    let lines: Vec<&str> = existing.split('\n').collect();
    if lines.len() > MIRRORED_OUTPUT_LINES {
        *existing = lines[lines.len() - MIRRORED_OUTPUT_LINES..].join("\n");
    }
}

async fn handle_node_message(state: &AppState, node_name: &str, value: Value) {
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        return;
    };
    match kind {
        "ServerInfo" => {
            let Ok(info) = serde_json::from_value::<ServerInfoMessage>(value) else {
                return;
            };
            {
                let mut nodes = state.nodes.lock().await;
                let node = nodes.entry(node_name.to_string()).or_default();
                let previous = std::mem::take(&mut node.servers);
                node.servers = info
                    .servers
                    .into_iter()
                    .map(|mut server| {
                        // A snapshot taken without output must not wipe the
                        // console the live stream has built up.
                        if server.output.is_empty() {
                            if let Some(old) = previous.iter().find(|old| {
                                same_server(old, server.server_uuid.as_deref(), &server.name)
                            }) {
                                server.output = old.output.clone();
                            }
                        }
                        server
                    })
                    .collect();
                node.archived_server_stats = info.archived_server_stats;
                node.global_player_activity = info.global_player_activity;
            }
            broadcast_merged_info(state).await;
        }
        "ServerOutput" => {
            let Ok(output) = serde_json::from_value::<ConsoleOutput>(value.clone()) else {
                return;
            };
            {
                let mut nodes = state.nodes.lock().await;
                if let Some(server) = nodes.get_mut(node_name).and_then(|node| {
                    node.servers.iter_mut().find(|s| {
                        same_server(s, output.server_uuid.as_deref(), &output.server_name)
                    })
                }) {
                    append_output(&mut server.output, &output.output);
                }
            }
            broadcast_json(state, &output);
        }
        "ServerSpecializationInfoUpdate" => {
            let Ok(update) = serde_json::from_value::<ServerSpecializationInfoUpdate>(value) else {
                return;
            };
            {
                let mut nodes = state.nodes.lock().await;
                if let Some(server) = nodes.get_mut(node_name).and_then(|node| {
                    node.servers.iter_mut().find(|s| {
                        same_server(s, update.server_uuid.as_deref(), &update.server_name)
                    })
                }) {
                    server.active = update.active;
                    server.specialized_info = Some(update.info.clone());
                    server.specialization_stats = update.stats.clone();
                    server.specialization_options = update.specialization_options.clone();
                }
            }
            broadcast_json(state, &update);
        }
        _ => {}
    }
}

/// The live info for a server the master's config assigns to a node, if that
/// node is connected and has reported it.
pub async fn remote_server_info(
    state: &AppState,
    descriptor: &ControlledProgramDescriptor,
) -> Option<ServerInfo> {
    let node_name = descriptor.node.as_deref()?;
    let nodes = state.nodes.lock().await;
    let node = nodes.get(node_name)?;
    if !node.connected {
        return None;
    }
    let mut info = node
        .servers
        .iter()
        .find(|server| same_server(server, descriptor.server_uuid.as_deref(), &descriptor.name))?
        .clone();
    info.host = node.descriptor.clone();
    Some(info)
}

/// Sends `text` (a client message) to the node that runs `server`.
/// Returns `true` when the server belongs to a node - whether or not the
/// message could be delivered - so callers skip local handling.
pub async fn forward_to_server_node(
    state: &AppState,
    server: &crate::servers::ServerRef,
    text: &str,
) -> bool {
    let Some(node_name) = server.node.as_deref() else {
        return false;
    };
    let sender = state
        .nodes
        .lock()
        .await
        .get(node_name)
        .and_then(|node| node.sender.clone());
    match sender {
        Some(sender) => {
            let _ = sender.send(text.to_string());
        }
        None => warn!(
            "Node '{}' is not connected; dropped message for server '{}'",
            node_name, server.name
        ),
    }
    true
}

/// Sends `text` to every connected node (e.g. terminate-all).
pub async fn forward_to_all_nodes(state: &AppState, text: &str) {
    let senders: Vec<_> = state
        .nodes
        .lock()
        .await
        .values()
        .filter_map(|node| node.sender.clone())
        .collect();
    for sender in senders {
        let _ = sender.send(text.to_string());
    }
}

/// Folds every node's archived stats and player activity into the master's,
/// so the stats page shows one network-wide view.
pub async fn merge_remote_aggregates(
    state: &AppState,
    archived: &mut Value,
    global: &mut Value,
) {
    let nodes = state.nodes.lock().await;
    let remote_archived: Vec<Value> = nodes
        .values()
        .filter(|node| node.connected)
        .map(|node| node.archived_server_stats.clone())
        .collect();
    let remote_global: Vec<Value> = nodes
        .values()
        .filter(|node| node.connected)
        .map(|node| node.global_player_activity.clone())
        .collect();
    drop(nodes);

    if !remote_archived.is_empty() {
        let mut items = match std::mem::take(archived) {
            Value::Array(items) => items,
            _ => Vec::new(),
        };
        for part in remote_archived {
            if let Value::Array(more) = part {
                items.extend(more);
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        items.retain(|item| {
            item.get("server_uuid")
                .and_then(Value::as_str)
                .is_none_or(|uuid| seen.insert(uuid.to_string()))
        });
        *archived = Value::Array(items);
    }
    if !remote_global.is_empty() {
        let mut parts = vec![std::mem::take(global)];
        parts.extend(remote_global);
        *global = merge_global_player_activity(parts, GLOBAL_PLAYERS_PER_GAME);
    }
}

/// Most players listed per game after merging nodes (same cap each node uses).
const GLOBAL_PLAYERS_PER_GAME: usize = 200;

/// Merges several nodes' network-wide player activity (`{players,
/// server_totals}`) into one. A player is one (game, name) pair no matter how
/// many nodes they were seen on: their per-server breakdowns are combined and
/// totals recomputed. Different games' same-named players stay separate.
pub fn merge_global_player_activity(parts: Vec<Value>, per_game_limit: usize) -> Value {
    let mut players: BTreeMap<(String, String), Value> = BTreeMap::new();
    let mut totals: Vec<Value> = Vec::new();

    for part in parts {
        let (part_players, part_totals) = match part {
            Value::Object(mut map) => (
                map.remove("players"),
                map.remove("server_totals"),
            ),
            Value::Array(items) => (Some(Value::Array(items)), None),
            _ => (None, None),
        };
        if let Some(Value::Array(items)) = part_totals {
            totals.extend(items);
        }
        let Some(Value::Array(items)) = part_players else {
            continue;
        };
        for player in items {
            let key = (
                player
                    .get("specialization")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                player
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            );
            match players.get_mut(&key) {
                Some(existing) => {
                    let extra = player
                        .get("servers")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    if let Some(servers) = existing.get_mut("servers").and_then(Value::as_array_mut)
                    {
                        for server in extra {
                            let uuid = server.get("server_uuid").cloned();
                            if !servers.iter().any(|s| s.get("server_uuid") == uuid.as_ref()) {
                                servers.push(server);
                            }
                        }
                    }
                }
                None => {
                    players.insert(key, player);
                }
            }
        }
    }

    let mut merged: Vec<Value> = players
        .into_values()
        .map(|mut player| {
            let servers = player
                .get("servers")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let seconds: i64 = servers
                .iter()
                .filter_map(|server| server.get("total_seconds").and_then(Value::as_i64))
                .sum();
            let online = servers
                .iter()
                .any(|server| server.get("online").and_then(Value::as_bool) == Some(true));
            let last_joined = servers
                .iter()
                .filter_map(|server| server.get("last_joined_at").and_then(Value::as_str))
                .filter_map(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .max();
            player["total_seconds"] = json!(seconds);
            player["total_hours"] = json!((seconds as f64 / 3600.0 * 100.0).round() / 100.0);
            player["online"] = json!(online);
            player["last_joined_at"] = json!(last_joined.map(|value| value.to_rfc3339()));
            player
        })
        .collect();

    let sort_key = |player: &Value| {
        (
            player.get("online").and_then(Value::as_bool).unwrap_or(false),
            player
                .get("last_joined_at")
                .and_then(Value::as_str)
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok()),
        )
    };
    merged.sort_by(|left, right| sort_key(right).cmp(&sort_key(left)));
    let mut per_game: BTreeMap<String, usize> = BTreeMap::new();
    merged.retain(|player| {
        let game = player
            .get("specialization")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let count = per_game.entry(game).or_default();
        *count += 1;
        *count <= per_game_limit
    });

    json!({ "players": merged, "server_totals": totals })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::Config;

    #[test]
    fn merging_nodes_combines_one_player_but_keeps_games_separate() {
        let player = |game: &str, server: &str, seconds: i64, online: bool| {
            json!({
                "name": "Sturdy",
                "specialization": game,
                "servers": [{
                    "server_uuid": server,
                    "total_seconds": seconds,
                    "online": online,
                    "last_joined_at": "2026-10-18T12:00:00+00:00",
                }],
            })
        };
        let merged = merge_global_player_activity(
            vec![
                json!({
                    "players": [player("Minecraft", "mc-1", 3600, false), player("Hytale", "ht-1", 60, false)],
                    "server_totals": [{"server_uuid": "mc-1"}],
                }),
                json!({
                    "players": [player("Minecraft", "mc-2", 1800, true)],
                    "server_totals": [{"server_uuid": "mc-2"}],
                }),
            ],
            200,
        );
        let players = merged["players"].as_array().unwrap();
        assert_eq!(players.len(), 2, "one Minecraft player, one Hytale player");
        let minecraft = players
            .iter()
            .find(|p| p["specialization"] == "Minecraft")
            .unwrap();
        assert_eq!(minecraft["total_seconds"], 5400);
        assert_eq!(minecraft["online"], true);
        assert_eq!(minecraft["servers"].as_array().unwrap().len(), 2);
        let hytale = players.iter().find(|p| p["specialization"] == "Hytale").unwrap();
        assert_eq!(hytale["total_seconds"], 60);
        assert_eq!(merged["server_totals"].as_array().unwrap().len(), 2);
    }

    fn test_state() -> AppState {
        let mut config = Config::default();
        let mut local = ControlledProgramDescriptor::new("local", "x", vec![], ".".into());
        local.server_uuid = Some("uuid-local".into());
        let mut remote = ControlledProgramDescriptor::new("remote", "y", vec![], "/srv".into());
        remote.server_uuid = Some("uuid-remote".into());
        remote.node = Some("east".into());
        config.servers = vec![local, remote];
        let store = crate::credential_store::CredentialStore::open_for_test("master_nodes").unwrap();
        let (tx, _rx) = tokio::sync::broadcast::channel(8);
        AppState::new(tx, config, crate::specializations::init_builtin_registry(), store)
    }

    fn remote_info(active: bool) -> ServerInfo {
        ServerInfo {
            name: "remote".into(),
            server_uuid: Some("uuid-remote".into()),
            output: "hello\n".into(),
            active,
            host: None,
            specialization: Some("Hytale".into()),
            specialized_info: Some(json!({"player_count": 2})),
            specialization_options: None,
            specialization_stats: None,
        }
    }

    #[tokio::test]
    async fn node_sync_carries_only_that_nodes_servers_without_the_node_field() {
        let state = test_state();
        let message: Value = serde_json::from_str(&node_sync_message(&state, "east").await).unwrap();
        let servers = message["servers"].as_array().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0]["name"], "remote");
        assert!(servers[0].get("node").is_none());
        let none: Value = serde_json::from_str(&node_sync_message(&state, "west").await).unwrap();
        assert!(none["servers"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn messages_for_remote_servers_are_forwarded_and_local_ones_are_not() {
        let state = test_state();
        let (tx, mut rx) = mpsc::unbounded_channel();
        state.nodes.lock().await.entry("east".into()).or_default().sender = Some(tx);

        let remote = crate::servers::resolve_server(&state, Some("uuid-remote"), "ignored name")
            .await
            .unwrap();
        let local = crate::servers::resolve_server(&state, None, "local").await.unwrap();
        assert_eq!(remote.name, "remote", "the UUID wins over a stale/wrong name");
        assert!(forward_to_server_node(&state, &remote, "{\"type\":\"killServer\"}").await);
        assert_eq!(rx.recv().await.as_deref(), Some("{\"type\":\"killServer\"}"));
        assert!(!forward_to_server_node(&state, &local, "x").await);
        assert!(rx.try_recv().is_err());
        // A node that isn't connected still claims its servers: nothing runs locally.
        state.nodes.lock().await.get_mut("east").unwrap().sender = None;
        assert!(forward_to_server_node(&state, &remote, "y").await);
    }

    #[tokio::test]
    async fn remote_info_comes_from_the_mirror_only_while_connected() {
        let state = test_state();
        let descriptor = state.config.lock().await.servers[1].clone();
        assert!(remote_server_info(&state, &descriptor).await.is_none());

        {
            let mut nodes = state.nodes.lock().await;
            let node = nodes.entry("east".into()).or_default();
            node.connected = true;
            node.servers = vec![remote_info(true)];
        }
        let info = remote_server_info(&state, &descriptor).await.unwrap();
        assert!(info.active);
        assert_eq!(info.specialization.as_deref(), Some("Hytale"));

        // Live updates reach the mirror the same way the node pushes them.
        handle_node_message(
            &state,
            "east",
            json!({
                "type": "ServerOutput",
                "output": "more\n",
                "server_name": "renamed on the node",
                "server_uuid": "uuid-remote",
                "server_type": "Hytale",
            }),
        )
        .await;
        let info = remote_server_info(&state, &descriptor).await.unwrap();
        assert!(info.output.ends_with("more\n"));

        state.nodes.lock().await.get_mut("east").unwrap().connected = false;
        assert!(remote_server_info(&state, &descriptor).await.is_none());
    }

    #[test]
    fn merging_caps_players_per_game() {
        let players: Vec<Value> = (0..5)
            .map(|i| json!({"name": format!("p{i}"), "specialization": "Hytale", "servers": []}))
            .collect();
        let merged = merge_global_player_activity(vec![json!({"players": players})], 3);
        assert_eq!(merged["players"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn node_name_defaults_to_address_and_public_copy_drops_secret() {
        let desc = SlaveConnectionDescriptor {
            address: "10.0.0.5".into(),
            port: "8080".into(),
            client_id: "master".into(),
            client_secret: "hunter2".into(),
            ..Default::default()
        };
        assert_eq!(desc.node_name(), "10.0.0.5:8080");
        assert!(desc.public().client_secret.is_empty());
        assert!(desc.public().client_id.is_empty());
    }

    #[test]
    fn mirrored_output_keeps_only_recent_lines() {
        let mut output = String::new();
        for i in 0..400 {
            append_output(&mut output, &format!("line {i}\n"));
        }
        assert!(output.split('\n').count() <= MIRRORED_OUTPUT_LINES + 1);
        assert!(output.contains("line 399"));
        assert!(!output.contains("line 0\n"));
    }
}
