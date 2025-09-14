use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::{broadcast, Mutex};

use crate::{
    configuration::Config, controlled_program::ControlledProgramInstance, master::SlaveConnection,
    messages::ServerInfo, specializations::SpecializationRegistry,
};

/// Shared application state for the server controller.
///
/// Holds references to all running servers, configuration, communication channels,
/// slave nodes, and the specialization registry.
#[derive(Clone)]
pub struct AppState {
    /// List of running server instances.
    pub servers: Arc<Mutex<Vec<ControlledProgramInstance>>>,
    /// Broadcast channel for sending messages to all listeners.
    pub tx: broadcast::Sender<String>,
    /// Atomic flag indicating if the application is running.
    running: Arc<AtomicBool>,
    /// Shared configuration.
    pub config: Arc<Mutex<Config>>,
    /// List of servers running on slave nodes.
    pub slave_servers: Arc<Mutex<Vec<ServerInfo>>>,
    /// List of slave node connections.
    pub slave_connections: Arc<Mutex<Vec<SlaveConnection>>>,
    /// Global crash prevention flag.
    #[allow(dead_code)]
    pub global_crash_prevention: Arc<AtomicBool>,
    /// Registry of available server specializations.
    pub specialization_registry: Arc<SpecializationRegistry>,
}
impl AppState {
    /// Creates a new AppState instance.
    ///
    /// # Arguments
    /// * `tx` - Broadcast sender for message passing.
    /// * `config` - Initial configuration.
    /// * `specialization_registry` - Registry of available server specializations.
    pub fn new(
        tx: broadcast::Sender<String>,
        config: Config,
        specialization_registry: Arc<SpecializationRegistry>,
    ) -> Self {
        Self {
            servers: Arc::new(Mutex::new(vec![])),
            tx,
            running: Arc::new(AtomicBool::new(true)),
            config: Arc::new(Mutex::new(config)),
            slave_servers: Arc::new(Mutex::new(vec![])),
            slave_connections: Arc::new(Mutex::new(vec![])),
            global_crash_prevention: Arc::new(AtomicBool::new(true)),
            specialization_registry,
        }
    }

    /// Stops the application by setting the running flag to false.
    ///
    /// # Safety
    /// This method uses unsafe code to directly modify the atomic flag.
    pub fn stop(&mut self) {
        unsafe {
            let ptr = self.running.as_ptr();
            *ptr = false; // Dereference the pointer and set its value
        }
    }
}
