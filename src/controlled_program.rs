use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::{
    io::*,
    process::*,
    time::{Duration, *},
};

use crate::ansi_to_html::ansi_to_html;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpecializedServerTypes {
    Minecraft,
    Terraria,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ControlledProgramDescriptor {
    pub name: String,
    pub exe_path: String,
    pub arguments: Vec<String>,
    pub working_dir: String,
    pub auto_start: bool,
    pub crash_prevention: bool,
    //optional, do not use unless you need specialization, remove if unused then fix errors by removing lines
    pub specialized_server_type: Option<SpecializedServerTypes>,
    #[serde(skip)]
    pub specialized_server_info: Option<serde_json::Value>,
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
            crash_prevention: true,
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
            crash_prevention: true,
            specialized_server_type: None,
            specialized_server_info: None,
        }
    }
    pub fn into_instance(
        self,
        registry: &crate::specializations::SpecializationRegistry,
    ) -> ControlledProgramInstance {
        let mut instance = ControlledProgramInstance::new(
            self.name.as_str(),
            self.exe_path.as_str(),
            self.arguments,
            self.working_dir,
        );
        instance.specialized_server_type = self.specialized_server_type.clone();
        instance.crash_prevention = self.crash_prevention;

        // Attach specialization handler if type is present
        if let Some(ref typ) = instance.specialized_server_type {
            let type_name = match typ {
                SpecializedServerTypes::Minecraft => "Minecraft",
                SpecializedServerTypes::Terraria => "Terraria",
            };
            // Build the handler first, then assign
            let mut handler = registry.get(type_name);
            if let Some(ref mut h) = handler {
                h.init(&mut instance);
            }
            instance.specialization_handler = handler;
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
            crash_prevention: true,
            specialized_server_type: None,
            specialized_server_info: None,
        }
    }
}

pub struct ControlledProgramInstance {
    pub name: String,
    pub executable_path: String,
    pub command_line_args: Vec<String>,
    pub process: Child,
    pub working_dir: String,
    #[allow(unused)]
    pub last_log_lines: usize,
    pub curr_output_in_progress: String,
    pub crash_prevention: bool,
    //optional, remove if unused then remove any references within this file
    pub specialized_server_type: Option<SpecializedServerTypes>,
    pub specialized_server_info: Option<serde_json::Value>,
    pub specialization_handler: Option<Box<dyn crate::specializations::ServerSpecialization>>,
}

impl Drop for ControlledProgramInstance {
    fn drop(&mut self) {
        // Attempt to kill the process if it's still running
        if let Some(id) = self.process.id() {
            // Try to kill the process gracefully
            let _ = self.process.kill();
            tracing::info!(
                "Terminated server process '{}' (PID {}) on drop.",
                self.name,
                id
            );
        }
    }
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
            crash_prevention: true,
            specialized_server_type: None,
            specialized_server_info: None,
            specialization_handler: None,
        }
    }
    pub fn set_specialization(&mut self, spec: SpecializedServerTypes) {
        self.specialized_server_type = Some(spec.clone());
        // No-op: logic now handled by new specialization system
    }
    pub async fn read_output(&mut self) -> Option<String> {
        let mut out = String::new();
        let mut line: usize = 0;
        let mut has_more = true;

        while has_more {
            let mut buf = [0u8; 4096];
            let take = self.process.stdout.as_mut();
            let read = match timeout(Duration::from_millis(10), take.unwrap().read(&mut buf)).await
            {
                Ok(val) => val.unwrap(),
                Err(_) => 0,
            };
            if read > 0 && line < read {
                let new_str = String::from_utf8_lossy(&buf[0..read]);
                // Use specialization handler if available, avoiding double mutable borrow
                if self.specialization_handler.is_some() {
                    let mut handler = self.specialization_handler.take();
                    if let Some(ref mut handler_inner) = handler {
                        for log_line in new_str.lines() {
                            if let Some(transformed) =
                                handler_inner.parse_output(log_line.to_string(), self)
                            {
                                out.push_str(&transformed);
                            }
                        }
                    }
                    self.specialization_handler = handler;
                } else {
                    out.push_str(ansi_to_html(&new_str).as_str());
                }
                line = read;
            }

            if read < 10 {
                has_more = false;
            }
        }

        self.curr_output_in_progress += &out[..];
        let cp = self.curr_output_in_progress.split('\n');
        let lines: Vec<&str> = cp.into_iter().collect();
        let mut inp = lines.len();
        if inp < 150 {
            inp = 0;
        } else {
            inp = inp - 150;
        }
        self.curr_output_in_progress = lines[std::cmp::max(0, inp)..lines.len()].join("\n");
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
    pub async fn stop(&mut self) -> Option<i32> {
        // Disable crash prevention so the process won't be restarted when killed
        self.crash_prevention = false;
        let _ = self.process.kill().await;

        // Wait a moment for the process to terminate and get the exit code
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        match self.process.try_wait() {
            Ok(Some(status)) => status.code(),
            _ => None,
        }
    }
}
