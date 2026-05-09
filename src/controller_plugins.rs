use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PluginCatalog {
    pub plugins: Vec<ControllerPluginManifest>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControllerPluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub frontend: PluginFrontendManifest,
    #[serde(default)]
    pub backend: PluginBackendManifest,
    #[serde(default)]
    pub specializations: Vec<PluginSpecializationManifest>,
    #[serde(skip)]
    pub root_dir: PathBuf,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PluginFrontendManifest {
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default)]
    pub styles: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PluginBackendManifest {
    #[serde(default)]
    pub wasm_module: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginSpecializationManifest {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default_options: Value,
    #[serde(default)]
    pub status: Value,
    #[serde(default)]
    pub stats: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct PublicPluginCatalog {
    pub plugins: Vec<PublicPluginManifest>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PublicPluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub frontend: PluginFrontendManifest,
    pub backend: PluginBackendManifest,
    pub specializations: Vec<PluginSpecializationManifest>,
}

fn default_enabled() -> bool {
    true
}

pub async fn load_plugin_catalog(folder: Option<&str>) -> PluginCatalog {
    let folder = folder
        .map(str::trim)
        .filter(|folder| !folder.is_empty())
        .unwrap_or("controller_plugins");
    let mut plugins = Vec::new();
    let mut entries = match tokio::fs::read_dir(folder).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return PluginCatalog::default()
        }
        Err(error) => {
            tracing::warn!("Failed to read plugin directory '{}': {}", folder, error);
            return PluginCatalog::default();
        }
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let root_dir = entry.path();
        let manifest_path = if root_dir.is_dir() {
            root_dir.join("manifest.json")
        } else if root_dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".plugin.json"))
        {
            root_dir.clone()
        } else {
            continue;
        };
        let Ok(text) = tokio::fs::read_to_string(&manifest_path).await else {
            tracing::warn!(
                "Skipping plugin with unreadable manifest: {:?}",
                manifest_path
            );
            continue;
        };
        let Ok(mut manifest) = serde_json::from_str::<ControllerPluginManifest>(&text) else {
            tracing::warn!(
                "Skipping plugin with invalid manifest JSON: {:?}",
                manifest_path
            );
            continue;
        };
        if !manifest.enabled || !valid_plugin_id(&manifest.id) {
            continue;
        }
        manifest.root_dir = if manifest_path == root_dir {
            manifest_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(folder))
        } else {
            root_dir
        };
        manifest
            .frontend
            .modules
            .retain(|asset| valid_asset_path(asset));
        manifest
            .frontend
            .styles
            .retain(|asset| valid_asset_path(asset));
        if manifest
            .backend
            .wasm_module
            .as_deref()
            .is_some_and(|asset| !valid_asset_path(asset) || !asset.ends_with(".wasm"))
        {
            tracing::warn!(
                "Disabling invalid WASM module path for plugin '{}'",
                manifest.id
            );
            manifest.backend.wasm_module = None;
        }
        manifest
            .specializations
            .retain(|specialization| valid_specialization_name(&specialization.name));
        plugins.push(manifest);
    }

    plugins.sort_by(|left, right| left.id.cmp(&right.id));
    PluginCatalog { plugins }
}

pub fn public_catalog(catalog: &PluginCatalog) -> PublicPluginCatalog {
    PublicPluginCatalog {
        plugins: catalog
            .plugins
            .iter()
            .map(|plugin| PublicPluginManifest {
                id: plugin.id.clone(),
                name: plugin.name.clone(),
                version: plugin.version.clone(),
                description: plugin.description.clone(),
                capabilities: plugin.capabilities.clone(),
                frontend: plugin.frontend.clone(),
                backend: plugin.backend.clone(),
                specializations: plugin.specializations.clone(),
            })
            .collect(),
    }
}

pub fn find_declared_asset<'a>(
    catalog: &'a PluginCatalog,
    plugin_id: &str,
    asset: &str,
) -> Option<(&'a ControllerPluginManifest, PathBuf)> {
    if !valid_plugin_id(plugin_id) || !valid_asset_path(asset) {
        return None;
    }
    let plugin = catalog
        .plugins
        .iter()
        .find(|plugin| plugin.id == plugin_id)?;
    let declared = plugin.frontend.modules.iter().any(|item| item == asset)
        || plugin.frontend.styles.iter().any(|item| item == asset);
    declared.then(|| (plugin, plugin.root_dir.join(asset)))
}

pub fn valid_plugin_id(value: &str) -> bool {
    let len = value.chars().count();
    (1..=64).contains(&len)
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn valid_specialization_name(value: &str) -> bool {
    let len = value.chars().count();
    (1..=64).contains(&len)
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
}

fn valid_asset_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.trim().is_empty()
        && value.len() <= 240
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}
