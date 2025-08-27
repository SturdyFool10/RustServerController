use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::Mutex;

use crate::{
    configuration::Config, master::SlaveConnection, messages::ServerInfo,
    ControlledProgram::ControlledProgramInstance,
};
use tokio::sync::broadcast;
#[derive(Clone)]
pub struct AppState {
    pub servers: Arc<Mutex<Vec<ControlledProgramInstance>>>,
    pub tx: broadcast::Sender<String>,
    running: Arc<AtomicBool>,
    pub config: Arc<Mutex<Config>>,
    pub slave_servers: Arc<Mutex<Vec<ServerInfo>>>,
    pub slave_connections: Arc<Mutex<Vec<SlaveConnection>>>,
    pub global_crash_prevention: Arc<AtomicBool>,
}
impl AppState {
    pub fn new(tx: broadcast::Sender<String>, config: Config) -> Self {
        Self {
            servers: Arc::new(Mutex::new(vec![])),
            tx,
            running: Arc::new(AtomicBool::new(true)),
            config: Arc::new(Mutex::new(config)),
            slave_servers: Arc::new(Mutex::new(vec![])),
            slave_connections: Arc::new(Mutex::new(vec![])),
            global_crash_prevention: Arc::new(AtomicBool::new(true)),
        }
    }
    pub fn stop(&mut self) {
        unsafe {
            let ptr = self.running.as_ptr();
            *ptr = false; //dereference the pointer and set it's value
        }
    }
}
