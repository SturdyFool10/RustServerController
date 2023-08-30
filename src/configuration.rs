use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub servers: Vec<crate::ControlledProgram::ControlledProgramDescriptor>,
}
