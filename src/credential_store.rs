use crate::configuration::{AuthUserConfig, OAuthClientConfig};
use ring::{aead, rand};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub struct CredentialStore {
    db_path: PathBuf,
    key: Arc<[u8; 32]>,
}

impl CredentialStore {
    pub fn open_default() -> Result<Self, Box<dyn std::error::Error>> {
        Self::open(
            PathBuf::from("controller_data/auth/credentials.sqlite3"),
            PathBuf::from("controller_data/auth/credential.key"),
        )
    }

    #[cfg(test)]
    pub fn open_for_test(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let base = std::env::temp_dir().join(format!(
            "rust_server_controller_{}_{}",
            name,
            uuid::Uuid::new_v4()
        ));
        Self::open(
            base.join("credentials.sqlite3"),
            base.join("credential.key"),
        )
    }

    fn open(db_path: PathBuf, key_path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = key_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let key = load_or_create_key(&key_path)?;
        let store = Self {
            db_path,
            key: Arc::new(key),
        };
        store.with_connection(initialize_schema)?;
        Ok(store)
    }

    pub fn upsert_auth_user(&self, user: &AuthUserConfig) -> rusqlite::Result<()> {
        let payload = self.encrypt_json(user)?;
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO auth_users (username, payload)
                 VALUES (?1, ?2)
                 ON CONFLICT(username) DO UPDATE SET payload = excluded.payload",
                params![user.username, payload],
            )?;
            Ok(())
        })
    }

    pub fn auth_user(&self, username: &str) -> rusqlite::Result<Option<AuthUserConfig>> {
        let payload = self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT payload FROM auth_users WHERE username = ?1",
                    params![username],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
        })?;
        payload
            .map(|payload| self.decrypt_json(&payload))
            .transpose()
    }

    pub fn upsert_oauth_client(&self, client: &OAuthClientConfig) -> rusqlite::Result<()> {
        let payload = self.encrypt_json(client)?;
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO oauth_clients (client_id, payload)
                 VALUES (?1, ?2)
                 ON CONFLICT(client_id) DO UPDATE SET payload = excluded.payload",
                params![client.client_id, payload],
            )?;
            Ok(())
        })
    }

    pub fn oauth_client(&self, client_id: &str) -> rusqlite::Result<Option<OAuthClientConfig>> {
        let payload = self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT payload FROM oauth_clients WHERE client_id = ?1",
                    params![client_id],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
        })?;
        payload
            .map(|payload| self.decrypt_json(&payload))
            .transpose()
    }

    pub fn reset_all(&self) -> rusqlite::Result<()> {
        self.with_connection(|connection| {
            connection.execute_batch(
                "DELETE FROM auth_users;
                 DELETE FROM oauth_clients;",
            )?;
            Ok(())
        })
    }

    fn encrypt_json<T: serde::Serialize>(&self, value: &T) -> rusqlite::Result<Vec<u8>> {
        let mut plaintext = serde_json::to_vec(value)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let nonce = random_nonce()?;
        let key = aead::LessSafeKey::new(
            aead::UnboundKey::new(&aead::AES_256_GCM, self.key.as_ref())
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        );
        key.seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::empty(),
            &mut plaintext,
        )
        .map_err(|_| rusqlite::Error::InvalidQuery)?;

        let mut encrypted = nonce.to_vec();
        encrypted.extend_from_slice(&plaintext);
        Ok(encrypted)
    }

    fn decrypt_json<T: serde::de::DeserializeOwned>(
        &self,
        encrypted: &[u8],
    ) -> rusqlite::Result<T> {
        if encrypted.len() < 12 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&encrypted[..12]);
        let mut ciphertext = encrypted[12..].to_vec();
        let key = aead::LessSafeKey::new(
            aead::UnboundKey::new(&aead::AES_256_GCM, self.key.as_ref())
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        );
        let plaintext = key
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut ciphertext,
            )
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        serde_json::from_slice(plaintext).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Blob,
                Box::new(error),
            )
        })
    }

    fn with_connection<T>(
        &self,
        action: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let connection = Connection::open(&self.db_path)?;
        action(&connection)
    }
}

pub fn migrate_config_credentials(
    config: &mut crate::configuration::Config,
    store: &CredentialStore,
) -> bool {
    let mut changed = false;
    for user in &mut config.auth.users {
        if !user.password_hash.is_empty() || !user.password_salt.is_empty() {
            if let Err(error) = store.upsert_auth_user(user) {
                tracing::error!("Failed to migrate auth user '{}': {}", user.username, error);
                continue;
            }
            user.password_hash.clear();
            user.password_salt.clear();
            changed = true;
        }
    }
    for client in &mut config.auth.oauth_clients {
        if !client.client_secret_hash.is_empty() {
            if let Err(error) = store.upsert_oauth_client(client) {
                tracing::error!(
                    "Failed to migrate OAuth client '{}': {}",
                    client.client_id,
                    error
                );
                continue;
            }
            client.client_secret_hash.clear();
            changed = true;
        }
    }
    changed
}

fn initialize_schema(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS auth_users (
            username TEXT PRIMARY KEY NOT NULL,
            payload BLOB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS oauth_clients (
            client_id TEXT PRIMARY KEY NOT NULL,
            payload BLOB NOT NULL
        );",
    )
}

fn load_or_create_key(path: &Path) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    if path.exists() {
        let text = fs::read_to_string(path)?;
        let key_bytes = hex_to_bytes(text.trim())?;
        let key: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| "credential key must be 32 bytes")?;
        return Ok(key);
    }

    let rng = rand::SystemRandom::new();
    let mut key = [0u8; 32];
    rand::SecureRandom::fill(&rng, &mut key).map_err(|_| "failed to generate credential key")?;
    fs::write(path, bytes_to_hex(&key))?;
    Ok(key)
}

fn random_nonce() -> rusqlite::Result<[u8; 12]> {
    let rng = rand::SystemRandom::new();
    let mut nonce = [0u8; 12];
    rand::SecureRandom::fill(&rng, &mut nonce).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(nonce)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn hex_to_bytes(text: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if text.len() % 2 != 0 {
        return Err("hex string has odd length".into());
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    for index in (0..text.len()).step_by(2) {
        bytes.push(u8::from_str_radix(&text[index..index + 2], 16)?);
    }
    Ok(bytes)
}
