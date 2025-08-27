use serde::{Deserialize, Serialize};

use crate::master::SlaveConnectionDescriptor;

#[derive(Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub output: String,
    pub active: bool,
    pub host: Option<SlaveConnectionDescriptor>,
    pub specialization: Option<crate::controlled_program::SpecializedServerTypes>,
    pub specialized_info: Option<crate::controlled_program::SpecializedServerInformation>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConsoleOutput {
    pub r#type: String,
    pub output: String,
    pub server_name: String,
    pub server_type: Option<crate::controlled_program::SpecializedServerTypes>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ServerInfoMessage {
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
pub struct StdinInput {
    pub r#type: String,
    pub server_name: String,
    pub value: String,
}
