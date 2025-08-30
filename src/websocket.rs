use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tracing::*;

use crate::{
    app_state::AppState, configuration::Config, controlled_program::ControlledProgramDescriptor,
    master::SlaveConnection, messages::*, servers::send_termination_message,
    theme::ThemeCollection,
};
#[no_mangle]
pub async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    println!("Handling a socket...");
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}
async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut reciever) = socket.split();
    let mut rx = state.tx.subscribe();

    let send_task_handle = async move {
        while let Ok(val) = rx.recv().await {
            let _out = sender.send(Message::Text(val)).await;
        }
    };
    let rc_state = state.clone();
    let listen_task_handle = async move {
        let mut val = reciever.next().await;
        while let Some(Ok(Message::Text(text))) = val {
            tokio::spawn(process_message(text, rc_state.clone()));
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
    match ev_type {
        "requestConfig" => {
            let config = state.config.lock().await;
            let config_info = crate::messages::ConfigInfo {
                r#type: "ConfigInfo".to_owned(),
                config: config.clone(),
            };
            let _ = state.tx.send(serde_json::to_string(&config_info).unwrap());
        }
        "requestInfo" => {
            let servers = state.servers.lock().await;
            let val: SInfoRequestMessage = serde_json::from_str(&text).unwrap();

            let config = state.config.lock().await;
            #[allow(unused)]
            let slave = config.slave.clone();
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
                if val.arguments[0] == true {
                    let cl: String = server.curr_output_in_progress.clone();
                    let split: Vec<&str> = cl.split("\n").into_iter().collect();
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
            let slave_servers = state.slave_servers.lock().await;
            for server_info in slave_servers.iter() {
                let mut output: String = "".to_owned();
                if val.arguments[0] == true {
                    output = server_info.output.clone();
                }
                let new_info = ServerInfo {
                    name: server_info.name.clone(),
                    output: output,
                    host: None,
                    active: server_info.active.clone(),
                    specialization: server_info.specialization.clone(),
                    specialized_info: server_info.specialized_info.clone(),
                };
                info.servers.push(new_info);
            }
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
            let value: Result<StdinInput, _> = serde_json::from_str(text.clone().as_str());
            match value {
                Ok(value) => {
                    let server_name = value.server_name.clone();
                    let mut servers = state.servers.lock().await;
                    let mut is_active_server = false;
                    let mut server_found = false;
                    for server in servers.iter_mut() {
                        if server.name == server_name && server_found == false {
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
                    let mut slave_servers = state.slave_connections.lock().await;
                    for slave in slave_servers.iter_mut() {
                        info!(
                            "Writing to slave: {}",
                            serde_json::to_string_pretty::<SlaveConnection>(slave).unwrap()
                        );
                        let res = slave
                            .write_stdin(value.server_name.clone(), value.value.clone())
                            .await;
                        if let Err(what) = res {
                            info!("there was an error writing: {}", what)
                        }
                    }
                    if is_active_server != true && value.value == "start" {
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
        "terminateServers" => {
            let global_cp_value = state
                .global_crash_prevention
                .load(std::sync::atomic::Ordering::SeqCst);
            let need_global_cp = global_cp_value.clone();
            if global_cp_value == true {
                state
                    .global_crash_prevention
                    .store(false, std::sync::atomic::Ordering::SeqCst);
            }
            let mut servers = state.servers.lock().await;
            // First set all servers to disable crash prevention

            // Then stop all servers
            for server in servers.iter_mut() {
                if let Some(exit_code) = server.stop().await {
                    // Send termination message to web console for each server
                    send_termination_message(
                        &state,
                        server.name.clone(),
                        exit_code,
                        server.specialized_server_type.clone(),
                    )
                    .await;
                }
            }

            if need_global_cp == true {
                state
                    .global_crash_prevention
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
            servers.clear();
            drop(servers);
        }
        "getThemesList" => {
            // Handle the request for themes list
            let config = state.config.lock().await;
            let themes_folder = config
                .themes_folder
                .clone()
                .unwrap_or_else(|| "themes".to_string());
            drop(config);

            // Load themes from the specified directory
            let theme_collection = match ThemeCollection::load_from_directory(&themes_folder) {
                Ok(collection) => collection,
                Err(_) => ThemeCollection::default(),
            };

            // Create the response message with just the list of theme names
            let theme_names: Vec<String> = theme_collection
                .themes
                .iter()
                .map(|theme| theme.name.clone())
                .collect();

            let themes_list = ThemesList {
                r#type: "themesList".to_string(),
                themes: theme_names,
            };

            // Send the response
            let _ = state.tx.send(serde_json::to_string(&themes_list).unwrap());
        }
        "getThemeCSS" => {
            // Parse the request to get the theme name
            let message: GetThemeCSS = match serde_json::from_str(&text) {
                Ok(msg) => msg,
                Err(_) => {
                    let _ = state
                        .tx
                        .send("Error parsing GetThemeCSS message".to_string());
                    return;
                }
            };

            // Get the themes directory from config
            let config = state.config.lock().await;
            let themes_folder = config
                .themes_folder
                .clone()
                .unwrap_or_else(|| "themes".to_string());
            drop(config);

            // Load theme collection
            let theme_collection = match ThemeCollection::load_from_directory(&themes_folder) {
                Ok(collection) => collection,
                Err(_) => ThemeCollection::default(),
            };

            // Find the requested theme
            let css = if let Some(theme) = theme_collection
                .themes
                .iter()
                .find(|t| t.name == message.theme_name)
            {
                theme.to_css()
            } else {
                // If requested theme wasn't found, return the default theme CSS
                let default_theme = ThemeCollection::default();
                if let Some(theme) = default_theme.themes.first() {
                    theme.to_css()
                } else {
                    // Highly unlikely but as a fallback
                    String::new()
                }
            };

            // Create the response
            let theme_css = ThemeCSS {
                r#type: "themeCSS".to_string(),
                theme_name: message.theme_name,
                css,
            };

            // Send the response
            let _ = state.tx.send(serde_json::to_string(&theme_css).unwrap());
        }
        "configChange" => {
            #[allow(unused)]
            #[derive(Deserialize)]
            struct ConfigChangeMessage {
                r#type: String,
                #[serde(alias = "updatedConfig", alias = "updated_config")]
                updated_config: Config,
            }
            let message: ConfigChangeMessage = serde_json::from_str(text.clone().as_str()).unwrap();
            let mut servers = state.servers.lock().await;
            let mut config = state.config.lock().await;

            for server in servers.iter_mut() {
                if let Some(exit_code) = server.stop().await {
                    // Send termination message to web console for each server
                    send_termination_message(
                        &state,
                        server.name.clone(),
                        exit_code,
                        server.specialized_server_type.clone(),
                    )
                    .await;
                }
            }
            servers.clear();
            config.change(message.updated_config);
            config.update_config_file("config.json");
            #[allow(unused)]
            for (index, desc) in config.servers.iter_mut().enumerate() {
                if desc.auto_start {
                    let desc_clone = desc.clone();
                    servers.push(desc_clone.into_instance(&state.specialization_registry));
                }
            }
            // Broadcast ConfigInfo to all clients
            let config_info = crate::messages::ConfigInfo {
                r#type: "ConfigInfo".to_owned(),
                config: config.clone(),
            };
            let _ = state.tx.send(serde_json::to_string(&config_info).unwrap());
            drop(config);
            drop(servers)
        }
        _ => {}
    }
}
