use super::minecraft::{
    account_entry_is_active, account_filter_flags, account_filter_group_ids,
    selected_account_filter_groups, AccountFilterFlags,
};
use super::{player_activity::PlayerActivityTracker, ServerSpecialization};
use crate::ansi_to_html::{ansi_to_html, ansi_to_plain_text};
use crate::app_state::AppState;
use crate::configuration::Config;
use crate::controlled_program::ControlledProgramInstance;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Hytale's own default when `MaxPlayers` isn't present in `config.json`.
const DEFAULT_MAX_PLAYERS: usize = 100;

/// How long the account filter watcher waits for the server to report it has
/// booted before pushing the filters anyway.
const FILTER_READY_TIMEOUT: Duration = Duration::from_secs(90);

/// Specialization for Hytale dedicated servers.
///
/// The server colors its own console with ANSI escape codes, so output is
/// converted to HTML as-is; the plain text (escape codes stripped) is what the
/// parsing below matches against. Tracks player join/leave events and
/// readiness, records per-player activity, and keeps the server's whitelist and
/// ban list in sync with the controller's account filter groups. Backups are
/// left to Hytale's built-in backup system.
#[derive(Default)]
pub struct HytaleSpecialization {
    player_count: usize,
    max_players: usize,
    ready: bool,
    player_list: Vec<String>,
    player_activity: PlayerActivityTracker,
    last_status_update: bool,
    /// Newest server version reported by the server's `UpdateService`.
    latest_version: Option<String>,
    /// Shared with the account filter watcher so it can wait for boot.
    ready_flag: Arc<AtomicBool>,
    account_filter_watcher_stop: Option<watch::Sender<bool>>,
    account_filter_watcher: Option<JoinHandle<()>>,
}

struct HytalePatterns {
    join: Regex,
    leave: Regex,
    ready: Regex,
    version: Regex,
}

fn patterns() -> &'static HytalePatterns {
    static PATTERNS: OnceLock<HytalePatterns> = OnceLock::new();
    PATTERNS.get_or_init(|| HytalePatterns {
        // `[Universe|P] Adding player 'SturdyFool10 (<uuid>)` (note the missing
        // closing quote). `[World|default]` and `[PlayerSystems]` print their
        // own lines for the same event, so only the Universe one is matched.
        join: Regex::new(
            r"\[Universe\|P\]\s*Adding player '([^'\s(]+)\s*\(([0-9a-fA-F-]{32,36})\)?",
        )
            .expect("valid Hytale join regex"),
        leave: Regex::new(r"\[Universe\|P\]\s*Removing player '([^']+)'")
            .expect("valid Hytale leave regex"),
        ready: Regex::new(r"\[HytaleServer\]\s+Hytale Server Booted!")
            .expect("valid Hytale ready regex"),
        version: Regex::new(r"\[UpdateService\]\s*Found version:\s*(\S+)")
            .expect("valid Hytale version regex"),
    })
}

impl ServerSpecialization for HytaleSpecialization {
    fn default_options(&self) -> serde_json::Value {
        json!({
            "account_filter_groups": [],
            "controller_controlled_whitelist": false,
            "controller_controlled_ban_list": false,
        })
    }

    fn on_start(&mut self, instance: &mut ControlledProgramInstance, state: &AppState) {
        self.start_account_filter_watcher(instance, state.clone());
    }

    fn has_status_update(&self) -> bool {
        self.last_status_update
    }

    fn set_status_update_sent(&mut self) {
        self.last_status_update = false;
    }

    /// Reads `MaxPlayers` from the server's `config.json` if available.
    fn init(&mut self, instance: &mut ControlledProgramInstance) {
        let mut path = instance.working_dir.clone();
        if !(path.ends_with('/') || path.ends_with('\\')) {
            path.push('/');
        }
        path.push_str("config.json");

        self.max_players = crate::files::read_file(path.as_str())
            .ok()
            .and_then(|contents| serde_json::from_str::<Value>(&contents).ok())
            .and_then(|config| config.get("MaxPlayers").and_then(Value::as_u64))
            .map(|max| max as usize)
            .unwrap_or(DEFAULT_MAX_PLAYERS);

        self.player_count = 0;
        self.ready = false;
        self.ready_flag = Arc::new(AtomicBool::new(false));
        self.player_list = Vec::new();
        self.latest_version = None;
        self.player_activity =
            PlayerActivityTracker::for_server(&instance.server_uuid, &instance.name, "Hytale");
        self.last_status_update = true;
    }

    fn parse_output(
        &mut self,
        line: String,
        _instance: &mut ControlledProgramInstance,
    ) -> Option<String> {
        let html = ansi_to_html(&line);
        let line = ansi_to_plain_text(&line);
        let patterns = patterns();
        let mut status_update = false;

        // Players are keyed by their (persistent) UUID: a join under a new name
        // migrates everything recorded under the old name.
        if let Some((uuid, player_name)) = patterns
            .join
            .captures(&line)
            .and_then(|caps| Some((caps.get(2)?.as_str(), caps.get(1)?.as_str())))
        {
            let (_, renamed_from) = self.player_activity.player_joined_with_uuid(uuid, player_name);
            if let Some(old_name) = renamed_from {
                tracing::info!(
                    "Detected Hytale name change '{}' -> '{}' ({}); migrated activity history",
                    old_name,
                    player_name,
                    uuid
                );
            }
            self.player_count = self.player_activity.online_count();
            self.player_list = self.player_activity.online_names();
            // Nobody can join a server that hasn't finished booting.
            self.set_ready();
            status_update = true;
        }

        if let Some(player_name) = patterns
            .leave
            .captures(&line)
            .and_then(|caps| caps.get(1))
            .map(|player| player.as_str())
        {
            self.player_activity.player_left(player_name);
            self.player_count = self.player_activity.online_count();
            self.player_list = self.player_activity.online_names();
            status_update = true;
        }

        if !self.ready && patterns.ready.is_match(&line) {
            self.set_ready();
            status_update = true;
        }

        if let Some(version) = patterns
            .version
            .captures(&line)
            .and_then(|caps| caps.get(1))
            .map(|version| version.as_str().to_string())
        {
            if self.latest_version.as_deref() != Some(version.as_str()) {
                self.latest_version = Some(version);
                status_update = true;
            }
        }

        self.last_status_update |= status_update;
        self.player_activity.maybe_sample_periodic();

        Some(html)
    }

    fn on_exit(
        &mut self,
        _instance: &mut ControlledProgramInstance,
        _state: &AppState,
        _exit_code: i32,
    ) {
        self.ready = false;
        self.ready_flag.store(false, Ordering::SeqCst);
        self.stop_account_filter_watcher();
        if self.player_activity.mark_all_offline() {
            self.player_count = 0;
            self.player_list = Vec::new();
        }
        self.last_status_update = true;
    }

    fn get_status(&self) -> serde_json::Value {
        json!({
            "player_count": self.player_count,
            "max_players": self.max_players,
            "ready": self.ready,
            "player_list": self.player_list,
            "latest_version": self.latest_version,
        })
    }

    fn get_stats(&self) -> serde_json::Value {
        json!({
            "Players Online": self.player_count,
            "Player Slots": self.max_players,
            "Ready": self.ready,
            "Online Names": self.player_list.len(),
            "Latest Version": self.latest_version,
            "Observed Names": self.player_activity.known_player_count(),
            "Total Session Hours": self.player_activity.total_hours(),
            "User Activity": self.player_activity.summaries(),
            "Recent Sessions": self.player_activity.recent_sessions(25),
            "Timeframe Stats": self.player_activity.timeframe_stats(),
        })
    }
}

impl HytaleSpecialization {
    fn set_ready(&mut self) {
        self.ready = true;
        self.ready_flag.store(true, Ordering::SeqCst);
    }

    fn start_account_filter_watcher(
        &mut self,
        instance: &ControlledProgramInstance,
        state: AppState,
    ) {
        self.stop_account_filter_watcher();
        let group_ids =
            account_filter_group_ids(instance.specialization_options.as_ref()).unwrap_or_default();
        let flags = account_filter_flags(instance.specialization_options.as_ref());
        if group_ids.is_empty() && !flags.whitelist && !flags.ban_list {
            return;
        }

        let (stop_tx, stop_rx) = watch::channel(false);
        self.account_filter_watcher_stop = Some(stop_tx);
        self.account_filter_watcher = Some(tokio::spawn(watch_account_filters(
            instance.server_uuid.clone(),
            state,
            self.ready_flag.clone(),
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

impl Drop for HytaleSpecialization {
    fn drop(&mut self) {
        self.stop_account_filter_watcher();
    }
}

/// What the controller wants the server's access lists to contain.
///
/// Entries are identified by player name: the UUIDs stored on group entries
/// come from Mojang (for the Minecraft sync) and mean nothing to Hytale, while
/// `/whitelist` and `/ban` resolve names themselves.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct DesiredFilters {
    whitelist: BTreeSet<String>,
    bans: BTreeSet<String>,
}

/// Console arguments are written to stdin verbatim, so only plain identifier
/// characters are allowed (no whitespace, no control characters).
fn hytale_argument_is_safe(argument: &str) -> bool {
    !argument.is_empty()
        && argument
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn desired_filters(
    config: &Config,
    group_ids: &[String],
    flags: AccountFilterFlags,
) -> DesiredFilters {
    let mut desired = DesiredFilters::default();
    for group in selected_account_filter_groups(config, group_ids) {
        if flags.whitelist {
            for entry in &group.whitelist {
                if !account_entry_is_active(entry.expires.as_deref()) {
                    continue;
                }
                let name = entry.name.trim();
                if hytale_argument_is_safe(name) {
                    desired.whitelist.insert(name.to_string());
                }
            }
        }
        if flags.ban_list {
            for entry in &group.ban_list {
                if !account_entry_is_active(entry.expires.as_deref()) {
                    continue;
                }
                let name = entry.name.trim();
                if hytale_argument_is_safe(name) {
                    desired.bans.insert(name.to_string());
                }
            }
        }
    }
    desired
}

async fn send_command(state: &AppState, server_uuid: &str, command: String) {
    if !command.split_whitespace().skip(1).all(hytale_argument_is_safe) {
        tracing::warn!(
            "Skipping unsafe Hytale account filter command for '{}': {}",
            server_uuid,
            command
        );
        return;
    }
    // Hytale documents its commands with a leading slash, including for the
    // server console.
    if !crate::servers::write_line_to_server_stdin(state, server_uuid, &format!("/{command}")).await
    {
        tracing::debug!(
            "Couldn't apply Hytale account filter command for '{}' (server not running)",
            server_uuid
        );
    }
}

async fn current_filter_config(
    state: &AppState,
    server_uuid: &str,
) -> (DesiredFilters, AccountFilterFlags) {
    let config = state.config.lock().await;
    let Some(server) = config.servers.iter().find(|server| server.server_uuid.as_deref() == Some(server_uuid)) else {
        return (DesiredFilters::default(), AccountFilterFlags::default());
    };
    let group_ids =
        account_filter_group_ids(server.specialization_options.as_ref()).unwrap_or_default();
    let flags = account_filter_flags(server.specialization_options.as_ref());
    (desired_filters(&config, &group_ids, flags), flags)
}

/// Pushes the controller's whitelist/ban groups to a running Hytale server
/// through its console commands (the server owns `whitelist.json` and
/// `bans.json` while running, and may overwrite hand edits).
///
/// Once the server has booted, every desired entry is added; after that, only
/// changes are sent. Unlike the Minecraft sync, nothing is imported back from
/// the server's own files, and an entry removed from a group while the server
/// was offline stays on the server's lists until removed in-game.
async fn watch_account_filters(
    server_uuid: String,
    state: AppState,
    ready: Arc<AtomicBool>,
    mut stop: watch::Receiver<bool>,
) {
    let started = Instant::now();
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    let mut applied: Option<(DesiredFilters, AccountFilterFlags)> = None;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if applied.is_none()
                    && !ready.load(Ordering::SeqCst)
                    && started.elapsed() < FILTER_READY_TIMEOUT
                {
                    continue;
                }
                let (desired, flags) = current_filter_config(&state, &server_uuid).await;
                match &applied {
                    Some((previous, previous_flags))
                        if *previous == desired && *previous_flags == flags => {}
                    Some((previous, previous_flags)) => {
                        apply_filter_diff(&state, &server_uuid, previous, &desired).await;
                        if previous_flags.whitelist != flags.whitelist {
                            let verb = if flags.whitelist { "enable" } else { "disable" };
                            send_command(&state, &server_uuid, format!("whitelist {verb}")).await;
                        }
                        applied = Some((desired, flags));
                    }
                    None => {
                        // Never assert "off" at startup: the whitelist may have
                        // been set up outside the controller.
                        if flags.whitelist {
                            send_command(&state, &server_uuid, "whitelist enable".to_string()).await;
                        }
                        apply_filter_diff(
                            &state,
                            &server_uuid,
                            &DesiredFilters::default(),
                            &desired,
                        )
                        .await;
                        applied = Some((desired, flags));
                    }
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

/// Gap between console commands, so applying a large list doesn't flood the
/// server's stdin (or hold its lock) in one burst.
const COMMAND_SPACING: Duration = Duration::from_millis(5);

async fn apply_filter_diff(
    state: &AppState,
    server_uuid: &str,
    previous: &DesiredFilters,
    next: &DesiredFilters,
) {
    let changes = previous.whitelist.symmetric_difference(&next.whitelist).count()
        + previous.bans.symmetric_difference(&next.bans).count();
    if changes > 1000 {
        tracing::warn!(
            "Applying {} Hytale access list changes to '{}' through console commands; \
             very large lists are slow to sync this way",
            changes,
            server_uuid
        );
    }
    for id in previous.whitelist.difference(&next.whitelist) {
        send_command(state, server_uuid, format!("whitelist remove {id}")).await;
        tokio::time::sleep(COMMAND_SPACING).await;
    }
    for id in next.whitelist.difference(&previous.whitelist) {
        send_command(state, server_uuid, format!("whitelist add {id}")).await;
        tokio::time::sleep(COMMAND_SPACING).await;
    }
    for id in previous.bans.difference(&next.bans) {
        send_command(state, server_uuid, format!("unban {id}")).await;
        tokio::time::sleep(COMMAND_SPACING).await;
    }
    for id in next.bans.difference(&previous.bans) {
        send_command(state, server_uuid, format!("ban {id}")).await;
        tokio::time::sleep(COMMAND_SPACING).await;
    }
}

pub fn factory() -> Box<dyn ServerSpecialization> {
    Box::new(HytaleSpecialization::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leave_pattern_matches_real_log_line() {
        let line = "[2026/10/18 16:12:19   INFO]             [Universe|P] Removing player 'Sgt_Nathanial' (0d9f716c-5437-40ae-8f3b-b1f64536c22c)";
        let name = patterns()
            .leave
            .captures(line)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str());
        assert_eq!(name, Some("Sgt_Nathanial"));
    }

    #[test]
    fn player_systems_line_is_not_a_leave() {
        let line = "[2026/10/18 16:12:19   INFO]          [PlayerSystems] Removing player 'Sgt_Nathanial (Sgt_Nathanial)' from world 'default' (0d9f716c)";
        assert!(!patterns().leave.is_match(line));
    }

    #[test]
    fn version_pattern_matches_update_service() {
        let line = "[2026/10/18 16:48:00   INFO]          [UpdateService] Found version: 0.7.2";
        let version = patterns()
            .version
            .captures(line)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str());
        assert_eq!(version, Some("0.7.2"));
    }

    #[test]
    fn join_pattern_matches_real_log_line_only_once_per_join() {
        let universe = "[2026/10/18 18:39:29   INFO]              [Universe|P] Adding player 'SturdyFool10 (eb856185-cc01-493d-9b6c-0540aa7b6af8)";
        let caps = patterns().join.captures(universe).expect("join line matches");
        assert_eq!(&caps[1], "SturdyFool10");
        assert_eq!(&caps[2], "eb856185-cc01-493d-9b6c-0540aa7b6af8");

        for line in [
            "[2026/10/18 18:39:29   INFO]           [World|default] Adding player 'SturdyFool10' to world 'default' at location null (eb856185)",
            "[2026/10/18 18:39:29   INFO]           [World|default] Player 'SturdyFool10' joined world 'default' at location (-4.041E+2  1.240E+2 -2.417E+2) (eb856185)",
        ] {
            assert!(!patterns().join.is_match(line), "{line}");
        }
    }

    #[test]
    fn ansi_output_is_matched_on_plain_text_and_kept_colored() {
        let line = "\x1b[32m[2026/10/18 16:12:19   INFO]\x1b[0m [Universe|P] Removing player 'Sgt_Nathanial' (abc)";
        assert!(patterns().leave.is_match(&ansi_to_plain_text(line)));
        assert!(ansi_to_html(line).contains("<span"));
    }

    #[test]
    fn ready_pattern_matches_real_boot_line_only() {
        let boot = "[2026/10/18 18:22:37   INFO]            [HytaleServer]          Hytale Server Booted! [Multiplayer] took 13sec 499ms 412us 715ns";
        let banner = "[2026/10/18 18:22:37   INFO]            [HytaleServer] ====================";
        assert!(patterns().ready.is_match(boot));
        assert!(!patterns().ready.is_match(banner));
    }

    #[test]
    fn unsafe_names_are_not_sent_to_the_console() {
        assert!(hytale_argument_is_safe("Sgt_Nathanial"));
        assert!(!hytale_argument_is_safe("bad name"));
        assert!(!hytale_argument_is_safe("x\nsay hi"));
        assert!(!hytale_argument_is_safe(""));
    }
}
