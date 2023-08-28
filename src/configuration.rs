use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub servers: Vec<crate::ControlledProgram::ControlledProgramDescriptor>
}