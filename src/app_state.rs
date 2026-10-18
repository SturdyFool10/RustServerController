use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::{broadcast, Mutex};

use crate::{
    auth::AuthState, configuration::Config, controlled_program::ControlledProgramInstance,
    credential_store::CredentialStore, master::NodeRegistry,
    specializations::SpecializationRegistry,
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
    /// Remote nodes (master mode): connection state plus mirrored servers.
    pub nodes: NodeRegistry,
    /// Global crash prevention flag.
    #[allow(dead_code)]
    pub global_crash_prevention: Arc<AtomicBool>,
    /// Registry of available server specializations.
    pub specialization_registry: Arc<SpecializationRegistry>,
    /// In-memory authentication sessions.
    pub auth: AuthState,
    /// Encrypted credential database.
    pub credentials: CredentialStore,
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
        credentials: CredentialStore,
    ) -> Self {
        Self {
            servers: Arc::new(Mutex::new(vec![])),
            tx,
            running: Arc::new(AtomicBool::new(true)),
            config: Arc::new(Mutex::new(config)),
            nodes: Arc::new(Mutex::new(Default::default())),
            global_crash_prevention: Arc::new(AtomicBool::new(true)),
            specialization_registry,
            auth: AuthState::default(),
            credentials,
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
