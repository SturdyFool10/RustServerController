use super::ServerSpecialization;
use crate::ansi_to_html::ansi_to_plain_text;
use crate::controlled_program::ControlledProgramInstance;
use crate::specializations::player_activity::PlayerActivityTracker;
use serde_json::{json, Value};

/// Specialization for Terraria servers.
///
/// Provides a basic status object and pass-through output handling.
#[derive(Default)]
pub struct TerrariaSpecialization {
    player_count: usize,
    max_players: usize,
    player_activity: PlayerActivityTracker,
    last_status_update: bool,
}

impl ServerSpecialization for TerrariaSpecialization {
    fn pre_init(
        &mut self,
        _env: &mut std::collections::HashMap<String, String>,
        _descriptor: &crate::controlled_program::ControlledProgramDescriptor,
    ) {
    }

    fn default_options(&self) -> Value {
        json!({
            "track_players": true,
        })
    }

    /// Initialize the Terraria specialization for a server instance.
    ///
    /// Sets up the initial specialized_server_info state for player tracking.
    fn init(&mut self, instance: &mut ControlledProgramInstance) {
        self.max_players = parse_max_players(&instance.command_line_args).unwrap_or(0);
        self.player_count = 0;
        self.player_activity = PlayerActivityTracker::for_server(&instance.name, "Terraria");
        instance.specialized_server_info = Some(json!({
            "player_count": 0,
            "max_players": self.max_players
        }));
        self.last_status_update = true;
    }

    /// Parses a single output line from the Terraria server process.
    ///
    /// Returns the line unchanged until Terraria-specific parsing is added.
    fn parse_output(
        &mut self,
        line: String,
        instance: &mut ControlledProgramInstance,
    ) -> Option<String> {
        if !tracks_players(instance) {
            return Some(line);
        }

        let plain_line = ansi_to_plain_text(&line);
        let joined = player_from_line(&plain_line, r"(?i)(?:^|: )(.+?) has joined\.$");
        let left = player_from_line(&plain_line, r"(?i)(?:^|: )(.+?) has left\.$");

        if let Some(player_name) = joined {
            self.player_activity.player_joined(&player_name);
            self.player_count = self.player_activity.online_count();
            self.last_status_update = true;
        }

        if let Some(player_name) = left {
            self.player_activity.player_left(&player_name);
            self.player_count = self.player_activity.online_count();
            self.last_status_update = true;
        }

        Some(line)
    }

    fn has_status_update(&self) -> bool {
        self.last_status_update
    }

    fn set_status_update_sent(&mut self) {
        self.last_status_update = false;
    }

    /// Returns the current status for this specialization.
    ///
    /// For Terraria, this is always `Null` as status is stored in the instance's specialized_server_info.
    fn get_status(&self) -> Value {
        json!({
            "player_count": self.player_count,
            "max_players": self.max_players,
        })
    }

    fn get_stats(&self) -> Value {
        json!({
            "Players Online": self.player_count,
            "Player Slots": self.max_players,
            "Known Players": self.player_activity.known_player_count(),
            "Total Player Hours": self.player_activity.total_hours(),
            "Player Activity": self.player_activity.summaries(),
            "Recent Sessions": self.player_activity.recent_sessions(25),
            "Timeframe Stats": self.player_activity.timeframe_stats(),
        })
    }

    /// Handles logic when the Terraria server process exits.
    ///
    /// Default implementation does nothing for Terraria.
    fn on_exit(
        &mut self,
        _instance: &mut crate::controlled_program::ControlledProgramInstance,
        _state: &crate::app_state::AppState,
        _exit_code: i32,
    ) {
        if self.player_activity.mark_all_offline() {
            self.player_count = 0;
            self.last_status_update = true;
        }
    }
}

fn tracks_players(instance: &ControlledProgramInstance) -> bool {
    instance
        .specialization_options
        .as_ref()
        .and_then(|options| options.get("track_players"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

fn player_from_line(line: &str, pattern: &str) -> Option<String> {
    let regex = match regex::Regex::new(pattern) {
        Ok(regex) => regex,
        Err(error) => {
            tracing::error!("Invalid Terraria player activity regex: {}", error);
            return None;
        }
    };

    regex
        .captures(line)
        .and_then(|captures| captures.get(1))
        .map(|player| player.as_str().trim().to_string())
        .filter(|player| !player.is_empty())
}

fn parse_max_players(args: &[String]) -> Option<usize> {
    let mut args_iter = args.iter();
    while let Some(arg) = args_iter.next() {
        let normalized = arg.trim_start_matches(['-', '/']).to_ascii_lowercase();
        if normalized == "maxplayers" || normalized == "players" {
            return args_iter
                .next()
                .and_then(|value| value.parse::<usize>().ok());
        }
        if let Some(value) = normalized
            .strip_prefix("maxplayers=")
            .or_else(|| normalized.strip_prefix("players="))
        {
            return value.parse::<usize>().ok();
        }
    }

    None
}

/// Factory function for Terraria specialization.
///
/// Returns a boxed instance of `TerrariaSpecialization`.
pub fn factory() -> Box<dyn ServerSpecialization> {
    Box::new(TerrariaSpecialization::default())
}
