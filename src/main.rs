use std::{fs::File, io::Write, process::exit};
mod AppState;
mod ControlledProgram;
mod configuration;
use axum_extra::response::JavaScript;
use configuration::Config;

use axum::{
    extract::{ws::*, State, WebSocketUpgrade},
    response::{Html, Response},
    routing::get,
    Router,
};
use futures_util::stream::*;
use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use serde_json::Value as serdeValue;
use tokio::io::AsyncWriteExt;
use tokio::{spawn, sync::broadcast};
use tower_http::services::ServeDir;
use tracing::*;
use ControlledProgram::ControlledProgramDescriptor;
macro_rules! spawn_tasks {
    ($state:expr, $($task:expr),*) => {
        {
            let handles: Vec<_> = vec![
                $(
                    spawn($task($state.clone())),
                )*
            ];

            handles
        }
    };
}

#[macro_export]
macro_rules! async_listener {
    ($key:expr, $app:expr) => {{
        use crossterm::event::{poll, read, Event, KeyCode};
        use tokio::task::yield_now;

        // Create a future that waits for the key combination
        let key_future = async move {
            loop {
                yield_now().await;

                if poll(std::time::Duration::from_millis(25)).expect("Failed to poll for events") {
                    if let Event::Key(key_event) = read().expect("Failed to read event") {
                        if key_event.code == KeyCode::Char($key.chars().next().unwrap()) {
                            $app.stop();
                            break;
                        }
                    }
                }
            }
        };
        // Return the key combination future
        key_future
    }};
}
#[tokio::main]
async fn main() -> Result<(), String> {
    let config = load_json("config.json");
    tracing_subscriber::FmtSubscriber::builder()
        .pretty()
        .with_line_number(false)
        .with_file(false)
        .without_time()
        .init();
    let (tx, _rx) = broadcast::channel(100);
    let mut app_state = AppState::AppState::new(tx, config);
    let handles = spawn_tasks!(app_state.clone(), start_web_server, start_servers);
    {
        info!("Starting {} tasks", handles.len());
    }
    let _ = tokio::spawn(async_listener!("t", app_state)).await;
    info!("Termination key pressed, closing the app.");
    exit(0);
    Ok(())
}

async fn js_serve(State(_state): State<AppState::AppState>) -> JavaScript<String> {
    JavaScript::from(include_str!("html_src/index.js").to_owned())
}

async fn get_router(_state: AppState::AppState) -> Router<AppState::AppState> {
    let router: Router<AppState::AppState> = Router::new()
        .nest_service("/html", ServeDir::new("html_src"))
        .route("/", get(main_serve))
        .route("/index.js", get(js_serve))
        .route("/ws", get(handle_ws_upgrade));
    router
}

#[no_mangle]
async fn start_web_server(_state: AppState::AppState) {
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
async fn main_serve(State(_state): State<AppState::AppState>) -> Html<String> {
    Html::from(include_str!("html_src/index.html").to_owned())
}
#[no_mangle]
fn read_file(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let data = std::fs::read_to_string(path)?;
    Ok(data)
}
#[no_mangle]
fn load_json(path: &str) -> Config {
    let data = read_file(path);
    let data: String = match data {
        Ok(d) => d,
        Err(error) => {
            error!(error);
            info!("this is likely ok, trying to salvage from error above by creating a default configuration.");
            info!("this can happen if it is your first launch");
            let str = include_str!("defaultconfig.json").to_owned();
            let mut f = File::create(path)
                .expect(&format!("There was an error creating the file specified: {}", &path)[..]);
            f.write_all(str.as_bytes()).expect("Error Writing to File");
            str
        }
    };
    let json = serde_json::from_str(&data.clone());
    json.unwrap()
}
#[no_mangle]
async fn start_servers(_state: AppState::AppState) {
    let mut config = _state.config.lock().await;
    for serverDesc in config.servers.iter_mut() {
        if (serverDesc.autoStart) { 
            let newDesc = serverDesc.clone();
            let mut servers = _state.servers.lock().await;
            servers.push(newDesc.into_instance());
            drop(servers);
        }
    }
    tokio::spawn(process_stdout(_state.clone()));
}
#[no_mangle]
async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState::AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState::AppState) {
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
#[derive(Serialize, Deserialize, Clone, Debug)]
struct stdinInput {
    r#type: String,
    server_name: String,
    value: String,
}
async fn process_message(text: String, state: AppState::AppState) {
    let json: serdeValue = serde_json::from_str(&text.clone()).unwrap();
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
            }
            #[derive(Debug, Deserialize)]
            struct SInfoRequestMessage {
                r#type: String,
                arguments: Vec<bool>,
            }
            let val: SInfoRequestMessage = serde_json::from_str(&text).unwrap();

            let mut info = serverInfoMessage {
                r#type: "ServerInfo".to_owned(),
                servers: vec![],
            };
            let mut usedNames: Vec<String> = vec![];
            for server in servers.iter() {
                usedNames.push(server.name.clone());
                let mut sInfo = ServerInfo {
                    name: server.name.clone(),
                    output: "".to_owned(),
                    active: true,
                };
                if (val.arguments[0] == true) {
                    sInfo.output = server.currOutputInProgress.clone();
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
        }
        _ => {}
    }
}

async fn process_stdout(state: AppState::AppState) {
    loop {
        {
            let mut new_instances = vec![];
            let mut to_remove = vec![];
            let mut servers = state.servers.lock().await;
            for (index, server) in servers.iter_mut().enumerate() {
                let status = server.process.try_wait();
                match status {
                    Ok(Some(stat)) => {
                        let exit_code = stat.code().unwrap();
                        warn!(
                            "A child process has closed! index: {} ExitCode: {}",
                            index, exit_code
                        );
                        if exit_code != 0 {
                            info!("Server ID: {} has crashed, restarting it...", index);
                            new_instances.push(ControlledProgramDescriptor::new(
                                server.name.as_str(),
                                server.executablePath.as_str(),
                                server.commandLineArgs.clone(),
                                server.working_dir.clone(),
                            ))
                        }
                        to_remove.push(index);
                    }
                    Ok(None) => {}
                    Err(_e) => {}
                }
            }
            for desc in new_instances {
                servers.push(desc.into_instance());
            }
            for index in to_remove {
                servers.remove(index);
            }
            //all of our process are valid at this point, no need to even be careful about it
            for server in servers.iter_mut() {
                let str = match tokio::time::timeout(
                    tokio::time::Duration::from_secs_f64(1. / 10.),
                    server.readOutput(),
                )
                .await
                {
                    Ok(val) => val,
                    _ => None,
                };
                #[derive(serde::Serialize)]
                struct ConsoleOutput {
                    r#type: String,
                    output: String,
                    server_name: String,
                }
                match str {
                    Some(val) => {
                        if !val.is_empty() {
                            let out = ConsoleOutput {
                                r#type: "ServerOutput".to_owned(),
                                output: val,
                                server_name: server.name.clone(),
                            };
                            let _ = state.tx.send(serde_json::to_string(&out).unwrap());
                        }
                    }
                    _ => {}
                }
            }
            drop(servers);
        }
        const REFRESHES_PER_SECOND: f64 = 10.;
        const SECONDS_TO_SLEEP: f64 = 1000. / REFRESHES_PER_SECOND / 1000.;
        std::thread::sleep(std::time::Duration::from_secs_f64(SECONDS_TO_SLEEP));
    }
}

async fn pass_stdin(message: stdinInput, server_name: String, state: AppState::AppState) {
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
