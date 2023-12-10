use crate::{
    messages::ConsoleOutput, AppState::AppState, ControlledProgram::ControlledProgramDescriptor,
};
use tracing::*;
#[no_mangle]
pub async fn start_servers(_state: AppState) {
    let mut config = _state.config.lock().await;
    for serverDesc in config.servers.iter_mut() {
        if (serverDesc.autoStart) {
            let newDesc = serverDesc.clone();
            let mut servers = _state.servers.lock().await;
            servers.push(newDesc.into_instance());
            drop(servers);
        }
    }
    tokio::spawn(process_stdout(_state.clone()));
}

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
                        if exit_code != 0 {
                            info!("Server ID: {} has crashed, restarting it...", index);
                            let mut descriptor = ControlledProgramDescriptor::new(
                                server.name.as_str(),
                                server.executablePath.as_str(),
                                server.commandLineArgs.clone(),
                                server.working_dir.clone(),
                            );
                            if server.specializedServerType.is_some() {
                                descriptor.setSpecialization(
                                    server.specializedServerType.clone().unwrap(),
                                );
                            }
                            new_instances.push(descriptor);
                        }
                        to_remove.push(index);
                    }
                    Ok(None) => {}
                    Err(_e) => {}
                }
            }
            for desc in new_instances {
                servers.push(desc.into_instance());
            }
            for index in to_remove {
                servers.remove(index);
            }
            //all of our process are valid at this point, no need to even be careful about it
            for server in servers.iter_mut() {
                let str = match tokio::time::timeout(
                    tokio::time::Duration::from_secs_f64(1. / 10.),
                    server.readOutput(),
                )
                .await
                {
                    Ok(val) => val,
                    _ => None,
                };
                match str {
                    Some(val) => {
                        if !val.is_empty() {
                            let out = ConsoleOutput {
                                r#type: "ServerOutput".to_owned(),
                                output: val,
                                server_name: server.name.clone(),
                                server_type: server.specializedServerType.clone(),
                            };
                            let _ = state.tx.send(serde_json::to_string(&out).unwrap());

                        }
                    }

                    _ => {}
                }
            }
            drop(servers);
        }
        const REFRESHES_PER_SECOND: f64 = 10.;
        const SECONDS_TO_SLEEP: f64 = 1000. / REFRESHES_PER_SECOND / 1000.;
        std::thread::sleep(std::time::Duration::from_secs_f64(SECONDS_TO_SLEEP));
    }
}
