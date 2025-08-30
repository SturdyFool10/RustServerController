use crate::servers::format_exit_message;
use axum::{
    extract::{
        ws::{Message, Utf8Bytes, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tracing::*;

/// Converts a String to Utf8Bytes for axum WebSocket messages.
fn string_to_utf8bytes(s: String) -> Utf8Bytes {
    Utf8Bytes::from(s)
}

/// Converts Utf8Bytes to String for tungstenite compatibility.
fn utf8bytes_to_string(bytes: Utf8Bytes) -> String {
    bytes.to_string()
}

use crate::{
    app_state::AppState, configuration::Config, controlled_program::ControlledProgramDescriptor,
    master::SlaveConnection, messages::*, servers::send_termination_message,
    theme::ThemeCollection,
};
#[no_mangle]
pub async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    // println!("Handling a socket...");
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}
async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut reciever) = socket.split();
    let mut rx = state.tx.subscribe();

    // Send task: axum expects Utf8Bytes for Message::Text
    let send_task_handle = async move {
        while let Ok(val) = rx.recv().await {
            let _out = sender.send(Message::Text(string_to_utf8bytes(val))).await;
        }
    };
    let rc_state = state.clone();
    // Listen task: convert Utf8Bytes to String for logic
    let listen_task_handle = async move {
        let mut val = reciever.next().await;
        while let Some(Ok(Message::Text(text))) = val {
            let text_str = utf8bytes_to_string(text);
            tokio::spawn(process_message(text_str, rc_state.clone()));
            val = reciever.next().await;
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

async fn pass_stdin(message: StdinInput, server_name: String, state: AppState) {
    let value = message.value + "\r\n";
    let mut servers = state.servers.lock().await;
    for server in servers.iter_mut() {
        if server.name == server_name {
            let stdi = server.process.stdin.as_mut().unwrap();
            let res = stdi.write_all(value.as_bytes()).await;
            match res {
                Ok(_) => {}
                Err(error) => {
                    error!("Error passing command to server: {}", error);
                }
            }
            break;
        }
    }
    drop(servers);
}
async fn process_message(text: String, state: AppState) {
    // (No change to this function, but ensure that all .send(Message::Text(...)) in this file use Utf8Bytes::from(val) and all received Message::Text(text) are handled as Utf8Bytes and converted to String as needed.)
    // The main changes are in handle_socket above.
    // If you need to propagate Utf8Bytes usage deeper, do so in the master.rs file as well.
    // (Function body unchanged here.)
    let json: serde_json::Value = serde_json::from_str(&text.clone()).unwrap();
    let ev_type = match json["type"].as_str() {
        None => {
            let _ = state
                .tx
                .send("Error Parsing Event: Event.type was not a string!".to_owned());
            return;
        }
        Some(val) => val,
    };
    let mut _args: Vec<String> = vec![];
    let arr = json["arguments"].as_array();
    match arr {
        None => {}
        Some(values) => {
            _args = values
                .iter()
                .filter(|value| -> bool { value.as_str().is_some() })
                .map(|value| -> String { value.as_str().unwrap().to_owned() })
                .collect();
        }
    }
    // Handle web client requests for themes and server info
    match ev_type {
        "requestConfig" => {
            let config = state.config.lock().await;
            let config_info = ConfigInfo {
                r#type: "ConfigInfo".to_owned(),
                config: config.clone(),
            };
            let _ = state.tx.send(serde_json::to_string(&config_info).unwrap());
        }
        "getThemesList" => {
            // Load themes from the specified directory in config
            let config = state.config.lock().await;
            let themes_folder = config
                .themes_folder
                .clone()
                .unwrap_or_else(|| "themes".to_string());
            drop(config);

            let theme_collection = match ThemeCollection::load_from_directory(&themes_folder) {
                Ok(collection) => collection,
                Err(_) => ThemeCollection::default(),
            };

            let theme_names: Vec<String> = theme_collection
                .themes
                .iter()
                .map(|theme| theme.name.clone())
                .collect();

            let themes_list = ThemesList {
                r#type: "themesList".to_string(),
                themes: theme_names,
            };

            // Send the response to the web client
            let _ = state.tx.send(serde_json::to_string(&themes_list).unwrap());
        }
        "getThemeCSS" => {
            #[derive(Deserialize)]
            struct GetThemeCSSWeb {
                r#type: String,
                theme_name: String,
            }
            let message: GetThemeCSSWeb = match serde_json::from_str(&text) {
                Ok(msg) => msg,
                Err(_) => {
                    let _ = state
                        .tx
                        .send("Error parsing GetThemeCSS message".to_string());
                    return;
                }
            };

            let config = state.config.lock().await;
            let themes_folder = config
                .themes_folder
                .clone()
                .unwrap_or_else(|| "themes".to_string());
            drop(config);

            let theme_collection = match ThemeCollection::load_from_directory(&themes_folder) {
                Ok(collection) => collection,
                Err(_) => ThemeCollection::default(),
            };

            let css = if let Some(theme) = theme_collection
                .themes
                .iter()
                .find(|t| t.name == message.theme_name)
            {
                theme.to_css()
            } else {
                let default_theme = ThemeCollection::default();
                if let Some(theme) = default_theme.themes.first() {
                    theme.to_css()
                } else {
                    String::new()
                }
            };

            let theme_css = ThemeCSS {
                r#type: "themeCSS".to_string(),
                theme_name: message.theme_name,
                css,
            };

            let _ = state.tx.send(serde_json::to_string(&theme_css).unwrap());
        }
        "requestInfo" => {
            // Compose ServerInfoMessage for web client
            let servers = state.servers.lock().await;
            let val = if let Ok(v) = serde_json::from_str::<SInfoRequestMessage>(&text) {
                v
            } else {
                SInfoRequestMessage {
                    r#type: "requestInfo".to_owned(),
                    arguments: vec![true],
                }
            };

            let config = state.config.lock().await;
            let mut info = ServerInfoMessage {
                r#type: "ServerInfo".to_owned(),
                servers: vec![],
                config: config.clone(),
            };
            drop(config);
            let mut used_names: Vec<String> = vec![];
            for server in servers.iter() {
                used_names.push(server.name.clone());
                let mut s_info = ServerInfo {
                    name: server.name.clone(),
                    output: "".to_owned(),
                    active: true,
                    specialization: server.specialized_server_type.clone(),
                    specialized_info: server.specialized_server_info.clone(),
                    host: None,
                };
                if val.arguments.get(0).copied().unwrap_or(false) {
                    let cl: String = server.curr_output_in_progress.clone();
                    let split: Vec<&str> = cl.split("\n").collect();
                    let mut inp = split.len();
                    if inp < 150 {
                        inp = 0;
                    } else {
                        inp = inp - 150;
                    }
                    s_info.output = split[inp..split.len()].join("\n");
                }
                info.servers.push(s_info);
            }
            drop(servers);

            // Add inactive servers from config if not already present
            let config = state.config.lock().await;
            for server_config in config.servers.iter() {
                if !used_names.contains(&server_config.name) {
                    info.servers.push(ServerInfo {
                        name: server_config.name.clone(),
                        output: "".to_owned(),
                        active: false,
                        specialization: server_config.specialized_server_type.clone(),
                        specialized_info: server_config.specialized_server_info.clone(),
                        host: None,
                    })
                }
            }
            drop(config);

            let _ = state.tx.send(serde_json::to_string(&info).unwrap());
        }
        "stdinInput" => {
            // Allow starting servers and sending stdin from the web UI
            let value: Result<StdinInput, _> = serde_json::from_str(text.clone().as_str());
            match value {
                Ok(value) => {
                    let server_name = value.server_name.clone();
                    let mut servers = state.servers.lock().await;
                    let mut is_active_server = false;
                    let mut server_found = false;
                    for server in servers.iter_mut() {
                        if server.name == server_name && !server_found {
                            is_active_server = true;
                            server_found = true;
                            tokio::spawn(pass_stdin(
                                value.clone(),
                                server.name.clone(),
                                state.clone(),
                            ));
                        }
                    }
                    drop(servers);
                    let config = state.config.lock().await;
                    #[allow(unused)]
                    let slave = config.slave.clone();
                    drop(config);
                    // If not active, and value is "start", start the server
                    if !is_active_server && value.value == "start" {
                        let config = state.config.lock().await;
                        let mut desc: ControlledProgramDescriptor =
                            ControlledProgramDescriptor::new("", "", vec![], "".to_owned());
                        let mut found = false;
                        for server_desc in config.servers.iter() {
                            if server_desc.name == value.server_name {
                                desc = server_desc.clone();
                                found = true;
                            }
                        }
                        if found {
                            let mut servers = state.servers.lock().await;
                            servers.push(desc.into_instance(&state.specialization_registry));
                            drop(servers);
                        }
                    }
                }
                Err(e) => {
                    dbg!(e, text);
                }
            }
        }
        "configChange" => {
            #[derive(Deserialize)]
            struct ConfigChangeMessage {
                r#type: String,
                #[serde(alias = "updatedConfig", alias = "updated_config")]
                updated_config: Config,
            }
            let message: ConfigChangeMessage = match serde_json::from_str(&text) {
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

            // Stop all servers before changing config
            for server in servers.iter_mut() {
                let _ = server.stop().await;
            }
            servers.clear();
            config.change(message.updated_config);
            config.update_config_file("config.json");
            // Auto-start servers if needed
            for desc in config.servers.iter_mut() {
                if desc.auto_start {
                    let desc_clone = desc.clone();
                    servers.push(desc_clone.into_instance(&state.specialization_registry));
                }
            }
            // Broadcast ConfigInfo to all clients
            let config_info = ConfigInfo {
                r#type: "ConfigInfo".to_owned(),
                config: config.clone(),
            };
            let _ = state.tx.send(serde_json::to_string(&config_info).unwrap());
            drop(config);
            drop(servers);
        }
        "getConfig" => {
            let config = state.config.lock().await;
            let config_info = ConfigInfo {
                r#type: "ConfigInfo".to_owned(),
                config: config.clone(),
            };
            let _ = state.tx.send(serde_json::to_string(&config_info).unwrap());
        }
        "terminateServers" => {
            let mut servers = state.servers.lock().await;
            for server in servers.iter_mut() {
                let exit_code = server.stop().await;
                let msg = format_exit_message(
                    exit_code
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                );
                let server_output = serde_json::json!({
                    "type": "ServerOutput",
                    "server_name": server.name.clone(),
                    "output": msg,
                });
                let _ = state.tx.send(server_output.to_string());
            }
            servers.clear();
        }
        _ => {}
    }
}
