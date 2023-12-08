use futures::io;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{error::Error, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{protocol::WebSocketConfig, Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::{error, info};

use crate::{
    configuration::Config,
    messages::{serverInfoMessage, stdinInput, SInfoRequestMessage, ServerInfo},
    slave,
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
    pub stream: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>, // Public member to store the TcpStream
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
        info!(
            "Attempting to create a connection to address: {} port: {}",
            self.address.clone(),
            self.port.clone()
        );
        let addr = format!("ws://{}:{}/ws", self.address, self.port);
        let (ws_stream, _) = connect_async(addr).await?;
        self.stream = Some(ws_stream);
        Ok(())
    }
    pub async fn request_info(&mut self, app_state: AppState) -> Result<(), Box<dyn Error>> {
        if let Some(stream) = &mut self.stream {
            // Prepare your requestInfo message
            let request_message = SInfoRequestMessage {
                r#type: "requestInfo".to_owned(),
                arguments: vec![true],
            };
            let messag = serde_json::to_string(&request_message).unwrap();
            // Send requestInfo message
            let message = Message::Text(messag);
            let _res = stream.send(message).await;

            // Read response
            let def = serverInfoMessage {
                r#type: "".to_string(),
                servers: vec![],
                config: serde_json::from_str(include_str!("defaultconfig.json")).unwrap(),
            };
            let mut sinfo: serverInfoMessage = def.clone();
            if let Some(stream) = &mut self.stream {
                while let Some(message) = stream.next().await {
                    match message {
                        Ok(msg) => {
                            // Handle the message, e.g., if it's a text message
                            if let Message::Text(text) = msg {
                                let serdeRes = serde_json::from_str(&text);
                                if let Ok(val) = serdeRes {
                                    sinfo = val;
                                    let mut slave_servers = app_state.slave_servers.lock().await;
                                    let sMessage = serde_json::to_string(&sinfo).unwrap();
                                    let sDef = serde_json::to_string(&def).unwrap();
                                    if (sMessage != sDef && sinfo.servers.len() != 0) {
                                        for serverInfo in sinfo.servers.iter() {
                                            let newInfo = ServerInfo {
                                                name: serverInfo.name.clone(),
                                                output: serverInfo.output.clone(),
                                                active: serverInfo.active,
                                                host: Some(SlaveConnectionDescriptor {
                                                    address: self.address.clone(),
                                                    port: self.port.clone(),
                                                }),
                                                specialization: serverInfo.specialization.clone(),
                                                specializedInfo: serverInfo.specializedInfo.clone(),
                                            };
                                            let mut foundExistingServer = false;
                                            for existing_server in slave_servers.iter_mut() {
                                                if existing_server.name == newInfo.name {
                                                    existing_server.output = newInfo.output.clone();
                                                    existing_server.specialization =
                                                        newInfo.specialization.clone();
                                                    existing_server.specializedInfo =
                                                        newInfo.specializedInfo.clone();
                                                    foundExistingServer = true;
                                                }
                                            }
                                            if !foundExistingServer {
                                                info!("did not find the server in the list, working on adding it");
                                                slave_servers.push(newInfo);
                                            } else {
                                                info!("Server existed so it was updatted in place");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            // Handle error
                        }
                    }
                }
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
            let message = serde_json::to_string(&stdin_message).unwrap();
            // Send stdin message
            if let Some(stream) = &mut self.stream {
                let message = Message::Text(message);
                stream.send(message).await?;
            }

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
                println!("Success connecting to a slave node!");
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
            let mut slaves = state.slave_connections.lock().await;
            for slave in slaves.iter_mut() {
                tokio::time::timeout(
                    Duration::from_secs_f64(10. / 1000.),
                    slave.request_info(state.clone()),
                )
                .await;
            }
        }
    }
}
