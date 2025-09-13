use crate::{controlled_program::ControlledProgramInstance, specializations::ServerSpecialization};
use std::env;
use std::path::{Path, PathBuf};

pub fn vintagestory_data_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = env::var("APPDATA") {
            return Path::new(&appdata).join("VintagestoryData");
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = env::var("HOME") {
            return Path::new(&home).join(".config").join("VintagestoryData");
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = env::var("HOME") {
            return Path::new(&home)
                .join("Library")
                .join("Application Support")
                .join("VintagestoryData");
        }
    }

    // fallback: current directory
    PathBuf::from("./VintagestoryData")
}
#[derive(Default)]
pub struct VintageStoryServerSpecialization {
    server_name: String,
    max_players: usize,
    player_count: i32,
    calendar_paused: bool,
    config_found: bool,
}

// Colorize a single Vintage Story log line using theme colors
fn colorize_vs_log_line(line: &str) -> String {
    // Map log type to color and match the full [Server {type}] for coloring
    let log_types = [
        ("Notification", "var(--info)"),
        ("Debug", "var(--debug)"),
        ("Event", "var(--event)"),
        ("ERROR", "var(--danger)"),
        ("FATAL", "var(--danger)"),
        ("WARN", "var(--warning)"),
        ("SUCCESS", "var(--success)"),
        ("INFO", "var(--info)"),
    ];

    if let Some(start) = line.find("[Server ") {
        if let Some(end) = line[start..].find(']') {
            let end = start + end + 1;
            let before = &line[..start];
            let bracketed = &line[start..end];
            let rest = &line[end..];

            // Find which log type matches
            let mut found_color = "var(--text)";
            for (ty, color) in &log_types {
                if bracketed.contains(ty) {
                    found_color = color;
                    break;
                }
            }

            format!(
                "{}{}{}",
                before,
                format!(
                    "<span style=\"color:{};font-weight:bold;\">{}</span>",
                    found_color, bracketed
                ),
                rest
            )
        } else {
            format!("<span style=\"color:var(--text);\">{}</span>", line)
        }
    } else {
        format!("<span style=\"color:var(--text);\">{}</span>", line)
    }
}

impl ServerSpecialization for VintageStoryServerSpecialization {
    fn pre_init(
        &mut self,
        _env: &mut std::collections::HashMap<String, String>,
        _descriptor: &crate::controlled_program::ControlledProgramDescriptor,
    ) {
        // Default: do nothing for VintageStory
    }

    fn init(&mut self, _instance: &mut ControlledProgramInstance) {
        // On init, try to read config and set fields
        let data_path = vintagestory_data_path();
        let config_path = data_path.join("serverconfig.json");
        self.server_name = "Vintage Story Server".to_string();
        self.max_players = 0;
        self.config_found = false;
        if let Ok(config_str) = std::fs::read_to_string(&config_path) {
            if let Ok(config_json) = serde_json::from_str::<serde_json::Value>(&config_str) {
                if let Some(name) = config_json.get("ServerName").and_then(|v| v.as_str()) {
                    self.server_name = name.to_string();
                }
                if let Some(max) = config_json.get("MaxClients").and_then(|v| v.as_u64()) {
                    self.max_players = max as usize;
                }
                self.config_found = true;
            }
        }
        self.player_count = 0;
        self.calendar_paused = false;
    }

    fn parse_output(
        &mut self,
        line: String,
        _instance: &mut ControlledProgramInstance,
    ) -> Option<String> {
        // Update player count and calendar paused state from log lines
        let join_re = regex::Regex::new(r"\[Server Event\].*joins\.").unwrap();
        let disconnect_re = regex::Regex::new(r"\[Server Event\].*disconnected\.").unwrap();
        let pause_re = regex::Regex::new(
            r"\[Server Notification\] All clients disconnected, pausing game calendar\.",
        )
        .unwrap();
        let resume_re = regex::Regex::new(
            r"\[Server Notification\] A client reconnected, resuming game calendar\.",
        )
        .unwrap();

        for l in line.lines() {
            if join_re.is_match(l) {
                self.player_count += 1;
            }
            if disconnect_re.is_match(l) {
                self.player_count -= 1;
                if self.player_count < 0 {
                    self.player_count = 0;
                }
            }
            if pause_re.is_match(l) {
                self.calendar_paused = true;
            }
            if resume_re.is_match(l) {
                self.calendar_paused = false;
            }
        }

        // Split multi-line output and colorize each line
        let colored_lines: Vec<String> = line.lines().map(|l| colorize_vs_log_line(l)).collect();
        Some(colored_lines.join("<br>"))
    }

    fn get_status(&self) -> serde_json::Value {
        serde_json::json!({
            "server_name": self.server_name,
            "max_players": self.max_players,
            "player_count": self.player_count,
            "calendar_paused": self.calendar_paused,
            "config_found": self.config_found
        })
    }
}

// Colorize a single Vintage Story log line using theme colors

pub fn vintage_story_factory() -> Box<dyn ServerSpecialization> {
    Box::new(VintageStoryServerSpecialization::default())
}
