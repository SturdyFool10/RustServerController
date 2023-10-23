use axum::{extract::{State, WebSocketUpgrade, ws::{WebSocket, Message}}, Router, routing::get, response::{IntoResponse, Response}, http::StatusCode, body::Body};
use axum_extra::response::*;
use futures_util::{StreamExt, SinkExt};
use serde::{Serialize, Deserialize};
use tokio::io::AsyncWriteExt;
use tower_http::services::ServeDir;
use tracing::*;

use crate::{AppState::AppState, ControlledProgram::ControlledProgramDescriptor, configuration::Config};


async fn js_serve(State(_state): State<AppState>) -> JavaScript<String> {
    JavaScript::from(include_str!("html_src/index.js").to_owned())
}

async fn get_router(_state: AppState) -> Router<AppState> {
    let router: Router<AppState> = Router::new()
        .nest_service("/html", ServeDir::new("html_src"))
        .route("/", get(main_serve))
        .route("/index.js", get(js_serve))
        .route("/ws", get(handle_ws_upgrade))
        .route("/favicon.ico", get(handle_icon));
    router
}
async fn handle_icon(State(state): State<AppState>) -> impl IntoResponse {
    let ico_bytes: &'static [u8] = include_bytes!("html_src/icon.ico");
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "image/x-icon")
        .body(Body::from(ico_bytes))
        .unwrap();

    response
}
#[no_mangle]
pub async fn start_web_server(_state: AppState) {
    let router = get_router(_state.clone()).await; //.route("/ws", get(handle_socket))
    let config = _state.config.lock().await;
    let mut address = config.interface.clone();
    address += (":".to_owned() + config.port.clone().as_str()).as_str();
    drop(config);
    info!("Starting server on {}", address.replace("0.0.0.0", "*"));
    
    let stateful_router = router.with_state(_state);
    axum::Server::bind(&address.parse().unwrap())
        .serve(stateful_router.into_make_service())
        .await
        .unwrap();
}
#[no_mangle]
async fn main_serve(State(_state): State<AppState>) -> Html<String> {
    Html::from(include_str!("html_src/index.html").to_owned())
}

#[no_mangle]
async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
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

#[derive(Serialize, Deserialize, Clone, Debug)]
struct stdinInput {
    r#type: String,
    server_name: String,
    value: String,
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
        "requestInfo" => {
            let servers = state.servers.lock().await;
            
            #[derive(Clone, Serialize)]
            struct ServerInfo {
                name: String,
                output: String,
                active: bool,
            }

            #[derive(Clone, Serialize)]
            struct serverInfoMessage {
                r#type: String,
                servers: Vec<ServerInfo>,
                config: crate::configuration::Config,
            }
            #[derive(Debug, Deserialize)]
            struct SInfoRequestMessage {
                r#type: String,
                arguments: Vec<bool>,
            }
            let val: SInfoRequestMessage = serde_json::from_str(&text).unwrap();

            let config = state.config.lock().await;

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
                };
                if (val.arguments[0] == true) {
                    let cl: String =  server.currOutputInProgress.clone();
                    let split: Vec<&str> = cl.split("\n").into_iter().collect();
                    let mut inp = split.len();
                    if (inp < 150) {
                        inp = 0;
                    } else {
                        inp = inp - 150;
                    }
                    sInfo.output = split[inp..split.len()].join("\n");
                }
                info.servers.push(sInfo);
            }
            drop(servers);
            let config = state.config.lock().await;
            for serverConfig in config.servers.iter() {
                if !usedNames.contains(&serverConfig.name) {
                    info.servers.push(ServerInfo {
                        name: serverConfig.name.clone(),
                        output: "".to_owned(),
                        active: false,
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
                    for server in servers.iter_mut() {
                        if server.name == serverName {
                            isActiveServer = true;
                            tokio::spawn(pass_stdin(
                                value.clone(),
                                server.name.clone(),
                                state.clone(),
                            ));
                        }
                    }
                    drop(servers);
                    if (isActiveServer != true && value.value == "start") {
                        let config = state.config.lock().await;
                        let mut desc: ControlledProgramDescriptor = ControlledProgramDescriptor::new(
                            "",
                            "",
                            vec![],
                            "".to_owned()
                        );
                        let mut found = false;
                        for serverDesc in config.servers.iter() {
                            if (serverDesc.name == value.server_name) {
                                desc = serverDesc.clone();
                                found = true;
                            }
                        }
                        if (found) {
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
        },
        "configChange" => {
            #[derive(Deserialize)]
            struct configChangeMessage {
                r#type: String,
                updatedConfig: Config
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
                if (desc.autoStart) {
                    let mut descClone = desc.clone();
                    servers.push(descClone.into_instance());
                }
            }
            drop(config);
            drop(servers)
        }
        _ => {}
    }
}
