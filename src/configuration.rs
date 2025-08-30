use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;

use crate::master::SlaveConnectionDescriptor;
use crate::specializations::SpecializationRegistry;
use serde_json::Value;

/// Validate specialized_server_type values in a config JSON, warning on unknown types.
/// Prints position (array index) and allowed values, and states defaulting to generic behavior.
pub fn validate_specializations_in_config(config_json: &Value, registry: &SpecializationRegistry) {
    let allowed: Vec<String> = registry
        .allowed_names()
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

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub interface: String,
    pub port: String,
    pub servers: Vec<crate::controlled_program::ControlledProgramDescriptor>,
    pub slave: bool,
    pub slave_connections: Vec<SlaveConnectionDescriptor>,
    pub themes_folder: Option<String>,
}

impl Config {
    pub fn change(&mut self, new_config: Config) {
        self.interface = new_config.interface;
        self.port = new_config.port;
        self.servers = new_config.servers.clone();
        self.themes_folder = new_config.themes_folder.clone();
        self.slave = new_config.slave;
        self.slave_connections = new_config.slave_connections.clone();
    }

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
