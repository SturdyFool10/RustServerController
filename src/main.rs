mod AppState;
mod ControlledProgram;
mod configuration;
mod files;
mod macros;
mod servers;
mod webserver;

use files::*;
use servers::start_servers;
use std::process::exit;
use tokio::{spawn, sync::broadcast};
use tracing::*;
use webserver::start_web_server;

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
