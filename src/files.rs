use std::{fs::File, io::Write};

use tracing::*;

use crate::configuration::{self, Config};

use crate::specializations;

/// Reads the contents of a file at the given path and returns it as a String.
///
/// # Arguments
/// * `path` - The path to the file to read.
///
/// # Returns
/// * `Ok(String)` containing the file contents if successful.
/// * `Err` if the file cannot be read.
#[no_mangle]
pub fn read_file(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let data = std::fs::read_to_string(path)?;
    Ok(data)
}

/// Loads a JSON configuration file from the given path, validating specializations.
///
/// If the file does not exist or cannot be read, a default configuration is created and saved.
///
/// # Arguments
/// * `path` - The path to the configuration file.
///
/// # Returns
/// * `Config` struct loaded from the file or default if not found.
#[no_mangle]
pub fn load_json(path: &str) -> Config {
    let data = read_file(path);
    let data: String = match data {
        Ok(d) => d,
        Err(error) => {
            error!(error);
            info!("this is likely ok, trying to salvage from error above by creating a default configuration.");
            info!("this can happen if it is your first launch");
            let default_config = Config::default();
            let str = match serde_json::to_string_pretty(&default_config) {
                Ok(str) => str,
                Err(error) => {
                    error!("Failed to serialize default configuration: {}", error);
                    return default_config;
                }
            };
            match File::create(path).and_then(|mut f| f.write_all(str.as_bytes())) {
                Ok(()) => str,
                Err(error) => {
                    error!("Failed to write default config to {}: {}", path, error);
                    return default_config;
                }
            }
        }
    };
    // Validate specializations before deserializing Config
    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&data) {
        let registry = specializations::init_builtin_registry();
        configuration::validate_specializations_in_config(&json_val, &registry);
    }
    match serde_json::from_str(&data) {
        Ok(config) => config,
        Err(error) => {
            error!("Failed to parse config {}: {}", path, error);
            Config::default()
        }
    }
}
