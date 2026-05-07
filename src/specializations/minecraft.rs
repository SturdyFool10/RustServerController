use super::{player_activity::PlayerActivityTracker, ServerSpecialization};
use crate::ansi_to_html::{ansi_to_plain_text, escape_html};
use crate::app_state::AppState;
use crate::configuration::{
    Config, MinecraftAccountFilterDetail, MinecraftAccountFilterDetailGroup,
    MinecraftIpBanFilterDetail,
};
use crate::controlled_program::ControlledProgramInstance;
use crate::messages::ConfigInfo;
use crate::servers::broadcast_json;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Specialization for Minecraft servers.
///
/// Handles Minecraft-specific logic such as parsing player join/leave events,
/// tracking readiness, and auto-accepting the EULA if needed.

#[derive(Default)]

pub struct MinecraftSpecialization {
    player_count: usize,

    max_players: usize,

    ready: bool,

    player_list: Vec<String>,

    player_activity: PlayerActivityTracker,

    last_status_update: bool,

    account_filter_watcher_stop: Option<watch::Sender<bool>>,

    account_filter_watcher: Option<JoinHandle<()>>,
}

impl ServerSpecialization for MinecraftSpecialization {
    fn pre_init(
        &mut self,

        _env: &mut std::collections::HashMap<String, String>,

        _descriptor: &crate::controlled_program::ControlledProgramDescriptor,
    ) {

        // Default: do nothing for Minecraft
    }

    fn default_options(&self) -> serde_json::Value {
        json!({
            "auto_accept_eula": true,
            "account_filter_groups": [],
        })
    }

    fn has_status_update(&self) -> bool {
        self.last_status_update
    }

    fn set_status_update_sent(&mut self) {
        self.last_status_update = false;
    }

    /// Initialize the Minecraft specialization for a server instance.
    ///
    /// Reads the `max-players` value from `server.properties` if available,
    /// and sets up the initial specialized_server_info state.
    fn init(&mut self, instance: &mut ControlledProgramInstance) {
        // Try to read max-players from server.properties

        let mut path_str = instance.working_dir.clone();

        if !(path_str.ends_with("/") || path_str.ends_with("\\")) {
            path_str += "/";
        }

        path_str += "server.properties";

        let file_result = crate::files::read_file(path_str.as_str());

        let mut max_players = 20; // Minecraft's default

        if let Ok(val) = file_result {
            if let Ok(regex) = Regex::new(r"max-players=(\d+)") {
                if let Some(caps) = regex.captures(&val) {
                    if let Some(mp) = caps.get(1) {
                        if let Ok(mp) = mp.as_str().parse::<usize>() {
                            max_players = mp;
                        }
                    }
                }
            }
        }

        self.player_count = 0;

        self.max_players = max_players;

        self.ready = false;

        self.player_list = Vec::new();

        self.player_activity =
            PlayerActivityTracker::for_server(&instance.server_uuid, &instance.name, "Minecraft");
        sync_account_filters(instance);

        self.last_status_update = true;
    }

    /// Parses a single output line from the Minecraft server process.
    ///
    /// Updates player count, readiness, and player list in specialized_server_info.
    /// Returns a colorized HTML string for the log line.
    fn parse_output(
        &mut self,

        line: String,

        _instance: &mut ControlledProgramInstance,
    ) -> Option<String> {
        let line = ansi_to_plain_text(&line);

        // Player join regex

        let join_pattern = match Regex::new(
            r"(\w+)\[/\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}:\d+\] logged in with entity id",
        ) {
            Ok(regex) => regex,
            Err(error) => {
                tracing::error!("Invalid Minecraft join regex: {}", error);
                return Some(colorize_minecraft_log_line(&line));
            }
        };

        // Player leave regex

        let leave_pattern = match Regex::new(r"\]: (\w+) lost connection") {
            Ok(regex) => regex,
            Err(error) => {
                tracing::error!("Invalid Minecraft leave regex: {}", error);
                return Some(colorize_minecraft_log_line(&line));
            }
        };

        // Ready regex

        let ready_pattern = match Regex::new(r#"Done \(\d+\.\d+s\)! For help, type "help""#) {
            Ok(regex) => regex,
            Err(error) => {
                tracing::error!("Invalid Minecraft ready regex: {}", error);
                return Some(colorize_minecraft_log_line(&line));
            }
        };

        // Track if status update occurs
        let mut status_update = false;

        // Player join

        if let Some(player_name) = join_pattern
            .captures(&line)
            .and_then(|caps| caps.get(1))
            .map(|player| player.as_str())
        {
            self.player_activity.player_joined(player_name);
            self.player_count = self.player_activity.online_count();
            self.player_list = self.player_activity.online_names();
            status_update = true;
        }

        // Player leave

        if let Some(player_name) = leave_pattern
            .captures(&line)
            .and_then(|caps| caps.get(1))
            .map(|player| player.as_str())
        {
            self.player_activity.player_left(player_name);
            self.player_count = self.player_activity.online_count();
            self.player_list = self.player_activity.online_names();
            status_update = true;
        }

        // Server ready

        if ready_pattern.is_match(&line) && !self.ready {
            self.ready = true;

            status_update = true;
        }

        self.last_status_update |= status_update;

        // Colorize the line using bracket counting

        Some(colorize_minecraft_log_line(&line))
    }

    /// Handles logic when the Minecraft server process exits.
    ///
    /// If the EULA was not accepted, automatically patches `eula.txt` and restarts the server.
    fn on_exit(
        &mut self,
        instance: &mut ControlledProgramInstance,
        state: &AppState,
        _exit_code: i32,
    ) {
        if self.player_activity.mark_all_offline() {
            self.player_count = 0;
            self.player_list = Vec::new();
            self.last_status_update = true;
        }
        self.stop_account_filter_watcher();
        self.start_account_filter_watcher(instance, state.clone());

        // Robust EULA auto-accept: check eula.txt for eula=false and patch/restart if needed
        let state = state.clone();
        let name = instance.name.clone();
        let server_uuid = instance.server_uuid.clone();
        let exe_path = instance.executable_path.clone();
        let args = instance.command_line_args.clone();
        let working_dir = instance.working_dir.clone();
        let specialized_server_type = instance.specialized_server_type.clone();
        let specialization_options = instance.specialization_options.clone();
        let crash_prevention = instance.crash_prevention;
        tokio::spawn(async move {
            let auto_accept_eula = specialization_options
                .as_ref()
                .and_then(|options| options.get("auto_accept_eula"))
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            if !auto_accept_eula {
                return;
            }

            // Build eula.txt path
            let mut eula_path = working_dir.clone();
            if !(eula_path.ends_with('/') || eula_path.ends_with('\\')) {
                eula_path += "/";
            }
            eula_path += "eula.txt";
            let eula_file_path = Path::new(&eula_path);

            // Check if eula.txt exists and contains eula=false
            let needs_patch = match tokio::fs::read_to_string(&eula_file_path).await {
                Ok(contents) => contents.lines().any(|l| l.trim() == "eula=false"),
                Err(_) => false,
            };

            if needs_patch {
                // Patch eula.txt to eula=true
                let _ = tokio::fs::write(&eula_file_path, b"eula=true\n").await;

                // Send message to UI
                let msg = "<span style=\"color: var(--warning, #FFA500);\">[EULA was set to false. Automatically set eula=true and restarting the server.\nby continuing, you are agreeing to Mojang's EULA]</span>";
                let eula_console_msg = crate::messages::ConsoleOutput {
                    r#type: "ServerOutput".to_owned(),
                    output: msg.to_string(),
                    server_name: name.clone(),
                    server_type: specialized_server_type.clone(),
                };
                crate::servers::broadcast_json(&state, &eula_console_msg);

                // Restart the server
                let mut desc = crate::controlled_program::ControlledProgramDescriptor::new(
                    &name,
                    &exe_path,
                    args,
                    working_dir,
                );
                desc.specialized_server_type = specialized_server_type;
                desc.server_uuid = Some(server_uuid);
                desc.specialization_options = specialization_options;
                desc.crash_prevention = crash_prevention;
                let mut servers = state.servers.lock().await;
                if let Some(instance) = crate::servers::create_instance(&state, desc) {
                    servers.push(instance);
                }
            }
        });
    }

    /// Returns the current status for this specialization.
    ///
    /// For Minecraft, this should return the current specialized_server_info if available.
    /// Returns the instance's specialized_server_info, or Null if not present.
    fn get_status(&self) -> serde_json::Value {
        json!({
            "player_count": self.player_count,
            "max_players": self.max_players,
            "ready": self.ready,
            "player_list": self.player_list,
        })
    }

    fn get_stats(&self) -> serde_json::Value {
        json!({
            "Players Online": self.player_count,
            "Player Slots": self.max_players,
            "Ready": self.ready,
            "Online Names": self.player_list.len(),
            "Observed Names": self.player_activity.known_player_count(),
            "Total Session Hours": self.player_activity.total_hours(),
            "Name Activity": self.player_activity.summaries(),
            "Recent Sessions": self.player_activity.recent_sessions(25),
            "Timeframe Stats": self.player_activity.timeframe_stats(),
        })
    }
}

impl MinecraftSpecialization {
    fn start_account_filter_watcher(
        &mut self,
        instance: &ControlledProgramInstance,
        state: AppState,
    ) {
        self.stop_account_filter_watcher();
        let Some(group_ids) = account_filter_group_ids(instance.specialization_options.as_ref())
        else {
            return;
        };
        if group_ids.is_empty() {
            return;
        }

        let working_dir = instance.working_dir.clone();
        let server_name = instance.name.clone();
        let (stop_tx, stop_rx) = watch::channel(false);
        self.account_filter_watcher_stop = Some(stop_tx);
        self.account_filter_watcher = Some(tokio::spawn(watch_account_filter_files(
            server_name,
            working_dir,
            group_ids,
            state,
            stop_rx,
        )));
    }

    fn stop_account_filter_watcher(&mut self) {
        if let Some(stop) = self.account_filter_watcher_stop.take() {
            let _ = stop.send(true);
        }
        if let Some(handle) = self.account_filter_watcher.take() {
            handle.abort();
        }
    }
}

impl Drop for MinecraftSpecialization {
    fn drop(&mut self) {
        self.stop_account_filter_watcher();
    }
}

fn sync_account_filters(instance: &ControlledProgramInstance) {
    let Some(group_ids) = account_filter_group_ids(instance.specialization_options.as_ref()) else {
        return;
    };
    sync_account_filters_for(&instance.working_dir, &group_ids);
}

async fn watch_account_filter_files(
    server_name: String,
    working_dir: String,
    group_ids: Vec<String>,
    state: AppState,
    mut stop: watch::Receiver<bool>,
) {
    let mut snapshot = filter_file_snapshot(&working_dir);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let next_snapshot = filter_file_snapshot(&working_dir);
                if next_snapshot != snapshot {
                    tracing::debug!("Minecraft account filter files changed for '{}'; syncing groups", server_name);
                    sync_account_filters_for_state(&state, &working_dir, &group_ids).await;
                    snapshot = filter_file_snapshot(&working_dir);
                }
            }
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break;
                }
            }
        }
    }
}

async fn sync_account_filters_for_state(state: &AppState, working_dir: &str, group_ids: &[String]) {
    let mut config = state.config.lock().await;
    if merge_instance_filter_files_into_groups(&mut config, working_dir, group_ids) {
        config.update_config_file("config.json");
        let message = ConfigInfo {
            r#type: "ConfigInfo".to_string(),
            config: config.clone(),
        };
        broadcast_json(state, &message);
    }
    fan_out_effective_filter_files(&config, group_ids);
}

fn sync_account_filters_for(working_dir: &str, group_ids: &[String]) {
    if group_ids.is_empty() {
        return;
    }

    let mut config = match crate::files::load_json("config.json") {
        config if !config.minecraft_account_filter_detail_groups.is_empty() => config,
        _ => return,
    };

    if merge_instance_filter_files_into_groups(&mut config, working_dir, group_ids) {
        config.update_config_file("config.json");
    }

    fan_out_effective_filter_files(&config, group_ids);
}

fn fan_out_effective_filter_files(config: &Config, group_ids: &[String]) {
    let minecraft_servers: Vec<_> = config
        .servers
        .iter()
        .filter(|server| server.specialized_server_type.as_deref() == Some("Minecraft"))
        .filter_map(|server| {
            let server_group_ids =
                account_filter_group_ids(server.specialization_options.as_ref())?;
            if server_group_ids
                .iter()
                .any(|group| group_ids.contains(group))
            {
                Some((server.working_dir.clone(), server_group_ids))
            } else {
                None
            }
        })
        .collect();

    for (working_dir, server_group_ids) in minecraft_servers {
        write_effective_filter_files(&config, &working_dir, &server_group_ids);
    }
}

fn filter_file_snapshot(working_dir: &str) -> Vec<Option<(SystemTime, u64)>> {
    ["whitelist.json", "banned-players.json", "banned-ips.json"]
        .iter()
        .map(|file_name| filter_file_state(Path::new(working_dir).join(file_name)))
        .collect()
}

fn filter_file_state(path: PathBuf) -> Option<(SystemTime, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

fn merge_instance_filter_files_into_groups(
    config: &mut Config,
    working_dir: &str,
    group_ids: &[String],
) -> bool {
    let whitelist =
        read_minecraft_filter_file::<MinecraftAccountFilterDetail>(working_dir, "whitelist.json");
    let bans = read_minecraft_filter_file::<MinecraftAccountFilterDetail>(
        working_dir,
        "banned-players.json",
    );
    let ip_bans =
        read_minecraft_filter_file::<MinecraftIpBanFilterDetail>(working_dir, "banned-ips.json");
    let mut changed = false;

    for group in config
        .minecraft_account_filter_detail_groups
        .iter_mut()
        .filter(|group| {
            group
                .uuid
                .as_ref()
                .is_some_and(|uuid| group_ids.contains(uuid))
        })
    {
        changed |= merge_account_entries(&mut group.whitelist, &whitelist, false);
        changed |= merge_account_entries(&mut group.ban_list, &bans, true);
        changed |= merge_ip_entries(&mut group.banned_ips, &ip_bans);
    }

    changed
}

fn write_effective_filter_files(config: &Config, working_dir: &str, group_ids: &[String]) {
    let selected: Vec<&MinecraftAccountFilterDetailGroup> = config
        .minecraft_account_filter_detail_groups
        .iter()
        .filter(|group| {
            group
                .uuid
                .as_ref()
                .is_some_and(|uuid| group_ids.contains(uuid))
        })
        .collect();
    if selected.is_empty() {
        return;
    }

    let mut whitelist = BTreeMap::new();
    let mut bans = BTreeMap::new();
    let mut ip_bans = BTreeMap::new();
    for group in selected {
        for entry in &group.whitelist {
            if !entry.name.trim().is_empty() {
                whitelist.insert(entry.name.to_ascii_lowercase(), entry);
            }
        }
        for entry in &group.ban_list {
            if !entry.name.trim().is_empty() {
                bans.insert(entry.name.to_ascii_lowercase(), entry);
            }
        }
        for entry in &group.banned_ips {
            if !entry.ip.trim().is_empty() {
                ip_bans.insert(entry.ip.clone(), entry);
            }
        }
    }

    if let Err(error) = write_minecraft_filter_file(
        working_dir,
        "whitelist.json",
        whitelist.values().map(|entry| {
            json!({
                "uuid": entry.uuid.clone().unwrap_or_default(),
                "name": entry.name,
            })
        }),
    ) {
        tracing::warn!("Failed to sync Minecraft whitelist: {}", error);
    }

    if let Err(error) = write_minecraft_filter_file(
        working_dir,
        "banned-players.json",
        bans.values().map(|entry| {
            json!({
                "uuid": entry.uuid.clone().unwrap_or_default(),
                "name": entry.name,
                "created": entry.created.clone().unwrap_or_else(|| "1970-01-01 00:00:00 +0000".to_string()),
                "source": entry.source.clone().unwrap_or_else(|| "RustServerController".to_string()),
                "expires": entry.expires.clone().unwrap_or_else(|| "forever".to_string()),
                "reason": entry.reason.clone().unwrap_or_else(|| "Banned by administrator".to_string()),
            })
        }),
    ) {
        tracing::warn!("Failed to sync Minecraft ban list: {}", error);
    }

    if let Err(error) = write_minecraft_filter_file(
        working_dir,
        "banned-ips.json",
        ip_bans.values().map(|entry| {
            json!({
                "ip": entry.ip,
                "created": entry.created.clone().unwrap_or_else(|| "1970-01-01 00:00:00 +0000".to_string()),
                "source": entry.source.clone().unwrap_or_else(|| "RustServerController".to_string()),
                "expires": entry.expires.clone().unwrap_or_else(|| "forever".to_string()),
                "reason": entry.reason.clone().unwrap_or_else(|| "Banned by administrator".to_string()),
            })
        }),
    ) {
        tracing::warn!("Failed to sync Minecraft IP ban list: {}", error);
    }
}

fn merge_account_entries(
    target: &mut Vec<MinecraftAccountFilterDetail>,
    incoming: &[MinecraftAccountFilterDetail],
    include_ban_metadata: bool,
) -> bool {
    let mut changed = false;
    for incoming_entry in incoming {
        if incoming_entry.name.trim().is_empty() {
            continue;
        }
        let key = incoming_entry.name.to_ascii_lowercase();
        match target
            .iter_mut()
            .find(|entry| entry.name.to_ascii_lowercase() == key)
        {
            Some(existing) => {
                changed |= fill_missing(&mut existing.uuid, &incoming_entry.uuid);
                if include_ban_metadata {
                    changed |= fill_missing(&mut existing.created, &incoming_entry.created);
                    changed |= fill_missing(&mut existing.source, &incoming_entry.source);
                    changed |= fill_missing(&mut existing.expires, &incoming_entry.expires);
                    changed |= fill_missing(&mut existing.reason, &incoming_entry.reason);
                }
            }
            None => {
                target.push(incoming_entry.clone());
                changed = true;
            }
        }
    }
    changed
}

fn merge_ip_entries(
    target: &mut Vec<MinecraftIpBanFilterDetail>,
    incoming: &[MinecraftIpBanFilterDetail],
) -> bool {
    let mut changed = false;
    for incoming_entry in incoming {
        if incoming_entry.ip.trim().is_empty() {
            continue;
        }
        match target
            .iter_mut()
            .find(|entry| entry.ip == incoming_entry.ip)
        {
            Some(existing) => {
                changed |= fill_missing(&mut existing.created, &incoming_entry.created);
                changed |= fill_missing(&mut existing.source, &incoming_entry.source);
                changed |= fill_missing(&mut existing.expires, &incoming_entry.expires);
                changed |= fill_missing(&mut existing.reason, &incoming_entry.reason);
            }
            None => {
                target.push(incoming_entry.clone());
                changed = true;
            }
        }
    }
    changed
}

fn fill_missing(target: &mut Option<String>, incoming: &Option<String>) -> bool {
    if target
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return false;
    }
    let Some(value) = incoming.as_ref().filter(|value| !value.trim().is_empty()) else {
        return false;
    };
    *target = Some(value.clone());
    true
}

fn read_minecraft_filter_file<T: serde::de::DeserializeOwned>(
    working_dir: &str,
    file_name: &str,
) -> Vec<T> {
    let path = Path::new(working_dir).join(file_name);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str::<Vec<T>>(&contents).ok())
        .unwrap_or_default()
}

fn account_filter_group_ids(options: Option<&Value>) -> Option<Vec<String>> {
    options
        .and_then(|options| options.get("account_filter_groups"))
        .and_then(Value::as_array)
        .map(|groups| {
            groups
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
}

fn write_minecraft_filter_file(
    working_dir: &str,
    file_name: &str,
    entries: impl Iterator<Item = Value>,
) -> std::io::Result<()> {
    let path = Path::new(working_dir).join(file_name);
    let json = serde_json::to_string_pretty(&entries.collect::<Vec<_>>())?;
    if std::fs::read_to_string(&path).ok().as_deref() == Some(json.as_str()) {
        return Ok(());
    }
    std::fs::write(path, json)
}

/// Factory function for Minecraft specialization.
///
/// Returns a boxed instance of `MinecraftSpecialization`.
pub fn factory() -> Box<dyn ServerSpecialization> {
    Box::new(MinecraftSpecialization::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specializations::ServerSpecialization;
    use tokio::process::Command;

    fn test_instance() -> std::io::Result<ControlledProgramInstance> {
        let child = Command::new("sh").arg("-c").arg("exit 0").spawn()?;

        Ok(ControlledProgramInstance {
            name: "test".to_string(),
            server_uuid: uuid::Uuid::new_v4().to_string(),
            executable_path: "sh".to_string(),
            command_line_args: vec!["-c".to_string(), "exit 0".to_string()],
            process: child,
            working_dir: ".".to_string(),
            last_log_lines: 0,
            curr_output_in_progress: String::new(),
            crash_prevention: false,
            active: true,
            specialized_server_type: Some("Minecraft".to_string()),
            specialized_server_info: None,
            specialization_options: None,
            specialization_handler: None,
            specialization_info_sent: false,
        })
    }

    #[tokio::test]
    async fn status_update_stays_pending_until_sent() -> std::io::Result<()> {
        let mut specialization = MinecraftSpecialization::default();
        let mut instance = test_instance()?;

        specialization.parse_output(
            r#"[Server thread/INFO]: Done (12.345s)! For help, type "help""#.to_string(),
            &mut instance,
        );
        specialization.parse_output(
            "[Server thread/INFO]: A later non-status line".to_string(),
            &mut instance,
        );

        assert!(specialization.has_status_update());

        specialization.set_status_update_sent();

        assert!(!specialization.has_status_update());
        Ok(())
    }

    #[test]
    fn account_filter_sync_imports_real_minecraft_ban_files() -> std::io::Result<()> {
        let working_dir =
            std::env::temp_dir().join(format!("rsc-minecraft-filter-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&working_dir)?;
        std::fs::write(
            working_dir.join("whitelist.json"),
            r#"[{"uuid":"7febb72b-4010-47ae-b810-b14394a89fd5","name":"Lord55DRAGON"}]"#,
        )?;
        std::fs::write(
            working_dir.join("banned-players.json"),
            r#"[{"uuid":"ca2d0ab0-e4a1-4d54-b4cd-a2d5ed0b6b8c","name":"DingDong7801","created":"2026-05-01 01:17:18 -0700","source":"SturdyFool10","expires":"forever","reason":"Nice try"}]"#,
        )?;
        std::fs::write(
            working_dir.join("banned-ips.json"),
            r#"[{"ip":"192.0.2.10","created":"2026-05-01 01:17:18 -0700","source":"SturdyFool10","expires":"forever","reason":"Nope"}]"#,
        )?;

        let mut config = Config {
            minecraft_account_filter_detail_groups: vec![MinecraftAccountFilterDetailGroup {
                name: "Shared".to_string(),
                uuid: Some("group-one".to_string()),
                ..Default::default()
            }],
            ..Config::default()
        };
        assert!(merge_instance_filter_files_into_groups(
            &mut config,
            working_dir.to_str().unwrap_or_default(),
            &["group-one".to_string()]
        ));

        let group = &config.minecraft_account_filter_detail_groups[0];
        assert_eq!(group.whitelist.len(), 1);
        assert_eq!(group.ban_list.len(), 1);
        assert_eq!(group.banned_ips.len(), 1);
        assert_eq!(group.ban_list[0].source.as_deref(), Some("SturdyFool10"));
        assert_eq!(
            group.ban_list[0].created.as_deref(),
            Some("2026-05-01 01:17:18 -0700")
        );

        write_effective_filter_files(
            &config,
            working_dir.to_str().unwrap_or_default(),
            &["group-one".to_string()],
        );
        let written_bans = std::fs::read_to_string(working_dir.join("banned-players.json"))?;
        assert!(written_bans.contains(r#""source": "SturdyFool10""#));
        assert!(written_bans.contains(r#""reason": "Nice try""#));

        let _ = std::fs::remove_dir_all(working_dir);
        Ok(())
    }
}

/// Colorizes a single Minecraft log line using bracket counting and HTML spans.
///
/// Applies faded color to the timestamp, semantic color to the log level,
/// and green to the third bracketed block if present. The message is escaped for HTML.
///
/// # Arguments
///
/// * `line` - The log line to colorize.
///
/// # Returns
///
/// A `String` containing HTML representing the colorized log line.
fn colorize_minecraft_log_line(line: &str) -> String {
    // Extract all bracketed blocks at the start
    let mut chars = line.chars().peekable();
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut bracket_count;

    while let Some(&c) = chars.peek() {
        if c == '[' {
            bracket_count = 1;
            current.push(c);
            chars.next();
            while let Some(&c2) = chars.peek() {
                current.push(c2);
                chars.next();
                if c2 == '[' {
                    bracket_count += 1;
                } else if c2 == ']' {
                    bracket_count -= 1;
                    if bracket_count == 0 {
                        break;
                    }
                }
            }
            blocks.push(current.clone());
            current.clear();
        } else if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }

    // After the last bracket, check for colon and message
    let after_brackets = chars.collect::<String>();
    let (colon, message) = if let Some(idx) = after_brackets.find(':') {
        (":", &after_brackets[idx + 1..])
    } else {
        ("", after_brackets.as_str())
    };

    // Theme variable mapping
    fn type_to_var(typ: &str) -> &'static str {
        if typ.contains("ERROR") || typ.contains("FATAL") {
            "var(--danger)"
        } else if typ.contains("WARN") {
            "var(--warning)"
        } else if typ.contains("INFO") {
            "var(--info)"
        } else if typ.contains("SUCCESS") {
            "var(--success)"
        } else if typ.contains("DEBUG") {
            "var(--debug)"
        } else if typ.contains("EVENT") {
            "var(--event)"
        } else {
            "var(--text)"
        }
    }

    // Prepare HTML for each block
    let faded_time = if !blocks.is_empty() {
        format!(
            "<span style=\"opacity:0.5;\">{}</span>",
            escape_html(&blocks[0])
        )
    } else {
        "".to_string()
    };
    // Extract type (INFO/WARN/ERROR) from inside brackets for both colored_type and colored_third
    let typ_str = if blocks.len() > 1 {
        match Regex::new(r"\[([^\]/]+/)?([A-Z]+)\]") {
            Ok(typ_caps) => typ_caps
                .captures(&blocks[1])
                .and_then(|c| c.get(2))
                .map(|m| m.as_str())
                .unwrap_or(""),
            Err(error) => {
                tracing::error!("Invalid Minecraft log type regex: {}", error);
                ""
            }
        }
    } else {
        ""
    };
    let colored_type = if blocks.len() > 1 {
        let color = type_to_var(typ_str);
        format!(
            "<span style=\"color:{};\">{}</span>",
            color,
            escape_html(&blocks[1])
        )
    } else {
        "".to_string()
    };
    let colored_third = if blocks.len() > 2 {
        format!(
            "<span style=\"color:{};\">{}</span>",
            type_to_var(typ_str),
            escape_html(&blocks[2])
        )
    } else {
        "".to_string()
    };

    let colon_html = if !colon.is_empty() { ": " } else { "" };
    let message_html = if !message.trim().is_empty() {
        escape_html(message.trim())
    } else {
        "&nbsp;".to_string()
    };

    // If the line is truly empty, output a <br>
    if line.trim().is_empty() {
        return "<br>".to_string();
    }

    // Compose line
    format!(
        "{}{}{}{}{}<br>",
        faded_time, colored_type, colored_third, colon_html, message_html
    )
}
