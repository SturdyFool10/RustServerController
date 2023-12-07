use futures::io;
use serde::{Deserialize, Serialize};
use std::{error::Error, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time,
};
use tracing::error;

use crate::{
    configuration::Config,
    messages::{stdinInput, SInfoRequestMessage},
    AppState::AppState,
};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SlaveConnectionDescriptor {
    pub address: String,
    pub port: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SlaveConnection {
    pub address: String,
    pub port: String,
    #[serde(skip)]
    pub stream: Option<TcpStream>, // Public member to store the TcpStream
}

impl SlaveConnection {
    // New method to create a new SlaveConnection instance
    pub fn new(address: String, port: String) -> Self {
        Self {
            address,
            port,
            stream: None,
        }
    }

    // Changed to async and return Result to handle connection success or failure
    pub async fn create_connection(&mut self) -> Result<(), Box<dyn Error>> {
        let addr = format!("{}:{}", self.address, self.port);
        let stream = TcpStream::connect(addr).await?;
        self.stream = Some(stream);
        Ok(())
    }
    pub async fn request_info(&mut self, app_state: AppState) -> Result<(), Box<dyn Error>> {
        if let Some(stream) = &mut self.stream {
            // Prepare your requestInfo message
            let request_message = SInfoRequestMessage {
                r#type: "requestInfo".to_owned(),
                arguments: vec![true],
            };

            // Send requestInfo message
            stream
                .write_all(serde_json::to_string(&request_message).unwrap().as_bytes())
                .await?;

            // Read response
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await?;

            // Process response here
            // ...

            // Update AppState with the new information
            {
                // Lock AppState only for the duration of the update
                let mut app_state_lock = app_state.servers.lock().await;
                // Update the state based on the response
                // ...
            }

            Ok(())
        } else {
            Err("No active connection".into())
        }
    }
    pub async fn write_stdin(
        &mut self,
        server_name: String,
        message: String,
    ) -> Result<(), Box<dyn Error>> {
        if let Some(stream) = &mut self.stream {
            // Prepare your stdin message
            let stdin_message = stdinInput {
                r#type: "passStdin".to_owned(),
                server_name,
                value: message,
            };

            // Send stdin message
            stream
                .write_all(serde_json::to_string(&stdin_message).unwrap().as_bytes())
                .await?;

            // You might want to read a response or confirmation from the slave
            // Depending on your protocol, handle the response here

            Ok(())
        } else {
            Err("No active connection".into())
        }
    }
}
pub async fn create_slave_connections(state: AppState) {
    let mut slaves: Vec<SlaveConnection> = vec![];
    let conf = state.config.lock().await;
    let config: Config = conf.clone();
    drop(conf);
    for slaveDesc in config.slaveConnections {
        let mut slave = SlaveConnection::new(slaveDesc.address.clone(), slaveDesc.port.clone());
        let connRes = slave.create_connection().await;
        match connRes {
            Ok(_) => {
                slaves.push(slave);
            }
            Err(what) => {
                error!(
                    "Error connecting to: {}:{}, Message: {}",
                    &slaveDesc.address, &slaveDesc.port, what
                );
            }
        }
    }
    {
        let mut slavesList = state.slave_connections.lock().await;
        for slave in slaves {
            slavesList.push(slave);
        }
    }
    //create the polling loop at 4 per second
    {
        let mut interval = time::interval(Duration::from_millis(250)); // 4 times per second
        loop {
            interval.tick().await;
            // Perform actions on each tick, such as checking the status of connections,
            // sending keep-alive messages, etc.
        }
    }
}
