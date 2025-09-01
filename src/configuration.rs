use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;

use crate::master::SlaveConnectionDescriptor;
use crate::specializations::SpecializationRegistry;
use serde_json::Value;

/// Validates `specialized_server_type` values in a config JSON, warning on unknown types.

///
/// Prints the position (array index) and allowed values, and states defaulting to generic behavior.

///
/// # Arguments
/// * `config_json` - The JSON value representing the configuration.
/// * `registry` - The specialization registry to check allowed types.
pub fn validate_specializations_in_config(config_json: &Value, registry: &SpecializationRegistry) {
    let allowed: Vec<String> = registry
        .existing_names()
        .into_iter()
        .map(|name| format!("\"{}\"", name))
        .collect();

    if let Some(servers) = config_json.get("servers").and_then(|v| v.as_array()) {
        for (i, server) in servers.iter().enumerate() {
            if let Some(spec_type) = server
                .get("specialized_server_type")
                .and_then(|v| v.as_str())
            {
                if !spec_type.is_empty() && !registry.contains_key(spec_type) {
                    let position = format!("at servers[{}]", i);

                    eprintln!(

                        "Warning: Server Specialization \"{}\" does not exist {}, allowed values: {}. Defaulting to generic, non-specialized behavior.",

                        spec_type,

                        position,

                        allowed.join(", ")

                    );
                }
            }
        }
    }
}

/// Main configuration struct for the server controller.
///
/// Contains network settings, server descriptors, slave node info, and theme folder location.
#[derive(Serialize, Deserialize, Clone)]

pub struct Config {
    /// Network interface to bind to (e.g., "0.0.0.0").
    pub interface: String,

    /// Port to listen on.
    pub port: String,

    /// List of server descriptors to manage.
    pub servers: Vec<crate::controlled_program::ControlledProgramDescriptor>,

    /// Whether this node is a slave.
    pub slave: bool,

    /// List of slave node connection descriptors.
    pub slave_connections: Vec<SlaveConnectionDescriptor>,

    /// Optional path to the themes folder.
    pub themes_folder: Option<String>,
}

impl Config {
    /// Updates this configuration with values from another config.
    ///
    /// # Arguments
    /// * `new_config` - The new configuration to copy values from.
    pub fn change(&mut self, new_config: Config) {
        self.interface = new_config.interface;

        self.port = new_config.port;

        self.servers = new_config.servers.clone();

        self.themes_folder = new_config.themes_folder.clone();

        self.slave = new_config.slave;

        self.slave_connections = new_config.slave_connections.clone();
    }

    /// Writes the configuration to a file as pretty-printed JSON.
    ///
    /// # Arguments
    /// * `file_path` - The path to the file to write.
    pub fn update_config_file(&self, file_path: &str) {
        // Check if the file already exists and delete it if it does

        if let Err(err) = fs::remove_file(file_path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                // Ignore errors other than "File not found"

                return;
            }
        }

        let json_data = serde_json::to_string_pretty(self);

        if let Ok(json_data) = json_data {
            let file = File::create(file_path);

            if let Ok(mut file) = file {
                let _ = file.write_all(json_data.as_bytes());
            }
        }
    }
}

impl Default for Config {
    /// Returns a default configuration with standard values.
    fn default() -> Self {
        Self {
            interface: "0.0.0.0".to_string(),

            port: "80".to_string(),

            servers: vec![],

            slave: false,

            slave_connections: vec![],

            themes_folder: Some("themes".to_string()),
        }
    }
}
