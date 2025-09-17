use crate::ansi_to_html::ansi_to_html;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::{
    io::*,
    process::*,
    time::{Duration, *},
};

/// Configuration descriptor for a server or program to be controlled by the application.
/// Used for configuration and instantiation of server processes.
#[derive(Clone, Serialize, Deserialize)]
pub struct ControlledProgramDescriptor {
    /// Display name of the server/program.
    pub name: String,
    /// Path to the executable.
    pub exe_path: String,
    /// Command-line arguments for the process.
    pub arguments: Vec<String>,
    /// Working directory for the process.
    pub working_dir: String,
    /// Whether to auto-start this server on launch.
    pub auto_start: bool,
    /// Whether to enable crash prevention (auto-restart).
    pub crash_prevention: bool,

    /// Optional specialized server type (e.g., "Minecraft", "Terraria", "VintageStory").
    pub specialized_server_type: Option<String>,
    /// Optional extra info for specialized servers (not serialized).
    #[serde(skip)]
    pub specialized_server_info: Option<serde_json::Value>,
    /// Optional specialization options for specializations to use (serialized).
    pub specialization_options: Option<serde_json::Value>,
}
impl ControlledProgramDescriptor {
    /// Creates a new descriptor with all fields specified.
    ///
    /// # Arguments
    /// * `name` - The display name of the server/program.
    /// * `exe_path` - Path to the executable.
    /// * `arguments` - Command-line arguments.
    /// * `working_dir` - Working directory for the process.
    /// * `auto_start` - Whether to auto-start this server on launch.
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
            specialization_options: None,
        }
    }

    /// Creates a new descriptor with auto_start set to false.
    ///
    /// # Arguments
    /// * `name` - The display name of the server/program.
    /// * `exe_path` - Path to the executable.
    /// * `arguments` - Command-line arguments.
    /// * `working_dir` - Working directory for the process.
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
            specialization_options: None,
        }
    }

    /// Converts this descriptor into a running [`ControlledProgramInstance`].
    ///
    /// Attaches a specialization handler if specified.
    ///
    /// # Arguments
    /// * `registry` - The specialization registry to use for handler lookup.
    pub fn into_instance(
        self,
        registry: &crate::specializations::SpecializationRegistry,
    ) -> ControlledProgramInstance {
        use std::collections::HashMap;

        // Prepare default environment variables
        let mut envs: HashMap<String, String> = HashMap::new();
        envs.insert("TERM".to_string(), "xterm-256color".to_string());
        envs.insert("COLORTERM".to_string(), "truecolor".to_string());
        envs.insert("COLUMNS".to_string(), "120".to_string());
        envs.insert("LINES".to_string(), "30".to_string());
        envs.insert(
            "TERM_PROGRAM".to_string(),
            "RustServerController".to_string(),
        );
        envs.insert("FORCE_COLOR".to_string(), "1".to_string());

        let mut specialization_handler = None;
        let mut specialized_server_type = self.specialized_server_type.clone();
        let crash_prevention = self.crash_prevention;

        // If specialization exists, allow it to modify envs before process spawn
        if let Some(ref typ) = self.specialized_server_type {
            tracing::trace!("Attempting to get specialization handler for type: {}", typ);
            if let Some(mut handler) = registry.get(typ) {
                tracing::trace!("Calling pre_init for specialization: {}", typ);
                handler.pre_init(&mut envs, &self);
                tracing::trace!("pre_init complete for specialization: {}", typ);
                specialization_handler = Some(handler);
            } else {
                eprintln!(
                    "Warning: Server specialization \"{}\" does not exist in the registry. Your configuration file will NOT be changed, but this server will be treated as unspecialized (generic) behavior.",
                    typ
                );
                specialized_server_type = None;
            }
        }

        let mut instance = ControlledProgramInstance::new(
            self.name.as_str(),
            self.exe_path.as_str(),
            self.arguments,
            self.working_dir,
            envs,
        );
        instance.specialized_server_type = specialized_server_type;
        instance.crash_prevention = crash_prevention;

        // If a specialization handler was attached, call init before assigning to instance
        if let Some(mut handler) = specialization_handler {
            handler.init(&mut instance);
            instance.specialization_handler = Some(handler);
        } else {
            instance.specialization_handler = None;
        }

        instance
    }
}
impl Default for ControlledProgramDescriptor {
    /// Returns a default descriptor with empty fields and crash prevention enabled.
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
            specialization_options: None,
        }
    }
}

/// Represents a running server/program process controlled by the application.
pub struct ControlledProgramInstance {
    /// Display name of the server/program.
    pub name: String,
    /// Path to the executable.
    pub executable_path: String,
    /// Command-line arguments for the process.
    pub command_line_args: Vec<String>,
    /// The running child process.
    pub process: Child,
    /// Working directory for the process.
    pub working_dir: String,
    /// Number of last log lines (unused).
    #[allow(unused)]
    pub last_log_lines: usize,
    /// Current output in progress (buffered).
    pub curr_output_in_progress: String,
    /// Whether crash prevention is enabled.
    pub crash_prevention: bool,
    /// Whether the process is currently active.
    pub active: bool,
    /// Optional specialized server type.
    pub specialized_server_type: Option<String>,
    /// Optional extra info for specialized servers.
    pub specialized_server_info: Option<serde_json::Value>,
    /// Optional handler for server specialization logic.
    pub specialization_handler: Option<Box<dyn crate::specializations::ServerSpecialization>>,
    /// Tracks if the first specialization info update has been sent after spawn.
    pub specialization_info_sent: bool,
}

impl Drop for ControlledProgramInstance {
    /// Ensures the process is killed when the instance is dropped.
    fn drop(&mut self) {
        // Attempt to kill the process if it's still running
        if let Some(id) = self.process.id() {
            // Try to kill the process gracefully
            std::mem::drop(self.process.kill());
            tracing::trace!(
                "Terminated server process '{}' (PID {}) on drop.",
                self.name,
                id
            );
        }
    }
}

impl ControlledProgramInstance {
    /// Creates a new running instance of a controlled program/server.
    ///
    /// Ensures the working directory exists, sets up environment variables, and spawns the process.
    ///
    /// # Arguments
    /// * `name` - The display name of the server/program.
    /// * `exe_path` - Path to the executable.
    /// * `arguments` - Command-line arguments.
    /// * `working_dir` - Working directory for the process.
    pub fn new(
        name: &str,
        exe_path: &str,
        arguments: Vec<String>,
        working_dir: String,
        envs: std::collections::HashMap<String, String>,
    ) -> Self {
        use std::fs;
        use std::path::Path;

        // Ensure the working directory exists, create if it doesn't
        let working_dir_path = Path::new(&working_dir);
        if !working_dir_path.exists() {
            if let Err(e) = fs::create_dir_all(working_dir_path) {
                panic!(
                    "Failed to create working directory {:?}: {}",
                    working_dir_path, e
                );
            }
        }

        let mut process = Command::new(exe_path);
        let mut process = process
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .current_dir(working_dir.clone());

        // Set environment variables from the provided map
        for (key, value) in envs.iter() {
            process = process.env(key, value);
        }

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
            active: true,
            specialized_server_type: None,
            specialized_server_info: None,
            specialization_handler: None,
            specialization_info_sent: false,
        }
    }

    /// Reads and processes output from the server process.
    ///
    /// Uses the specialization handler if available, otherwise applies ANSI to HTML conversion.
    /// Maintains a buffer of recent output lines.
    pub async fn read_output(&mut self) -> Option<String> {
        let mut out = String::new();
        let mut has_more = true;

        while has_more {
            let mut buf = [0u8; 4096];
            let take = self.process.stdout.as_mut();
            let read = match timeout(Duration::from_millis(10), take.unwrap().read(&mut buf)).await
            {
                Ok(val) => val.unwrap(),
                Err(_) => 0,
            };
            if read > 0 {
                let new_str = String::from_utf8_lossy(&buf[0..read]);
                // Always split into lines and process one at a time
                for log_line in new_str.lines() {
                    let single_line = log_line.replace('\r', "").replace('\n', "");
                    if self.specialization_handler.is_some() {
                        let mut handler = self.specialization_handler.take();
                        if let Some(ref mut handler_inner) = handler {
                            if let Some(transformed) =
                                handler_inner.parse_output(single_line.clone(), self)
                            {
                                // If parse_output returns multi-line output, respect each line
                                for output_line in transformed.lines() {
                                    out.push_str(output_line);
                                    out.push('\n');
                                }
                            }
                        }
                        self.specialization_handler = handler;
                    } else {
                        out.push_str(ansi_to_html(&single_line).as_str());
                        out.push('\n');
                    }
                }
            }

            if read < 1 {
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
            inp -= 150;
        }
        self.curr_output_in_progress = lines[std::cmp::max(0, inp)..lines.len()].join("\n");
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    /// Stops the running server/program process.
    ///
    /// Disables crash prevention, kills the process, and returns the exit code if available.
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
