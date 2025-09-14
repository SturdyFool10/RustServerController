use crate::{app_state::AppState, configuration::Config, messages::*};
use futures_util::{SinkExt, StreamExt};
use rmp_serde::{from_slice, to_vec};
use serde::{Deserialize, Serialize};
use std::{error::Error, time::Duration};
use tokio::{net::TcpStream, time};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Bytes, Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::error;

/// Descriptor for a slave connection, including address and port.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SlaveConnectionDescriptor {
    pub address: String,
    pub port: String,
}

/// Represents a connection to a slave node, including the websocket stream.
#[derive(Serialize, Deserialize, Debug)]
pub struct SlaveConnection {
    pub address: String,
    pub port: String,
    #[serde(skip)]
    pub stream: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>, // Public member to store the TcpStream
}

impl SlaveConnection {
    /// Creates a new SlaveConnection instance with the given address and port.
    pub fn new(address: String, port: String) -> Self {
        Self {
            address,
            port,
            stream: None,
        }
    }

    /// Attempts to establish a websocket connection to the slave node.
    ///
    /// # Returns
    /// * `Ok(())` if the connection is successful.
    /// * `Err` if the connection fails.
    pub async fn create_connection(&mut self) -> Result<(), Box<dyn Error>> {
        let addr = format!("ws://{}:{}/ws", self.address, self.port);
        let (ws_stream, _) = connect_async(addr).await?;
        self.stream = Some(ws_stream);
        Ok(())
    }

    /// Requests server info from the slave node and updates the shared state.
    ///
    /// # Arguments
    /// * `app_state` - The shared application state to update.
    ///
    /// # Returns
    /// * `Ok(())` if the request and update succeed.
    /// * `Err` if there is no active connection or another error occurs.
    pub async fn request_info(&mut self, app_state: AppState) -> Result<(), Box<dyn Error>> {
        if let Some(stream) = &mut self.stream {
            // Prepare your requestInfo message
            let request_message = SInfoRequestMessage {
                r#type: "requestInfo".to_owned(),
                arguments: vec![true],
            };
            let msg_bytes = to_vec(&request_message).unwrap();
            // Send requestInfo message as MessagePack binary
            let message = Message::Binary(Bytes::from(msg_bytes));
            let _res = stream.send(message).await;

            // Read response
            let def = ServerInfoMessage {
                r#type: "".to_string(),
                servers: vec![],
                config: Config::default(),
            };
            let mut _sinfo: ServerInfoMessage = def.clone();
            if let Some(stream) = &mut self.stream {
                while let Some(message) = stream.next().await {
                    match message {
                        Ok(msg) => {
                            if let Message::Binary(bin) = msg {
                                // Try to decode as MessagePack
                                if let Ok(server_info_msg) = from_slice::<ServerInfoMessage>(&bin) {
                                    let mut slave_servers = app_state.slave_servers.lock().await;
                                    let s_message = to_vec(&server_info_msg).unwrap();
                                    let s_def = to_vec(&def).unwrap();
                                    if s_message != s_def && !server_info_msg.servers.is_empty() {
                                        for server_info in server_info_msg.servers.iter() {
                                            let new_info = ServerInfo {
                                                name: server_info.name.clone(),
                                                output: server_info.output.clone(),
                                                active: server_info.active,
                                                host: Some(SlaveConnectionDescriptor {
                                                    address: self.address.clone(),
                                                    port: self.port.clone(),
                                                }),
                                                specialization: server_info.specialization.clone(),
                                                specialized_info: server_info
                                                    .specialized_info
                                                    .clone(),
                                            };
                                            let mut found_existing_server = false;
                                            for existing_server in slave_servers.iter_mut() {
                                                if existing_server.name == new_info.name {
                                                    existing_server.output =
                                                        new_info.output.clone();
                                                    existing_server.specialization =
                                                        new_info.specialization.clone();
                                                    existing_server.specialized_info =
                                                        new_info.specialized_info.clone();
                                                    found_existing_server = true;
                                                }
                                            }
                                            if !found_existing_server {
                                                slave_servers.push(new_info);
                                            }
                                        }
                                    }
                                } else if let Ok(text) = std::str::from_utf8(&bin) {
                                    // If not valid MessagePack, treat as UTF-8 text (e.g., ServerOutput)
                                    let _ = app_state.tx.send(text.to_string());
                                }
                            }
                        }
                        Err(_e) => {
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

    /// Sends stdin input to a server on the slave node.
    ///
    /// # Arguments
    /// * `server_name` - The name of the server to send input to.
    /// * `message` - The input message to send.
    ///
    /// # Returns
    /// * `Ok(())` if the message is sent successfully.
    /// * `Err` if there is a connection or protocol error.
    #[allow(dead_code)]
    pub async fn write_stdin(
        &mut self,
        server_name: String,
        message: String,
    ) -> Result<(), Box<dyn Error>> {
        // Prepare your stdin message
        let stdin_message = StdinInput {
            r#type: "stdinInput".to_owned(),
            server_name,
            value: message,
        };
        let msg_bytes = to_vec(&stdin_message).unwrap();
        // Send stdin message as MessagePack binary
        if let Some(stream) = &mut self.stream {
            let message = Message::Binary(Bytes::from(msg_bytes));
            let _ = tokio::time::timeout(Duration::from_secs_f64(1. / 1000.), stream.send(message))
                .await;
        }

        // You might want to read a response or confirmation from the slave
        // Depending on your protocol, handle the response here

        Ok(())
    }
}

/// Creates and manages connections to all configured slave nodes.
/// Polls each slave for server info at a fixed interval and updates shared state.
///
/// # Arguments
/// * `state` - The shared application state.
pub async fn create_slave_connections(state: AppState) {
    let mut slaves: Vec<SlaveConnection> = vec![];
    let conf = state.config.lock().await;
    let config: Config = conf.clone();
    drop(conf);
    for slave_desc in config.slave_connections {
        let mut slave = SlaveConnection::new(slave_desc.address.clone(), slave_desc.port.clone());
        let conn_res = slave.create_connection().await;
        match conn_res {
            Ok(_) => {
                println!("Success connecting to a slave node!");
                slaves.push(slave);
            }
            Err(what) => {
                error!(
                    "Error connecting to: {}:{}, Message: {}",
                    &slave_desc.address, &slave_desc.port, what
                );
            }
        }
    }
    {
        let mut slaves_list = state.slave_connections.lock().await;
        for slave in slaves {
            slaves_list.push(slave);
        }
    }
    // Create the polling loop at 4 polls per second
    {
        let mut interval = time::interval(Duration::from_millis(250)); // 4 times per second
        loop {
            interval.tick().await;
            // Perform actions on each tick, such as checking the status of connections,
            // sending keep-alive messages, etc.
            let mut slaves = state.slave_connections.lock().await;
            for slave in slaves.iter_mut() {
                let _ = tokio::time::timeout(
                    Duration::from_secs_f64(10. / 1000.),
                    slave.request_info(state.clone()),
                )
                .await;
            }
        }
    }
}
