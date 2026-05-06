/// Websocket upgrade and message handling for the Rust Server Controller.
///
/// Provides websocket upgrade, message processing, and helpers for communication
/// between the web UI and the backend using [`AppState`].
use crate::{
    servers::broadcast_json,
    servers::{create_instance, send_termination_message},
    websocket_protocol::handle_client_request,
};
use axum::{
    extract::{
        ws::{Message, Utf8Bytes, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
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
pub async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    // println!("Handling a socket...");
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handles a websocket connection, spawning send and receive tasks.
///
/// # Arguments
/// * `socket` - The websocket connection.
/// * `state` - The shared application state.

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (sender, mut reciever) = socket.split();
    let sender = Arc::new(Mutex::new(sender));
    let mut rx = state.tx.subscribe();

    // Send task: send MessagePack binary for all except config (which is JSON/text)
    let send_task_handle = {
        let sender = sender.clone();
        async move {
            while let Ok(val) = rx.recv().await {
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
        async move {
            while let Some(msg) = reciever.next().await {
                let state = state.clone();
                let sender = sender.clone();
                let mut handled = false;
                match msg {
                    Ok(Message::Text(text)) => {
                        let text_str = utf8bytes_to_string(text);
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text_str) {
                            if let Some(ev_type) = json.get("type").and_then(|v| v.as_str()) {
                                handled = handle_client_request(
                                    &sender, &state, ev_type, &text_str, false,
                                )
                                .await;
                            }
                        }
                        if !handled {
                            tokio::spawn(process_message(text_str, state.clone()));
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
                                            &sender, &state, ev_type, &decoded, true,
                                        )
                                        .await;
                                    }
                                }
                            }
                        }
                        if !handled {
                            if let Ok(decoded) = rmp_serde::from_slice::<serde_json::Value>(&bin) {
                                if let Ok(decoded_str) = serde_json::to_string(&decoded) {
                                    tokio::spawn(process_message(decoded_str, state.clone()));
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

/// Passes stdin input to a running server process.
///
/// # Arguments
/// * `message` - The stdin input message.
/// * `server_name` - The name of the server to send input to.
/// * `state` - The shared application state.
async fn pass_stdin(message: StdinInput, server_name: String, state: AppState) {
    let value = message.value + "\r\n";
    let mut servers = state.servers.lock().await;
    for server in servers.iter_mut() {
        if server.name == server_name {
            let Some(stdi) = server.process.stdin.as_mut() else {
                error!("Server '{}' has no stdin pipe", server_name);
                break;
            };
            if let Err(error) = stdi.write_all(value.as_bytes()).await {
                error!("Error passing command to server: {}", error);
            }
            break;
        }
    }
    drop(servers);
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

async fn start_inactive_server(server_name: &str, state: &AppState) {
    let config = state.config.lock().await;
    let desc = config
        .servers
        .iter()
        .find(|server_desc| server_desc.name == server_name)
        .cloned();
    drop(config);

    if let Some(desc) = desc {
        let mut servers = state.servers.lock().await;
        let Some(instance) = create_instance(state, desc) else {
            return;
        };
        let specialized_info = if let Some(handler) = instance.specialization_handler.as_ref() {
            handler.get_status()
        } else {
            instance
                .specialized_server_info
                .clone()
                .unwrap_or(serde_json::Value::Null)
        };
        let update = ServerSpecializationInfoUpdate {
            r#type: "ServerSpecializationInfoUpdate".to_string(),
            server_name: instance.name.clone(),
            info: specialized_info,
            specialization: instance.specialized_server_type.clone().unwrap_or_default(),
            active: true,
        };
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
    let update = ServerSpecializationInfoUpdate {
        r#type: "ServerSpecializationInfoUpdate".to_string(),
        server_name: server.name.clone(),
        info: serde_json::Value::Null,
        specialization,
        active: false,
    };
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

async fn start_server_action(text: &str, state: AppState) {
    let Some(message) = parse_server_action(text, &state) else {
        return;
    };
    start_inactive_server(&message.server_name, &state).await;
}

async fn kill_server_action(text: &str, state: AppState) {
    let Some(message) = parse_server_action(text, &state) else {
        return;
    };
    stop_active_server(&message.server_name, &state).await;
}

async fn restart_server_action(text: &str, state: AppState) {
    let Some(message) = parse_server_action(text, &state) else {
        return;
    };
    stop_active_server(&message.server_name, &state).await;
    start_inactive_server(&message.server_name, &state).await;
}

async fn handle_stdin_input(text: &str, state: AppState) {
    let value: StdinInput = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            error!("Error parsing stdinInput message: {}", error);
            return;
        }
    };

    let server_name = value.server_name.clone();
    let servers = state.servers.lock().await;
    let is_active_server = servers.iter().any(|server| server.name == server_name);
    drop(servers);

    if is_active_server {
        tokio::spawn(pass_stdin(value, server_name, state));
    } else if value.value == "start" {
        start_inactive_server(&value.server_name, &state).await;
    }
}

async fn apply_config_change(text: &str, state: AppState) {
    let message: ConfigChangeMessage = match serde_json::from_str(text) {
        Ok(msg) => msg,
        Err(_) => {
            let _ = state
                .tx
                .send("Error parsing configChange message".to_string());
            return;
        }
    };

    let mut servers = state.servers.lock().await;
    let mut config = state.config.lock().await;

    for server in servers.iter_mut() {
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
        let update = ServerSpecializationInfoUpdate {
            r#type: "ServerSpecializationInfoUpdate".to_string(),
            server_name: server.name.clone(),
            info: serde_json::Value::Null,
            specialization: server.specialized_server_type.clone().unwrap_or_default(),
            active: false,
        };
        broadcast_json(&state, &update);
    }
    servers.clear();

    config.change(message.updated_config);
    config.update_config_file("config.json");

    for desc in config.servers.iter_mut().filter(|desc| desc.auto_start) {
        if let Some(instance) = create_instance(&state, desc.clone()) {
            servers.push(instance);
        }
    }

    let config_info = ConfigInfo {
        r#type: "ConfigInfo".to_owned(),
        config: config.clone(),
    };
    broadcast_json(&state, &config_info);
}

async fn terminate_servers(state: AppState) {
    let mut servers = state.servers.lock().await;
    for server in servers.iter_mut() {
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
        let update = ServerSpecializationInfoUpdate {
            r#type: "ServerSpecializationInfoUpdate".to_string(),
            server_name: server.name.clone(),
            info: serde_json::Value::Null,
            specialization: server.specialized_server_type.clone().unwrap_or_default(),
            active: false,
        };
        broadcast_json(&state, &update);
    }
    servers.clear();
}

/// Processes a message received from the web client over websocket.
///
/// Handles requests for config, themes, server info, stdin input, config changes, and server termination.
///
/// # Arguments
/// * `text` - The received message as a string.
/// * `state` - The shared application state.
async fn process_message(text: String, state: AppState) {
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
        "stdinInput" => handle_stdin_input(&text, state).await,
        "startServer" => start_server_action(&text, state).await,
        "killServer" => kill_server_action(&text, state).await,
        "restartServer" => restart_server_action(&text, state).await,
        "configChange" => apply_config_change(&text, state).await,
        "getConfig" => {}
        "terminateServers" => terminate_servers(state).await,
        _ => {}
    }
}
