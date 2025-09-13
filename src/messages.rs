/// Message types and data structures for server communication and web API.
///
/// This module defines the types used for exchanging information between the
/// controller, slave nodes, and web clients.
use serde::{Deserialize, Serialize};

use crate::master::SlaveConnectionDescriptor;

/// Information about a server instance, including its name, output, status, and specialization.
#[derive(Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// The name of the server.
    pub name: String,
    /// The latest output from the server.
    pub output: String,
    /// Whether the server is currently active.
    pub active: bool,
    /// Optional host information if this server is on a slave node.
    pub host: Option<SlaveConnectionDescriptor>,
    /// Optional specialized server type.
    pub specialization: Option<String>,
    /// Optional extra info for specialized servers.
    pub specialized_info: Option<serde_json::Value>,
}

/// Message for sending console output to the web client.
#[derive(Clone, Serialize, Deserialize)]
pub struct ConsoleOutput {
    /// The type of message (should be "ServerOutput").
    pub r#type: String,
    /// The output text (HTML).
    pub output: String,
    /// The name of the server this output is for.
    pub server_name: String,
    /// The specialized server type, if any.
    pub server_type: Option<String>,
}

/// Message containing a list of servers and the current configuration.
#[derive(Clone, Serialize, Deserialize)]
pub struct ServerInfoMessage {
    /// The type of message (should be "ServerInfo").
    pub r#type: String,
    /// List of server info objects.
    pub servers: Vec<ServerInfo>,
    /// The current configuration.
    pub config: crate::configuration::Config,
}
/// Message for requesting server info from a node.
#[derive(Clone, Serialize, Deserialize)]
pub struct SInfoRequestMessage {
    /// The type of message (should be "requestInfo").
    pub r#type: String,
    /// Arguments for the request (e.g., whether to include output).
    pub arguments: Vec<bool>,
}
/// Message for sending stdin input to a server.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StdinInput {
    /// The type of message (should be "stdinInput").
    pub r#type: String,
    /// The name of the server to send input to.
    pub server_name: String,
    /// The input value to send.
    pub value: String,
}

/// Message containing configuration information.
#[derive(Clone, Serialize, Deserialize)]
pub struct ConfigInfo {
    /// The type of message (should be "ConfigInfo").
    pub r#type: String,
    /// The configuration data.
    pub config: crate::configuration::Config,
}

/// Message to request the list of available themes.
#[derive(Clone, Serialize, Deserialize)]
pub struct GetThemesList {
    /// The type of message (should be "getThemesList").
    pub r#type: String, // Should be "getThemesList"
}

/// Message to request the CSS for a specific theme.
#[derive(Clone, Serialize, Deserialize)]
pub struct GetThemeCSS {
    /// The type of message (should be "getThemeCSS").
    pub r#type: String, // Should be "getThemeCSS"
    /// The name of the theme to get CSS for.
    pub theme_name: String,
}

/// Message containing the list of available theme names.
#[derive(Clone, Serialize, Deserialize)]
pub struct ThemesList {
    /// The type of message (will be "themesList").
    pub r#type: String, // Will be "themesList"
    /// The list of theme names.
    pub themes: Vec<String>,
}

/// Message containing the CSS for a specific theme.
#[derive(Clone, Serialize, Deserialize)]
pub struct ThemeCSS {
    /// The type of message (will be "themeCSS").
    pub r#type: String, // Will be "themeCSS"
    /// The name of the theme.
    pub theme_name: String,
    /// The CSS string for the theme.
    pub css: String,
}
