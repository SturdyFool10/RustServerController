use super::ServerSpecialization;
use crate::ansi_to_html::escape_html;
use crate::app_state::AppState;
use crate::controlled_program::ControlledProgramInstance;
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;

/// Specialization for Minecraft servers.
///
/// Handles Minecraft-specific logic such as parsing player join/leave events,
/// tracking readiness, and auto-accepting the EULA if needed.
#[derive(Default)]
pub struct MinecraftSpecialization {
    // No internal state needed; all info is stored in ControlledProgramInstance
}

impl ServerSpecialization for MinecraftSpecialization {
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
            let regex = Regex::new(r"max-players=(\d+)").unwrap();
            if let Some(caps) = regex.captures(&val) {
                if let Some(mp) = caps.get(1) {
                    if let Ok(mp) = mp.as_str().parse::<usize>() {
                        max_players = mp;
                    }
                }
            }
        }
        instance.specialized_server_info = Some(json!({
            "player_count": 0,
            "max_players": max_players,
            "ready": false,
            "player_list": [],
        }));
    }

    /// Parses a single output line from the Minecraft server process.
    ///
    /// Updates player count, readiness, and player list in specialized_server_info.
    /// Returns a colorized HTML string for the log line.
    fn parse_output(
        &mut self,
        line: String,
        instance: &mut ControlledProgramInstance,
    ) -> Option<String> {
        // Player join regex
        let join_pattern = Regex::new(
            r"(\w+)\[/\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}:\d+\] logged in with entity id",
        )
        .unwrap();
        // Player leave regex
        let leave_pattern = Regex::new(r"\]: (\w+) lost connection").unwrap();
        // Ready regex
        let ready_pattern = Regex::new(r#"Done \(\d+\.\d+s\)! For help, type "help""#).unwrap();

        #[allow(clippy::type_complexity)]
        let mut update_info =
            |f: &mut dyn FnMut(&mut usize, usize, &mut bool, &mut Vec<String>)| {
                if let Some(Value::Object(ref mut obj)) = instance.specialized_server_info {
                    let current_players_count =
                        obj.get("player_count").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let max_player_count =
                        obj.get("max_players").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let ready = obj.get("ready").and_then(Value::as_bool).unwrap_or(false);
                    let mut player_list: Vec<String> = obj
                        .get("player_list")
                        .and_then(Value::as_array)
                        .map(|arr| {
                            arr.iter()
                                .filter_map(Value::as_str)
                                .map(|s| s.to_string())
                                .collect()
                        })
                        .unwrap_or_else(Vec::new);

                    let mut ready_mut = ready;
                    let mut current_players_count_mut = current_players_count;
                    f(
                        &mut current_players_count_mut,
                        max_player_count,
                        &mut ready_mut,
                        &mut player_list,
                    );

                    obj.insert("player_count".to_string(), json!(current_players_count_mut));
                    obj.insert("ready".to_string(), json!(ready_mut));
                    obj.insert("player_list".to_string(), json!(player_list));
                }
            };

        // Player join
        if let Some(caps) = join_pattern.captures(&line) {
            let player_name = &caps[1];
            update_info(&mut |current_players_count, _max, _ready, player_list| {
                *current_players_count += 1;
                player_list.push(player_name.to_string());
            });
        }

        // Player leave
        if let Some(caps) = leave_pattern.captures(&line) {
            let player_name = &caps[1];
            update_info(&mut |current_players_count, _max, _ready, player_list| {
                if *current_players_count > 0 {
                    *current_players_count -= 1;
                }
                player_list.retain(|n| n != player_name);
            });
        }

        // Server ready
        if ready_pattern.is_match(&line) {
            update_info(&mut |_c, _m, ready, _pl| {
                *ready = true;
            });
        }

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
        // Robust EULA auto-accept: check eula.txt for eula=false and patch/restart if needed
        let state = state.clone();
        let name = instance.name.clone();
        let exe_path = instance.executable_path.clone();
        let args = instance.command_line_args.clone();
        let working_dir = instance.working_dir.clone();
        let specialized_server_type = instance.specialized_server_type.clone();
        let crash_prevention = instance.crash_prevention;
        tokio::spawn(async move {
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
                let msg = "<span style=\"color: var(--warning, #FFA500);\">[EULA was set to false. Automatically set eula=true and restarting the server.]</span>";
                let eula_console_msg = crate::messages::ConsoleOutput {
                    r#type: "ServerOutput".to_owned(),
                    output: msg.to_string(),
                    server_name: name.clone(),
                    server_type: specialized_server_type.clone(),
                };
                let _ = state
                    .tx
                    .send(serde_json::to_string(&eula_console_msg).unwrap());

                // Restart the server
                let mut desc = crate::controlled_program::ControlledProgramDescriptor::new(
                    &name,
                    &exe_path,
                    args,
                    working_dir,
                );
                desc.specialized_server_type = specialized_server_type;
                desc.crash_prevention = crash_prevention;
                let mut servers = state.servers.lock().await;
                servers.push(desc.into_instance(&state.specialization_registry));
            }
        });
    }

    /// Returns the current status for this specialization.
    ///
    /// For Minecraft, this is always `Null` as status is stored in the instance's specialized_server_info.
    fn get_status(&self) -> serde_json::Value {
        // This function is called on the handler, which is stateless.
        // The actual status is stored in the instance's specialized_server_info.
        // So, this function should not be used directly for Minecraft.
        // Instead, status should be read from the instance's specialized_server_info in the UI layer.
        // Returning Null here for compatibility.
        serde_json::Value::Null
    }
}

/// Factory function for Minecraft specialization.
///
/// Returns a boxed instance of `MinecraftSpecialization`.
pub fn factory() -> Box<dyn ServerSpecialization> {
    Box::new(MinecraftSpecialization::default())
}

/// Colorizes a single Minecraft log line using bracket counting.
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
        if typ.contains("ERROR") {
            "var(--danger)"
        } else if typ.contains("WARN") {
            "var(--warning)"
        } else if typ.contains("INFO") {
            "var(--info)"
        } else {
            "var(--success)"
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
    let colored_type = if blocks.len() > 1 {
        // Extract type (INFO/WARN/ERROR) from inside brackets
        let typ_caps = Regex::new(r"\[([^\]/]+/)?([A-Z]+)\]").unwrap();
        let typ_str = typ_caps
            .captures(&blocks[1])
            .and_then(|c| c.get(2))
            .map(|m| m.as_str())
            .unwrap_or("");
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
            "<span style=\"color:var(--success);\">{}</span>",
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
