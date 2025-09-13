/// Server management and process monitoring utilities.
///
/// Provides helpers for formatting exit messages, sending termination notifications,
/// and starting and monitoring server processes.
use crate::{
    app_state::AppState, controlled_program::ControlledProgramDescriptor, messages::ConsoleOutput,
};
use tracing::*;

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
    exit_code: i32,
    server_type: Option<String>,
) {
    let termination_msg = ConsoleOutput {
        r#type: "ServerOutput".to_owned(),
        output: format_exit_message(exit_code),
        server_name,
        server_type,
    };
    let _ = state
        .tx
        .send(serde_json::to_string(&termination_msg).unwrap());
}
/// Starts all servers marked for auto-start in the configuration.
///
/// Spawns a background task to process server stdout.
///
/// # Arguments
/// * `_state` - The shared application state.
#[no_mangle]
pub async fn start_servers(_state: AppState) {
    let mut config = _state.config.lock().await;
    for server_desc in config.servers.iter_mut() {
        if server_desc.auto_start {
            let new_desc = server_desc.clone();
            let mut servers = _state.servers.lock().await;
            servers.push(new_desc.into_instance(&_state.specialization_registry));
            drop(servers);
        }
    }
    tokio::spawn(process_stdout(_state.clone()));
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
                        let exit_code = stat.code().unwrap();
                        warn!(
                            "A child process has closed! index: {} ExitCode: {}",
                            index, exit_code
                        );
                        // Mark as inactive
                        server.active = false;
                        // Send termination message to web console
                        send_termination_message(
                            &state,
                            server.name.clone(),
                            exit_code,
                            server.specialized_server_type.clone(),
                        )
                        .await;

                        // Call specialization on_exit if present
                        if let Some(mut handler) = server.specialization_handler.take() {
                            handler.on_exit(server, &state, exit_code);
                            server.specialization_handler = Some(handler);
                        }
                        if exit_code != 0 && server.crash_prevention {
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

                            // Lookup the original crash_prevention setting from config to preserve it
                            let config = state.config.lock().await;
                            for server_config in config.servers.iter() {
                                if server_config.name == server.name {
                                    descriptor.crash_prevention = server_config.crash_prevention;
                                    break;
                                }
                            }
                            drop(config);

                            new_instances.push(descriptor);
                        } else if exit_code != 0 {
                            info!("Server ID: {} has crashed, but crash prevention is disabled. Not restarting.", index);
                        }
                        to_remove.push(index);
                    }
                    Ok(None) => {}
                    Err(_e) => {}
                }
            }
            for desc in new_instances {
                servers.push(desc.into_instance(&state.specialization_registry));
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
                        let _ = state.tx.send(serde_json::to_string(&out).unwrap());
                    }
                }
            }
            drop(servers);
        }
        const REFRESHES_PER_SECOND: f64 = 10.;
        const SECONDS_TO_SLEEP: f64 = 1000. / REFRESHES_PER_SECOND / 1000.;
        std::thread::sleep(std::time::Duration::from_secs_f64(SECONDS_TO_SLEEP));
    }
}
