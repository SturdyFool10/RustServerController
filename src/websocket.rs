/// Websocket upgrade and message handling for the Rust Server Controller.
///
/// Provides websocket upgrade, message processing, and helpers for communication
/// between the web UI and the backend using [`AppState`].
use crate::{
    servers::broadcast_json,
    servers::{
        create_instance_async, send_termination_message, specialization_update, stats_if_present,
    },
    websocket_protocol::handle_client_request,
};
use axum::{
    extract::{
        ws::{Message, Utf8Bytes, WebSocket},
        State, WebSocketUpgrade,
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::*;

/// Converts a `String` to `Utf8Bytes` for axum WebSocket messages.
///
/// # Arguments
/// * `s` - The string to convert.
///
/// # Returns
/// * `Utf8Bytes` containing the UTF-8 encoded string.
fn string_to_utf8bytes(s: String) -> Utf8Bytes {
    Utf8Bytes::from(s)
}

/// Converts `Utf8Bytes` to `String` for tungstenite compatibility.
///
/// # Arguments
/// * `bytes` - The UTF-8 bytes to convert.
///
/// # Returns
/// * `String` decoded from the bytes.
fn utf8bytes_to_string(bytes: Utf8Bytes) -> String {
    bytes.to_string()
}

#[allow(unused_imports)]
use crate::master::SlaveConnection;
use crate::{app_state::AppState, configuration::Config, messages::*};
/// Handles websocket upgrade requests from the web client.
///
/// # Arguments
/// * `ws` - The websocket upgrade request.
/// * `state` - The shared application state.
///
/// # Returns
/// * `Response` that upgrades the connection to a websocket.
#[no_mangle]
pub async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    // println!("Handling a socket...");
    let Some(session) = crate::auth::auth_session_from_headers(&headers, &state).await else {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    };
    ws.on_upgrade(move |socket| handle_socket(socket, state, session))
}

/// Handles a websocket connection, spawning send and receive tasks.
///
/// # Arguments
/// * `socket` - The websocket connection.
/// * `state` - The shared application state.
async fn handle_socket(socket: WebSocket, state: AppState, session: crate::auth::AuthSession) {
    let (sender, mut reciever) = socket.split();
    let sender = Arc::new(Mutex::new(sender));
    let mut rx = state.tx.subscribe();

    // Send task: send MessagePack binary for all except config (which is JSON/text)
    let send_task_handle = {
        let sender = sender.clone();
        let state = state.clone();
        let session = session.clone();
        async move {
            while let Ok(val) = rx.recv().await {
                let Some(val) = filter_broadcast_message(&val, &state, Some(&session)).await else {
                    continue;
                };
                if val.trim_start().starts_with('{') && val.contains("\"type\":\"ConfigInfo\"") {
                    let _ = sender
                        .lock()
                        .await
                        .send(Message::Text(string_to_utf8bytes(val)))
                        .await;
                } else {
                    match serde_json::from_str::<serde_json::Value>(&val) {
                        Ok(json_val) => match rmp_serde::to_vec_named(&json_val) {
                            Ok(bin) => {
                                let _ = sender.lock().await.send(Message::Binary(bin.into())).await;
                            }
                            Err(_) => {
                                let _ = sender
                                    .lock()
                                    .await
                                    .send(Message::Text(string_to_utf8bytes(val)))
                                    .await;
                            }
                        },
                        Err(_) => {
                            let _ = sender
                                .lock()
                                .await
                                .send(Message::Text(string_to_utf8bytes(
                                    "Error parsing message".to_string(),
                                )))
                                .await;
                        }
                    }
                }
            }
        }
    };

    // Listen task: handle both MessagePack binary and JSON text
    let listen_task_handle = {
        let sender = sender.clone();
        let session = session.clone();
        async move {
            while let Some(msg) = reciever.next().await {
                let state = state.clone();
                let sender = sender.clone();
                let session = session.clone();
                let mut handled = false;
                match msg {
                    Ok(Message::Text(text)) => {
                        let text_str = utf8bytes_to_string(text);
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text_str) {
                            if let Some(ev_type) = json.get("type").and_then(|v| v.as_str()) {
                                handled = handle_client_request(
                                    &sender,
                                    &state,
                                    Some(&session),
                                    ev_type,
                                    &text_str,
                                    false,
                                )
                                .await;
                            }
                        }
                        if !handled {
                            tokio::spawn(process_message(text_str, state.clone(), Some(session)));
                        }
                    }
                    Ok(Message::Binary(bin)) => {
                        let mut handled = false;
                        if let Ok(val) = rmp_serde::from_slice::<serde_json::Value>(&bin) {
                            if let Ok(decoded) = serde_json::to_string(&val) {
                                if let Ok(json) =
                                    serde_json::from_str::<serde_json::Value>(&decoded)
                                {
                                    if let Some(ev_type) = json.get("type").and_then(|v| v.as_str())
                                    {
                                        handled = handle_client_request(
                                            &sender,
                                            &state,
                                            Some(&session),
                                            ev_type,
                                            &decoded,
                                            true,
                                        )
                                        .await;
                                    }
                                }
                            }
                        }
                        if !handled {
                            if let Ok(decoded) = rmp_serde::from_slice::<serde_json::Value>(&bin) {
                                if let Ok(decoded_str) = serde_json::to_string(&decoded) {
                                    tokio::spawn(process_message(
                                        decoded_str,
                                        state.clone(),
                                        Some(session),
                                    ));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    };

    let mut send_task = tokio::spawn(send_task_handle);
    let mut listen_task = tokio::spawn(listen_task_handle);
    tokio::select! {
        _ = (&mut send_task) => {
            listen_task.abort()
        },
        _ = (&mut listen_task) => {
            send_task.abort()
        },
    };
}

async fn filter_broadcast_message(
    message: &str,
    state: &AppState,
    session: Option<&crate::auth::AuthSession>,
) -> Option<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(message) else {
        return None;
    };
    let Some(event_type) = value.get("type").and_then(|value| value.as_str()) else {
        return None;
    };
    match event_type {
        "ServerInfo" => {
            let info = serde_json::from_value::<ServerInfoMessage>(value).ok()?;
            let filtered =
                crate::websocket_protocol::filter_server_info_message_for_session(info, session);
            serde_json::to_string(&filtered).ok()
        }
        "ConfigInfo" => {
            let info = serde_json::from_value::<ConfigInfo>(value).ok()?;
            let filtered = crate::websocket_protocol::filter_config_info_for_session(info, session);
            serde_json::to_string(&filtered).ok()
        }
        "ServerOutput" => {
            let output = serde_json::from_value::<ConsoleOutput>(value).ok()?;
            if crate::websocket_protocol::can_view_console_by_name(
                state,
                session,
                &output.server_name,
            )
            .await
            {
                serde_json::to_string(&output).ok()
            } else {
                None
            }
        }
        "ServerSpecializationInfoUpdate" => {
            let update = serde_json::from_value::<ServerSpecializationInfoUpdate>(value).ok()?;
            let filtered =
                crate::websocket_protocol::filter_server_update_for_session(update, session)?;
            serde_json::to_string(&filtered).ok()
        }
        _ => None,
    }
}

/// Passes stdin input to a running server process.
///
/// # Arguments
/// * `message` - The stdin input message.
/// * `server_name` - The name of the server to send input to.
/// * `state` - The shared application state.
async fn pass_stdin(message: StdinInput, server_name: String, state: AppState) {
    let value = message.value + "\r\n";
    let mut servers = state.servers.lock().await;
    let Some(index) = servers.iter().position(|server| server.name == server_name) else {
        return;
    };
    let mut server = servers.remove(index);
    drop(servers);

    if let Some(stdi) = server.process.stdin.as_mut() {
        if let Err(error) = stdi.write_all(value.as_bytes()).await {
            error!("Error passing command to server: {}", error);
        }
    } else {
        error!("Server '{}' has no stdin pipe", server_name);
    }

    let mut servers = state.servers.lock().await;
    servers.push(server);
}

#[derive(Deserialize)]
struct ConfigChangeMessage {
    #[serde(alias = "updatedConfig", alias = "updated_config")]
    updated_config: Config,
}

#[derive(Deserialize)]
struct ServerActionMessage {
    server_name: String,
}

#[derive(Deserialize)]
struct DeleteServerStatsMessage {
    server_uuid: String,
}

async fn start_inactive_server(server_name: &str, state: &AppState) {
    let config = state.config.lock().await;
    let desc = config
        .servers
        .iter()
        .find(|server_desc| server_desc.name == server_name)
        .cloned();
    drop(config);

    if let Some(desc) = desc {
        let Some(instance) = create_instance_async(state, desc).await else {
            return;
        };
        let (specialized_info, specialization_stats) =
            match instance.specialization_handler.as_ref() {
                Some(handler) => (handler.get_status(), stats_if_present(handler.get_stats())),
                None => (
                    instance
                        .specialized_server_info
                        .clone()
                        .unwrap_or(serde_json::Value::Null),
                    None,
                ),
            };
        let update = specialization_update(
            instance.name.clone(),
            Some(instance.server_uuid.clone()),
            specialized_info,
            specialization_stats,
            instance.specialization_options.clone(),
            instance.specialized_server_type.clone().unwrap_or_default(),
            true,
        );
        let mut servers = state.servers.lock().await;
        servers.push(instance);
        broadcast_json(state, &update);
    }
}

async fn stop_active_server(server_name: &str, state: &AppState) -> bool {
    let mut servers = state.servers.lock().await;
    let Some(index) = servers.iter().position(|server| server.name == server_name) else {
        return false;
    };

    let mut server = servers.remove(index);
    drop(servers);
    let specialization = server.specialized_server_type.clone().unwrap_or_default();
    let server_type = server.specialized_server_type.clone();
    let exit_code = server.stop().await;
    send_termination_message(
        state,
        server.name.clone(),
        exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        server_type,
    )
    .await;
    let update = specialization_update(
        server.name.clone(),
        Some(server.server_uuid.clone()),
        serde_json::Value::Null,
        None,
        server.specialization_options.clone(),
        specialization,
        false,
    );
    broadcast_json(state, &update);
    true
}

fn parse_server_action(text: &str, state: &AppState) -> Option<ServerActionMessage> {
    match serde_json::from_str(text) {
        Ok(message) => Some(message),
        Err(error) => {
            let _ = state
                .tx
                .send(format!("Error parsing server action message: {}", error));
            None
        }
    }
}

async fn start_server_action(
    text: &str,
    state: AppState,
    session: Option<&crate::auth::AuthSession>,
) {
    let Some(message) = parse_server_action(text, &state) else {
        return;
    };
    if !crate::websocket_protocol::can_access_server_by_name(
        &state,
        session,
        &message.server_name,
        crate::auth::PERMISSION_CONTROL,
    )
    .await
    {
        warn!(
            "Rejected unauthorized start for server '{}'",
            message.server_name
        );
        return;
    }
    start_inactive_server(&message.server_name, &state).await;
}

async fn kill_server_action(
    text: &str,
    state: AppState,
    session: Option<&crate::auth::AuthSession>,
) {
    let Some(message) = parse_server_action(text, &state) else {
        return;
    };
    if !crate::websocket_protocol::can_access_server_by_name(
        &state,
        session,
        &message.server_name,
        crate::auth::PERMISSION_CONTROL,
    )
    .await
    {
        warn!(
            "Rejected unauthorized stop for server '{}'",
            message.server_name
        );
        return;
    }
    stop_active_server(&message.server_name, &state).await;
}

async fn restart_server_action(
    text: &str,
    state: AppState,
    session: Option<&crate::auth::AuthSession>,
) {
    let Some(message) = parse_server_action(text, &state) else {
        return;
    };
    if !crate::websocket_protocol::can_access_server_by_name(
        &state,
        session,
        &message.server_name,
        crate::auth::PERMISSION_CONTROL,
    )
    .await
    {
        warn!(
            "Rejected unauthorized restart for server '{}'",
            message.server_name
        );
        return;
    }
    stop_active_server(&message.server_name, &state).await;
    start_inactive_server(&message.server_name, &state).await;
}

async fn delete_archived_server_stats(text: &str, state: AppState) {
    let message: DeleteServerStatsMessage = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(error) => {
            error!("Error parsing deleteArchivedServerStats message: {}", error);
            return;
        }
    };

    let active = {
        let config = state.config.lock().await;
        config
            .servers
            .iter()
            .any(|server| server.server_uuid.as_deref() == Some(message.server_uuid.as_str()))
    };
    if active {
        let _ = state
            .tx
            .send("Refusing to delete stats for a server still present in config".to_string());
        return;
    }

    if let Err(error) =
        crate::specializations::player_activity::delete_server_stats(&message.server_uuid)
    {
        error!(
            "Failed to delete archived server stats for '{}': {}",
            message.server_uuid, error
        );
        return;
    }

    let info = crate::websocket_protocol::build_server_info_message(&state, false).await;
    broadcast_json(&state, &info);
}

async fn handle_stdin_input(
    text: &str,
    state: AppState,
    session: Option<&crate::auth::AuthSession>,
) {
    let value: StdinInput = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            error!("Error parsing stdinInput message: {}", error);
            return;
        }
    };

    let server_name = value.server_name.clone();
    if !crate::websocket_protocol::can_access_server_by_name(
        &state,
        session,
        &server_name,
        crate::auth::PERMISSION_CONTROL,
    )
    .await
    {
        warn!("Rejected unauthorized stdin for server '{}'", server_name);
        return;
    }
    let servers = state.servers.lock().await;
    let is_active_server = servers.iter().any(|server| server.name == server_name);
    drop(servers);

    if is_active_server {
        tokio::spawn(pass_stdin(value, server_name, state));
    } else if value.value == "start" {
        start_inactive_server(&value.server_name, &state).await;
    }
}

async fn apply_config_change(
    text: &str,
    state: AppState,
    session: Option<&crate::auth::AuthSession>,
) {
    let message: ConfigChangeMessage = match serde_json::from_str(text) {
        Ok(msg) => msg,
        Err(_) => {
            let _ = state
                .tx
                .send("Error parsing configChange message".to_string());
            return;
        }
    };
    let global_config = has_permission(session, crate::auth::PERMISSION_CONFIG);
    let admin = has_permission(session, crate::auth::PERMISSION_ADMIN);
    if !global_config
        && !has_server_scoped_config_change(&state, &message.updated_config, session).await
    {
        warn!("Rejected unauthorized configChange message");
        return;
    }

    if !global_config {
        apply_scoped_config_change(message.updated_config, state, session).await;
        return;
    }

    let mut servers_to_stop = {
        let mut servers = state.servers.lock().await;
        std::mem::take(&mut *servers)
    };

    for server in servers_to_stop.iter_mut() {
        let exit_code = server.stop().await;
        send_termination_message(
            &state,
            server.name.clone(),
            exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            server.specialized_server_type.clone(),
        )
        .await;
        let update = specialization_update(
            server.name.clone(),
            Some(server.server_uuid.clone()),
            serde_json::Value::Null,
            None,
            server.specialization_options.clone(),
            server.specialized_server_type.clone().unwrap_or_default(),
            false,
        );
        broadcast_json(&state, &update);
    }

    let (config_info, auto_start_servers, config_snapshot) = {
        let mut config = state.config.lock().await;

        if global_config {
            let auth = config.auth.clone();
            config.change(message.updated_config);
            if !admin {
                config.auth = auth;
            }
        } else {
            apply_server_scoped_config_change(&mut config, message.updated_config, session);
        }
        crate::configuration::ensure_server_uuids(&mut config);
        crate::configuration::ensure_account_filter_group_uuids(&mut config);
        for server in &config.servers {
            if let Some(server_uuid) = server.server_uuid.as_deref() {
                crate::specializations::player_activity::migrate_server_name_to_uuid(
                    &server.name,
                    server_uuid,
                );
            }
        }
        crate::configuration::apply_specialization_option_defaults(
            &mut config,
            &state.specialization_registry,
        );

        let auto_start_servers = config
            .servers
            .iter()
            .filter(|desc| desc.auto_start)
            .cloned()
            .collect::<Vec<_>>();

        let config_snapshot = config.clone();
        let config_info = ConfigInfo {
            r#type: "ConfigInfo".to_owned(),
            config: config_snapshot.clone(),
        };
        (config_info, auto_start_servers, config_snapshot)
    };

    if let Err(error) = config_snapshot
        .update_config_file_async("config.json")
        .await
    {
        error!("Failed to write config.json: {}", error);
    }
    crate::specializations::minecraft::sync_configured_account_filters_async(&config_snapshot)
        .await;

    broadcast_json(&state, &config_info);

    for desc in auto_start_servers {
        if let Some(instance) = create_instance_async(&state, desc).await {
            let mut servers = state.servers.lock().await;
            servers.push(instance);
        }
    }
}

async fn apply_scoped_config_change(
    updated_config: Config,
    state: AppState,
    session: Option<&crate::auth::AuthSession>,
) {
    let (config_info, config_snapshot) = {
        let mut config = state.config.lock().await;
        apply_server_scoped_config_change(&mut config, updated_config, session);
        crate::configuration::ensure_server_uuids(&mut config);
        crate::configuration::apply_specialization_option_defaults(
            &mut config,
            &state.specialization_registry,
        );
        let config_snapshot = config.clone();
        let config_info = ConfigInfo {
            r#type: "ConfigInfo".to_owned(),
            config: config_snapshot.clone(),
        };
        (config_info, config_snapshot)
    };

    if let Err(error) = config_snapshot
        .update_config_file_async("config.json")
        .await
    {
        error!("Failed to write config.json: {}", error);
    }
    crate::specializations::minecraft::sync_configured_account_filters_async(&config_snapshot)
        .await;

    broadcast_json(&state, &config_info);
}

fn apply_server_scoped_config_change(
    config: &mut Config,
    updated_config: Config,
    session: Option<&crate::auth::AuthSession>,
) {
    let mut next_servers = Vec::new();
    for current in config.servers.iter() {
        let can_configure = session
            .map(|session| {
                crate::auth::permissions_include_server(
                    &session.permissions,
                    crate::auth::PERMISSION_CONFIG,
                    current.server_uuid.as_deref(),
                    &current.name,
                )
            })
            .unwrap_or(false);
        if !can_configure {
            next_servers.push(current.clone());
            continue;
        }
        if let Some(updated) = updated_config.servers.iter().find(|updated| {
            updated.server_uuid.is_some() && updated.server_uuid == current.server_uuid
                || updated.server_uuid.is_none() && updated.name == current.name
        }) {
            next_servers.push(updated.clone());
        }
    }
    config.servers = next_servers;
}

async fn has_server_scoped_config_change(
    state: &AppState,
    updated_config: &Config,
    session: Option<&crate::auth::AuthSession>,
) -> bool {
    let Some(session) = session else {
        return false;
    };
    let config = state.config.lock().await;
    config.servers.iter().any(|current| {
        let present_in_update = updated_config.servers.iter().any(|updated| {
            updated.server_uuid.is_some() && updated.server_uuid == current.server_uuid
                || updated.server_uuid.is_none() && updated.name == current.name
        });
        let deleted_in_update = !present_in_update;
        (present_in_update || deleted_in_update)
            && crate::auth::permissions_include_server(
                &session.permissions,
                crate::auth::PERMISSION_CONFIG,
                current.server_uuid.as_deref(),
                &current.name,
            )
    })
}

async fn terminate_servers(state: AppState) {
    let mut servers_to_stop = {
        let mut servers = state.servers.lock().await;
        std::mem::take(&mut *servers)
    };

    for server in servers_to_stop.iter_mut() {
        let exit_code = server.stop().await;
        send_termination_message(
            &state,
            server.name.clone(),
            exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            server.specialized_server_type.clone(),
        )
        .await;
        let update = specialization_update(
            server.name.clone(),
            Some(server.server_uuid.clone()),
            serde_json::Value::Null,
            None,
            server.specialization_options.clone(),
            server.specialized_server_type.clone().unwrap_or_default(),
            false,
        );
        broadcast_json(&state, &update);
    }
}

/// Processes a message received from the web client over websocket.
///
/// Handles requests for config, themes, server info, stdin input, config changes, and server termination.
///
/// # Arguments
/// * `text` - The received message as a string.
/// * `state` - The shared application state.
async fn process_message(text: String, state: AppState, session: Option<crate::auth::AuthSession>) {
    let json: serde_json::Value = match serde_json::from_str(&text) {
        Ok(json) => json,
        Err(error) => {
            error!("Error parsing websocket message: {}", error);
            return;
        }
    };
    let ev_type = match json["type"].as_str() {
        None => {
            let _ = state
                .tx
                .send("Error Parsing Event: Event.type was not a string!".to_owned());
            return;
        }
        Some(val) => val,
    };

    match ev_type {
        "requestConfig" => {}
        "stdinInput" => handle_stdin_input(&text, state, session.as_ref()).await,
        "startServer" => start_server_action(&text, state, session.as_ref()).await,
        "killServer" => kill_server_action(&text, state, session.as_ref()).await,
        "restartServer" => restart_server_action(&text, state, session.as_ref()).await,
        "configChange" => apply_config_change(&text, state, session.as_ref()).await,
        "deleteArchivedServerStats"
            if has_permission(session.as_ref(), crate::auth::PERMISSION_ADMIN) =>
        {
            delete_archived_server_stats(&text, state).await
        }
        "getConfig" => {}
        "terminateServers" if has_permission(session.as_ref(), crate::auth::PERMISSION_CONTROL) => {
            terminate_servers(state).await
        }
        "deleteArchivedServerStats" | "terminateServers" => {
            warn!("Rejected unauthorized websocket message type '{}'", ev_type);
        }
        _ => {}
    }
}

fn has_permission(session: Option<&crate::auth::AuthSession>, permission: &str) -> bool {
    session
        .map(|session| crate::auth::permissions_include(&session.permissions, permission))
        .unwrap_or(false)
}
