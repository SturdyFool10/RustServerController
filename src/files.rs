use std::{fs::File, io::Write};

use tracing::*;

use crate::configuration::{self, Config};
use crate::specializations;

#[no_mangle]
pub fn read_file(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let data = std::fs::read_to_string(path)?;
    Ok(data)
}

#[no_mangle]
pub fn load_json(path: &str) -> Config {
    let data = read_file(path);
    let data: String = match data {
        Ok(d) => d,
        Err(error) => {
            error!(error);
            info!("this is likely ok, trying to salvage from error above by creating a default configuration.");
            info!("this can happen if it is your first launch");
            let str = serde_json::to_string_pretty(&Config::default()).unwrap();
            let mut f = File::create(path)
                .expect(&format!("There was an error creating the file specified: {}", &path)[..]);
            f.write_all(str.as_bytes()).expect("Error Writing to File");
            str
        }
    };
    // Validate specializations before deserializing Config
    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&data) {
        let registry = specializations::init_builtin_registry();
        configuration::validate_specializations_in_config(&json_val, &registry);
    }
    let json = serde_json::from_str(&data.clone());
    json.unwrap()
}
