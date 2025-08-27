use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{error::Error, time::Duration};
use tokio::{net::TcpStream, time};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use tracing::error;

use crate::{app_state::AppState, configuration::Config, messages::*};

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
                            // Handle the message, e.g., if it's a text message
                            if let Message::Text(text) = msg {
                                let js: Result<Value, _> = serde_json::from_str(&text);
                                let js2: Value;
                                if js.is_ok() {
                                    js2 = js.unwrap();
                                    match js2["type"].as_str().unwrap() {
                                        "ServerInfo" => {
                                            let serde_res = serde_json::from_str(&text);
                                            if let Ok(val) = serde_res {
                                                _sinfo = val;
                                                let mut slave_servers =
                                                    app_state.slave_servers.lock().await;
                                                let s_message =
                                                    serde_json::to_string(&_sinfo).unwrap();
                                                let s_def = serde_json::to_string(&def).unwrap();
                                                if s_message != s_def && _sinfo.servers.len() != 0 {
                                                    for server_info in _sinfo.servers.iter() {
                                                        let new_info = ServerInfo {
                                                            name: server_info.name.clone(),
                                                            output: server_info.output.clone(),
                                                            active: server_info.active,
                                                            host: Some(SlaveConnectionDescriptor {
                                                                address: self.address.clone(),
                                                                port: self.port.clone(),
                                                            }),
                                                            specialization: server_info
                                                                .specialization
                                                                .clone(),
                                                            specialized_info: server_info
                                                                .specialized_info
                                                                .clone(),
                                                        };
                                                        let mut found_existing_server = false;
                                                        for existing_server in
                                                            slave_servers.iter_mut()
                                                        {
                                                            if existing_server.name == new_info.name
                                                            {
                                                                existing_server.output =
                                                                    new_info.output.clone();
                                                                existing_server.specialization =
                                                                    new_info.specialization.clone();
                                                                existing_server.specialized_info =
                                                                    new_info
                                                                        .specialized_info
                                                                        .clone();
                                                                found_existing_server = true;
                                                            }
                                                        }
                                                        if !found_existing_server {
                                                            slave_servers.push(new_info);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        "ServerOutput" => {
                                            let _ = app_state.tx.send(text.clone());
                                        }
                                        _ => {}
                                    }
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
        let message = serde_json::to_string(&stdin_message).unwrap();
        // Send stdin message
        if let Some(stream) = &mut self.stream {
            let message = Message::Text(message);

            let _ = tokio::time::timeout(Duration::from_secs_f64(1. / 1000.), stream.send(message))
                .await;
        }

        // You might want to read a response or confirmation from the slave
        // Depending on your protocol, handle the response here

        Ok(())
    }
}
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
    //create the polling loop at 4 polls per second
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
