//! Specialization trait and registry for built-in server specializations.

pub mod minecraft;
pub mod terraria;

use crate::controlled_program::ControlledProgramInstance;
use dashmap::DashMap;
use std::sync::Arc;

/// Trait for all server specializations (built-in or plugin).
pub trait ServerSpecialization: Send + Sync {
    /// Called when the specialization is first attached to a server instance.
    fn init(&mut self, instance: &mut ControlledProgramInstance);

    /// Called for each output line from the server process.
    /// Takes ownership of the log line. Return Some(String) to transform the line, None to omit it.
    fn parse_output(
        &mut self,
        line: String,
        instance: &mut ControlledProgramInstance,
    ) -> Option<String>;

    /// Returns the current status/info for this specialization.
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
    pub fn allowed_names(&self) -> Vec<String> {
        self.map
            .iter()
            .map(|entry| entry.key().to_string())
            .collect()
    }

    /// Checks if a specialization exists by name.
    pub fn contains_key(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }
}

impl SpecializationRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    /// Register a specialization factory under a given name.
    pub fn register(&self, name: &str, factory: SpecializationFactory) {
        self.map.insert(name.to_string(), factory);
    }

    /// Get a new instance of a specialization by name.
    pub fn get(&self, name: &str) -> Option<Box<dyn ServerSpecialization>> {
        self.map.get(name).map(|f| f())
    }
}

/// Helper to initialize the registry with built-in specializations.
/// Call this at startup and store in AppState.
pub fn init_builtin_registry() -> Arc<SpecializationRegistry> {
    let registry = Arc::new(SpecializationRegistry::new());
    registry.register("Minecraft", minecraft::factory);
    registry.register("Terraria", terraria::factory);
    registry
}

// pub mod minecraft;
// pub mod terraria;
// (Uncomment and implement these modules as you migrate logic.)
