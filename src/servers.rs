use crate::{
    app_state::AppState, controlled_program::ControlledProgramDescriptor, messages::ConsoleOutput,
};
/// Server management and process monitoring utilities.
///
/// Provides helpers for formatting exit messages, sending termination notifications,
/// and starting and monitoring server processes.
use tokio::io::AsyncWriteExt;
use tracing::*;

/// A server picked out of the config: by UUID when the caller has one (the
/// stable identity), or by name for clients that only know the name.
#[derive(Clone, Debug)]
pub struct ServerRef {
    pub uuid: Option<String>,
    pub name: String,
    /// The node running it, if it's remote.
    pub node: Option<String>,
}

pub async fn resolve_server(
    state: &AppState,
    server_uuid: Option<&str>,
    server_name: &str,
) -> Option<ServerRef> {
    let config = state.config.lock().await;
    let by_uuid = server_uuid
        .map(str::trim)
        .filter(|uuid| !uuid.is_empty())
        .and_then(|uuid| {
            config
                .servers
                .iter()
                .find(|server| server.server_uuid.as_deref() == Some(uuid))
        });
    let found = by_uuid.or_else(|| {
        // Name is only a fallback when no UUID was supplied at all.
        if server_uuid.is_some_and(|uuid| !uuid.trim().is_empty()) {
            None
        } else {
            config.servers.iter().find(|server| server.name == server_name)
        }
    })?;
    Some(ServerRef {
        uuid: found.server_uuid.clone(),
        name: found.name.clone(),
        node: found.node.clone(),
    })
}

/// Writes a single line (with a trailing `\r\n`) to the stdin of the running
/// server with this UUID. Returns `true` if the write succeeded, `false` if
/// no such server is running or it has no stdin.
///
/// Generic across every specialization; callers are responsible for
/// validating/sanitizing `line` before calling this, since it's written
/// verbatim as raw console input.
pub async fn write_line_to_server_stdin(state: &AppState, server_uuid: &str, line: &str) -> bool {
    // Only clone the stdin handle under the lock; the write itself happens
    // outside it, and the server never leaves the list.
    let stdin = {
        let servers = state.servers.lock().await;
        servers
            .iter()
            .find(|server| server.server_uuid == server_uuid)
            .and_then(|server| server.stdin.clone())
    };
    let Some(stdin) = stdin else {
        return false;
    };
    let mut stdin = stdin.lock().await;
    stdin
        .write_all(format!("{}\r\n", line).as_bytes())
        .await
        .is_ok()
}

pub fn format_controller_message(message: impl std::fmt::Display) -> String {
    format!(
        "<span style=\"color: var(--danger, #FF5555);\">[Controller: {}]</span>",
        message
    )
}

pub fn send_controller_message(
    state: &AppState,
    server_name: String,
    server_uuid: Option<String>,
    server_type: Option<String>,
    message: impl std::fmt::Display,
) {
    let output = ConsoleOutput {
        r#type: "ServerOutput".to_owned(),
        output: format_controller_message(message),
        server_name,
        server_uuid,
        server_type,
    };
    if let Ok(payload) = serde_json::to_string(&output) {
        let _ = state.tx.send(payload);
    }
}

pub fn broadcast_json<T: serde::Serialize>(state: &AppState, value: &T) {
    if let Ok(payload) = serde_json::to_string(value) {
        let _ = state.tx.send(payload);
    }
}

pub async fn create_instance_async(
    state: &AppState,
    desc: ControlledProgramDescriptor,
) -> Option<crate::controlled_program::ControlledProgramInstance> {
    let mut instance = match desc
        .clone()
        .into_instance_async(&state.specialization_registry)
        .await
    {
        Ok(instance) => instance,
        Err(error) => {
            error!("Failed to start server '{}': {}", desc.name, error);
            send_controller_message(
                state,
                desc.name,
                desc.server_uuid,
                desc.specialized_server_type,
                format!("failed to start server: {}", error),
            );
            return None;
        }
    };

    if let Some(mut handler) = instance.specialization_handler.take() {
        handler.on_start(&mut instance, state);
        instance.specialization_handler = Some(handler);
    }
    Some(instance)
}

pub fn specialization_update(
    server_name: String,
    server_uuid: Option<String>,
    info: serde_json::Value,
    stats: Option<serde_json::Value>,
    specialization_options: Option<serde_json::Value>,
    specialization: String,
    active: bool,
) -> crate::messages::ServerSpecializationInfoUpdate {
    crate::messages::ServerSpecializationInfoUpdate {
        r#type: "ServerSpecializationInfoUpdate".to_owned(),
        server_name,
        server_uuid,
        info,
        stats,
        specialization_options,
        specialization,
        active,
    }
}

pub fn stats_if_present(stats: serde_json::Value) -> Option<serde_json::Value> {
    if stats.is_null() {
        None
    } else {
        Some(stats)
    }
}

// Helper to format exit code message for web console
/// Formats a server exit code as an HTML message for the web console.
///
/// # Arguments
/// * `exit_code` - The exit code to display.
///
/// # Returns
/// An HTML string with the exit code highlighted.
pub fn format_exit_message(exit_code: impl std::fmt::Display) -> String {
    format!(
        "<span style=\"color: var(--warning, #FFA500);\">[Server exited with code {}]</span>",
        exit_code
    )
}

// Helper function to send server termination message to web console
/// Sends a server termination message to the web console.
///
/// # Arguments
/// * `state` - The shared application state.
/// * `server_name` - The name of the server that exited.
/// * `server_uuid` - The UUID of the server that exited.
/// * `exit_code` - The exit code of the server.
/// * `server_type` - The specialized server type, if any.
pub async fn send_termination_message(
    state: &AppState,
    server_name: String,
    server_uuid: Option<String>,
    exit_code: impl std::fmt::Display,
    server_type: Option<String>,
) {
    let termination_msg = ConsoleOutput {
        r#type: "ServerOutput".to_owned(),
        output: format_exit_message(exit_code),
        server_name,
        server_uuid,
        server_type,
    };
    broadcast_json(state, &termination_msg);
}
/// Starts all servers marked for auto-start in the configuration.
///
/// Spawns a background task to process server stdout.
///
/// # Arguments
/// * `_state` - The shared application state.
#[no_mangle]
pub async fn start_servers(state: AppState) {
    let auto_start_servers = {
        let config = state.config.lock().await;
        config
            .servers
            .iter()
            .filter(|server_desc| server_desc.auto_start && server_desc.node.is_none())
            .cloned()
            .collect::<Vec<_>>()
    };

    for server_desc in auto_start_servers {
        if let Some(instance) = create_instance_async(&state, server_desc).await {
            // After starting a new server, send specialization info update
            if let Some(handler) = instance.specialization_handler.as_ref() {
                let info = handler.get_status();
                let update = specialization_update(
                    instance.name.clone(),
                    Some(instance.server_uuid.clone()),
                    info,
                    stats_if_present(handler.get_stats()),
                    instance.specialization_options.clone(),
                    instance.specialized_server_type.clone().unwrap_or_default(),
                    instance.active,
                );
                broadcast_json(&state, &update);
            }
            let mut servers = state.servers.lock().await;
            servers.push(instance);
        }
    }
    tokio::spawn(process_stdout(state.clone()));
}

/// How often live stats are re-pushed to clients for every running server
/// that exposes them, even if no log line triggered a status update.
const STATS_PUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

fn broadcast_specialization_state(
    state: &AppState,
    server: &mut crate::controlled_program::ControlledProgramInstance,
) {
    let Some(handler) = server.specialization_handler.as_mut() else {
        return;
    };
    let update = specialization_update(
        server.name.clone(),
        Some(server.server_uuid.clone()),
        handler.get_status(),
        stats_if_present(handler.get_stats()),
        server.specialization_options.clone(),
        server.specialized_server_type.clone().unwrap_or_default(),
        server.active,
    );
    broadcast_json(state, &update);
    handler.set_status_update_sent();
}

fn broadcast_server_output(
    state: &AppState,
    server: &crate::controlled_program::ControlledProgramInstance,
    output: String,
) {
    let out = ConsoleOutput {
        r#type: "ServerOutput".to_owned(),
        output,
        server_name: server.name.clone(),
        server_uuid: Some(server.server_uuid.clone()),
        server_type: server.specialized_server_type.clone(),
    };
    broadcast_json(state, &out);
}

/// Monitors all running servers, handles process exits, restarts crashed servers if needed,
/// and relays server output to the web console.
///
/// The shared server list is only locked for the short, non-awaiting pass over
/// the servers (output is read by per-process reader tasks), so other tasks
/// never observe a running server as missing. Exit handling, which awaits,
/// happens after the lock is released.
///
/// # Arguments
/// * `state` - The shared application state.
pub async fn process_stdout(state: AppState) {
    let mut last_stats_push = tokio::time::Instant::now();
    loop {
        let push_stats = last_stats_push.elapsed() >= STATS_PUSH_INTERVAL;
        if push_stats {
            last_stats_push = tokio::time::Instant::now();
        }

        let mut exited = vec![];
        {
            let mut servers = state.servers.lock().await;
            let mut index = 0;
            while index < servers.len() {
                let server = &mut servers[index];

                if let Some(mut handler) = server.specialization_handler.take() {
                    if let Some(interval) = handler.schedule_interval() {
                        let now = tokio::time::Instant::now();
                        let due = server.next_schedule_at.is_none_or(|at| now >= at);
                        if due {
                            handler.on_schedule(server, &state);
                            server.next_schedule_at = Some(now + interval);
                        }
                    }
                    server.specialization_handler = Some(handler);
                }

                let output = server.read_output();
                let has_output = output.is_some();
                if let Some(val) = output.filter(|val| !val.is_empty()) {
                    broadcast_server_output(&state, server, val);
                }

                if let Some(handler) = server.specialization_handler.as_ref() {
                    // The first update waits for the first output after spawn.
                    let first_update = !server.specialization_info_sent && has_output;
                    if first_update
                        || handler.has_status_update()
                        || (push_stats && server.specialization_info_sent)
                    {
                        server.specialization_info_sent = true;
                        broadcast_specialization_state(&state, server);
                    }
                }

                if matches!(server.process.try_wait(), Ok(Some(_))) {
                    exited.push(servers.remove(index));
                } else {
                    index += 1;
                }
            }
        }

        let mut new_instances = vec![];
        for mut server in exited {
            let exit_code = server.process.try_wait().ok().flatten().and_then(|s| s.code());
            let exit_code_label = exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            warn!(
                "A child process has closed! name: {} ExitCode: {}",
                server.name, exit_code_label
            );
            server.active = false;

            // Flush whatever the process wrote right before exiting.
            if let Some(val) = server
                .drain_remaining_output(tokio::time::Duration::from_millis(500))
                .await
                .filter(|val| !val.is_empty())
            {
                broadcast_server_output(&state, &server, val);
            }

            send_termination_message(
                &state,
                server.name.clone(),
                Some(server.server_uuid.clone()),
                exit_code_label.clone(),
                server.specialized_server_type.clone(),
            )
            .await;

            if let Some(mut handler) = server.specialization_handler.take() {
                handler.on_exit(&mut server, &state, exit_code.unwrap_or(-1));
                server.specialization_handler = Some(handler);
            }

            // Always send specialization info update when server goes inactive
            broadcast_specialization_state(&state, &mut server);

            if exit_code != Some(0) && server.crash_prevention {
                info!("Server '{}' has crashed, restarting it...", server.name);
                let mut descriptor = ControlledProgramDescriptor::new(
                    server.name.as_str(),
                    server.executable_path.as_str(),
                    server.command_line_args.clone(),
                    server.working_dir.clone(),
                );
                descriptor.specialized_server_type = server.specialized_server_type.clone();
                descriptor.specialization_options = server.specialization_options.clone();
                descriptor.server_uuid = Some(server.server_uuid.clone());

                // Lookup the original crash_prevention setting from config to preserve it
                if let Some((crash_prevention, specialization_options)) = {
                    let config = state.config.lock().await;
                    config
                        .servers
                        .iter()
                        .find(|server_config| server_config.name == server.name)
                        .map(|server_config| {
                            (
                                server_config.crash_prevention,
                                server_config.specialization_options.clone(),
                            )
                        })
                } {
                    descriptor.crash_prevention = crash_prevention;
                    descriptor.specialization_options = specialization_options;
                }

                new_instances.push(descriptor);
            } else if exit_code != Some(0) {
                info!(
                    "Server '{}' has crashed, but crash prevention is disabled. Not restarting.",
                    server.name
                );
            }
        }

        for desc in new_instances {
            let Some(mut instance) = create_instance_async(&state, desc).await else {
                continue;
            };
            broadcast_specialization_state(&state, &mut instance);
            state.servers.lock().await.push(instance);
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}
