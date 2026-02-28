//! macOS API to store the data of a session, using the macOS Keychain.

use matrix_sdk::authentication::oauth::ClientId;
use ruma::UserId;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::error;
use url::Url;

use super::{SecretError, SecretExt, StoredSession};
use crate::{APP_ID, PROFILE, spawn_tokio};

/// `errSecItemNotFound` from `Security.framework`.
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// The current version of the stored session.
const CURRENT_VERSION: u8 = 7;

/// The fixed Keychain `account` attribute used for the session-list item.
///
/// All sessions are stored as a single generic-password Keychain item whose
/// `service` attribute is `"{APP_ID}.{PROFILE}"` and whose `account`
/// attribute is this constant.  The item's secret data is the JSON
/// serialisation of [`SessionList`].
const SESSIONS_ACCOUNT: &str = "sessions";

/// Session data as it is serialised into the Keychain item.
#[derive(Clone, Serialize, Deserialize)]
struct MacOSSecretData {
    version: u8,
    homeserver: String,
    user_id: String,
    device_id: String,
    id: String,
    client_id: Option<String>,
    passphrase: String,
}

/// Wrapper that is serialised as the secret data of the Keychain item.
#[derive(Default, Serialize, Deserialize)]
struct SessionList {
    sessions: Vec<MacOSSecretData>,
}

/// Secret API for macOS.
pub(crate) struct MacOSSecret;

impl SecretExt for MacOSSecret {
    async fn restore_sessions() -> Result<Vec<StoredSession>, SecretError> {
        let handle = spawn_tokio!(async move {
            let service = format!("{}.{}", APP_ID, PROFILE.as_str());
            restore_sessions_sync(&service)
        });
        match handle.await.expect("task was not aborted") {
            Ok(sessions) => Ok(sessions),
            Err(error) => {
                error!("Could not restore previous sessions: {error}");
                Err(SecretError::Service(error.to_string()))
            }
        }
    }

    async fn store_session(session: StoredSession) -> Result<(), SecretError> {
        let handle = spawn_tokio!(async move {
            let service = format!("{}.{}", APP_ID, PROFILE.as_str());
            upsert_session(&service, &session)
        });
        match handle.await.expect("task was not aborted") {
            Ok(()) => Ok(()),
            Err(error) => {
                error!("Could not store session: {error}");
                Err(SecretError::Service(error.to_string()))
            }
        }
    }

    async fn delete_session(session: &StoredSession) {
        let service = format!("{}.{}", APP_ID, PROFILE.as_str());
        let account = format!("{}.{}", session.user_id, session.device_id);
        spawn_tokio!(async move {
            if let Err(error) = remove_session(&service, &account) {
                error!("Could not delete session from keychain: {error}");
            }
        })
        .await
        .expect("task was not aborted");
    }
}

/// Reads all sessions from the Keychain, migrating from legacy file storage
/// if no Keychain item exists yet.
fn restore_sessions_sync(service: &str) -> Result<Vec<StoredSession>, MacOSSecretError> {
    match get_generic_password(service, SESSIONS_ACCOUNT) {
        Ok(data) => deserialize_sessions(&data),
        Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => migrate_from_legacy(service),
        Err(e) => Err(MacOSSecretError::KeychainAccess(e.to_string())),
    }
}

/// Inserts or updates a session in the Keychain.
fn upsert_session(service: &str, session: &StoredSession) -> Result<(), MacOSSecretError> {
    let mut list = match get_generic_password(service, SESSIONS_ACCOUNT) {
        Ok(data) => serde_json::from_slice::<SessionList>(&data)
            .map_err(|e| MacOSSecretError::Serialization(e.to_string()))?,
        Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => SessionList::default(),
        Err(e) => return Err(MacOSSecretError::KeychainAccess(e.to_string())),
    };

    let new_entry = MacOSSecretData {
        version: CURRENT_VERSION,
        homeserver: session.homeserver.to_string(),
        user_id: session.user_id.to_string(),
        device_id: session.device_id.to_string(),
        id: session.id.clone(),
        client_id: session.client_id.as_ref().map(|c| c.as_str().to_string()),
        passphrase: session.passphrase.to_string(),
    };

    if let Some(idx) = list
        .sessions
        .iter()
        .position(|s| s.user_id == new_entry.user_id && s.device_id == new_entry.device_id)
    {
        list.sessions[idx] = new_entry;
    } else {
        list.sessions.push(new_entry);
    }

    let data =
        serde_json::to_vec(&list).map_err(|e| MacOSSecretError::Serialization(e.to_string()))?;
    set_generic_password(service, SESSIONS_ACCOUNT, &data)
        .map_err(|e| MacOSSecretError::KeychainAccess(e.to_string()))
}

/// Removes a session (identified by `account = "{user_id}.{device_id}"`) from
/// the Keychain.
fn remove_session(service: &str, account: &str) -> Result<(), MacOSSecretError> {
    let data = match get_generic_password(service, SESSIONS_ACCOUNT) {
        Ok(data) => data,
        Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => return Ok(()),
        Err(e) => return Err(MacOSSecretError::KeychainAccess(e.to_string())),
    };

    let mut list: SessionList = serde_json::from_slice(&data)
        .map_err(|e| MacOSSecretError::Serialization(e.to_string()))?;

    list.sessions
        .retain(|s| format!("{}.{}", s.user_id, s.device_id) != account);

    if list.sessions.is_empty() {
        match delete_generic_password(service, SESSIONS_ACCOUNT) {
            Ok(()) => Ok(()),
            // Another concurrent removal already deleted the item; that is fine.
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(MacOSSecretError::KeychainAccess(e.to_string())),
        }
    } else {
        let data = serde_json::to_vec(&list)
            .map_err(|e| MacOSSecretError::Serialization(e.to_string()))?;
        set_generic_password(service, SESSIONS_ACCOUNT, &data)
            .map_err(|e| MacOSSecretError::KeychainAccess(e.to_string()))
    }
}

fn deserialize_sessions(data: &[u8]) -> Result<Vec<StoredSession>, MacOSSecretError> {
    let list: SessionList =
        serde_json::from_slice(data).map_err(|e| MacOSSecretError::Serialization(e.to_string()))?;
    list.sessions.into_iter().map(parse_session).collect()
}

fn parse_session(data: MacOSSecretData) -> Result<StoredSession, MacOSSecretError> {
    let homeserver = Url::parse(&data.homeserver)
        .map_err(|e| MacOSSecretError::InvalidData(format!("Invalid homeserver URL: {e}")))?;
    let user_id = UserId::parse(&data.user_id)
        .map_err(|e| MacOSSecretError::InvalidData(format!("Invalid user ID: {e}")))?;
    let client_id = data.client_id.map(ClientId::new);

    Ok(StoredSession {
        homeserver,
        user_id,
        device_id: data.device_id.into(),
        id: data.id,
        client_id,
        passphrase: data.passphrase.into(),
    })
}

/// Migrates sessions from the legacy file-based storage (if any) into the
/// Keychain, then removes the legacy files.
///
/// This is a one-time migration that runs on the first launch after upgrading
/// to the Keychain-backed implementation.
fn migrate_from_legacy(service: &str) -> Result<Vec<StoredSession>, MacOSSecretError> {
    let keychain_dir = match dirs::data_dir() {
        Some(d) => d.join("fractal").join("keychain"),
        None => return Ok(Vec::new()),
    };

    if !keychain_dir.exists() {
        return Ok(Vec::new());
    }

    let prefix = format!("{service}.");
    let mut legacy: Vec<MacOSSecretData> = Vec::new();

    let entries = std::fs::read_dir(&keychain_dir)
        .map_err(|e| MacOSSecretError::KeychainAccess(e.to_string()))?;

    for entry in entries {
        let path = entry
            .map_err(|e| MacOSSecretError::KeychainAccess(e.to_string()))?
            .path();

        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }

        let file_stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        if !file_stem.starts_with(&prefix) {
            continue;
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| MacOSSecretError::KeychainAccess(e.to_string()))?;

        match serde_json::from_str::<MacOSSecretData>(&content) {
            Ok(data) => legacy.push(data),
            Err(e) => {
                error!(
                    "Could not parse legacy session file {}: {e}",
                    path.display()
                );
            }
        }
    }

    if legacy.is_empty() {
        return Ok(Vec::new());
    }

    // Write to Keychain before removing the old files, so that if the write
    // fails the user's sessions are not lost.
    let list = SessionList {
        sessions: legacy.clone(),
    };
    let json =
        serde_json::to_vec(&list).map_err(|e| MacOSSecretError::Serialization(e.to_string()))?;
    set_generic_password(service, SESSIONS_ACCOUNT, &json)
        .map_err(|e| MacOSSecretError::KeychainAccess(e.to_string()))?;

    // Remove the legacy directory now that migration succeeded.
    if let Err(e) = std::fs::remove_dir_all(&keychain_dir) {
        error!("Could not remove legacy keychain directory: {e}");
    }

    legacy.into_iter().map(parse_session).collect()
}

/// Any error that can happen when interacting with the macOS Keychain.
#[derive(Debug, Error)]
enum MacOSSecretError {
    /// An error occurred while accessing the keychain.
    #[error("Keychain access error: {0}")]
    KeychainAccess(String),

    /// An error occurred while serializing/deserializing data.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Invalid data was found in the keychain.
    #[error("Invalid data: {0}")]
    InvalidData(String),
}
