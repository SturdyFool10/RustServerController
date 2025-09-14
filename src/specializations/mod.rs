//! Specialization trait and registry for built-in server specializations.
//!
//! This module defines the [`ServerSpecialization`] trait for implementing
//! server-specific logic (such as Minecraft or Terraria), and provides a
//! thread-safe registry for managing available specializations.

pub mod minecraft;
pub mod terraria;
pub mod vintage_story;

use crate::controlled_program::ControlledProgramInstance;
use dashmap::DashMap;
use std::sync::Arc;

/// Trait for implementing server-specific logic and output parsing.
///
/// Implement this trait for each supported server type (e.g., Minecraft, Terraria).
pub trait ServerSpecialization: Send + Sync {
    /// Called before the server process is spawned, allowing environment variables to be customized.
    ///
    /// Use this to modify the environment variables for the server process.
    /// The default implementation does nothing.
    fn pre_init(
        &mut self,
        _env: &mut std::collections::HashMap<String, String>,
        _descriptor: &crate::controlled_program::ControlledProgramDescriptor,
    ) {
        // Default: do nothing
    }

    /// Returns true if the last processed log line resulted in a status update (e.g., player count changed).
    /// Should be set to true only for meaningful status changes.
    fn has_status_update(&self) -> bool {
        false
    }

    /// Sets the status update flag to false after an update has been sent.
    fn set_status_update_sent(&mut self) {
        // Default: do nothing
    }

    /// Called when the specialization is first attached to a server instance.
    ///
    /// Use this to initialize any state or inspect the instance.
    fn init(&mut self, instance: &mut ControlledProgramInstance);

    /// Called for each output line from the server process.
    ///
    /// Takes ownership of the log line. Return `Some(String)` to transform the line,
    /// or `None` to omit it from output.
    fn parse_output(
        &mut self,
        line: String,
        instance: &mut ControlledProgramInstance,
    ) -> Option<String>;

    /// Called when the server process exits.
    ///
    /// Allows the specialization to handle exit-specific logic (e.g., EULA patching, auto-restart).
    /// Default implementation does nothing.
    fn on_exit(
        &mut self,
        _instance: &mut ControlledProgramInstance,
        _state: &crate::app_state::AppState,
        _exit_code: i32,
    ) {
        // Default: do nothing
    }

    /// Returns the current status/info for this specialization.
    ///
    /// By convention, status is usually stored in the instance's `specialized_server_info`.
    #[allow(unused)]
    fn get_status(&self) -> serde_json::Value;
}

/// Factory type for creating new specialization instances.
pub type SpecializationFactory = fn() -> Box<dyn ServerSpecialization>;

/// Thread-safe registry for all available specializations.
pub struct SpecializationRegistry {
    map: DashMap<String, SpecializationFactory>,
}

impl SpecializationRegistry {
    /// Returns a Vec of allowed specialization names.
    pub fn existing_names(&self) -> Vec<String> {
        self.map
            .iter()
            .map(|entry| entry.key().to_string())
            .collect()
    }

    /// Checks if a specialization exists by name.
    pub fn contains_key(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    /// Register a specialization factory under a given name.
    ///
    /// # Arguments
    /// * `name` - The specialization name (e.g., "Minecraft").
    /// * `factory` - The factory function to create new instances.
    pub fn register(&self, name: &str, factory: SpecializationFactory) {
        self.map.insert(name.to_string(), factory);
    }

    /// Get a new instance of a specialization by name.
    ///
    /// # Arguments
    /// * `name` - The specialization name.
    ///
    /// # Returns
    /// * `Some(Box<dyn ServerSpecialization>)` if found, else `None`.
    pub fn get(&self, name: &str) -> Option<Box<dyn ServerSpecialization>> {
        self.map.get(name).map(|f| f())
    }
}

/// Helper to initialize the registry with built-in specializations.
///
/// Registers "Minecraft" and "Terraria" specializations by default.
pub fn init_builtin_registry() -> Arc<SpecializationRegistry> {
    let registry = Arc::new(SpecializationRegistry::new());
    registry.register("Minecraft", minecraft::factory);
    registry.register("Terraria", terraria::factory);
    registry.register("VintageStory", vintage_story::vintage_story_factory);
    registry
}
