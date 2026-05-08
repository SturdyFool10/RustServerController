use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;

use crate::master::SlaveConnectionDescriptor;
use crate::specializations::{merge_option_defaults, SpecializationRegistry};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PermissionDecisionState {
    Granted,
    #[default]
    Default,
    Blocked,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PermissionDecisionConfig {
    pub permission: String,
    #[serde(default)]
    pub state: PermissionDecisionState,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AuthGroupConfig {
    pub name: String,
    #[serde(default)]
    pub permissions: Vec<PermissionDecisionConfig>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AuthUserConfig {
    pub username: String,
    #[serde(default)]
    pub password_salt: String,
    #[serde(default)]
    pub password_hash: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub permission_overrides: Vec<PermissionDecisionConfig>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub password_required: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AccountRequestConfig {
    pub username: String,
    #[serde(default)]
    pub requested_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AuthConfig {
    #[serde(default = "default_auth_cookie_name")]
    pub cookie_name: String,
    #[serde(default = "default_auth_session_hours")]
    pub session_ttl_hours: u64,
    #[serde(default = "default_oauth_access_token_minutes")]
    pub oauth_access_token_minutes: u64,
    #[serde(default = "default_oauth_refresh_token_days")]
    pub oauth_refresh_token_days: u64,
    #[serde(default)]
    pub users: Vec<AuthUserConfig>,
    #[serde(default = "default_auth_default_permissions")]
    pub default_permissions: Vec<PermissionDecisionConfig>,
    #[serde(default)]
    pub groups: Vec<AuthGroupConfig>,
    #[serde(default)]
    pub account_requests: Vec<AccountRequestConfig>,
    #[serde(default)]
    pub oauth_clients: Vec<OAuthClientConfig>,
}

fn default_auth_cookie_name() -> String {
    "rsc_session".to_string()
}

fn default_auth_session_hours() -> u64 {
    12
}

fn default_oauth_access_token_minutes() -> u64 {
    15
}

fn default_oauth_refresh_token_days() -> u64 {
    30
}

fn default_auth_default_permissions() -> Vec<PermissionDecisionConfig> {
    ["view", "stats", "console"]
        .into_iter()
        .map(|permission| PermissionDecisionConfig {
            permission: permission.to_string(),
            state: PermissionDecisionState::Granted,
        })
        .chain(
            ["control", "config", "admin"]
                .into_iter()
                .map(|permission| PermissionDecisionConfig {
                    permission: permission.to_string(),
                    state: PermissionDecisionState::Blocked,
                }),
        )
        .collect()
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            cookie_name: default_auth_cookie_name(),
            session_ttl_hours: default_auth_session_hours(),
            oauth_access_token_minutes: default_oauth_access_token_minutes(),
            oauth_refresh_token_days: default_oauth_refresh_token_days(),
            users: vec![],
            default_permissions: default_auth_default_permissions(),
            groups: vec![],
            account_requests: vec![],
            oauth_clients: vec![],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OAuthClientConfig {
    pub client_id: String,
    #[serde(default)]
    pub client_secret_hash: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WebTransportConfig {
    #[serde(default)]
    pub enable_https: bool,
    #[serde(default)]
    pub enable_http3: bool,
    #[serde(default)]
    pub https_port: Option<String>,
    #[serde(default)]
    pub http3_port: Option<String>,
    #[serde(default)]
    pub acme: AcmeCertificateConfig,
    #[serde(default)]
    pub local_certificate: LocalCertificateConfig,
    #[serde(default)]
    pub self_signed: SelfSignedCertificateConfig,
}

impl Default for WebTransportConfig {
    fn default() -> Self {
        Self {
            enable_https: false,
            enable_http3: false,
            https_port: Some("443".to_string()),
            http3_port: Some("443".to_string()),
            acme: AcmeCertificateConfig::default(),
            local_certificate: LocalCertificateConfig::default(),
            self_signed: SelfSignedCertificateConfig::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalCertificateConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cert_path: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SelfSignedCertificateConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cert_path: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub subject_alt_names: Vec<String>,
}

impl Default for SelfSignedCertificateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cert_path: Some("controller_data/tls/self_signed_cert.pem".to_string()),
            key_path: Some("controller_data/tls/self_signed_key.pem".to_string()),
            subject_alt_names: vec!["localhost".to_string()],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AcmeCertificateConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub production: bool,
    #[serde(default)]
    pub contact_email: Option<String>,
    #[serde(default)]
    pub cache_dir: Option<String>,
    #[serde(default)]
    pub certificate_targets: Vec<String>,
}

impl Default for AcmeCertificateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            production: false,
            contact_email: None,
            cache_dir: Some("controller_data/acme".to_string()),
            certificate_targets: Vec::new(),
        }
    }
}

pub fn effective_certificate_targets(config: &Config) -> Vec<String> {
    let mut targets: Vec<String> = config
        .web_transport
        .acme
        .certificate_targets
        .iter()
        .map(|target| target.trim().trim_end_matches('.').to_ascii_lowercase())
        .filter(|target| !target.is_empty())
        .collect();
    targets.sort();
    targets.dedup();
    targets
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct MinecraftAccountFilterDetail {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct MinecraftIpBanFilterDetail {
    pub ip: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct MinecraftAccountFilterDetailGroup {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(default)]
    pub whitelist: Vec<MinecraftAccountFilterDetail>,
    #[serde(default)]
    pub ban_list: Vec<MinecraftAccountFilterDetail>,
    #[serde(default)]
    pub banned_ips: Vec<MinecraftIpBanFilterDetail>,
}

/// Validates `specialized_server_type` values in a config JSON, warning on unknown types.
///
/// Prints the position (array index) and allowed values, and states defaulting to generic behavior.
///
/// # Arguments
/// * `config_json` - The JSON value representing the configuration.
/// * `registry` - The specialization registry to check allowed types.
pub fn validate_specializations_in_config(config_json: &Value, registry: &SpecializationRegistry) {
    let allowed: Vec<String> = registry
        .existing_names()
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

/// Applies registered specialization defaults to every matching server descriptor.
///
/// Existing configured values are preserved, with defaults filling only missing
/// keys. Unknown specializations are left untouched so validation can report
/// them without mutating user config.
pub fn apply_specialization_option_defaults(
    config: &mut Config,
    registry: &SpecializationRegistry,
) {
    for server in &mut config.servers {
        let Some(specialization) = server.specialized_server_type.as_deref() else {
            continue;
        };
        let Some(defaults) = registry.default_options_for(specialization) else {
            continue;
        };

        server.specialization_options =
            merge_option_defaults(server.specialization_options.take(), defaults);
    }
}

/// Ensures every server descriptor has a stable UUID for persisted server data.
pub fn ensure_server_uuids(config: &mut Config) -> bool {
    let mut changed = false;
    for server in &mut config.servers {
        if server
            .server_uuid
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            continue;
        }

        server.server_uuid = Some(uuid::Uuid::new_v4().to_string());
        changed = true;
    }
    changed
}

pub fn ensure_account_filter_group_uuids(config: &mut Config) -> bool {
    let mut changed = false;
    for group in &mut config.minecraft_account_filter_detail_groups {
        if group
            .uuid
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            continue;
        }

        group.uuid = Some(uuid::Uuid::new_v4().to_string());
        changed = true;
    }
    changed
}

/// Main configuration struct for the server controller.
///
/// Contains network settings, server descriptors, slave node info, and theme folder location.
#[derive(Serialize, Deserialize, Clone)]

pub struct Config {
    /// Network interface to bind to (e.g., "0.0.0.0").
    pub interface: String,

    /// Port to listen on.
    pub port: String,

    /// List of server descriptors to manage.
    pub servers: Vec<crate::controlled_program::ControlledProgramDescriptor>,

    /// Whether this node is a slave.
    pub slave: bool,

    /// List of slave node connection descriptors.
    pub slave_connections: Vec<SlaveConnectionDescriptor>,

    /// Optional path to the themes folder.
    pub themes_folder: Option<String>,

    #[serde(default)]
    pub web_transport: WebTransportConfig,

    #[serde(default)]
    pub auth: AuthConfig,

    #[serde(default)]
    pub minecraft_account_filter_detail_groups: Vec<MinecraftAccountFilterDetailGroup>,
}

impl Config {
    /// Updates this configuration with values from another config.
    ///
    /// # Arguments
    /// * `new_config` - The new configuration to copy values from.
    pub fn change(&mut self, new_config: Config) {
        self.interface = new_config.interface;

        self.port = new_config.port;

        self.servers = new_config.servers.clone();

        self.themes_folder = new_config.themes_folder.clone();

        self.web_transport = new_config.web_transport.clone();

        self.auth = new_config.auth.clone();

        self.slave = new_config.slave;

        self.slave_connections = new_config.slave_connections.clone();

        self.minecraft_account_filter_detail_groups =
            new_config.minecraft_account_filter_detail_groups.clone();
    }

    /// Writes the configuration to a file as pretty-printed JSON.
    ///
    /// # Arguments
    /// * `file_path` - The path to the file to write.
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

    pub async fn update_config_file_async(&self, file_path: &str) -> std::io::Result<()> {
        let json_data = serde_json::to_string_pretty(self)?;
        tokio::fs::write(file_path, json_data).await
    }
}

impl Default for Config {
    /// Returns a default configuration with standard values.
    fn default() -> Self {
        Self {
            interface: "0.0.0.0".to_string(),

            port: "80".to_string(),

            servers: vec![],

            slave: false,

            slave_connections: vec![],

            themes_folder: Some("themes".to_string()),

            web_transport: WebTransportConfig::default(),

            auth: AuthConfig::default(),

            minecraft_account_filter_detail_groups: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controlled_program::ControlledProgramDescriptor;
    use crate::specializations::init_builtin_registry;
    use serde_json::json;

    fn descriptor(name: &str, specialization: &str) -> ControlledProgramDescriptor {
        ControlledProgramDescriptor {
            name: name.to_string(),
            specialized_server_type: Some(specialization.to_string()),
            ..ControlledProgramDescriptor::default()
        }
    }

    #[test]
    fn applies_builtin_specialization_defaults_to_configured_servers() {
        let registry = init_builtin_registry();
        let mut config = Config {
            servers: vec![descriptor("minecraft", "Minecraft")],
            ..Config::default()
        };

        apply_specialization_option_defaults(&mut config, &registry);

        assert_eq!(
            config.servers[0].specialization_options,
            Some(json!({
                "auto_accept_eula": true,
                "account_filter_groups": [],
            }))
        );
    }

    #[test]
    fn config_specialization_defaults_preserve_user_values() {
        let registry = init_builtin_registry();
        let mut server = descriptor("minecraft", "Minecraft");
        server.specialization_options = Some(json!({
            "auto_accept_eula": false,
        }));
        let mut config = Config {
            servers: vec![server],
            ..Config::default()
        };

        apply_specialization_option_defaults(&mut config, &registry);

        assert_eq!(
            config.servers[0].specialization_options,
            Some(json!({
                "auto_accept_eula": false,
                "account_filter_groups": [],
            }))
        );
    }

    #[test]
    fn config_specialization_defaults_ignore_unknown_specializations() {
        let registry = init_builtin_registry();
        let mut config = Config {
            servers: vec![descriptor("unknown", "Missing")],
            ..Config::default()
        };

        apply_specialization_option_defaults(&mut config, &registry);

        assert_eq!(config.servers[0].specialization_options, None);
    }

    #[test]
    fn ensure_server_uuids_fills_missing_values_without_replacing_existing() {
        let mut config = Config {
            servers: vec![descriptor("Server One", "Minecraft"), {
                let mut server = descriptor("Server Two", "Terraria");
                server.server_uuid = Some("existing-uuid".to_string());
                server
            }],
            ..Config::default()
        };

        assert!(ensure_server_uuids(&mut config));
        assert!(config.servers[0].server_uuid.is_some());
        assert_eq!(
            config.servers[1].server_uuid.as_deref(),
            Some("existing-uuid")
        );
        assert!(!ensure_server_uuids(&mut config));
    }

    #[test]
    fn ensure_account_filter_group_uuids_fills_missing_values() {
        let mut config = Config {
            minecraft_account_filter_detail_groups: vec![
                MinecraftAccountFilterDetailGroup {
                    name: "Shared Survival".to_string(),
                    ..Default::default()
                },
                MinecraftAccountFilterDetailGroup {
                    name: "Moderated".to_string(),
                    uuid: Some("existing-group".to_string()),
                    ..Default::default()
                },
            ],
            ..Config::default()
        };

        assert!(ensure_account_filter_group_uuids(&mut config));
        assert!(config.minecraft_account_filter_detail_groups[0]
            .uuid
            .is_some());
        assert_eq!(
            config.minecraft_account_filter_detail_groups[1]
                .uuid
                .as_deref(),
            Some("existing-group")
        );
        assert!(!ensure_account_filter_group_uuids(&mut config));
    }

    #[test]
    fn effective_certificate_targets_normalizes_and_deduplicates_targets() {
        let config = Config {
            web_transport: WebTransportConfig {
                acme: AcmeCertificateConfig {
                    certificate_targets: vec![
                        "Example.COM.".to_string(),
                        "example.com".to_string(),
                        "  play.example.com  ".to_string(),
                        "".to_string(),
                    ],
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Config::default()
        };

        assert_eq!(
            effective_certificate_targets(&config),
            vec!["example.com".to_string(), "play.example.com".to_string()]
        );
    }
}
