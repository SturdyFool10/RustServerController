use serde::{Deserialize, Serialize};

use crate::master::{SlaveConnection, SlaveConnectionDescriptor};

#[derive(Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub output: String,
    pub active: bool,
    pub host: Option<SlaveConnectionDescriptor>,
    pub specialization: Option<crate::ControlledProgram::SpecializedServerTypes>,
    pub specializedInfo: Option<crate::ControlledProgram::SpecializedServerInformation>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConsoleOutput {
    pub r#type: String,
    pub output: String,
    pub server_name: String,
    pub server_type: Option<crate::ControlledProgram::SpecializedServerTypes>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct serverInfoMessage {
    pub r#type: String,
    pub servers: Vec<ServerInfo>,
    pub config: crate::configuration::Config,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct SInfoRequestMessage {
    pub r#type: String,
    pub arguments: Vec<bool>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct stdinInput {
    pub r#type: String,
    pub server_name: String,
    pub value: String,
}
