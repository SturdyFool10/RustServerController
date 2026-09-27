//! Scheduled world-backup support for the Minecraft specialization.
//!
//! Built on the generic `schedule_interval`/`on_schedule` hooks in
//! [`super::ServerSpecialization`]: the Minecraft specialization polls on a
//! fixed short cadence, and this module decides (based on the user-configured,
//! humantime-parsed interval) whether a backup is actually due, then performs
//! it as a tar archive compressed with the configured algorithm.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Compression algorithm used for backup archives.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompressionAlgorithm {
    None,
    Gzip,
    Zstd,
    Xz,
}

impl Default for CompressionAlgorithm {
    fn default() -> Self {
        CompressionAlgorithm::Xz
    }
}

impl CompressionAlgorithm {
    fn extension(self) -> &'static str {
        match self {
            CompressionAlgorithm::None => "tar",
            CompressionAlgorithm::Gzip => "tar.gz",
            CompressionAlgorithm::Zstd => "tar.zst",
            CompressionAlgorithm::Xz => "tar.xz",
        }
    }
}

/// Per-server, user-configurable backup settings.
///
/// Stored under the `"backup"` key of a Minecraft server's
/// `specialization_options`, merged against [`BackupConfig::default`] the
/// same way every other specialization option is defaulted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupConfig {
    /// Whether scheduled backups are enabled for this server.
    #[serde(default)]
    pub enabled: bool,

    /// How often backups are taken, e.g. `"6h"`, `"30m"`, `"1d"`.
    #[serde(default = "default_interval", with = "humantime_serde")]
    pub interval: Duration,

    /// World folder(s) to back up. Each entry is resolved relative to the
    /// server's working directory (the instance's CWD) unless it's itself an
    /// absolute path. Accepts either a single folder name (`"world"`) or a
    /// list (`["world", "world_nether", "world_the_end"]`); all configured
    /// folders are bundled into the same archive. When empty/unset, it's read
    /// from `level-name` in `server.properties` (falling back to Minecraft's
    /// own default of `"world"`).
    #[serde(default, deserialize_with = "deserialize_world_folders")]
    pub world_folder: Vec<String>,

    /// Directory backups are written to. Resolved relative to the server's
    /// working directory (the instance's CWD) unless it's itself an absolute
    /// path.
    #[serde(default = "default_backup_dir")]
    pub backup_dir: String,

    /// Compression algorithm applied to each backup archive.
    #[serde(default)]
    pub compression: CompressionAlgorithm,

    /// Compression level. Clamped to whatever range the chosen algorithm
    /// supports (gzip: 0-9, zstd: 1-22, xz: 0-9); ignored for `none`.
    #[serde(default = "default_compression_level")]
    pub compression_level: u32,

    /// How long a completed backup is kept before it's eligible for
    /// deletion, e.g. `"14d"`, `"2w"`.
    #[serde(default = "default_retention", with = "humantime_serde")]
    pub retention: Duration,

    /// Maximum total size that this server's backups are allowed to occupy
    /// on disk, as a human-readable size string (e.g. `"5GB"`, `"5GiB"`,
    /// `"750MB"`, plain byte counts also work). Decimal units (KB/MB/GB/TB,
    /// base 1000) and binary units (KiB/MiB/GiB/TiB, base 1024) are both
    /// accepted. When exceeded, the oldest backups are deleted first (the
    /// single newest backup is never deleted this way, so a quota smaller
    /// than one backup can't wipe out all history). `None` means unlimited.
    #[serde(default)]
    pub max_disk_size: Option<bytesize::ByteSize>,

    /// Message announced via `say` in the server console right before a
    /// backup starts. Set to an empty (or whitespace-only) string to disable
    /// the announcement entirely.
    ///
    /// Supports substitution tokens: `{server_name}`, `{world_folder}`
    /// (comma-separated list of the world folder(s) actually being backed
    /// up), `{date}`, `{time}`, and `{datetime}` (all UTC). The rendered
    /// message is sanitized before being sent — see
    /// [`render_announce_message`] for what that guards against.
    #[serde(default = "default_announce_message")]
    pub announce_message: String,
}

fn default_interval() -> Duration {
    Duration::from_secs(6 * 3600)
}

fn default_backup_dir() -> String {
    "backups".to_string()
}

fn default_compression_level() -> u32 {
    6
}

fn default_retention() -> Duration {
    Duration::from_secs(14 * 24 * 3600)
}

fn default_announce_message() -> String {
    "Backing up server world...".to_string()
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: default_interval(),
            world_folder: Vec::new(),
            backup_dir: default_backup_dir(),
            compression: CompressionAlgorithm::default(),
            compression_level: default_compression_level(),
            retention: default_retention(),
            max_disk_size: None,
            announce_message: default_announce_message(),
        }
    }
}

/// Accepts either a single folder name or a list of folder names for
/// `world_folder`, plus `null`/missing (both meaning "auto-detect").
fn deserialize_world_folders<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(folder)) => vec![folder],
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    })
}

/// Returns the default `"backup"` options block merged into a Minecraft
/// server's `default_options()`.
pub fn default_backup_options_json() -> Value {
    serde_json::to_value(BackupConfig::default()).unwrap_or(Value::Null)
}

/// Reads the `"backup"` block out of a server's `specialization_options`,
/// falling back to defaults for anything missing or malformed.
pub fn backup_config(options: Option<&Value>) -> BackupConfig {
    options
        .and_then(|options| options.get("backup"))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

/// Checks whether a scheduled backup for `server_name` is due, and if so,
/// performs it. Safe to call frequently; it's a no-op unless the configured
/// interval has actually elapsed since the newest backup on disk.
///
/// `state` is used only to announce `config.announce_message` through the
/// server's console (`say`) right before the archive starts; a server that
/// isn't running (or has no stdin) simply doesn't get an announcement.
pub async fn run_scheduled_backup(
    server_name: String,
    working_dir: String,
    config: BackupConfig,
    state: crate::app_state::AppState,
) {
    let backup_dir = resolve_dir(&working_dir, &config.backup_dir);
    if let Err(error) = tokio::fs::create_dir_all(&backup_dir).await {
        tracing::warn!(
            "Minecraft backup: failed to create backup dir for '{}': {}",
            server_name,
            error
        );
        return;
    }

    let prefix = sanitize_prefix(&server_name);
    if !is_backup_due(&backup_dir, &prefix, config.interval).await {
        return;
    }

    // Resolving world folder names (which may require a synchronous read of
    // server.properties) and checking which of them actually exist on disk
    // are both filesystem-blocking operations; run them inside
    // `spawn_blocking` so the async runtime's worker threads are never
    // blocked by them, however briefly.
    let world_dirs = {
        let working_dir = working_dir.clone();
        let configured_world_folders = config.world_folder.clone();
        let server_name = server_name.clone();
        tokio::task::spawn_blocking(move || {
            resolve_world_dirs(&working_dir, &configured_world_folders, &server_name)
        })
        .await
        .unwrap_or_default()
    };
    if world_dirs.is_empty() {
        tracing::warn!(
            "Minecraft backup: no configured world folders exist for '{}'; skipping backup",
            server_name
        );
        return;
    }

    if let Some(message) = render_announce_message(&config.announce_message, &server_name, &world_dirs)
    {
        let command = format!("say {message}");
        if !crate::servers::write_line_to_server_stdin(&state, &server_name, &command).await {
            tracing::debug!(
                "Minecraft backup: couldn't announce backup for '{}' (server not running or stdin unavailable)",
                server_name
            );
        }
    }

    let prefix_for_blocking = prefix.clone();
    let backup_dir_for_blocking = backup_dir.clone();
    let compression = config.compression;
    let compression_level = config.compression_level;
    let retention = config.retention;
    let max_disk_size = config.max_disk_size;

    let result = tokio::task::spawn_blocking(move || {
        let archive_path = create_backup_archive(
            &world_dirs,
            &backup_dir_for_blocking,
            &prefix_for_blocking,
            compression,
            compression_level,
        )?;
        enforce_retention(
            &backup_dir_for_blocking,
            &prefix_for_blocking,
            retention,
            max_disk_size,
        )?;
        Ok::<PathBuf, std::io::Error>(archive_path)
    })
    .await;

    match result {
        Ok(Ok(path)) => {
            tracing::info!(
                "Minecraft backup for '{}' completed: {}",
                server_name,
                path.display()
            );
        }
        Ok(Err(error)) => {
            tracing::warn!("Minecraft backup for '{}' failed: {}", server_name, error);
        }
        Err(error) => {
            tracing::warn!(
                "Minecraft backup task for '{}' panicked: {}",
                server_name,
                error
            );
        }
    }
}

/// Renders `announce_message`'s substitution tokens and sanitizes the result
/// for safe use as a single line of server console input.
///
/// Supported tokens: `{server_name}`, `{world_folder}` (the resolved world
/// folder name(s) being backed up, comma-separated), `{date}` (UTC,
/// `YYYY-MM-DD`), `{time}` (UTC, `HH:MM:SS`), `{datetime}` (UTC,
/// `YYYY-MM-DD HH:MM:SS`).
///
/// Returns `None` if the template is empty/whitespace-only (used to disable
/// the announcement) or if sanitization leaves nothing behind.
fn render_announce_message(
    template: &str,
    server_name: &str,
    world_dirs: &[PathBuf],
) -> Option<String> {
    let trimmed = template.trim();
    if trimmed.is_empty() {
        return None;
    }

    let world_folder_names = world_dirs
        .iter()
        .map(|dir| {
            dir.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("world")
        })
        .collect::<Vec<_>>()
        .join(", ");

    let now = chrono::Utc::now();
    let rendered = trimmed
        .replace("{server_name}", &strip_control_chars(server_name))
        .replace("{world_folder}", &strip_control_chars(&world_folder_names))
        .replace("{date}", &now.format("%Y-%m-%d").to_string())
        .replace("{time}", &now.format("%H:%M:%S").to_string())
        .replace("{datetime}", &now.format("%Y-%m-%d %H:%M:%S").to_string());

    // The critical sanitization step: this message is about to be written
    // directly to the server process's stdin as a single console command
    // (`say <message>`). Every substituted value above is either
    // admin-configured or filesystem-derived, but stripping control
    // characters here too means a stray `\r`/`\n` anywhere in the final
    // string (however it got there) can never inject a second line of input
    // for the server to interpret as its own console command.
    let sanitized = strip_control_chars(&rendered).trim().to_string();
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

/// Removes all control characters (including `\r`, `\n`, and `\0`) from a
/// string. See [`render_announce_message`] for why this matters.
fn strip_control_chars(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_control()).collect()
}

/// Resolves configured (or auto-detected) world folder names to their
/// on-disk paths, relative to the server's working directory unless a name
/// is itself an absolute path. Folders that don't exist are logged and
/// omitted rather than failing the whole backup. Synchronous by design —
/// callers must run this inside `spawn_blocking`.
fn resolve_world_dirs(working_dir: &str, configured: &[String], server_name: &str) -> Vec<PathBuf> {
    let world_folders: Vec<String> = if configured.is_empty() {
        vec![read_level_name(working_dir)]
    } else {
        configured
            .iter()
            .map(|folder| folder.trim().to_string())
            .filter(|folder| !folder.is_empty())
            .collect()
    };

    let mut world_dirs = Vec::with_capacity(world_folders.len());
    for folder in &world_folders {
        let world_dir = resolve_dir(working_dir, folder);
        if world_dir.is_dir() {
            world_dirs.push(world_dir);
        } else {
            tracing::warn!(
                "Minecraft backup: world folder '{}' not found for '{}'; omitting from backup",
                world_dir.display(),
                server_name
            );
        }
    }
    world_dirs
}

fn resolve_dir(working_dir: &str, configured: &str) -> PathBuf {
    let configured_path = Path::new(configured);
    if configured_path.is_absolute() {
        configured_path.to_path_buf()
    } else {
        Path::new(working_dir).join(configured_path)
    }
}

fn sanitize_prefix(server_name: &str) -> String {
    let sanitized: String = server_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "server".to_string()
    } else {
        sanitized
    }
}

async fn is_backup_due(backup_dir: &Path, prefix: &str, interval: Duration) -> bool {
    match newest_backup_mtime(backup_dir, prefix).await {
        Some(mtime) => match SystemTime::now().duration_since(mtime) {
            Ok(elapsed) => elapsed >= interval,
            Err(_) => false,
        },
        None => true,
    }
}

async fn newest_backup_mtime(backup_dir: &Path, prefix: &str) -> Option<SystemTime> {
    let mut read_dir = tokio::fs::read_dir(backup_dir).await.ok()?;
    let mut newest: Option<SystemTime> = None;
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !is_backup_file(file_name, prefix) {
            continue;
        }
        let Ok(metadata) = entry.metadata().await else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        newest = Some(newest.map_or(modified, |current| current.max(modified)));
    }
    newest
}

fn is_backup_file(file_name: &str, prefix: &str) -> bool {
    if !file_name.starts_with(prefix) {
        return false;
    }
    [
        CompressionAlgorithm::None,
        CompressionAlgorithm::Gzip,
        CompressionAlgorithm::Zstd,
        CompressionAlgorithm::Xz,
    ]
    .iter()
    .any(|algo| file_name.ends_with(algo.extension()))
}

/// Reads `level-name` from `server.properties`, defaulting to Minecraft's own
/// default world folder name (`"world"`) when unset or unreadable.
fn read_level_name(working_dir: &str) -> String {
    let mut path = working_dir.to_string();
    if !(path.ends_with('/') || path.ends_with('\\')) {
        path.push('/');
    }
    path.push_str("server.properties");

    let Ok(contents) = crate::files::read_file(path.as_str()) else {
        return "world".to_string();
    };
    let Ok(regex) = Regex::new(r"(?m)^level-name=(.+)$") else {
        return "world".to_string();
    };
    regex
        .captures(&contents)
        .and_then(|caps| caps.get(1))
        .map(|value| value.as_str().trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "world".to_string())
}

/// Appends every configured world directory into the archive, each as its
/// own top-level entry named after that directory (so `world`,
/// `world_nether`, and `world_the_end` all land side by side in the tar).
fn append_world_dirs<W: std::io::Write>(
    builder: &mut tar::Builder<W>,
    world_dirs: &[PathBuf],
) -> std::io::Result<()> {
    for world_dir in world_dirs {
        let world_dir_name = world_dir
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("world"));
        builder.append_dir_all(world_dir_name, world_dir)?;
    }
    Ok(())
}

/// Creates a compressed tar archive of `world_dirs` inside `backup_dir`,
/// writing to a `.tmp` file first and renaming it into place only once the
/// archive has been fully written and flushed. This keeps a crash or I/O
/// error mid-backup from ever leaving a partial file that could be mistaken
/// for a real backup (or corrupt retention accounting). Synchronous by
/// design — callers must run this inside `spawn_blocking`.
fn create_backup_archive(
    world_dirs: &[PathBuf],
    backup_dir: &Path,
    prefix: &str,
    compression: CompressionAlgorithm,
    compression_level: u32,
) -> std::io::Result<PathBuf> {
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let file_name = format!("{prefix}-{timestamp}.{}", compression.extension());
    let final_path = backup_dir.join(&file_name);
    let tmp_path = backup_dir.join(format!("{file_name}.tmp"));

    let file = std::fs::File::create(&tmp_path)?;
    let write_result = (|| -> std::io::Result<()> {
        match compression {
            CompressionAlgorithm::None => {
                let mut builder = tar::Builder::new(file);
                append_world_dirs(&mut builder, world_dirs)?;
                builder.into_inner()?.sync_all()
            }
            CompressionAlgorithm::Gzip => {
                let level = compression_level.min(9);
                let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::new(level));
                let mut builder = tar::Builder::new(encoder);
                append_world_dirs(&mut builder, world_dirs)?;
                builder.into_inner()?.finish()?.sync_all()
            }
            CompressionAlgorithm::Zstd => {
                let level = compression_level.clamp(1, 22) as i32;
                let encoder = zstd::stream::write::Encoder::new(file, level)?;
                let mut builder = tar::Builder::new(encoder);
                append_world_dirs(&mut builder, world_dirs)?;
                builder.into_inner()?.finish()?.sync_all()
            }
            CompressionAlgorithm::Xz => {
                let level = compression_level.min(9);
                let encoder = xz2::write::XzEncoder::new(file, level);
                let mut builder = tar::Builder::new(encoder);
                append_world_dirs(&mut builder, world_dirs)?;
                builder.into_inner()?.finish()?.sync_all()
            }
        }
    })();

    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(error);
    }

    std::fs::rename(&tmp_path, &final_path)?;
    Ok(final_path)
}

/// Deletes backups older than `retention`, then (if a disk quota is set)
/// deletes the oldest remaining backups until the total is back under quota.
/// The single newest backup is always kept, even if it alone exceeds the
/// quota, so a too-small quota can't delete every backup a server has.
fn enforce_retention(
    backup_dir: &Path,
    prefix: &str,
    retention: Duration,
    max_disk_size: Option<bytesize::ByteSize>,
) -> std::io::Result<()> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(backup_dir)? {
        let entry = entry?;
        let Some(file_name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !is_backup_file(&file_name, prefix) || file_name.ends_with(".tmp") {
            continue;
        }
        let metadata = entry.metadata()?;
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        entries.push((entry.path(), modified, metadata.len()));
    }
    entries.sort_by_key(|(_, modified, _)| *modified);

    let cutoff = SystemTime::now().checked_sub(retention);
    if let Some(cutoff) = cutoff {
        entries.retain(|(path, modified, _)| {
            if *modified < cutoff {
                let _ = std::fs::remove_file(path);
                false
            } else {
                true
            }
        });
    }

    if let Some(max_bytes) = max_disk_size.map(|size| size.as_u64()) {
        let mut total: u64 = entries.iter().map(|(_, _, size)| size).sum();
        let mut index = 0;
        // Keep at least the newest backup regardless of quota.
        while total > max_bytes && entries.len() - index > 1 {
            let (path, _, size) = &entries[index];
            if std::fs::remove_file(path).is_ok() {
                total = total.saturating_sub(*size);
            }
            index += 1;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips_through_json() {
        let json = default_backup_options_json();
        let parsed = backup_config(Some(&serde_json::json!({ "backup": json })));
        assert!(!parsed.enabled);
        assert_eq!(parsed.interval, Duration::from_secs(6 * 3600));
        assert_eq!(parsed.retention, Duration::from_secs(14 * 24 * 3600));
        assert_eq!(parsed.compression, CompressionAlgorithm::Xz);
        assert_eq!(parsed.announce_message, "Backing up server world...");
    }

    #[test]
    fn announce_message_substitutes_known_tokens() {
        let world_dirs = vec![PathBuf::from("/srv/mc/world"), PathBuf::from("/srv/mc/world_nether")];
        let message = render_announce_message(
            "[{server_name}] backing up {world_folder} at {time}",
            "SurvivalServer",
            &world_dirs,
        )
        .expect("non-empty template should render");
        assert!(message.starts_with("[SurvivalServer] backing up world, world_nether at "));
    }

    #[test]
    fn announce_message_empty_or_whitespace_disables_announcement() {
        assert_eq!(render_announce_message("", "Server", &[]), None);
        assert_eq!(render_announce_message("   ", "Server", &[]), None);
    }

    #[test]
    fn announce_message_strips_control_characters_to_prevent_stdin_injection() {
        // A crafted template (or a server_name pulled from config) containing
        // a raw newline must never survive into the rendered message: that
        // newline is what would otherwise become a second line of raw input
        // on the server's stdin, executed as its own console command.
        let message = render_announce_message(
            "Backing up now\r\nop {server_name}",
            "attacker\ntell everyone hi",
            &[],
        )
        .expect("template has non-control content");
        assert!(!message.contains('\n'));
        assert!(!message.contains('\r'));
        assert_eq!(message, "Backing up nowop attackertell everyone hi");
    }

    #[test]
    fn humantime_strings_parse_into_durations() {
        let value = serde_json::json!({
            "backup": {
                "enabled": true,
                "interval": "30m",
                "retention": "2w",
                "compression": "gzip",
                "compression_level": 9,
                "max_disk_size": "1GiB",
            }
        });
        let config = backup_config(Some(&value));
        assert!(config.enabled);
        assert_eq!(config.interval, Duration::from_secs(30 * 60));
        assert_eq!(config.retention, Duration::from_secs(14 * 24 * 3600));
        assert_eq!(config.compression, CompressionAlgorithm::Gzip);
        assert_eq!(config.compression_level, 9);
        assert_eq!(config.max_disk_size, Some(bytesize::ByteSize::gib(1)));
    }

    #[test]
    fn max_disk_size_accepts_decimal_binary_and_plain_byte_forms() {
        let decimal = backup_config(Some(&serde_json::json!({
            "backup": { "max_disk_size": "5GB" }
        })));
        assert_eq!(decimal.max_disk_size, Some(bytesize::ByteSize::gb(5)));

        let binary = backup_config(Some(&serde_json::json!({
            "backup": { "max_disk_size": "5GiB" }
        })));
        assert_eq!(binary.max_disk_size, Some(bytesize::ByteSize::gib(5)));

        let plain_bytes = backup_config(Some(&serde_json::json!({
            "backup": { "max_disk_size": 2048 }
        })));
        assert_eq!(plain_bytes.max_disk_size, Some(bytesize::ByteSize::b(2048)));

        let unset = backup_config(Some(&serde_json::json!({ "backup": {} })));
        assert_eq!(unset.max_disk_size, None);
    }

    #[test]
    fn world_folder_accepts_single_string_list_or_null() {
        let single = backup_config(Some(&serde_json::json!({
            "backup": { "world_folder": "world" }
        })));
        assert_eq!(single.world_folder, vec!["world".to_string()]);

        let list = backup_config(Some(&serde_json::json!({
            "backup": { "world_folder": ["world", "world_nether", "world_the_end"] }
        })));
        assert_eq!(
            list.world_folder,
            vec![
                "world".to_string(),
                "world_nether".to_string(),
                "world_the_end".to_string(),
            ]
        );

        let unset = backup_config(Some(&serde_json::json!({
            "backup": { "world_folder": null }
        })));
        assert!(unset.world_folder.is_empty());

        let missing = backup_config(Some(&serde_json::json!({ "backup": {} })));
        assert!(missing.world_folder.is_empty());
    }

    #[test]
    fn is_backup_file_matches_prefix_and_known_extensions() {
        assert!(is_backup_file("MyServer-20260101-000000.tar.xz", "MyServer"));
        assert!(is_backup_file("MyServer-20260101-000000.tar.gz", "MyServer"));
        assert!(!is_backup_file("Other-20260101-000000.tar.xz", "MyServer"));
        assert!(!is_backup_file("MyServer-20260101-000000.tar.xz.tmp", "MyServer"));
    }

    #[test]
    fn create_and_retain_backup_round_trip() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("rsc-backup-test-{}", uuid::Uuid::new_v4()));
        let world_dir = root.join("world");
        let backup_dir = root.join("backups");
        std::fs::create_dir_all(&world_dir)?;
        std::fs::create_dir_all(&backup_dir)?;
        std::fs::write(world_dir.join("level.dat"), b"fake level data")?;

        let path = create_backup_archive(
            &[world_dir],
            &backup_dir,
            "TestServer",
            CompressionAlgorithm::Xz,
            6,
        )?;
        assert!(path.exists());
        assert!(path.to_string_lossy().ends_with(".tar.xz"));

        // No .tmp file should remain.
        let leftover_tmp = std::fs::read_dir(&backup_dir)?
            .filter_map(|entry| entry.ok())
            .any(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"));
        assert!(!leftover_tmp);

        enforce_retention(&backup_dir, "TestServer", Duration::from_secs(3600), None)?;
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
