use regex::Regex;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::{
    io::*,
    process::*,
    time::{Duration, *},
};
use tracing::info;

use crate::ansi_to_html::ansi_to_html;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpecializedServerTypes {
    Minecraft,
    Terraria,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpecializedServerInformation {
    Minecraft(usize, usize, bool, Vec<String>), //unique information: PlayerCount, MaxPlayers, serverReady, playerlist
    Terraria(usize, usize),                     //unique information: PlayerCount, MaxPlayers
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ControlledProgramDescriptor {
    pub name: String,
    pub exe_path: String,
    pub arguments: Vec<String>,
    pub working_dir: String,
    pub auto_start: bool,
    //optional, do not use unless you need specialization, remove if unused then fix errors by removing lines
    pub specialized_server_type: Option<SpecializedServerTypes>,
    pub specialized_server_info: Option<SpecializedServerInformation>,
}
impl ControlledProgramDescriptor {
    #[allow(unused)]
    pub fn new_as(
        name: &str,
        exe_path: &str,
        arguments: Vec<String>,
        working_dir: String,
        auto_start: bool,
    ) -> Self {
        Self {
            name: name.to_owned(),
            exe_path: exe_path.to_owned(),
            arguments,
            working_dir,
            auto_start,
            specialized_server_type: None,
            specialized_server_info: None,
        }
    }
    pub fn new(name: &str, exe_path: &str, arguments: Vec<String>, working_dir: String) -> Self {
        Self {
            name: name.to_owned(),
            exe_path: exe_path.to_owned(),
            arguments,
            working_dir,
            auto_start: false,
            specialized_server_type: None,
            specialized_server_info: None,
        }
    }
    pub fn into_instance(self) -> ControlledProgramInstance {
        let mut instance = ControlledProgramInstance::new(
            self.name.as_str(),
            self.exe_path.as_str(),
            self.arguments,
            self.working_dir,
        );
        match self.specialized_server_type {
            None => {}
            Some(value) => {
                instance.set_specialization(value);
            }
        }
        instance
    }
    pub fn set_specialization(&mut self, spec: SpecializedServerTypes) {
        self.specialized_server_type = Some(spec.clone())
    }
}
impl Default for ControlledProgramDescriptor {
    fn default() -> Self {
        Self {
            name: "".to_owned(),
            exe_path: "".to_owned(),
            arguments: vec![],
            working_dir: "".to_owned(),
            auto_start: false,
            specialized_server_type: None,
            specialized_server_info: None,
        }
    }
}

#[derive(Debug)]
pub struct ControlledProgramInstance {
    pub name: String,
    pub executable_path: String,
    pub command_line_args: Vec<String>,
    pub process: Child,
    pub working_dir: String,
    #[allow(unused)]
    pub last_log_lines: usize,
    pub curr_output_in_progress: String,
    //optional, remove if unused then remove any references within this file
    pub specialized_server_type: Option<SpecializedServerTypes>,
    pub specialized_server_info: Option<SpecializedServerInformation>,
}
impl ControlledProgramInstance {
    pub fn new(name: &str, exe_path: &str, arguments: Vec<String>, working_dir: String) -> Self {
        let mut process = Command::new(exe_path);
        let mut process = process //this line needs to be here to prevent dropped value error
            .stdin(Stdio::piped()) //pipe stdin
            .stdout(Stdio::piped()) //pipe stdout
            .current_dir(working_dir.clone()) //set the working directory, makes the app think that its being run from within working_dir
            // Set environment variables to simulate a full terminal
            .env("TERM", "xterm-256color") // Standard terminal type with 256 colors
            .env("COLORTERM", "truecolor") // Indicate 24-bit color support
            .env("COLUMNS", "120") // Default terminal width
            .env("LINES", "30") // Default terminal height
            .env("TERM_PROGRAM", "RustServerController") // Terminal program name
            .env("FORCE_COLOR", "1"); // Force colored output in many applications

        for arg in arguments.iter() {
            process = process.arg(arg.replace("\\\\", "\\").replace('\"', ""));
        }
        let child = process
            .spawn()
            .expect("Could not spawn process for server.");
        Self {
            name: name.to_owned(),
            executable_path: exe_path.to_owned(),
            command_line_args: arguments,
            process: child,
            working_dir,
            last_log_lines: 0,
            curr_output_in_progress: "".to_string(),
            specialized_server_type: None,
            specialized_server_info: None,
        }
    }
    pub fn set_specialization(&mut self, spec: SpecializedServerTypes) {
        self.specialized_server_type = Some(spec.clone());
        info!("Setting server specialization...");
        match spec {
            SpecializedServerTypes::Minecraft => {
                let mut path_str = self.working_dir.clone();
                if !(path_str.ends_with("/")) && !(path_str.ends_with("\\")) {
                    path_str += "/";
                }
                path_str += "server.properties";

                let file_result = crate::files::read_file(path_str.as_str());
                info!("Reading server.properties...");
                match file_result {
                    Ok(val) => {
                        // Regex to find the max-players line
                        let regex = Regex::new(r"max-players=(\d+)").unwrap();
                        if let Some(caps) = regex.captures(&val) {
                            if let Some(max_players) = caps.get(1) {
                                if let Ok(max_players) = max_players.as_str().parse::<usize>() {
                                    self.specialized_server_info =
                                        Some(SpecializedServerInformation::Minecraft(
                                            0,
                                            max_players,
                                            false,
                                            vec![],
                                        ));
                                }
                            }
                        }
                    }
                    _ => {
                        tracing::log::error!("Could not find server.properties for minecraft server: \"{}\" at location: {}", self.name, path_str.clone());
                    }
                }
                if let Some(SpecializedServerInformation::Minecraft(_, _, _, _)) =
                    self.specialized_server_info
                {
                } else {
                    self.specialized_server_info =
                        Some(SpecializedServerInformation::Minecraft(0, 0, false, vec![]));
                }
            }
            SpecializedServerTypes::Terraria => {
                self.specialized_server_info = Some(SpecializedServerInformation::Terraria(0, 0))
            }
        }
    }
    pub async fn read_output(&mut self) -> Option<String> {
        #[allow(unused)]
        let mut out = None;
        {
            let mut out2 = String::new();
            let mut line: usize = 0;
            let mut has_more = true;

            while has_more {
                let mut buf = [0u8; 4096];
                let take = self.process.stdout.as_mut();
                let read =
                    match timeout(Duration::from_millis(10), take.unwrap().read(&mut buf)).await {
                        Ok(val) => val.unwrap(),
                        Err(_) => 0,
                    };
                if read > 0 && line < read {
                    let new_str = String::from_utf8_lossy(&buf[0..read]);
                    out2.push_str(ansi_to_html(&new_str).as_str());
                    line = read;
                }

                if read < 10 {
                    has_more = false;
                }
            }

            out = Some(out2);
        };
        match out {
            Some(val) => {
                if let Some(typ) = &self.specialized_server_type {
                    #[allow(unreachable_patterns)]
                    //we allow this because I am guarding using a default path
                    match typ {
                        SpecializedServerTypes::Minecraft => {
                            //join regex
                            {
                                let pattern: Regex = Regex::new(r"(\w+)\[/\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}:\d+\] logged in with entity id").unwrap();
                                let lines = val.split("\n");
                                for line in lines {
                                    match pattern.captures(&line) {
                                        Some(caps) => {
                                            if let Some(SpecializedServerInformation::Minecraft(
                                                mut current_players_count,
                                                max_player_count,
                                                ready,
                                                mut player_list,
                                            )) = self.specialized_server_info.clone()
                                            {
                                                let second = &caps[1];
                                                current_players_count += 1;
                                                let player_name = second;
                                                player_list.push(player_name.to_string());
                                                self.specialized_server_info =
                                                    Some(SpecializedServerInformation::Minecraft(
                                                        current_players_count,
                                                        max_player_count,
                                                        ready,
                                                        player_list,
                                                    ));
                                            } //we found something, do something with it
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            //leave regex
                            {
                                let pattern: Regex =
                                    Regex::new(r"\]: (\w+) lost connection").unwrap();
                                let lines = val.split("\n");
                                for line in lines {
                                    match pattern.captures(&line) {
                                        Some(caps) => {
                                            if let Some(SpecializedServerInformation::Minecraft(
                                                mut current_players_count,
                                                max_player_count,
                                                ready,
                                                mut player_list,
                                            )) = self.specialized_server_info.clone()
                                            {
                                                if current_players_count > 0 {
                                                    current_players_count =
                                                        current_players_count - 1;
                                                }
                                                let player_name0 = &caps[1];
                                                player_list.retain(|player_name| {
                                                    player_name != player_name0
                                                });
                                                self.specialized_server_info =
                                                    Some(SpecializedServerInformation::Minecraft(
                                                        current_players_count,
                                                        max_player_count,
                                                        ready,
                                                        player_list,
                                                    ));
                                            }
                                            //we found something, do something with it
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            //ready regex
                            {
                                let pattern = r#"Done \(\d+\.\d+s\)! For help, type "help""#;
                                let regex = Regex::new(pattern).unwrap();
                                let lines = val.split("\n");
                                for line in lines {
                                    #[allow(unused)]
                                    if let Some(SpecializedServerInformation::Minecraft(
                                        current_players_count,
                                        max_player_count,
                                        ready,
                                        player_list,
                                    )) = &mut self.specialized_server_info
                                    {
                                        if regex.is_match(&line) {
                                            *ready = true; // Set the server ready state to true
                                        }
                                    }
                                }
                            }
                        }
                        SpecializedServerTypes::Terraria => {}
                        _ => {}
                    }
                }
                self.curr_output_in_progress += &val[..];
                let cp = self.curr_output_in_progress.split("\n");
                let lines: Vec<&str> = cp.into_iter().collect();
                let mut inp = lines.len();
                if inp < 150 {
                    inp = 0;
                } else {
                    inp = inp - 150;
                }
                self.curr_output_in_progress = lines[std::cmp::max(0, inp)..lines.len()].join("\n");
                Some(val.clone())
            }
            None => None,
        }
    }
    pub async fn stop(&mut self) {
        let _ = self.process.kill().await;
    }
}
