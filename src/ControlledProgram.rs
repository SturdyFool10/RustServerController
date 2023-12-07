use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{fs::File, process::Stdio};
use tokio::{
    io::*,
    process::*,
    time::{Duration, *},
};
use tracing::info;
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
    pub exePath: String,
    pub arguments: Vec<String>,
    pub working_dir: String,
    pub autoStart: bool,
    //optional, do not use unless you need specialization, remove if unused then fix errors by removing lines
    pub specializedServerType: Option<SpecializedServerTypes>,
    pub specializedServerInfo: Option<SpecializedServerInformation>,
}
impl ControlledProgramDescriptor {
    pub fn newAS(
        name: &str,
        exePath: &str,
        arguments: Vec<String>,
        working_dir: String,
        autoStart: bool,
    ) -> Self {
        Self {
            name: name.to_owned(),
            exePath: exePath.to_owned(),
            arguments,
            working_dir,
            autoStart: autoStart,
            specializedServerType: None,
            specializedServerInfo: None,
        }
    }
    pub fn new(name: &str, exePath: &str, arguments: Vec<String>, working_dir: String) -> Self {
        Self {
            name: name.to_owned(),
            exePath: exePath.to_owned(),
            arguments,
            working_dir,
            autoStart: false,
            specializedServerType: None,
            specializedServerInfo: None,
        }
    }
    pub fn into_instance(self) -> ControlledProgramInstance {
        let mut instance = ControlledProgramInstance::new(
            self.name.as_str(),
            self.exePath.as_str(),
            self.arguments,
            self.working_dir,
        );
        match self.specializedServerType {
            None => {}
            Some(value) => {
                instance.setSpecialization(value);
            }
        }
        instance
    }
    pub fn setSpecialization(&mut self, spec: SpecializedServerTypes) {
        self.specializedServerType = Some(spec.clone())
    }
}
impl Default for ControlledProgramDescriptor {
    fn default() -> Self {
        Self {
            name: "".to_owned(),
            exePath: "".to_owned(),
            arguments: vec![],
            working_dir: "".to_owned(),
            autoStart: false,
            specializedServerType: None,
            specializedServerInfo: None,
        }
    }
}

#[derive(Debug)]
pub struct ControlledProgramInstance {
    pub name: String,
    pub executablePath: String,
    pub commandLineArgs: Vec<String>,
    pub process: Child,
    pub working_dir: String,
    pub lastLogLines: usize,
    pub currOutputInProgress: String,
    //optional, remove if unused then remove any references within this file
    pub specializedServerType: Option<SpecializedServerTypes>,
    pub specializedServerInfo: Option<SpecializedServerInformation>,
}
impl ControlledProgramInstance {
    pub fn new(name: &str, exePath: &str, arguments: Vec<String>, working_dir: String) -> Self {
        let mut process = Command::new(exePath);
        let mut process = process.stdin(Stdio::piped());
        process = process.stdout(Stdio::piped());
        process = process.current_dir(working_dir.clone());
        for arg in arguments.iter() {
            process = process.arg(arg.replace("\\\\", "\\").replace('\"', ""));
        }
        let child = process
            .spawn()
            .expect("Could not spawn process for server.");
        Self {
            name: name.to_owned(),
            executablePath: exePath.to_owned(),
            commandLineArgs: arguments,
            process: child,
            working_dir,
            lastLogLines: 0,
            currOutputInProgress: "".to_string(),
            specializedServerType: None,
            specializedServerInfo: None,
        }
    }
    pub fn setSpecialization(&mut self, spec: SpecializedServerTypes) {
        self.specializedServerType = Some(spec.clone());
        info!("Setting server specialization...");
        match spec {
            SpecializedServerTypes::Minecraft => {
                let mut pathStr = self.working_dir.clone();
                if !(pathStr.ends_with("/")) && !(pathStr.ends_with("\\")) {
                    pathStr += "/";
                }
                pathStr += "server.properties";

                let fileResult = crate::files::read_file(pathStr.as_str());
                info!("Reading server.properties...");
                match fileResult {
                    Ok(val) => {
                        // Regex to find the max-players line
                        let regex = Regex::new(r"max-players=(\d+)").unwrap();
                        if let Some(caps) = regex.captures(&val) {
                            if let Some(max_players) = caps.get(1) {
                                if let Ok(max_players) = max_players.as_str().parse::<usize>() {
                                    self.specializedServerInfo =
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
                        tracing::log::error!("Could not find server.properties for minecraft server: \"{}\" at location: {}", self.name, pathStr.clone());
                    }
                }
                if let Some(SpecializedServerInformation::Minecraft(_, _, _, _)) =
                    self.specializedServerInfo
                {
                } else {
                    self.specializedServerInfo =
                        Some(SpecializedServerInformation::Minecraft(0, 0, false, vec![]));
                }
            }
            SpecializedServerTypes::Terraria => {
                self.specializedServerInfo = Some(SpecializedServerInformation::Terraria(0, 0))
            }
        }
    }
    pub async fn readOutput(&mut self) -> Option<String> {
        let mut out = None;
        {
            let mut out2 = String::new();
            let mut line: usize = 0;
            let mut has_more = true;

            while has_more {
                let mut buf = [0u8; 10000];
                let take = self.process.stdout.as_mut();
                let read =
                    match timeout(Duration::from_millis(10), take.unwrap().read(&mut buf)).await {
                        Ok(val) => val.unwrap(),
                        Err(_) => 0,
                    };

                if read > 0 && line < read {
                    let new_str = String::from_utf8_lossy(&buf[0..read]);
                    out2.push_str(&new_str);
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
                if let Some(typ) = &self.specializedServerType {
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
                                                mut currentPlayersCount,
                                                maxPlayerCount,
                                                ready,
                                                mut playerList,
                                            )) = self.specializedServerInfo.clone()
                                            {
                                                let second = &caps[1];
                                                currentPlayersCount += 1;
                                                let playerName = second;
                                                playerList.push(playerName.to_string());
                                                self.specializedServerInfo =
                                                    Some(SpecializedServerInformation::Minecraft(
                                                        currentPlayersCount,
                                                        maxPlayerCount,
                                                        ready,
                                                        playerList,
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
                                                mut currentPlayersCount,
                                                maxPlayerCount,
                                                ready,
                                                mut playerList,
                                            )) = self.specializedServerInfo.clone()
                                            {
                                                if currentPlayersCount > 0 {
                                                    currentPlayersCount = currentPlayersCount - 1;
                                                }
                                                let player_name = &caps[1];
                                                playerList
                                                    .retain(|playerName| playerName != player_name);
                                                self.specializedServerInfo =
                                                    Some(SpecializedServerInformation::Minecraft(
                                                        currentPlayersCount,
                                                        maxPlayerCount,
                                                        ready,
                                                        playerList,
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
                                    if let Some(SpecializedServerInformation::Minecraft(
                                        currentPlayersCount,
                                        maxPlayerCount,
                                        ready,
                                        playerList,
                                    )) = &mut self.specializedServerInfo
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
                self.currOutputInProgress += &val[..];
                let cp = self.currOutputInProgress.split("\n");
                let lines: Vec<&str> = cp.into_iter().collect();
                let mut inp = lines.len();
                if (inp < 150) {
                    inp = 0;
                } else {
                    inp = inp - 150;
                }
                self.currOutputInProgress = lines[std::cmp::max(0, inp)..lines.len()].join("\n");
                Some(val.clone())
            }
            None => None,
        }
    }
    pub async fn stop(&mut self) {
        self.process.kill().await;
    }
}
