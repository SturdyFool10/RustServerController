use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use futures_util::{stream::SplitSink, SinkExt};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{app_state::AppState, messages::*, theme::ThemeCollection};

type WsSender = Arc<Mutex<SplitSink<WebSocket, Message>>>;

fn text_message(s: String) -> Message {
    Message::Text(Utf8Bytes::from(s))
}

async fn build_server_info_message(state: &AppState, include_output: bool) -> ServerInfoMessage {
    let servers = state.servers.lock().await;
    let mut server_infos = vec![];
    let mut used_names: Vec<String> = vec![];

    for server in servers.iter() {
        used_names.push(server.name.clone());
        let specialized_info = if let Some(handler) = server.specialization_handler.as_ref() {
            handler.get_status()
        } else {
            server
                .specialized_server_info
                .clone()
                .unwrap_or(serde_json::Value::Null)
        };
        let specialization_stats = server
            .specialization_handler
            .as_ref()
            .map(|handler| handler.get_stats())
            .filter(|stats| !stats.is_null());
        let mut s_info = ServerInfo {
            name: server.name.clone(),
            output: "".to_owned(),
            active: true,
            specialization: server.specialized_server_type.clone(),
            specialized_info: Some(specialized_info),
            specialization_options: server.specialization_options.clone(),
            specialization_stats,
            host: None,
        };
        if include_output {
            let split: Vec<&str> = server.curr_output_in_progress.split('\n').collect();
            let start = split.len().saturating_sub(150);
            s_info.output = split[start..].join("\n");
        }
        server_infos.push(s_info);
    }
    drop(servers);

    let config = state.config.lock().await.clone();
    for server_config in config.servers.iter() {
        if !used_names.contains(&server_config.name) {
            server_infos.push(ServerInfo {
                name: server_config.name.clone(),
                output: "".to_owned(),
                active: false,
                specialization: server_config.specialized_server_type.clone(),
                specialized_info: server_config.specialized_server_info.clone(),
                specialization_options: server_config.specialization_options.clone(),
                specialization_stats: None,
                host: None,
            });
        }
    }

    ServerInfoMessage {
        r#type: "ServerInfo".to_owned(),
        servers: server_infos,
        config,
    }
}

async fn send_text_json<T: serde::Serialize>(sender: &WsSender, value: &T) {
    match serde_json::to_string(value) {
        Ok(msg) => {
            let _ = sender.lock().await.send(text_message(msg)).await;
        }
        Err(error) => {
            let _ = sender
                .lock()
                .await
                .send(text_message(format!(
                    "Error serializing response: {}",
                    error
                )))
                .await;
        }
    }
}

async fn send_msgpack_or_text<T: serde::Serialize>(sender: &WsSender, value: &T) {
    if let Ok(bin) = rmp_serde::to_vec_named(value) {
        let _ = sender.lock().await.send(Message::Binary(bin.into())).await;
    } else {
        send_text_json(sender, value).await;
    }
}

async fn send_response<T: serde::Serialize>(sender: &WsSender, value: &T, prefer_msgpack: bool) {
    if prefer_msgpack {
        send_msgpack_or_text(sender, value).await;
    } else {
        send_text_json(sender, value).await;
    }
}

async fn send_server_info(
    sender: &WsSender,
    state: &AppState,
    request_text: &str,
    prefer_msgpack: bool,
) {
    let include_output = serde_json::from_str::<SInfoRequestMessage>(request_text)
        .ok()
        .and_then(|val| val.arguments.first().copied())
        .unwrap_or(true);
    let info = build_server_info_message(state, include_output).await;
    send_response(sender, &info, prefer_msgpack).await;
}

async fn build_config_info(state: &AppState) -> ConfigInfo {
    let config = state.config.lock().await;
    ConfigInfo {
        r#type: "ConfigInfo".to_owned(),
        config: config.clone(),
    }
}

async fn load_theme_names(state: &AppState) -> Vec<String> {
    let config = state.config.lock().await;
    let themes_folder = config
        .themes_folder
        .clone()
        .unwrap_or_else(|| "themes".to_string());
    drop(config);

    ThemeCollection::load_from_directory(&themes_folder)
        .unwrap_or_default()
        .themes
        .iter()
        .map(|theme| theme.name.clone())
        .collect()
}

async fn build_themes_list(state: &AppState) -> ThemesList {
    ThemesList {
        r#type: "themesList".to_string(),
        themes: load_theme_names(state).await,
    }
}

#[derive(Deserialize)]
struct GetThemeCSSWeb {
    theme_name: String,
}

async fn build_theme_css(state: &AppState, theme_name: String) -> ThemeCSS {
    let config = state.config.lock().await;
    let themes_folder = config
        .themes_folder
        .clone()
        .unwrap_or_else(|| "themes".to_string());
    drop(config);

    let theme_collection = ThemeCollection::load_from_directory(&themes_folder).unwrap_or_default();
    let css = if let Some(theme) = theme_collection
        .themes
        .iter()
        .find(|theme| theme.name == theme_name)
    {
        theme.to_css()
    } else {
        ThemeCollection::default()
            .themes
            .first()
            .map(|theme| theme.to_css())
            .unwrap_or_default()
    };

    ThemeCSS {
        r#type: "themeCSS".to_string(),
        theme_name,
        css,
    }
}

async fn send_theme_css_for_request(
    sender: &WsSender,
    state: &AppState,
    request_text: &str,
    prefer_msgpack: bool,
) {
    let message: GetThemeCSSWeb = match serde_json::from_str(request_text) {
        Ok(msg) => msg,
        Err(_) => {
            let _ = sender
                .lock()
                .await
                .send(text_message(
                    "Error parsing GetThemeCSS message".to_string(),
                ))
                .await;
            return;
        }
    };
    let theme_css = build_theme_css(state, message.theme_name).await;
    send_response(sender, &theme_css, prefer_msgpack).await;
}

pub(crate) async fn handle_client_request(
    sender: &WsSender,
    state: &AppState,
    event_type: &str,
    request_text: &str,
    prefer_msgpack: bool,
) -> bool {
    match event_type {
        "requestInfo" => {
            send_server_info(sender, state, request_text, prefer_msgpack).await;
            true
        }
        "getConfig" | "requestConfig" => {
            let config_info = build_config_info(state).await;
            send_response(sender, &config_info, prefer_msgpack).await;
            true
        }
        "getThemesList" => {
            let themes_list = build_themes_list(state).await;
            send_response(sender, &themes_list, prefer_msgpack).await;
            true
        }
        "getThemeCSS" => {
            send_theme_css_for_request(sender, state, request_text, prefer_msgpack).await;
            true
        }
        _ => false,
    }
}
