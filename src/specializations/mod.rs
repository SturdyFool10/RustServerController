//! Specialization trait and registry for built-in server specializations.
//!
//! This module defines the [`ServerSpecialization`] trait for implementing
//! server-specific logic (such as Minecraft or Terraria), and provides a
//! thread-safe registry for managing available specializations.

pub mod minecraft;
pub mod player_activity;
pub mod terraria;
pub mod vintage_story;

use crate::controlled_program::ControlledProgramInstance;
use crate::controller_plugins::{PluginCatalog, PluginSpecializationManifest};
use crate::wasm_plugins::WasmPluginRuntime;
use dashmap::DashMap;
use serde_json::Value;
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

    /// Called after a server instance has been created and can receive controller state.
    fn on_start(
        &mut self,
        _instance: &mut ControlledProgramInstance,
        _state: &crate::app_state::AppState,
    ) {
        // Default: do nothing
    }

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

    /// Returns optional stats for the web UI stats page.
    fn get_stats(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Returns default persisted options for this specialization.
    ///
    /// Defaults are merged with the server descriptor's `specialization_options`
    /// before the process starts. Configured values always win.
    fn default_options(&self) -> serde_json::Value {
        serde_json::Value::Null
    }
}

/// Factory type for creating new specialization instances.
pub type SpecializationFactory = Arc<dyn Fn() -> Box<dyn ServerSpecialization> + Send + Sync>;

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

    pub fn register_fn(&self, name: &str, factory: fn() -> Box<dyn ServerSpecialization>) {
        self.register(name, Arc::new(factory));
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

    /// Returns the default options supplied by a registered specialization.
    pub fn default_options_for(&self, name: &str) -> Option<Value> {
        self.get(name).map(|handler| handler.default_options())
    }
}

/// Merges specialization defaults with configured options.
///
/// Only object values are merged recursively. Configured values are preserved,
/// and non-object configured values are left untouched.
pub fn merge_option_defaults(configured: Option<Value>, defaults: Value) -> Option<Value> {
    if defaults.is_null() {
        return configured;
    }

    match configured {
        Some(mut configured_value) => {
            merge_json_defaults(&mut configured_value, &defaults);
            Some(configured_value)
        }
        None => Some(defaults),
    }
}

fn merge_json_defaults(configured: &mut Value, defaults: &Value) {
    let (Some(configured_object), Some(defaults_object)) =
        (configured.as_object_mut(), defaults.as_object())
    else {
        return;
    };

    for (key, default_value) in defaults_object {
        match configured_object.get_mut(key) {
            Some(configured_child) => merge_json_defaults(configured_child, default_value),
            None => {
                configured_object.insert(key.clone(), default_value.clone());
            }
        }
    }
}

/// Helper to initialize the registry with built-in specializations.
///
/// Registers "Minecraft" and "Terraria" specializations by default.
pub fn init_builtin_registry() -> Arc<SpecializationRegistry> {
    let registry = Arc::new(SpecializationRegistry::new());
    registry.register_fn("Minecraft", minecraft::factory);
    registry.register_fn("Terraria", terraria::factory);
    registry.register_fn("VintageStory", vintage_story::vintage_story_factory);
    registry
}

pub fn register_plugin_specializations(registry: &SpecializationRegistry, catalog: &PluginCatalog) {
    for plugin in &catalog.plugins {
        let wasm = plugin
            .backend
            .wasm_module
            .as_ref()
            .and_then(|module| WasmPluginRuntime::load(plugin.root_dir.join(module)));
        for specialization in &plugin.specializations {
            let plugin_id = plugin.id.clone();
            let manifest = specialization.clone();
            let wasm = wasm.clone();
            registry.register(
                &specialization.name,
                Arc::new(move || {
                    Box::new(ManifestSpecialization {
                        plugin_id: plugin_id.clone(),
                        manifest: manifest.clone(),
                        wasm: wasm.clone(),
                        status_update: false,
                    })
                }),
            );
        }
    }
}

#[derive(Clone)]
struct ManifestSpecialization {
    plugin_id: String,
    manifest: PluginSpecializationManifest,
    wasm: Option<WasmPluginRuntime>,
    status_update: bool,
}

impl ServerSpecialization for ManifestSpecialization {
    fn init(&mut self, instance: &mut ControlledProgramInstance) {
        instance.specialized_server_info = Some(self.get_status());
        self.status_update = true;
    }

    fn has_status_update(&self) -> bool {
        self.status_update
    }

    fn set_status_update_sent(&mut self) {
        self.status_update = false;
    }

    fn parse_output(
        &mut self,
        line: String,
        instance: &mut ControlledProgramInstance,
    ) -> Option<String> {
        if let Some(wasm) = &self.wasm {
            let input = serde_json::json!({
                "plugin_id": self.plugin_id,
                "specialization": self.manifest.name,
                "server": instance.name,
                "server_uuid": instance.server_uuid,
                "line": line.clone(),
                "options": instance.specialization_options,
            });
            if let Some(value) = wasm.call_json_hook("rsc_parse_output", &input) {
                self.status_update = value
                    .get("status_update")
                    .and_then(Value::as_bool)
                    .unwrap_or(self.status_update);
                if let Some(status) = value.get("status").cloned() {
                    instance.specialized_server_info = Some(status);
                }
                return match value.get("line") {
                    Some(Value::Null) => None,
                    Some(Value::String(line)) => Some(line.clone()),
                    _ => Some(line),
                };
            }
        }
        Some(line)
    }

    fn get_status(&self) -> serde_json::Value {
        if let Some(wasm) = &self.wasm {
            let input = serde_json::json!({
                "plugin_id": self.plugin_id,
                "specialization": self.manifest.name,
            });
            if let Some(status) = wasm.call_json_hook("rsc_status", &input) {
                return status;
            }
        }
        serde_json::json!({
            "plugin_id": self.plugin_id,
            "specialization": self.manifest.name,
            "display_name": if self.manifest.display_name.is_empty() {
                self.manifest.name.as_str()
            } else {
                self.manifest.display_name.as_str()
            },
            "description": self.manifest.description,
            "status": self.manifest.status,
        })
    }

    fn get_stats(&self) -> serde_json::Value {
        if let Some(wasm) = &self.wasm {
            let input = serde_json::json!({
                "plugin_id": self.plugin_id,
                "specialization": self.manifest.name,
            });
            if let Some(stats) = wasm.call_json_hook("rsc_stats", &input) {
                return stats;
            }
        }
        self.manifest.stats.clone()
    }

    fn default_options(&self) -> serde_json::Value {
        if let Some(wasm) = &self.wasm {
            let input = serde_json::json!({
                "plugin_id": self.plugin_id,
                "specialization": self.manifest.name,
                "manifest_defaults": self.manifest.default_options,
            });
            if let Some(defaults) = wasm.call_json_hook("rsc_default_options", &input) {
                return defaults;
            }
        }
        self.manifest.default_options.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_option_defaults_preserves_configured_values() {
        let configured = json!({
            "enabled": false,
            "nested": {
                "level": 3
            }
        });
        let defaults = json!({
            "enabled": true,
            "mode": "auto",
            "nested": {
                "level": 1,
                "label": "default"
            }
        });

        let merged = merge_option_defaults(Some(configured), defaults);

        assert_eq!(
            merged,
            Some(json!({
                "enabled": false,
                "mode": "auto",
                "nested": {
                    "level": 3,
                    "label": "default"
                }
            }))
        );
    }

    #[test]
    fn merge_option_defaults_leaves_non_object_config_untouched() {
        let configured = json!("custom");
        let defaults = json!({
            "enabled": true,
        });

        let merged = merge_option_defaults(Some(configured.clone()), defaults);

        assert_eq!(merged, Some(configured));
    }

    #[test]
    fn merge_option_defaults_uses_defaults_when_missing() {
        let defaults = json!({
            "enabled": true,
        });

        let merged = merge_option_defaults(None, defaults.clone());

        assert_eq!(merged, Some(defaults));
    }
}
