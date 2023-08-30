use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub interface: String,
    pub port: String,
    pub servers: Vec<crate::ControlledProgram::ControlledProgramDescriptor>,
}
