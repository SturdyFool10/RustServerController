use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;

use crate::master::SlaveConnectionDescriptor;

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub interface: String,
    pub port: String,
    pub servers: Vec<crate::controlled_program::ControlledProgramDescriptor>,
    pub slave: bool,
    pub slave_connections: Vec<SlaveConnectionDescriptor>,
}

impl Config {
    pub fn change(&mut self, new_config: Config) {
        self.interface = new_config.interface;
        self.port = new_config.port;
        self.servers = new_config.servers.clone()
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
        }
    }
}
