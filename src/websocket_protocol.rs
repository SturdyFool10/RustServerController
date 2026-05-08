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

pub(crate) async fn build_server_info_message(
    state: &AppState,
    include_output: bool,
) -> ServerInfoMessage {
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
            server_uuid: Some(server.server_uuid.clone()),
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
                server_uuid: server_config.server_uuid.clone(),
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

    let active_server_uuids: Vec<String> = config
        .servers
        .iter()
        .filter_map(|server| server.server_uuid.clone())
        .collect();

    ServerInfoMessage {
        r#type: "ServerInfo".to_owned(),
        servers: server_infos,
        archived_server_stats: crate::specializations::player_activity::archived_server_stats(
            &active_server_uuids,
        ),
        config,
    }
}

fn has_server_permission(
    session: Option<&crate::auth::AuthSession>,
    server_uuid: Option<&str>,
    server_name: &str,
    permission: &str,
) -> bool {
    session
        .map(|session| {
            crate::auth::permissions_include_server(
                &session.permissions,
                permission,
                server_uuid,
                server_name,
            )
        })
        .unwrap_or(false)
}

fn has_any_server_permission(session: Option<&crate::auth::AuthSession>, permission: &str) -> bool {
    session
        .map(|session| {
            crate::auth::permissions_include(&session.permissions, permission)
                || session.permissions.iter().any(|value| {
                    value.starts_with("server:") && value.ends_with(&format!(":{permission}"))
                })
        })
        .unwrap_or(false)
}

pub(crate) fn filter_config_for_session(
    mut config: crate::configuration::Config,
    session: Option<&crate::auth::AuthSession>,
) -> crate::configuration::Config {
    if has_permission(session, crate::auth::PERMISSION_ADMIN) {
        return config;
    }
    if has_permission(session, crate::auth::PERMISSION_CONFIG) {
        config.auth = crate::configuration::AuthConfig::default();
        return config;
    }
    config.servers.retain(|server| {
        has_server_permission(
            session,
            server.server_uuid.as_deref(),
            &server.name,
            crate::auth::PERMISSION_VIEW,
        )
    });
    config.auth = crate::configuration::AuthConfig::default();
    config.slave_connections.clear();
    config.minecraft_account_filter_detail_groups.clear();
    config
}

pub(crate) fn filter_server_info_message_for_session(
    mut info: ServerInfoMessage,
    session: Option<&crate::auth::AuthSession>,
) -> ServerInfoMessage {
    if has_permission(session, crate::auth::PERMISSION_ADMIN) {
        return info;
    }
    let visible_stats_server_uuids = info
        .servers
        .iter()
        .filter(|server| {
            has_server_permission(
                session,
                server.server_uuid.as_deref(),
                &server.name,
                crate::auth::PERMISSION_VIEW,
            ) && has_server_permission(
                session,
                server.server_uuid.as_deref(),
                &server.name,
                crate::auth::PERMISSION_STATS,
            )
        })
        .filter_map(|server| server.server_uuid.clone())
        .collect::<Vec<_>>();
    info.servers = info
        .servers
        .into_iter()
        .filter_map(|mut server| {
            if !has_server_permission(
                session,
                server.server_uuid.as_deref(),
                &server.name,
                crate::auth::PERMISSION_VIEW,
            ) {
                return None;
            }
            if !has_server_permission(
                session,
                server.server_uuid.as_deref(),
                &server.name,
                crate::auth::PERMISSION_CONSOLE,
            ) {
                server.output.clear();
            }
            if !has_server_permission(
                session,
                server.server_uuid.as_deref(),
                &server.name,
                crate::auth::PERMISSION_STATS,
            ) {
                server.specialization_stats = None;
            }
            Some(server)
        })
        .collect();
    info.config = filter_config_for_session(info.config, session);
    info.archived_server_stats =
        filter_archived_server_stats(info.archived_server_stats, &visible_stats_server_uuids);
    info
}

pub(crate) fn filter_server_update_for_session(
    mut update: ServerSpecializationInfoUpdate,
    session: Option<&crate::auth::AuthSession>,
) -> Option<ServerSpecializationInfoUpdate> {
    if !has_server_permission(
        session,
        update.server_uuid.as_deref(),
        &update.server_name,
        crate::auth::PERMISSION_VIEW,
    ) {
        return None;
    }
    if !has_server_permission(
        session,
        update.server_uuid.as_deref(),
        &update.server_name,
        crate::auth::PERMISSION_STATS,
    ) {
        update.stats = None;
    }
    Some(update)
}

pub(crate) fn can_view_console_for_server(
    session: Option<&crate::auth::AuthSession>,
    server_uuid: Option<&str>,
    server_name: &str,
) -> bool {
    has_server_permission(
        session,
        server_uuid,
        server_name,
        crate::auth::PERMISSION_VIEW,
    ) && has_server_permission(
        session,
        server_uuid,
        server_name,
        crate::auth::PERMISSION_CONSOLE,
    )
}

pub(crate) fn filter_config_info_for_session(
    mut info: ConfigInfo,
    session: Option<&crate::auth::AuthSession>,
) -> ConfigInfo {
    info.config = filter_config_for_session(info.config, session);
    info
}

pub(crate) async fn can_access_server_by_name(
    state: &AppState,
    session: Option<&crate::auth::AuthSession>,
    server_name: &str,
    permission: &str,
) -> bool {
    if has_permission(session, crate::auth::PERMISSION_ADMIN) {
        return true;
    }
    let server_uuid = {
        let config = state.config.lock().await;
        config
            .servers
            .iter()
            .find(|server| server.name == server_name)
            .and_then(|server| server.server_uuid.clone())
    };
    has_server_permission(session, server_uuid.as_deref(), server_name, permission)
}

pub(crate) async fn can_view_console_by_name(
    state: &AppState,
    session: Option<&crate::auth::AuthSession>,
    server_name: &str,
) -> bool {
    if has_permission(session, crate::auth::PERMISSION_ADMIN) {
        return true;
    }
    let server_uuid = {
        let config = state.config.lock().await;
        config
            .servers
            .iter()
            .find(|server| server.name == server_name)
            .and_then(|server| server.server_uuid.clone())
    };
    can_view_console_for_server(session, server_uuid.as_deref(), server_name)
}

fn filter_archived_server_stats(
    value: serde_json::Value,
    visible_uuids: &[String],
) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .into_iter()
                .filter(|item| {
                    item.get("server_uuid")
                        .and_then(|value| value.as_str())
                        .is_some_and(|uuid| visible_uuids.iter().any(|visible| visible == uuid))
                })
                .collect(),
        ),
        serde_json::Value::Object(mut map) => {
            map.retain(|key, item| {
                visible_uuids.iter().any(|visible| visible == key)
                    || item
                        .get("server_uuid")
                        .and_then(|value| value.as_str())
                        .is_some_and(|uuid| visible_uuids.iter().any(|visible| visible == uuid))
            });
            serde_json::Value::Object(map)
        }
        other => other,
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
    session: Option<&crate::auth::AuthSession>,
    request_text: &str,
    prefer_msgpack: bool,
) {
    let include_output = serde_json::from_str::<SInfoRequestMessage>(request_text)
        .ok()
        .and_then(|val| val.arguments.first().copied())
        .unwrap_or(true);
    let info = build_server_info_message(state, include_output).await;
    let info = filter_server_info_message_for_session(info, session);
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

    ThemeCollection::load_from_directory_async(&themes_folder)
        .await
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

    let theme_collection = ThemeCollection::load_from_directory_async(&themes_folder)
        .await
        .unwrap_or_default();
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
    session: Option<&crate::auth::AuthSession>,
    event_type: &str,
    request_text: &str,
    prefer_msgpack: bool,
) -> bool {
    match event_type {
        "requestInfo" => {
            if session.is_none() {
                send_auth_required(sender).await;
                return true;
            }
            send_server_info(sender, state, session, request_text, prefer_msgpack).await;
            true
        }
        "getConfig" | "requestConfig" => {
            if !has_any_server_permission(session, crate::auth::PERMISSION_CONFIG) {
                send_auth_required(sender).await;
                return true;
            }
            let config_info = build_config_info(state).await;
            let config_info = filter_config_info_for_session(config_info, session);
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

fn has_permission(session: Option<&crate::auth::AuthSession>, permission: &str) -> bool {
    session
        .map(|session| crate::auth::permissions_include(&session.permissions, permission))
        .unwrap_or(false)
}

async fn send_auth_required(sender: &WsSender) {
    let _ = sender
        .lock()
        .await
        .send(text_message(
            r#"{"type":"AuthRequired","authenticated":false}"#.to_string(),
        ))
        .await;
}
