use std::{
    fs::File,
    io::{Read, Write},
};
mod AppState;
mod ControlledProgram;
use async_std::io::{self, BufReader};
use axum::{
    extract::{ws::*, State, WebSocketUpgrade},
    response::{Html, Response},
    routing::get,
    Router, ServiceExt,
};
use futures_util::stream::*;
use futures_util::SinkExt;
use serde_json::Value as serdeValue;
use tokio::io::{self as tokio_io, AsyncBufReadExt, BufReader as tokioBufReader};
use tokio::{spawn, sync::broadcast};
use tower_http::services::ServeDir;
use tracing::*;
use ControlledProgram::{ControlledProgramDescriptor, ControlledProgramInstance};
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
    tracing_subscriber::FmtSubscriber::builder()
        .pretty()
        .with_line_number(false)
        .with_file(false)
        .without_time()
        .init();
    let (tx, _rx) = broadcast::channel(100);
    let mut app_state = AppState::AppState::new(tx);
    let handles = spawn_tasks!(app_state.clone(), start_web_server, start_servers);
    {
        info!("Starting {} tasks", handles.len());
    }
    let _ = tokio::spawn(async_listener!("t", app_state)).await;
    info!("Termination key pressed, closing the app.");
    Ok(())
}
async fn get_router(_state: AppState::AppState) -> Router<AppState::AppState> {
    let router: Router<AppState::AppState> = Router::new()
        .nest_service("/html", ServeDir::new("html_src"))
        .route("/", get(main_serve))
        .route("/ws", get(handle_ws_upgrade));
    router
}

#[no_mangle]
async fn start_web_server(_state: AppState::AppState) {
    let router = get_router(_state.clone()).await; //.route("/ws", get(handle_socket))
    info!("Starting server on *:81");
    let stateful_router = router.with_state(_state);
    axum::Server::bind(&"0.0.0.0:81".parse().unwrap())
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
fn load_json(path: &str) -> serdeValue {
    let data = read_file(path);
    let data: String = match data {
        Ok(d) => d,
        Err(error) => {
            error!(error);
            info!("this is likely ok, trying to salvage from error above by creating a default configuration.");
            info!("this can happen if it is your first launch");
            let str = "{\n\t\"servers\": []\n}".to_owned();
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
    let mut json = load_json("config.json");
    let servers: &mut Vec<serdeValue> = json["servers"].as_array_mut().unwrap();
    let mut _server_instances: Vec<ControlledProgramInstance> = vec![];
    for i in servers.iter_mut() {
        /*
        Server Format as follows:
        {
            "name": ""
            "exePath": "",
            "arguments": [

            ]
        }
         */
        let _name = i["name"]
            .as_str()
            .expect("server was not in proper format: server.name not found");
        let _exe_path = i["exePath"]
            .as_str()
            .expect("server was not in proper format: server.exePath not found");
        let _args: Vec<String> = i["arguments"]
            .as_array()
            .expect("server was not in proper format: server.arguments not found")
            .iter()
            .map(|val| -> String { val.to_string() })
            .collect();
        let _working_dir: String = i["working_dir"].as_str().expect("server was not in proper format: server.working_dir was either not a string or does not exist").to_owned();
        let instance = ControlledProgramInstance::new(_name, _exe_path, _args, _working_dir);
        _server_instances.push(instance);
    }
    tokio::spawn(process_stdout(_server_instances, _state.clone()));
}
#[no_mangle]
async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState::AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState::AppState) {
    info!("Socket Inbound!");
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
            info!("A socket has terminated!");
            listen_task.abort()
        },
        _ = (&mut listen_task) => {
            info!("A socket has terminated!");
            send_task.abort()
        },
    };
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
    let arr = json["arguemnts"].as_array();
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
            info!("Sending information...");
            let servers = state.servers.lock().await;
            let mut response = "{\n\t\"Servers\": [".to_owned();
            let server_length = servers.len();
            servers.iter().enumerate().for_each(|(index, server)| {
                response += format!("\"{}\"", server.name.as_str()).as_str();
                if index < server_length - 1 {
                    response += ", ";
                }
            });
            drop(servers);
            response += "]\n}";
            let _ = state.tx.send(response);
        }
        _ => {}
    }
}

async fn process_stdout(instances: Vec<ControlledProgramInstance>, state: AppState::AppState) {
    let mut servers = state.servers.lock().await;
    for server in instances {
        servers.push(server);
    }
    drop(servers);
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
                match str {
                    Some(val) => {
                        if val != "" {
                            let _ = state.tx.send(format!("{{\"type\": \"ServerOutput\",\"out\": \"{}\",\"serverName\": \"{}\"}}", val, server.name.clone()).replace("\n", "\\n"));
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
