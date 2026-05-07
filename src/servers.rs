use crate::{
    app_state::AppState, controlled_program::ControlledProgramDescriptor, messages::ConsoleOutput,
};
/// Server management and process monitoring utilities.
///
/// Provides helpers for formatting exit messages, sending termination notifications,
/// and starting and monitoring server processes.
use tracing::*;

pub fn format_controller_message(message: impl std::fmt::Display) -> String {
    format!(
        "<span style=\"color: var(--danger, #FF5555);\">[Controller: {}]</span>",
        message
    )
}

pub fn send_controller_message(
    state: &AppState,
    server_name: String,
    server_type: Option<String>,
    message: impl std::fmt::Display,
) {
    let output = ConsoleOutput {
        r#type: "ServerOutput".to_owned(),
        output: format_controller_message(message),
        server_name,
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

pub fn create_instance(
    state: &AppState,
    desc: ControlledProgramDescriptor,
) -> Option<crate::controlled_program::ControlledProgramInstance> {
    match desc.clone().into_instance(&state.specialization_registry) {
        Ok(mut instance) => {
            if let Some(mut handler) = instance.specialization_handler.take() {
                handler.on_start(&mut instance, state);
                instance.specialization_handler = Some(handler);
            }
            Some(instance)
        }
        Err(error) => {
            error!("Failed to start server '{}': {}", desc.name, error);
            send_controller_message(
                state,
                desc.name,
                desc.specialized_server_type,
                format!("failed to start server: {}", error),
            );
            None
        }
    }
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
/// * `exit_code` - The exit code of the server.
/// * `server_type` - The specialized server type, if any.
pub async fn send_termination_message(
    state: &AppState,
    server_name: String,
    exit_code: impl std::fmt::Display,
    server_type: Option<String>,
) {
    let termination_msg = ConsoleOutput {
        r#type: "ServerOutput".to_owned(),
        output: format_exit_message(exit_code),
        server_name,
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
    let mut config = state.config.lock().await;
    for server_desc in config.servers.iter_mut() {
        if server_desc.auto_start {
            let new_desc = server_desc.clone();
            let mut servers = state.servers.lock().await;
            let Some(instance) = create_instance(&state, new_desc) else {
                continue;
            };
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
            servers.push(instance);
            drop(servers);
        }
    }
    tokio::spawn(process_stdout(state.clone()));
}

/// Monitors all running servers, handles process exits, restarts crashed servers if needed,
/// and relays server output to the web console.
///
/// This function runs in a loop, checking server status and output at a fixed refresh rate.
///
/// # Arguments
/// * `state` - The shared application state.
pub async fn process_stdout(state: AppState) {
    loop {
        {
            let mut new_instances = vec![];
            let mut to_remove = vec![];
            let mut servers = state.servers.lock().await;
            for (index, server) in servers.iter_mut().enumerate() {
                let status = server.process.try_wait();
                match status {
                    Ok(Some(stat)) => {
                        let exit_code = stat.code();
                        let exit_code_label = exit_code
                            .map(|code| code.to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        warn!(
                            "A child process has closed! index: {} ExitCode: {}",
                            index, exit_code_label
                        );
                        // Mark as inactive
                        server.active = false;
                        // Send termination message to web console
                        send_termination_message(
                            &state,
                            server.name.clone(),
                            exit_code_label.clone(),
                            server.specialized_server_type.clone(),
                        )
                        .await;

                        // Call specialization on_exit if present
                        if let Some(mut handler) = server.specialization_handler.take() {
                            handler.on_exit(server, &state, exit_code.unwrap_or(-1));
                            server.specialization_handler = Some(handler);
                        }

                        // Always send specialization info update when server goes inactive
                        if let Some(handler) = server.specialization_handler.as_mut() {
                            let info = handler.get_status();
                            let update = specialization_update(
                                server.name.clone(),
                                Some(server.server_uuid.clone()),
                                info,
                                stats_if_present(handler.get_stats()),
                                server.specialization_options.clone(),
                                server.specialized_server_type.clone().unwrap_or_default(),
                                server.active,
                            );
                            broadcast_json(&state, &update);
                        }
                        if exit_code != Some(0) && server.crash_prevention {
                            info!("Server ID: {} has crashed, restarting it...", index);
                            let mut descriptor = ControlledProgramDescriptor::new(
                                server.name.as_str(),
                                server.executable_path.as_str(),
                                server.command_line_args.clone(),
                                server.working_dir.clone(),
                            );
                            // set_specialization removed; assign directly if needed
                            descriptor.specialized_server_type =
                                server.specialized_server_type.clone();
                            descriptor.specialization_options =
                                server.specialization_options.clone();

                            // Lookup the original crash_prevention setting from config to preserve it
                            let config = state.config.lock().await;
                            for server_config in config.servers.iter() {
                                if server_config.name == server.name {
                                    descriptor.crash_prevention = server_config.crash_prevention;
                                    descriptor.specialization_options =
                                        server_config.specialization_options.clone();
                                    break;
                                }
                            }
                            drop(config);

                            new_instances.push(descriptor);
                        } else if exit_code != Some(0) {
                            info!("Server ID: {} has crashed, but crash prevention is disabled. Not restarting.", index);
                        }
                        to_remove.push(index);
                    }
                    Ok(None) => {}
                    Err(_e) => {}
                }
            }
            for desc in new_instances {
                let Some(instance) = create_instance(&state, desc) else {
                    continue;
                };
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
                servers.push(instance);
            }
            // Remove servers in reverse order to avoid index shifting
            to_remove.sort_unstable_by(|a, b| b.cmp(a));
            for index in to_remove {
                servers.remove(index);
            }
            //all of our process are valid at this point, no need to even be careful about it
            for server in servers.iter_mut() {
                let str = tokio::time::timeout(
                    tokio::time::Duration::from_secs_f64(1. / 10.),
                    server.read_output(),
                )
                .await
                .unwrap_or_default();
                if let Some(val) = str {
                    if !val.is_empty() {
                        let out = ConsoleOutput {
                            r#type: "ServerOutput".to_owned(),
                            output: val,
                            server_name: server.name.clone(),
                            server_type: server.specialized_server_type.clone(),
                        };
                        broadcast_json(&state, &out);
                    }
                    // Send specialization info update only after first output after spawn
                    if let Some(handler) = server.specialization_handler.as_mut() {
                        if !server.specialization_info_sent {
                            let info = handler.get_status();
                            let update = specialization_update(
                                server.name.clone(),
                                Some(server.server_uuid.clone()),
                                info,
                                stats_if_present(handler.get_stats()),
                                server.specialization_options.clone(),
                                server.specialized_server_type.clone().unwrap_or_default(),
                                server.active,
                            );
                            broadcast_json(&state, &update);
                            handler.set_status_update_sent();
                            server.specialization_info_sent = true;
                        } else if handler.has_status_update() {
                            let info = handler.get_status();
                            let update = specialization_update(
                                server.name.clone(),
                                Some(server.server_uuid.clone()),
                                info,
                                stats_if_present(handler.get_stats()),
                                server.specialization_options.clone(),
                                server.specialized_server_type.clone().unwrap_or_default(),
                                server.active,
                            );
                            broadcast_json(&state, &update);
                            handler.set_status_update_sent();
                        }
                    }
                }
            }
            drop(servers);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}
