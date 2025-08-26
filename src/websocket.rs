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
    configuration::Config, messages::*, AppState::AppState,
    ControlledProgram::ControlledProgramDescriptor,
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

async fn pass_stdin(message: stdinInput, server_name: String, state: AppState) {
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
    if ev_type != "requestInfo" {
        info!("Message: {}", text.clone());
    }
    match ev_type {
        "requestInfo" => {
            let servers = state.servers.lock().await;
            let val: SInfoRequestMessage = serde_json::from_str(&text).unwrap();

            let config = state.config.lock().await;
            let slave = config.slave.clone();
            let mut info = serverInfoMessage {
                r#type: "ServerInfo".to_owned(),
                servers: vec![],
                config: config.clone(),
            };
            drop(config);
            let mut usedNames: Vec<String> = vec![];
            for server in servers.iter() {
                usedNames.push(server.name.clone());
                let mut sInfo = ServerInfo {
                    name: server.name.clone(),
                    output: "".to_owned(),
                    active: true,
                    specialization: server.specialized_server_type.clone(),
                    specializedInfo: server.specialized_server_info.clone(),
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
                    sInfo.output = split[inp..split.len()].join("\n");
                }
                info.servers.push(sInfo);
            }
            drop(servers);
            let slave_servers = state.slave_servers.lock().await;
            for serverInfo in slave_servers.iter() {
                let mut output: String = "".to_owned();
                if val.arguments[0] == true {
                    output = serverInfo.output.clone();
                }
                let newInfo = ServerInfo {
                    name: serverInfo.name.clone(),
                    output: output,
                    host: None,
                    active: serverInfo.active.clone(),
                    specialization: serverInfo.specialization.clone(),
                    specializedInfo: serverInfo.specializedInfo.clone(),
                };
                info.servers.push(newInfo);
            }
            let config = state.config.lock().await;
            for serverConfig in config.servers.iter() {
                if !usedNames.contains(&serverConfig.name) {
                    info.servers.push(ServerInfo {
                        name: serverConfig.name.clone(),
                        output: "".to_owned(),
                        active: false,
                        specialization: serverConfig.specialized_server_type.clone(),
                        specializedInfo: serverConfig.specialized_server_info.clone(),
                        host: None,
                    })
                }
            }
            drop(config);
            let _ = state.tx.send(serde_json::to_string(&info).unwrap());
        }
        "stdinInput" => {
            let value: Result<stdinInput, _> = serde_json::from_str(text.clone().as_str());
            match value {
                Ok(value) => {
                    let serverName = value.server_name.clone();
                    let mut servers = state.servers.lock().await;
                    let mut isActiveServer = false;
                    let mut serverFound = false;
                    for server in servers.iter_mut() {
                        if server.name == serverName && serverFound == false {
                            isActiveServer = true;
                            serverFound = true;
                            tokio::spawn(pass_stdin(
                                value.clone(),
                                server.name.clone(),
                                state.clone(),
                            ));
                        }
                    }
                    drop(servers);
                    let config = state.config.lock().await;
                    let slave = config.slave.clone();
                    drop(config);
                    let mut slaveServers = state.slave_connections.lock().await;
                    for slave in slaveServers.iter_mut() {
                        info!(
                            "Writing to slave: {}",
                            serde_json::to_string_pretty(slave).unwrap()
                        );
                        let res = slave
                            .write_stdin(value.server_name.clone(), value.value.clone())
                            .await;
                        if let Err(what) = res {
                            info!("there was an error writing: {}", what)
                        }
                    }
                    if isActiveServer != true && value.value == "start" {
                        let config = state.config.lock().await;
                        let mut desc: ControlledProgramDescriptor =
                            ControlledProgramDescriptor::new("", "", vec![], "".to_owned());
                        let mut found = false;
                        for serverDesc in config.servers.iter() {
                            if serverDesc.name == value.server_name {
                                desc = serverDesc.clone();
                                found = true;
                            }
                        }
                        if found {
                            let mut servers = state.servers.lock().await;
                            servers.push(desc.into_instance());
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
            let mut servers = state.servers.lock().await;
            for server in servers.iter_mut() {
                server.stop().await;
            }
        }
        "configChange" => {
            #[derive(Deserialize)]
            struct configChangeMessage {
                r#type: String,
                updatedConfig: Config,
            }
            let message: configChangeMessage = serde_json::from_str(text.clone().as_str()).unwrap();
            let mut servers = state.servers.lock().await;
            let mut config = state.config.lock().await;

            for server in servers.iter_mut() {
                server.stop().await
            }
            servers.clear();
            config.change(message.updatedConfig);
            config.update_config_file("config.json");
            for (index, desc) in config.servers.iter_mut().enumerate() {
                if desc.auto_start {
                    let descClone = desc.clone();
                    servers.push(descClone.into_instance());
                }
            }
            drop(config);
            drop(servers)
        }
        _ => {}
    }
}
