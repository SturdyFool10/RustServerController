use crate::{
    app_state::AppState,
    configuration::{
        AccountRequestConfig, AuthGroupConfig, AuthUserConfig, Config, PermissionDecisionConfig,
        PermissionDecisionState,
    },
    credential_store::WebAuthnCredentialRecord,
};
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use ring::{digest, rand};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use webauthn_rs::prelude::{
    CreationChallengeResponse, Passkey, PasskeyAuthentication, PasskeyRegistration,
    PublicKeyCredential, RegisterPublicKeyCredential, RequestChallengeResponse, Url, Uuid,
    Webauthn, WebauthnBuilder,
};

pub const PERMISSION_VIEW: &str = "view";
pub const PERMISSION_CONTROL: &str = "control";
pub const PERMISSION_CONFIG: &str = "config";
pub const PERMISSION_STATS: &str = "stats";
pub const PERMISSION_CONSOLE: &str = "console";
pub const PERMISSION_ADMIN: &str = "admin";
const SERVER_PERMISSION_PREFIX: &str = "server:";

const CHALLENGE_TTL_SECONDS: i64 = 120;
const OAUTH_TOKEN_BYTES: usize = 32;

#[derive(Clone, Debug)]
pub struct AuthSession {
    pub username: String,
    pub permissions: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub password_required: bool,
}

#[derive(Clone, Debug)]
pub struct OAuthSession {
    pub client_id: String,
    pub access_token_hash: String,
    pub refresh_token_hash: String,
    pub access_expires_at: DateTime<Utc>,
    pub refresh_expires_at: DateTime<Utc>,
    pub scopes: Vec<String>,
    pub revoked: bool,
}

#[derive(Clone, Default)]
pub struct AuthState {
    sessions: Arc<Mutex<HashMap<String, AuthSession>>>,
    challenges: Arc<Mutex<HashMap<String, AuthChallenge>>>,
    oauth_sessions: Arc<Mutex<HashMap<String, OAuthSession>>>,
    webauthn_registrations: Arc<Mutex<HashMap<String, WebAuthnRegistrationChallenge>>>,
    webauthn_authentications: Arc<Mutex<HashMap<String, WebAuthnAuthenticationChallenge>>>,
}

#[derive(Clone, Debug)]
struct AuthChallenge {
    username: String,
    expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct WebAuthnRegistrationChallenge {
    username: String,
    label: String,
    state: PasskeyRegistration,
    expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct WebAuthnAuthenticationChallenge {
    username: String,
    state: PasskeyAuthentication,
    expires_at: DateTime<Utc>,
    mode: WebAuthnAuthenticationMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WebAuthnAuthenticationMode {
    Passwordless,
    SecondFactor,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    nonce: String,
    proof: Option<String>,
}

#[derive(Deserialize)]
pub struct SetupRequest {
    username: String,
    password_salt: String,
    password_hash: String,
}

#[derive(Deserialize)]
pub struct ChallengeRequest {
    username: String,
}

#[derive(Serialize, Deserialize)]
pub struct ChallengeResponse {
    username: String,
    nonce: String,
    password_salt: Option<String>,
    password_required: bool,
}

#[derive(Serialize, Deserialize)]
pub struct AuthStatusResponse {
    authenticated: bool,
    setup_required: bool,
    username: Option<String>,
    permissions: Vec<String>,
    password_required: bool,
    #[serde(default)]
    webauthn_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    webauthn: Option<WebAuthnAuthenticationStartResponse>,
}

#[derive(Serialize, Deserialize)]
pub struct WebAuthnSettingsResponse {
    enabled: bool,
    passwordless_enabled: bool,
    require_2fa_for_password_login: bool,
}

#[derive(Deserialize)]
pub struct WebAuthnRegistrationStartRequest {
    label: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct WebAuthnRegistrationStartResponse {
    registration_id: String,
    public_key: CreationChallengeResponse,
}

#[derive(Deserialize)]
pub struct WebAuthnRegistrationFinishRequest {
    registration_id: String,
    credential: RegisterPublicKeyCredential,
}

#[derive(Serialize)]
pub struct WebAuthnCredentialResponse {
    id: String,
    label: String,
    created_at: String,
    last_used_at: Option<String>,
}

#[derive(Serialize)]
pub struct WebAuthnCredentialsResponse {
    credentials: Vec<WebAuthnCredentialResponse>,
}

#[derive(Deserialize)]
pub struct DeleteWebAuthnCredentialRequest {
    id: String,
}

#[derive(Deserialize)]
pub struct WebAuthnAuthenticationStartRequest {
    username: String,
    #[serde(default)]
    second_factor: bool,
}

#[derive(Serialize, Deserialize)]
pub struct WebAuthnAuthenticationStartResponse {
    authentication_id: String,
    public_key: RequestChallengeResponse,
}

#[derive(Deserialize)]
pub struct WebAuthnAuthenticationFinishRequest {
    authentication_id: String,
    credential: PublicKeyCredential,
}

#[derive(Deserialize)]
pub struct AccountRequestRequest {
    username: String,
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    username: String,
    password_salt: String,
    password_hash: String,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    permission_overrides: Vec<PermissionDecisionConfig>,
    #[serde(default)]
    groups: Vec<String>,
}

#[derive(Deserialize)]
pub struct AccountDecisionRequest {
    username: String,
    #[serde(default)]
    permissions: Option<Vec<String>>,
    #[serde(default)]
    permission_overrides: Vec<PermissionDecisionConfig>,
    #[serde(default)]
    groups: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateUserPermissionsRequest {
    username: String,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    permission_overrides: Vec<PermissionDecisionConfig>,
    #[serde(default)]
    groups: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdatePermissionModelRequest {
    default_permissions: Vec<PermissionDecisionConfig>,
    groups: Vec<AuthGroupConfig>,
}

#[derive(Deserialize)]
pub struct SetPasswordRequest {
    password_salt: String,
    password_hash: String,
}

#[derive(Serialize)]
pub struct AccountUserResponse {
    username: String,
    permissions: Vec<String>,
    effective_permissions: Vec<String>,
    permission_overrides: Vec<PermissionDecisionConfig>,
    groups: Vec<String>,
    disabled: bool,
    password_required: bool,
}

#[derive(Serialize)]
pub struct AccountRequestResponse {
    username: String,
    requested_at: String,
}

#[derive(Serialize)]
pub struct AccountsResponse {
    users: Vec<AccountUserResponse>,
    requests: Vec<AccountRequestResponse>,
    groups: Vec<AuthGroupConfig>,
    default_permissions: Vec<PermissionDecisionConfig>,
    documented_default_permissions: Vec<PermissionDecisionConfig>,
}

#[derive(Deserialize)]
pub struct OAuthTokenRequest {
    grant_type: String,
    client_id: Option<String>,
    client_secret: Option<String>,
    refresh_token: Option<String>,
    scope: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct OAuthTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i64,
    refresh_token: String,
    refresh_expires_in: i64,
    scope: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_state::AppState,
        configuration::{AuthConfig, Config, OAuthClientConfig},
        credential_store::{migrate_config_credentials, CredentialStore},
        specializations,
    };
    use axum::body::to_bytes;
    use tokio::sync::broadcast;

    fn test_state() -> AppState {
        let mut config = Config {
            auth: AuthConfig {
                oauth_access_token_minutes: 15,
                oauth_refresh_token_days: 30,
                oauth_clients: vec![OAuthClientConfig {
                    client_id: "slave-a".to_string(),
                    client_secret_hash: sha256_hex("secret-a"),
                    scopes: vec!["slave:read".to_string(), "slave:write".to_string()],
                    disabled: false,
                }],
                ..AuthConfig::default()
            },
            ..Config::default()
        };
        crate::configuration::ensure_server_uuids(&mut config);
        let store = CredentialStore::open_for_test("oauth_refresh").unwrap();
        migrate_config_credentials(&mut config, &store);
        let (tx, _rx) = broadcast::channel(8);
        AppState::new(tx, config, specializations::init_builtin_registry(), store)
    }

    async fn response_json(response: Response) -> OAuthTokenResponse {
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    async fn response_json_as<T: serde::de::DeserializeOwned>(response: Response) -> T {
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn oauth_refresh_rotates_refresh_token() {
        let state = test_state();
        let issued = oauth_client_credentials(
            state.clone(),
            OAuthTokenRequest {
                grant_type: "client_credentials".to_string(),
                client_id: Some("slave-a".to_string()),
                client_secret: Some("secret-a".to_string()),
                refresh_token: None,
                scope: Some("slave:read".to_string()),
            },
        )
        .await;
        assert_eq!(issued.status(), StatusCode::OK);
        let issued = response_json(issued).await;

        let refreshed = oauth_refresh_token(
            state.clone(),
            OAuthTokenRequest {
                grant_type: "refresh_token".to_string(),
                client_id: None,
                client_secret: None,
                refresh_token: Some(issued.refresh_token.clone()),
                scope: None,
            },
        )
        .await;
        assert_eq!(refreshed.status(), StatusCode::OK);
        let refreshed = response_json(refreshed).await;
        assert_ne!(issued.refresh_token, refreshed.refresh_token);

        let reused = oauth_refresh_token(
            state,
            OAuthTokenRequest {
                grant_type: "refresh_token".to_string(),
                client_id: None,
                client_secret: None,
                refresh_token: Some(issued.refresh_token),
                scope: None,
            },
        )
        .await;
        assert_eq!(reused.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn approved_account_sets_password_after_passwordless_login() {
        let state = test_state();
        {
            let mut config = state.config.lock().await;
            config.auth.users.push(AuthUserConfig {
                username: "admin".to_string(),
                password_salt: String::new(),
                password_hash: String::new(),
                permissions: all_permissions(),
                permission_overrides: vec![],
                groups: vec![],
                disabled: false,
                password_required: false,
            });
        }
        state.auth.sessions.lock().await.insert(
            "admin-token".to_string(),
            AuthSession {
                username: "admin".to_string(),
                permissions: all_permissions(),
                expires_at: Utc::now() + Duration::hours(1),
                password_required: false,
            },
        );
        let mut admin_headers = HeaderMap::new();
        admin_headers.insert(header::COOKIE, "rsc_session=admin-token".parse().unwrap());

        let requested = request_account(
            State(state.clone()),
            Json(AccountRequestRequest {
                username: "pending_user".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(requested.status(), StatusCode::NO_CONTENT);

        let approved = approve_account_request(
            State(state.clone()),
            admin_headers,
            Json(AccountDecisionRequest {
                username: "pending_user".to_string(),
                permissions: Some(vec![PERMISSION_VIEW.to_string()]),
                permission_overrides: vec![],
                groups: vec![],
            }),
        )
        .await
        .into_response();
        assert_eq!(approved.status(), StatusCode::NO_CONTENT);

        let challenge_response = challenge(
            State(state.clone()),
            Json(ChallengeRequest {
                username: "pending_user".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(challenge_response.status(), StatusCode::OK);
        let challenge: ChallengeResponse = response_json_as(challenge_response).await;
        assert!(challenge.password_required);
        assert!(challenge.password_salt.is_none());

        let login_response = login(
            State(state.clone()),
            HeaderMap::new(),
            Json(LoginRequest {
                username: "pending_user".to_string(),
                nonce: challenge.nonce,
                proof: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(login_response.status(), StatusCode::OK);
        let set_cookie = login_response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .clone();
        let login_status: AuthStatusResponse = response_json_as(login_response).await;
        assert!(login_status.password_required);

        let mut user_headers = HeaderMap::new();
        user_headers.insert(header::COOKIE, set_cookie);
        let password_response = set_password(
            State(state.clone()),
            user_headers,
            Json(SetPasswordRequest {
                password_salt: "salt".to_string(),
                password_hash: "hash".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(password_response.status(), StatusCode::OK);
        let password_status: AuthStatusResponse = response_json_as(password_response).await;
        assert!(!password_status.password_required);

        let config = state.config.lock().await;
        let user = config
            .auth
            .users
            .iter()
            .find(|user| user.username == "pending_user")
            .unwrap();
        assert!(!user.password_required);
        drop(config);
        let credential = state
            .credentials
            .auth_user("pending_user")
            .unwrap()
            .unwrap();
        assert_eq!(credential.password_salt, "salt");
        assert_eq!(credential.password_hash, "hash");
    }

    #[test]
    fn permission_resolution_prioritizes_user_group_default() {
        let config = Config {
            auth: AuthConfig {
                default_permissions: documented_default_permissions(),
                groups: vec![AuthGroupConfig {
                    name: "operators".to_string(),
                    permissions: vec![
                        PermissionDecisionConfig {
                            permission: PERMISSION_CONTROL.to_string(),
                            state: PermissionDecisionState::Granted,
                        },
                        PermissionDecisionConfig {
                            permission: PERMISSION_STATS.to_string(),
                            state: PermissionDecisionState::Blocked,
                        },
                    ],
                }],
                ..AuthConfig::default()
            },
            ..Config::default()
        };
        let user = AuthUserConfig {
            username: "user".to_string(),
            password_salt: String::new(),
            password_hash: String::new(),
            permissions: vec![],
            permission_overrides: vec![PermissionDecisionConfig {
                permission: PERMISSION_CONTROL.to_string(),
                state: PermissionDecisionState::Blocked,
            }],
            groups: vec!["operators".to_string()],
            disabled: false,
            password_required: false,
        };

        let permissions = resolved_permissions(&config, &user);
        assert!(permissions.contains(&PERMISSION_VIEW.to_string()));
        assert!(permissions.contains(&PERMISSION_CONSOLE.to_string()));
        assert!(!permissions.contains(&PERMISSION_STATS.to_string()));
        assert!(!permissions.contains(&PERMISSION_CONTROL.to_string()));
    }

    #[test]
    fn actor_cannot_assign_or_edit_above_effective_permissions() {
        let actor = AuthSession {
            username: "limited-admin".to_string(),
            permissions: vec![PERMISSION_ADMIN.to_string(), PERMISSION_VIEW.to_string()],
            expires_at: Utc::now(),
            password_required: false,
        };
        assert!(can_assign_permission(&actor, PERMISSION_VIEW));
        assert!(can_assign_permission(&actor, PERMISSION_CONTROL));

        let actor = AuthSession {
            username: "limited-manager".to_string(),
            permissions: vec![PERMISSION_VIEW.to_string()],
            expires_at: Utc::now(),
            password_required: false,
        };
        assert!(can_assign_permission(&actor, PERMISSION_VIEW));
        assert!(!can_assign_permission(&actor, PERMISSION_CONTROL));
        assert!(!can_assign_permission_set(
            &actor,
            &[PERMISSION_VIEW.to_string(), PERMISSION_CONTROL.to_string()]
        ));
    }
}

#[derive(Serialize)]
struct AuthErrorResponse {
    error: String,
}

pub(crate) fn error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(AuthErrorResponse {
            error: message.to_string(),
        }),
    )
        .into_response()
}

fn cookie_name(config: &Config) -> String {
    let name = config.auth.cookie_name.trim();
    if name.is_empty() {
        "rsc_session".to_string()
    } else {
        name.to_string()
    }
}

fn session_ttl(config: &Config) -> Duration {
    let hours = config.auth.session_ttl_hours.max(1);
    Duration::hours(i64::try_from(hours).unwrap_or(12))
}

fn auth_users_exist(config: &Config) -> bool {
    config
        .auth
        .users
        .iter()
        .any(|user| !user.disabled && !user.username.trim().is_empty())
}

fn validate_username(username: &str) -> bool {
    let len = username.chars().count();
    (3..=32).contains(&len)
        && username
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '_' || char == '-' || char == '.')
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|cookie| {
        let (key, value) = cookie.trim().split_once('=')?;
        (key == name).then(|| value.to_string())
    })
}

async fn session_token_from_headers(headers: &HeaderMap, state: &AppState) -> Option<String> {
    let config = state.config.lock().await;
    let name = cookie_name(&config);
    drop(config);
    cookie_value(headers, &name)
}

fn set_cookie(name: &str, token: &str, expires_at: DateTime<Utc>) -> String {
    format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Expires={}",
        name,
        token,
        expires_at.format("%a, %d %b %Y %H:%M:%S GMT")
    )
}

fn clear_cookie(name: &str) -> String {
    format!(
        "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
        name
    )
}

fn challenge_proof(password_hash: &str, nonce: &str) -> String {
    let input = format!("{}:{}", password_hash, nonce);
    let hash = digest::digest(&digest::SHA256, input.as_bytes());
    hash.as_ref()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

fn sha256_hex(input: &str) -> String {
    let hash = digest::digest(&digest::SHA256, input.as_bytes());
    hash.as_ref()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

fn token_hash(token: &str) -> String {
    sha256_hex(token)
}

fn random_token() -> Result<String, ()> {
    let rng = rand::SystemRandom::new();
    let mut bytes = [0u8; OAUTH_TOKEN_BYTES];
    rand::SecureRandom::fill(&rng, &mut bytes).map_err(|_| ())?;
    Ok(bytes.iter().map(|byte| format!("{:02x}", byte)).collect())
}

fn oauth_access_ttl(config: &Config) -> Duration {
    Duration::minutes(i64::try_from(config.auth.oauth_access_token_minutes.max(1)).unwrap_or(15))
}

fn oauth_refresh_ttl(config: &Config) -> Duration {
    Duration::days(i64::try_from(config.auth.oauth_refresh_token_days.max(1)).unwrap_or(30))
}

fn parse_scope_list(scope: Option<&str>) -> Vec<String> {
    scope
        .unwrap_or("")
        .split_whitespace()
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn allowed_scopes(configured: &[String], requested: Option<&str>) -> Option<Vec<String>> {
    let requested = parse_scope_list(requested);
    if requested.is_empty() {
        return Some(configured.to_vec());
    }
    if requested.iter().all(|scope| configured.contains(scope)) {
        Some(requested)
    } else {
        None
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

pub fn all_permissions() -> Vec<String> {
    vec![
        PERMISSION_VIEW.to_string(),
        PERMISSION_CONTROL.to_string(),
        PERMISSION_CONFIG.to_string(),
        PERMISSION_STATS.to_string(),
        PERMISSION_CONSOLE.to_string(),
        PERMISSION_ADMIN.to_string(),
    ]
}

fn documented_default_permissions() -> Vec<PermissionDecisionConfig> {
    [
        (PERMISSION_VIEW, PermissionDecisionState::Granted),
        (PERMISSION_STATS, PermissionDecisionState::Granted),
        (PERMISSION_CONSOLE, PermissionDecisionState::Granted),
        (PERMISSION_CONTROL, PermissionDecisionState::Blocked),
        (PERMISSION_CONFIG, PermissionDecisionState::Blocked),
        (PERMISSION_ADMIN, PermissionDecisionState::Blocked),
    ]
    .into_iter()
    .map(|(permission, state)| PermissionDecisionConfig {
        permission: permission.to_string(),
        state,
    })
    .collect()
}

fn decision_for_permission<'a>(
    decisions: impl Iterator<Item = &'a PermissionDecisionConfig>,
    permission: &str,
) -> PermissionDecisionState {
    decisions
        .filter(|decision| decision.permission == permission)
        .map(|decision| decision.state.clone())
        .find(|state| *state != PermissionDecisionState::Default)
        .unwrap_or(PermissionDecisionState::Default)
}

fn group_decision_for_permission(
    config: &Config,
    user: &AuthUserConfig,
    permission: &str,
) -> PermissionDecisionState {
    let mut saw_granted = false;
    for group_name in &user.groups {
        let Some(group) = config
            .auth
            .groups
            .iter()
            .find(|group| group.name == *group_name)
        else {
            continue;
        };
        match decision_for_permission(group.permissions.iter(), permission) {
            PermissionDecisionState::Blocked => return PermissionDecisionState::Blocked,
            PermissionDecisionState::Granted => saw_granted = true,
            PermissionDecisionState::Default => {}
        }
    }
    if saw_granted {
        PermissionDecisionState::Granted
    } else {
        PermissionDecisionState::Default
    }
}

fn resolved_permissions(config: &Config, user: &AuthUserConfig) -> Vec<String> {
    let mut permissions = documented_default_permissions()
        .into_iter()
        .map(|decision| decision.permission)
        .collect::<Vec<_>>();
    permissions.extend(
        config
            .auth
            .default_permissions
            .iter()
            .map(|decision| decision.permission.clone()),
    );
    permissions.extend(user.permissions.iter().cloned());
    permissions.extend(
        user.permission_overrides
            .iter()
            .map(|decision| decision.permission.clone()),
    );
    for group_name in &user.groups {
        if let Some(group) = config
            .auth
            .groups
            .iter()
            .find(|group| group.name == *group_name)
        {
            permissions.extend(
                group
                    .permissions
                    .iter()
                    .map(|decision| decision.permission.clone()),
            );
        }
    }
    permissions.sort();
    permissions.dedup();

    permissions
        .into_iter()
        .filter(|permission| {
            let user_decision =
                decision_for_permission(user.permission_overrides.iter(), permission);
            let user_decision = if user_decision == PermissionDecisionState::Default
                && user.permissions.contains(permission)
            {
                PermissionDecisionState::Granted
            } else {
                user_decision
            };
            match user_decision {
                PermissionDecisionState::Granted => return true,
                PermissionDecisionState::Blocked => return false,
                PermissionDecisionState::Default => {}
            }

            match group_decision_for_permission(config, user, permission) {
                PermissionDecisionState::Granted => return true,
                PermissionDecisionState::Blocked => return false,
                PermissionDecisionState::Default => {}
            }

            decision_for_permission(config.auth.default_permissions.iter(), permission)
                == PermissionDecisionState::Granted
        })
        .collect()
}

pub fn permissions_include(permissions: &[String], required: &str) -> bool {
    permissions
        .iter()
        .any(|permission| permission == PERMISSION_ADMIN || permission == required)
}

pub fn server_permission(server_id: &str, permission: &str) -> String {
    format!(
        "{}{}:{}",
        SERVER_PERMISSION_PREFIX,
        server_id.trim(),
        permission.trim()
    )
}

pub fn permissions_include_server(
    permissions: &[String],
    required: &str,
    server_uuid: Option<&str>,
    server_name: &str,
) -> bool {
    if permissions_include(permissions, required) {
        return true;
    }
    let required_for_uuid = server_uuid.map(|uuid| server_permission(uuid, required));
    let required_for_name = server_permission(server_name, required);
    permissions.iter().any(|permission| {
        required_for_uuid
            .as_ref()
            .is_some_and(|required| permission == required)
            || permission == &required_for_name
    })
}

pub async fn session_from_headers(headers: &HeaderMap, state: &AppState) -> Option<AuthSession> {
    let config = state.config.lock().await;
    let name = cookie_name(&config);
    drop(config);

    let token = cookie_value(headers, &name)?;
    let mut sessions = state.auth.sessions.lock().await;
    let Some(session) = sessions.get(&token).cloned() else {
        return None;
    };
    if session.expires_at <= Utc::now() {
        sessions.remove(&token);
        return None;
    }
    Some(session)
}

pub async fn oauth_session_from_headers(
    headers: &HeaderMap,
    state: &AppState,
) -> Option<OAuthSession> {
    let token = bearer_token(headers)?;
    let access_hash = token_hash(&token);
    let now = Utc::now();
    let mut sessions = state.auth.oauth_sessions.lock().await;
    let expired_refreshes = sessions
        .iter()
        .filter_map(|(refresh_hash, session)| {
            (session.revoked || session.refresh_expires_at <= now).then(|| refresh_hash.clone())
        })
        .collect::<Vec<_>>();
    for refresh_hash in expired_refreshes {
        sessions.remove(&refresh_hash);
    }

    sessions
        .values()
        .find(|session| {
            !session.revoked
                && session.access_expires_at > now
                && constant_time_eq(&session.access_token_hash, &access_hash)
        })
        .cloned()
}

pub async fn auth_session_from_headers(
    headers: &HeaderMap,
    state: &AppState,
) -> Option<AuthSession> {
    if let Some(session) = session_from_headers(headers, state).await {
        return Some(session);
    }
    oauth_session_from_headers(headers, state)
        .await
        .map(|session| AuthSession {
            username: format!("oauth:{}", session.client_id),
            permissions: session.scopes,
            expires_at: session.access_expires_at,
            password_required: false,
        })
}

async fn require_admin(headers: &HeaderMap, state: &AppState) -> Result<AuthSession, Response> {
    let Some(session) = auth_session_from_headers(headers, state).await else {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "authentication required",
        ));
    };
    if !permissions_include(&session.permissions, PERMISSION_ADMIN) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "admin permission required",
        ));
    }
    Ok(session)
}

fn can_assign_permission(actor: &AuthSession, permission: &str) -> bool {
    if permissions_include(&actor.permissions, PERMISSION_ADMIN) {
        return true;
    }
    if actor.permissions.iter().any(|item| item == permission) {
        return true;
    }
    let Some((_, base_permission)) = permission.rsplit_once(':') else {
        return false;
    };
    permissions_include(&actor.permissions, base_permission)
}

fn can_assign_decisions(actor: &AuthSession, decisions: &[PermissionDecisionConfig]) -> bool {
    decisions.iter().all(|decision| {
        decision.state != PermissionDecisionState::Granted
            || can_assign_permission(actor, &decision.permission)
    })
}

fn can_assign_permission_set(actor: &AuthSession, permissions: &[String]) -> bool {
    permissions
        .iter()
        .all(|permission| can_assign_permission(actor, permission))
}

fn can_assign_groups(actor: &AuthSession, groups: &[String], config: &Config) -> bool {
    groups.iter().all(|group_name| {
        let Some(group) = config
            .auth
            .groups
            .iter()
            .find(|group| group.name == *group_name)
        else {
            return false;
        };
        can_assign_decisions(actor, &group.permissions)
    })
}

fn candidate_user_with_permissions(
    current: &AuthUserConfig,
    permissions: Vec<String>,
    permission_overrides: Vec<PermissionDecisionConfig>,
    groups: Vec<String>,
) -> AuthUserConfig {
    let mut candidate = current.clone();
    candidate.permissions = permissions;
    candidate.permission_overrides = permission_overrides;
    candidate.groups = groups;
    candidate
}

fn origin_from_headers(config: &Config, headers: &HeaderMap) -> Option<String> {
    if let Some(origin) = config.auth.webauthn.origin.as_deref().map(str::trim) {
        if !origin.is_empty() {
            return Some(origin.to_string());
        }
    }
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
    {
        return Some(origin.to_string());
    }
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())?
        .trim();
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|scheme| *scheme == "https" || *scheme == "http")
        .unwrap_or(if config.web_transport.enable_https {
            "https"
        } else {
            "http"
        });
    Some(format!("{}://{}", scheme, host))
}

fn build_webauthn(config: &Config, headers: &HeaderMap) -> Result<Webauthn, Response> {
    let Some(origin_text) = origin_from_headers(config, headers) else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "WebAuthn origin could not be determined",
        ));
    };
    let origin = Url::parse(&origin_text).map_err(|_| {
        error_response(
            StatusCode::BAD_REQUEST,
            "WebAuthn origin must be an absolute URL",
        )
    })?;
    let rp_id = config
        .auth
        .webauthn
        .relying_party_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .or_else(|| origin.domain())
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "WebAuthn requires a configured relying_party_id for this origin",
            )
        })?;
    WebauthnBuilder::new(rp_id, &origin)
        .map(|builder| {
            builder
                .rp_name(&config.auth.webauthn.relying_party_name)
                .allow_any_port(true)
        })
        .and_then(|builder| builder.build())
        .map_err(|error| {
            tracing::warn!("Invalid WebAuthn configuration: {}", error);
            error_response(StatusCode::BAD_REQUEST, "invalid WebAuthn configuration")
        })
}

fn stable_user_uuid(username: &str) -> Uuid {
    let hash = digest::digest(&digest::SHA256, username.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash.as_ref()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn passkeys_for_user(state: &AppState, username: &str) -> Vec<(WebAuthnCredentialRecord, Passkey)> {
    state
        .credentials
        .webauthn_credentials(username)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|record| {
            serde_json::from_value::<Passkey>(record.passkey.clone())
                .ok()
                .map(|passkey| (record, passkey))
        })
        .collect()
}

async fn start_webauthn_challenge_for_user(
    state: &AppState,
    headers: &HeaderMap,
    username: String,
    mode: WebAuthnAuthenticationMode,
) -> Result<WebAuthnAuthenticationStartResponse, Response> {
    let webauthn = {
        let config = state.config.lock().await;
        if !config.auth.webauthn.enabled {
            return Err(error_response(
                StatusCode::FORBIDDEN,
                "WebAuthn is disabled",
            ));
        }
        if mode == WebAuthnAuthenticationMode::Passwordless
            && !config.auth.webauthn.passwordless_enabled
        {
            return Err(error_response(
                StatusCode::FORBIDDEN,
                "passwordless WebAuthn is disabled",
            ));
        }
        if !config
            .auth
            .users
            .iter()
            .any(|user| !user.disabled && user.username == username)
        {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "invalid username or security key",
            ));
        }
        build_webauthn(&config, headers)?
    };
    let passkeys = passkeys_for_user(state, &username)
        .into_iter()
        .map(|(_, passkey)| passkey)
        .collect::<Vec<_>>();
    if passkeys.is_empty() {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid username or security key",
        ));
    }
    let (public_key, authentication) =
        webauthn
            .start_passkey_authentication(&passkeys)
            .map_err(|error| {
                tracing::warn!("Failed to start WebAuthn authentication: {}", error);
                error_response(
                    StatusCode::BAD_REQUEST,
                    "failed to start WebAuthn authentication",
                )
            })?;
    let authentication_id = uuid::Uuid::new_v4().to_string();
    state.auth.webauthn_authentications.lock().await.insert(
        authentication_id.clone(),
        WebAuthnAuthenticationChallenge {
            username,
            state: authentication,
            expires_at: Utc::now() + Duration::seconds(CHALLENGE_TTL_SECONDS),
            mode,
        },
    );
    Ok(WebAuthnAuthenticationStartResponse {
        authentication_id,
        public_key,
    })
}

async fn issue_browser_session(
    state: AppState,
    cookie: String,
    expires_at: DateTime<Utc>,
    user_meta: AuthUserConfig,
    permissions: Vec<String>,
) -> Response {
    let session = AuthSession {
        username: user_meta.username,
        permissions,
        expires_at,
        password_required: user_meta.password_required,
    };
    let token = uuid::Uuid::new_v4().to_string();
    state
        .auth
        .sessions
        .lock()
        .await
        .insert(token.clone(), session.clone());

    let mut headers = HeaderMap::new();
    if let Ok(value) = set_cookie(&cookie, &token, expires_at).parse() {
        headers.insert(header::SET_COOKIE, value);
    }
    (
        headers,
        Json(AuthStatusResponse {
            authenticated: true,
            setup_required: false,
            username: Some(session.username),
            permissions: session.permissions,
            password_required: session.password_required,
            webauthn_required: false,
            webauthn: None,
        }),
    )
        .into_response()
}

pub async fn auth_status(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let config = state.config.lock().await;
    let setup_required = !auth_users_exist(&config);
    drop(config);

    if let Some(session) = session_from_headers(&headers, &state).await {
        Json(AuthStatusResponse {
            authenticated: true,
            setup_required,
            username: Some(session.username),
            permissions: session.permissions,
            password_required: session.password_required,
            webauthn_required: false,
            webauthn: None,
        })
        .into_response()
    } else {
        Json(AuthStatusResponse {
            authenticated: false,
            setup_required,
            username: None,
            permissions: vec![],
            password_required: false,
            webauthn_required: false,
            webauthn: None,
        })
        .into_response()
    }
}

pub async fn webauthn_settings(State(state): State<AppState>) -> impl IntoResponse {
    let config = state.config.lock().await;
    Json(WebAuthnSettingsResponse {
        enabled: config.auth.webauthn.enabled,
        passwordless_enabled: config.auth.webauthn.passwordless_enabled,
        require_2fa_for_password_login: config.auth.webauthn.require_2fa_for_password_login,
    })
    .into_response()
}

pub async fn start_webauthn_registration(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WebAuthnRegistrationStartRequest>,
) -> impl IntoResponse {
    let Some(session) = session_from_headers(&headers, &state).await else {
        return error_response(StatusCode::UNAUTHORIZED, "authentication required");
    };
    if session.password_required {
        return error_response(StatusCode::FORBIDDEN, "password setup required first");
    }

    let (webauthn, existing) = {
        let config = state.config.lock().await;
        if !config.auth.webauthn.enabled {
            return error_response(StatusCode::FORBIDDEN, "WebAuthn is disabled");
        }
        let webauthn = match build_webauthn(&config, &headers) {
            Ok(webauthn) => webauthn,
            Err(response) => return response,
        };
        let existing = state
            .credentials
            .webauthn_credentials(&session.username)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|record| serde_json::from_value::<Passkey>(record.passkey).ok())
            .map(|passkey| passkey.cred_id().clone())
            .collect::<Vec<_>>();
        (webauthn, existing)
    };

    let label = request
        .label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .unwrap_or("Security key")
        .chars()
        .take(80)
        .collect::<String>();
    let user_id = stable_user_uuid(&session.username);
    let (public_key, registration) = match webauthn.start_passkey_registration(
        user_id,
        &session.username,
        &session.username,
        Some(existing),
    ) {
        Ok(challenge) => challenge,
        Err(error) => {
            tracing::warn!("Failed to start WebAuthn registration: {}", error);
            return error_response(
                StatusCode::BAD_REQUEST,
                "failed to start WebAuthn registration",
            );
        }
    };
    let registration_id = uuid::Uuid::new_v4().to_string();
    state.auth.webauthn_registrations.lock().await.insert(
        registration_id.clone(),
        WebAuthnRegistrationChallenge {
            username: session.username,
            label,
            state: registration,
            expires_at: Utc::now() + Duration::seconds(CHALLENGE_TTL_SECONDS),
        },
    );

    Json(WebAuthnRegistrationStartResponse {
        registration_id,
        public_key,
    })
    .into_response()
}

pub async fn finish_webauthn_registration(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WebAuthnRegistrationFinishRequest>,
) -> impl IntoResponse {
    let Some(session) = session_from_headers(&headers, &state).await else {
        return error_response(StatusCode::UNAUTHORIZED, "authentication required");
    };
    let challenge = state
        .auth
        .webauthn_registrations
        .lock()
        .await
        .remove(&request.registration_id);
    let Some(challenge) = challenge else {
        return error_response(StatusCode::UNAUTHORIZED, "WebAuthn challenge expired");
    };
    if challenge.username != session.username || challenge.expires_at <= Utc::now() {
        return error_response(StatusCode::UNAUTHORIZED, "WebAuthn challenge expired");
    }
    let webauthn = {
        let config = state.config.lock().await;
        if !config.auth.webauthn.enabled {
            return error_response(StatusCode::FORBIDDEN, "WebAuthn is disabled");
        }
        match build_webauthn(&config, &headers) {
            Ok(webauthn) => webauthn,
            Err(response) => return response,
        }
    };
    let passkey = match webauthn.finish_passkey_registration(&request.credential, &challenge.state)
    {
        Ok(passkey) => passkey,
        Err(error) => {
            tracing::warn!("Failed to finish WebAuthn registration: {}", error);
            return error_response(StatusCode::UNAUTHORIZED, "invalid WebAuthn registration");
        }
    };
    let id = bytes_to_hex(passkey.cred_id());
    let passkey = match serde_json::to_value(passkey) {
        Ok(passkey) => passkey,
        Err(error) => {
            tracing::error!("Failed to encode WebAuthn credential: {}", error);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to save WebAuthn credential",
            );
        }
    };
    let record = WebAuthnCredentialRecord {
        id,
        username: session.username,
        label: challenge.label,
        created_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        last_used_at: None,
        passkey,
    };
    if let Err(error) = state.credentials.upsert_webauthn_credential(&record) {
        tracing::error!("Failed to save WebAuthn credential: {}", error);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to save WebAuthn credential",
        );
    }
    StatusCode::NO_CONTENT.into_response()
}

pub async fn list_webauthn_credentials(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(session) = session_from_headers(&headers, &state).await else {
        return error_response(StatusCode::UNAUTHORIZED, "authentication required");
    };
    let credentials = match state.credentials.webauthn_credentials(&session.username) {
        Ok(credentials) => credentials,
        Err(error) => {
            tracing::error!("Failed to list WebAuthn credentials: {}", error);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load WebAuthn credentials",
            );
        }
    };
    Json(WebAuthnCredentialsResponse {
        credentials: credentials
            .into_iter()
            .map(|credential| WebAuthnCredentialResponse {
                id: credential.id,
                label: credential.label,
                created_at: credential.created_at,
                last_used_at: credential.last_used_at,
            })
            .collect(),
    })
    .into_response()
}

pub async fn delete_webauthn_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DeleteWebAuthnCredentialRequest>,
) -> impl IntoResponse {
    let Some(session) = session_from_headers(&headers, &state).await else {
        return error_response(StatusCode::UNAUTHORIZED, "authentication required");
    };
    match state
        .credentials
        .delete_webauthn_credential(&session.username, request.id.trim())
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error_response(StatusCode::NOT_FOUND, "WebAuthn credential not found"),
        Err(error) => {
            tracing::error!("Failed to delete WebAuthn credential: {}", error);
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to delete WebAuthn credential",
            )
        }
    }
}

pub async fn start_webauthn_authentication(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WebAuthnAuthenticationStartRequest>,
) -> impl IntoResponse {
    let username = request.username.trim().to_string();
    if username.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "username is required");
    }
    if request.second_factor {
        return error_response(
            StatusCode::FORBIDDEN,
            "second-factor WebAuthn challenges must start after password verification",
        );
    }
    match start_webauthn_challenge_for_user(
        &state,
        &headers,
        username,
        WebAuthnAuthenticationMode::Passwordless,
    )
    .await
    {
        Ok(challenge) => Json(challenge).into_response(),
        Err(response) => response,
    }
}

pub async fn finish_webauthn_authentication(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<WebAuthnAuthenticationFinishRequest>,
) -> impl IntoResponse {
    let challenge = state
        .auth
        .webauthn_authentications
        .lock()
        .await
        .remove(&request.authentication_id);
    let Some(challenge) = challenge else {
        return error_response(StatusCode::UNAUTHORIZED, "WebAuthn challenge expired");
    };
    if challenge.expires_at <= Utc::now() {
        return error_response(StatusCode::UNAUTHORIZED, "WebAuthn challenge expired");
    }
    let (webauthn, cookie, expires_at, user_meta, effective_permissions) = {
        let config = state.config.lock().await;
        if !config.auth.webauthn.enabled {
            return error_response(StatusCode::FORBIDDEN, "WebAuthn is disabled");
        }
        if challenge.mode == WebAuthnAuthenticationMode::Passwordless
            && !config.auth.webauthn.passwordless_enabled
        {
            return error_response(StatusCode::FORBIDDEN, "passwordless WebAuthn is disabled");
        }
        let Some(user_meta) = config
            .auth
            .users
            .iter()
            .find(|user| !user.disabled && user.username == challenge.username)
            .cloned()
        else {
            return error_response(StatusCode::UNAUTHORIZED, "invalid username or security key");
        };
        let webauthn = match build_webauthn(&config, &headers) {
            Ok(webauthn) => webauthn,
            Err(response) => return response,
        };
        let cookie = cookie_name(&config);
        let expires_at = Utc::now() + session_ttl(&config);
        let effective_permissions = resolved_permissions(&config, &user_meta);
        (
            webauthn,
            cookie,
            expires_at,
            user_meta,
            effective_permissions,
        )
    };
    let result = match webauthn.finish_passkey_authentication(&request.credential, &challenge.state)
    {
        Ok(result) => result,
        Err(error) => {
            tracing::warn!("Failed to finish WebAuthn authentication: {}", error);
            return error_response(StatusCode::UNAUTHORIZED, "invalid username or security key");
        }
    };
    let mut matched = false;
    for (mut record, mut passkey) in passkeys_for_user(&state, &challenge.username) {
        if passkey.update_credential(&result).is_some() {
            matched = true;
            record.last_used_at =
                Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
            match serde_json::to_value(passkey) {
                Ok(value) => {
                    record.passkey = value;
                    if let Err(error) = state.credentials.upsert_webauthn_credential(&record) {
                        tracing::error!("Failed to update WebAuthn credential: {}", error);
                    }
                }
                Err(error) => tracing::error!("Failed to encode WebAuthn credential: {}", error),
            }
            break;
        }
    }
    if !matched {
        return error_response(StatusCode::UNAUTHORIZED, "invalid username or security key");
    }
    issue_browser_session(state, cookie, expires_at, user_meta, effective_permissions).await
}

pub async fn challenge(
    State(state): State<AppState>,
    Json(request): Json<ChallengeRequest>,
) -> impl IntoResponse {
    let username = request.username.trim().to_string();
    if username.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "username is required");
    }
    let user_meta = {
        let config = state.config.lock().await;
        config
            .auth
            .users
            .iter()
            .find(|user| !user.disabled && user.username == username)
            .cloned()
    };
    let password_required = user_meta
        .as_ref()
        .is_some_and(|user| user.password_required);
    let password_salt = if password_required {
        None
    } else {
        state
            .credentials
            .auth_user(&username)
            .ok()
            .flatten()
            .filter(|user| !user.disabled)
            .map(|user| user.password_salt)
    };

    let nonce = uuid::Uuid::new_v4().to_string();
    state.auth.challenges.lock().await.insert(
        nonce.clone(),
        AuthChallenge {
            username: username.clone(),
            expires_at: Utc::now() + Duration::seconds(CHALLENGE_TTL_SECONDS),
        },
    );

    Json(ChallengeResponse {
        username,
        nonce,
        password_salt,
        password_required,
    })
    .into_response()
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> impl IntoResponse {
    let config = state.config.lock().await;
    if !auth_users_exist(&config) {
        return error_response(StatusCode::PRECONDITION_REQUIRED, "setup required");
    }
    let cookie = cookie_name(&config);
    let expires_at = Utc::now() + session_ttl(&config);
    let Some(user_meta) = config
        .auth
        .users
        .iter()
        .find(|user| !user.disabled && user.username == request.username)
        .cloned()
    else {
        return error_response(StatusCode::UNAUTHORIZED, "invalid username or password");
    };
    let effective_permissions = resolved_permissions(&config, &user_meta);
    let require_webauthn_2fa =
        config.auth.webauthn.enabled && config.auth.webauthn.require_2fa_for_password_login;
    drop(config);

    let challenge = state.auth.challenges.lock().await.remove(&request.nonce);
    if !challenge.is_some_and(|challenge| {
        challenge.username == request.username && challenge.expires_at > Utc::now()
    }) {
        return error_response(StatusCode::UNAUTHORIZED, "login challenge expired");
    }

    if !user_meta.password_required {
        let Some(user) = state
            .credentials
            .auth_user(&request.username)
            .ok()
            .flatten()
        else {
            return error_response(StatusCode::UNAUTHORIZED, "invalid username or password");
        };
        if user.disabled {
            return error_response(StatusCode::UNAUTHORIZED, "invalid username or password");
        }
        let Some(proof) = request.proof.as_deref() else {
            return error_response(StatusCode::UNAUTHORIZED, "invalid username or password");
        };
        let expected_proof = challenge_proof(&user.password_hash, &request.nonce);
        if !constant_time_eq(&expected_proof, proof) {
            return error_response(StatusCode::UNAUTHORIZED, "invalid username or password");
        }
    }
    if require_webauthn_2fa
        && !user_meta.password_required
        && !passkeys_for_user(&state, &request.username).is_empty()
    {
        let webauthn = match start_webauthn_challenge_for_user(
            &state,
            &headers,
            request.username,
            WebAuthnAuthenticationMode::SecondFactor,
        )
        .await
        {
            Ok(challenge) => challenge,
            Err(response) => return response,
        };
        return Json(AuthStatusResponse {
            authenticated: false,
            setup_required: false,
            username: Some(user_meta.username),
            permissions: vec![],
            password_required: false,
            webauthn_required: true,
            webauthn: Some(webauthn),
        })
        .into_response();
    }
    issue_browser_session(state, cookie, expires_at, user_meta, effective_permissions).await
}

pub async fn setup(
    State(state): State<AppState>,
    Json(request): Json<SetupRequest>,
) -> impl IntoResponse {
    let username = request.username.trim();
    if username.is_empty()
        || request.password_salt.trim().is_empty()
        || request.password_hash.trim().is_empty()
    {
        return error_response(
            StatusCode::BAD_REQUEST,
            "username, password_salt, and password_hash are required",
        );
    }

    let (config_snapshot, cookie, expires_at) = {
        let mut config = state.config.lock().await;
        if auth_users_exist(&config) {
            return error_response(StatusCode::CONFLICT, "setup is already complete");
        }
        config.auth.users.push(AuthUserConfig {
            username: username.to_string(),
            password_salt: String::new(),
            password_hash: String::new(),
            permissions: all_permissions(),
            permission_overrides: vec![],
            groups: vec![],
            disabled: false,
            password_required: false,
        });
        let cookie = cookie_name(&config);
        let expires_at = Utc::now() + session_ttl(&config);
        (config.clone(), cookie, expires_at)
    };

    if let Err(error) = state.credentials.upsert_auth_user(&AuthUserConfig {
        username: username.to_string(),
        password_salt: request.password_salt,
        password_hash: request.password_hash,
        permissions: all_permissions(),
        permission_overrides: vec![],
        groups: vec![],
        disabled: false,
        password_required: false,
    }) {
        tracing::error!("Failed to persist auth credentials: {}", error);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to save credentials",
        );
    }

    if let Err(error) = config_snapshot
        .update_config_file_async("config.json")
        .await
    {
        tracing::error!("Failed to persist auth setup config: {}", error);
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to save config");
    }
    let session = AuthSession {
        username: username.to_string(),
        permissions: all_permissions(),
        expires_at,
        password_required: false,
    };

    let token = uuid::Uuid::new_v4().to_string();
    state
        .auth
        .sessions
        .lock()
        .await
        .insert(token.clone(), session.clone());

    let mut headers = HeaderMap::new();
    if let Ok(value) = set_cookie(&cookie, &token, expires_at).parse() {
        headers.insert(header::SET_COOKIE, value);
    }
    (
        headers,
        Json(AuthStatusResponse {
            authenticated: true,
            setup_required: false,
            username: Some(session.username),
            permissions: session.permissions,
            password_required: false,
            webauthn_required: false,
            webauthn: None,
        }),
    )
        .into_response()
}

pub async fn request_account(
    State(state): State<AppState>,
    Json(request): Json<AccountRequestRequest>,
) -> impl IntoResponse {
    let username = request.username.trim().to_string();
    if !validate_username(&username) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "username must be 3-32 characters using letters, numbers, _, -, or .",
        );
    }

    let config_snapshot = {
        let mut config = state.config.lock().await;
        let user_exists = config
            .auth
            .users
            .iter()
            .any(|user| user.username.eq_ignore_ascii_case(&username));
        let request_exists = config
            .auth
            .account_requests
            .iter()
            .any(|request| request.username.eq_ignore_ascii_case(&username));
        if !user_exists && !request_exists {
            config.auth.account_requests.push(AccountRequestConfig {
                username: username.clone(),
                requested_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            });
        }
        config.clone()
    };

    if let Err(error) = config_snapshot
        .update_config_file_async("config.json")
        .await
    {
        tracing::error!("Failed to persist account request: {}", error);
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to save config");
    }

    StatusCode::NO_CONTENT.into_response()
}

pub async fn list_accounts(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(response) = require_admin(&headers, &state).await {
        return response;
    }

    let config = state.config.lock().await;
    Json(AccountsResponse {
        users: config
            .auth
            .users
            .iter()
            .map(|user| AccountUserResponse {
                username: user.username.clone(),
                permissions: user.permissions.clone(),
                effective_permissions: resolved_permissions(&config, user),
                permission_overrides: user.permission_overrides.clone(),
                groups: user.groups.clone(),
                disabled: user.disabled,
                password_required: user.password_required,
            })
            .collect(),
        requests: config
            .auth
            .account_requests
            .iter()
            .map(|request| AccountRequestResponse {
                username: request.username.clone(),
                requested_at: request.requested_at.clone(),
            })
            .collect(),
        groups: config.auth.groups.clone(),
        default_permissions: config.auth.default_permissions.clone(),
        documented_default_permissions: documented_default_permissions(),
    })
    .into_response()
}

pub async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateUserRequest>,
) -> impl IntoResponse {
    let actor = match require_admin(&headers, &state).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let username = request.username.trim().to_string();
    if !validate_username(&username)
        || request.password_salt.trim().is_empty()
        || request.password_hash.trim().is_empty()
    {
        return error_response(
            StatusCode::BAD_REQUEST,
            "username, password_salt, and password_hash are required",
        );
    }
    let permissions = request.permissions;
    let permission_overrides = if request.permission_overrides.is_empty() {
        permissions
            .iter()
            .map(|permission| PermissionDecisionConfig {
                permission: permission.clone(),
                state: PermissionDecisionState::Granted,
            })
            .collect::<Vec<_>>()
    } else {
        request.permission_overrides
    };
    let groups = request.groups;
    {
        let config = state.config.lock().await;
        if !can_assign_decisions(&actor, &permission_overrides)
            || !can_assign_groups(&actor, &groups, &config)
        {
            return error_response(
                StatusCode::FORBIDDEN,
                "cannot grant permissions above your effective permissions",
            );
        }
        let Some(current_user) = config
            .auth
            .users
            .iter()
            .find(|user| user.username == username)
        else {
            return error_response(StatusCode::NOT_FOUND, "user not found");
        };
        let current_effective = resolved_permissions(&config, current_user);
        if !can_assign_permission_set(&actor, &current_effective) {
            return error_response(
                StatusCode::FORBIDDEN,
                "cannot edit a user with effective permissions above your own",
            );
        }
        let candidate = candidate_user_with_permissions(
            current_user,
            permissions.clone(),
            permission_overrides.clone(),
            groups.clone(),
        );
        let candidate_effective = resolved_permissions(&config, &candidate);
        if !can_assign_permission_set(&actor, &candidate_effective) {
            return error_response(
                StatusCode::FORBIDDEN,
                "cannot assign effective permissions above your own",
            );
        }
    }
    let user = AuthUserConfig {
        username: username.clone(),
        password_salt: request.password_salt,
        password_hash: request.password_hash,
        permissions: permissions.clone(),
        permission_overrides: permission_overrides.clone(),
        groups: groups.clone(),
        disabled: false,
        password_required: false,
    };
    {
        let config = state.config.lock().await;
        let effective_permissions = resolved_permissions(&config, &user);
        if !can_assign_permission_set(&actor, &effective_permissions) {
            return error_response(
                StatusCode::FORBIDDEN,
                "cannot create a user with effective permissions above your own",
            );
        }
    }

    let config_snapshot = {
        let mut config = state.config.lock().await;
        if config
            .auth
            .users
            .iter()
            .any(|user| user.username.eq_ignore_ascii_case(&username))
        {
            return error_response(StatusCode::CONFLICT, "user already exists");
        }
        config.auth.users.push(AuthUserConfig {
            username,
            password_salt: String::new(),
            password_hash: String::new(),
            permissions,
            permission_overrides,
            groups,
            disabled: false,
            password_required: false,
        });
        config.clone()
    };

    if let Err(error) = state.credentials.upsert_auth_user(&user) {
        tracing::error!("Failed to persist auth credentials: {}", error);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to save credentials",
        );
    }

    if let Err(error) = config_snapshot
        .update_config_file_async("config.json")
        .await
    {
        tracing::error!("Failed to persist auth user config: {}", error);
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to save config");
    }

    StatusCode::NO_CONTENT.into_response()
}

pub async fn approve_account_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AccountDecisionRequest>,
) -> impl IntoResponse {
    let actor = match require_admin(&headers, &state).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let username = request.username.trim().to_string();
    let permissions = request.permissions.unwrap_or_default();
    let permission_overrides = if request.permission_overrides.is_empty() {
        permissions
            .iter()
            .map(|permission| PermissionDecisionConfig {
                permission: permission.clone(),
                state: PermissionDecisionState::Granted,
            })
            .collect::<Vec<_>>()
    } else {
        request.permission_overrides
    };
    let groups = request.groups;
    {
        let config = state.config.lock().await;
        if !can_assign_decisions(&actor, &permission_overrides)
            || !can_assign_groups(&actor, &groups, &config)
        {
            return error_response(
                StatusCode::FORBIDDEN,
                "cannot grant permissions above your effective permissions",
            );
        }
    }
    {
        let config = state.config.lock().await;
        let candidate = AuthUserConfig {
            username: username.clone(),
            password_salt: String::new(),
            password_hash: String::new(),
            permissions: permissions.clone(),
            permission_overrides: permission_overrides.clone(),
            groups: groups.clone(),
            disabled: false,
            password_required: true,
        };
        let effective_permissions = resolved_permissions(&config, &candidate);
        if !can_assign_permission_set(&actor, &effective_permissions) {
            return error_response(
                StatusCode::FORBIDDEN,
                "cannot approve a user with effective permissions above your own",
            );
        }
    }

    let config_snapshot = {
        let mut config = state.config.lock().await;
        let Some(index) = config
            .auth
            .account_requests
            .iter()
            .position(|request| request.username.eq_ignore_ascii_case(&username))
        else {
            return error_response(StatusCode::NOT_FOUND, "account request not found");
        };
        if config
            .auth
            .users
            .iter()
            .any(|user| user.username.eq_ignore_ascii_case(&username))
        {
            return error_response(StatusCode::CONFLICT, "user already exists");
        }
        config.auth.account_requests.remove(index);
        config.auth.users.push(AuthUserConfig {
            username,
            password_salt: String::new(),
            password_hash: String::new(),
            permissions,
            permission_overrides,
            groups,
            disabled: false,
            password_required: true,
        });
        config.clone()
    };

    if let Err(error) = config_snapshot
        .update_config_file_async("config.json")
        .await
    {
        tracing::error!("Failed to approve account request: {}", error);
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to save config");
    }

    StatusCode::NO_CONTENT.into_response()
}

pub async fn reject_account_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AccountDecisionRequest>,
) -> impl IntoResponse {
    if let Err(response) = require_admin(&headers, &state).await {
        return response;
    }
    let username = request.username.trim().to_string();
    let config_snapshot = {
        let mut config = state.config.lock().await;
        let before = config.auth.account_requests.len();
        config
            .auth
            .account_requests
            .retain(|request| !request.username.eq_ignore_ascii_case(&username));
        if before == config.auth.account_requests.len() {
            return error_response(StatusCode::NOT_FOUND, "account request not found");
        }
        config.clone()
    };

    if let Err(error) = config_snapshot
        .update_config_file_async("config.json")
        .await
    {
        tracing::error!("Failed to reject account request: {}", error);
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to save config");
    }

    StatusCode::NO_CONTENT.into_response()
}

pub async fn update_user_permissions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UpdateUserPermissionsRequest>,
) -> impl IntoResponse {
    let actor = match require_admin(&headers, &state).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let username = request.username.trim().to_string();
    let permissions = request.permissions;
    let permission_overrides = request.permission_overrides;
    let groups = request.groups;
    {
        let config = state.config.lock().await;
        if !can_assign_decisions(&actor, &permission_overrides)
            || !can_assign_groups(&actor, &groups, &config)
        {
            return error_response(
                StatusCode::FORBIDDEN,
                "cannot grant permissions above your effective permissions",
            );
        }
    }
    let config_user = {
        let mut config = state.config.lock().await;
        let Some(user) = config
            .auth
            .users
            .iter_mut()
            .find(|user| user.username == username)
        else {
            return error_response(StatusCode::NOT_FOUND, "user not found");
        };
        user.permissions = permissions.clone();
        user.permission_overrides = permission_overrides.clone();
        user.groups = groups.clone();
        let user = user.clone();
        let config_snapshot = config.clone();
        drop(config);
        if let Err(error) = config_snapshot
            .update_config_file_async("config.json")
            .await
        {
            tracing::error!("Failed to persist user permissions: {}", error);
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to save config");
        }
        user
    };

    if let Ok(Some(mut credential_user)) = state.credentials.auth_user(&username) {
        credential_user.permissions = config_user.permissions;
        credential_user.permission_overrides = config_user.permission_overrides;
        credential_user.groups = config_user.groups;
        credential_user.disabled = config_user.disabled;
        credential_user.password_required = config_user.password_required;
        if let Err(error) = state.credentials.upsert_auth_user(&credential_user) {
            tracing::error!("Failed to persist credential permissions: {}", error);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to save credentials",
            );
        }
    }

    let effective_permissions = {
        let config = state.config.lock().await;
        config
            .auth
            .users
            .iter()
            .find(|user| user.username == username)
            .map(|user| resolved_permissions(&config, user))
            .unwrap_or_default()
    };
    let mut sessions = state.auth.sessions.lock().await;
    for session in sessions.values_mut() {
        if session.username == username {
            session.permissions = effective_permissions.clone();
        }
    }

    StatusCode::NO_CONTENT.into_response()
}

pub async fn update_permission_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UpdatePermissionModelRequest>,
) -> impl IntoResponse {
    let actor = match require_admin(&headers, &state).await {
        Ok(session) => session,
        Err(response) => return response,
    };

    if !can_assign_decisions(&actor, &request.default_permissions)
        || !request
            .groups
            .iter()
            .all(|group| can_assign_decisions(&actor, &group.permissions))
    {
        return error_response(
            StatusCode::FORBIDDEN,
            "cannot grant permissions above your effective permissions",
        );
    }

    {
        let config = state.config.lock().await;
        for user in &config.auth.users {
            let effective_permissions = resolved_permissions(&config, user);
            if !can_assign_permission_set(&actor, &effective_permissions) {
                return error_response(
                    StatusCode::FORBIDDEN,
                    "cannot modify the permission model while it affects users above your permissions",
                );
            }
        }
        let mut candidate = config.clone();
        candidate.auth.default_permissions = request.default_permissions.clone();
        candidate.auth.groups = request.groups.clone();
        for user in &candidate.auth.users {
            let effective_permissions = resolved_permissions(&candidate, user);
            if !can_assign_permission_set(&actor, &effective_permissions) {
                return error_response(
                    StatusCode::FORBIDDEN,
                    "permission model would create users above your permissions",
                );
            }
        }
    }

    let config_snapshot = {
        let mut config = state.config.lock().await;
        config.auth.default_permissions = request.default_permissions;
        config.auth.groups = request.groups;
        config.clone()
    };

    if let Err(error) = config_snapshot
        .update_config_file_async("config.json")
        .await
    {
        tracing::error!("Failed to persist permission model: {}", error);
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to save config");
    }

    let users = {
        let config = state.config.lock().await;
        config
            .auth
            .users
            .iter()
            .map(|user| (user.username.clone(), resolved_permissions(&config, user)))
            .collect::<HashMap<_, _>>()
    };
    let mut sessions = state.auth.sessions.lock().await;
    for session in sessions.values_mut() {
        if let Some(permissions) = users.get(&session.username) {
            session.permissions = permissions.clone();
        }
    }

    StatusCode::NO_CONTENT.into_response()
}

pub async fn set_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SetPasswordRequest>,
) -> impl IntoResponse {
    if request.password_salt.trim().is_empty() || request.password_hash.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "password_salt and password_hash are required",
        );
    }
    let Some(token) = session_token_from_headers(&headers, &state).await else {
        return error_response(StatusCode::UNAUTHORIZED, "authentication required");
    };
    let session = {
        let sessions = state.auth.sessions.lock().await;
        let Some(session) = sessions.get(&token).cloned() else {
            return error_response(StatusCode::UNAUTHORIZED, "authentication required");
        };
        if session.expires_at <= Utc::now() {
            return error_response(StatusCode::UNAUTHORIZED, "authentication expired");
        }
        session
    };
    if !session.password_required {
        return error_response(StatusCode::FORBIDDEN, "password is already set");
    }

    let (config_snapshot, user) = {
        let mut config = state.config.lock().await;
        let Some(user) = config
            .auth
            .users
            .iter_mut()
            .find(|user| user.username == session.username && !user.disabled)
        else {
            return error_response(StatusCode::UNAUTHORIZED, "authentication required");
        };
        user.password_required = false;
        let credential_user = AuthUserConfig {
            username: user.username.clone(),
            password_salt: request.password_salt,
            password_hash: request.password_hash,
            permissions: user.permissions.clone(),
            permission_overrides: user.permission_overrides.clone(),
            groups: user.groups.clone(),
            disabled: user.disabled,
            password_required: false,
        };
        (config.clone(), credential_user)
    };

    if let Err(error) = state.credentials.upsert_auth_user(&user) {
        tracing::error!("Failed to persist auth credentials: {}", error);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to save credentials",
        );
    }
    if let Err(error) = config_snapshot
        .update_config_file_async("config.json")
        .await
    {
        tracing::error!("Failed to persist auth user config: {}", error);
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to save config");
    }

    {
        let mut sessions = state.auth.sessions.lock().await;
        if let Some(session) = sessions.get_mut(&token) {
            session.password_required = false;
        }
    }

    Json(AuthStatusResponse {
        authenticated: true,
        setup_required: false,
        username: Some(session.username),
        permissions: session.permissions,
        password_required: false,
        webauthn_required: false,
        webauthn: None,
    })
    .into_response()
}

pub async fn reset_credential_stores(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = require_admin(&headers, &state).await {
        return response;
    }

    if let Err(error) = state.credentials.reset_all() {
        tracing::error!("Failed to reset credential store: {}", error);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to reset credential store",
        );
    }

    let config_snapshot = {
        let mut config = state.config.lock().await;
        config.auth.users.clear();
        config.auth.account_requests.clear();
        config.auth.oauth_clients.clear();
        config.clone()
    };
    if let Err(error) = config_snapshot
        .update_config_file_async("config.json")
        .await
    {
        tracing::error!("Failed to persist credential reset config: {}", error);
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to save config");
    }

    {
        let mut sessions = state.auth.sessions.lock().await;
        sessions.clear();
    }
    {
        let mut challenges = state.auth.challenges.lock().await;
        challenges.clear();
    }
    {
        let mut oauth_sessions = state.auth.oauth_sessions.lock().await;
        oauth_sessions.clear();
    }

    let config = state.config.lock().await;
    let cookie = cookie_name(&config);
    drop(config);
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = clear_cookie(&cookie).parse() {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

pub async fn oauth_token(
    State(state): State<AppState>,
    Json(request): Json<OAuthTokenRequest>,
) -> impl IntoResponse {
    match request.grant_type.as_str() {
        "client_credentials" => oauth_client_credentials(state, request).await,
        "refresh_token" => oauth_refresh_token(state, request).await,
        _ => error_response(StatusCode::BAD_REQUEST, "unsupported grant_type"),
    }
}

async fn oauth_client_credentials(state: AppState, request: OAuthTokenRequest) -> Response {
    let Some(client_id) = request.client_id.as_deref().map(str::trim) else {
        return error_response(StatusCode::BAD_REQUEST, "client_id is required");
    };
    let Some(client_secret) = request.client_secret.as_deref() else {
        return error_response(StatusCode::BAD_REQUEST, "client_secret is required");
    };
    let config = state.config.lock().await;
    let Some(client_meta) = config
        .auth
        .oauth_clients
        .iter()
        .find(|client| !client.disabled && client.client_id == client_id)
    else {
        return error_response(StatusCode::UNAUTHORIZED, "invalid client credentials");
    };
    let Some(client) = state.credentials.oauth_client(client_id).ok().flatten() else {
        return error_response(StatusCode::UNAUTHORIZED, "invalid client credentials");
    };
    if client.disabled || client_meta.disabled {
        return error_response(StatusCode::UNAUTHORIZED, "invalid client credentials");
    }
    let expected_hash = client.client_secret_hash.to_ascii_lowercase();
    if !constant_time_eq(&expected_hash, &sha256_hex(client_secret)) {
        return error_response(StatusCode::UNAUTHORIZED, "invalid client credentials");
    }
    let Some(scopes) = allowed_scopes(&client_meta.scopes, request.scope.as_deref()) else {
        return error_response(StatusCode::FORBIDDEN, "requested scope is not allowed");
    };
    let access_ttl = oauth_access_ttl(&config);
    let refresh_ttl = oauth_refresh_ttl(&config);
    drop(config);

    issue_oauth_session(
        state,
        client_id.to_string(),
        scopes,
        access_ttl,
        refresh_ttl,
    )
    .await
}

async fn oauth_refresh_token(state: AppState, request: OAuthTokenRequest) -> Response {
    let Some(refresh_token) = request.refresh_token.as_deref().map(str::trim) else {
        return error_response(StatusCode::BAD_REQUEST, "refresh_token is required");
    };
    let refresh_hash = token_hash(refresh_token);
    let now = Utc::now();
    let old_session = {
        let mut sessions = state.auth.oauth_sessions.lock().await;
        let Some(session) = sessions.remove(&refresh_hash) else {
            return error_response(StatusCode::UNAUTHORIZED, "invalid refresh token");
        };
        if session.revoked || session.refresh_expires_at <= now {
            return error_response(StatusCode::UNAUTHORIZED, "refresh token expired");
        }
        session
    };

    let config = state.config.lock().await;
    let Some(client_meta) = config
        .auth
        .oauth_clients
        .iter()
        .find(|client| !client.disabled && client.client_id == old_session.client_id)
    else {
        return error_response(StatusCode::UNAUTHORIZED, "client is disabled");
    };
    let Some(client) = state
        .credentials
        .oauth_client(&old_session.client_id)
        .ok()
        .flatten()
    else {
        return error_response(StatusCode::UNAUTHORIZED, "client is disabled");
    };
    if client.disabled || client_meta.disabled {
        return error_response(StatusCode::UNAUTHORIZED, "client is disabled");
    }
    let Some(scopes) = allowed_scopes(&client_meta.scopes, request.scope.as_deref()) else {
        return error_response(StatusCode::FORBIDDEN, "requested scope is not allowed");
    };
    let scopes = if scopes.is_empty() {
        old_session.scopes
    } else {
        scopes
    };
    let access_ttl = oauth_access_ttl(&config);
    let refresh_ttl = oauth_refresh_ttl(&config);
    drop(config);

    issue_oauth_session(
        state,
        old_session.client_id,
        scopes,
        access_ttl,
        refresh_ttl,
    )
    .await
}

async fn issue_oauth_session(
    state: AppState,
    client_id: String,
    scopes: Vec<String>,
    access_ttl: Duration,
    refresh_ttl: Duration,
) -> Response {
    let access_token = match random_token() {
        Ok(token) => token,
        Err(()) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "token generation failed")
        }
    };
    let refresh_token = match random_token() {
        Ok(token) => token,
        Err(()) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "token generation failed")
        }
    };
    let now = Utc::now();
    let access_expires_at = now + access_ttl;
    let refresh_expires_at = now + refresh_ttl;
    let session = OAuthSession {
        client_id,
        access_token_hash: token_hash(&access_token),
        refresh_token_hash: token_hash(&refresh_token),
        access_expires_at,
        refresh_expires_at,
        scopes: scopes.clone(),
        revoked: false,
    };
    state
        .auth
        .oauth_sessions
        .lock()
        .await
        .insert(session.refresh_token_hash.clone(), session);

    Json(OAuthTokenResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: access_ttl.num_seconds(),
        refresh_token,
        refresh_expires_in: refresh_ttl.num_seconds(),
        scope: scopes.join(" "),
    })
    .into_response()
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let config = state.config.lock().await;
    let cookie = cookie_name(&config);
    drop(config);

    if let Some(token) = cookie_value(&headers, &cookie) {
        state.auth.sessions.lock().await.remove(&token);
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = clear_cookie(&cookie).parse() {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}
