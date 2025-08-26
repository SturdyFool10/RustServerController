mod AppState;
mod ControlledProgram;
mod ansi_to_html;
mod configuration;
mod files;
mod macros;
mod master;
mod messages;
mod servers;
mod slave;
mod webserver;
mod websocket;

use files::*;
use std::process::exit;
use tokio::{spawn, sync::broadcast};
use tracing::*;

use crate::{
    master::create_slave_connections, servers::start_servers, slave::start_slave,
    webserver::start_web_server,
};

#[tokio::main]
async fn main() -> Result<(), String> {
    let config = load_json("config.json");
    let slave: bool = config.slave.clone();
    tracing_subscriber::FmtSubscriber::builder()
        .pretty()
        .with_line_number(false)
        .with_file(false)
        .without_time()
        .init();
    let (tx, _rx) = broadcast::channel(100);
    let mut app_state = AppState::AppState::new(tx, config);
    let handles: Vec<tokio::task::JoinHandle<()>>;
    if slave {
        handles = spawn_tasks!(app_state.clone(), start_servers, start_slave)
    } else {
        handles = spawn_tasks!(
            app_state.clone(),
            start_web_server,
            start_servers,
            create_slave_connections
        );
    }
    {
        info!("Starting {} tasks", handles.len());
    }
    let _ = tokio::spawn(async_listener!("t", app_state)).await;
    info!("Termination key pressed, closing the app.");
    exit(0);
}
