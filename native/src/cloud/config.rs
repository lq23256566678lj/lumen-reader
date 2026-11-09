use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::state::models::now_secs;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WebDavConfig {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub username: String,
    /// Stored locally only. Set `LUMEN_WEBDAV_PASSWORD` to keep it out of the
    /// JSON file entirely.
    #[serde(default)]
    pub password: String,
    /// Path below the WebDAV root, e.g. `books/lumen`.
    #[serde(default)]
    pub remote_dir: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub last_sync_at: Option<u64>,
    #[serde(default)]
    pub last_sync_status: Option<String>,
}

impl Default for WebDavConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            username: String::new(),
            password: String::new(),
            remote_dir: String::new(),
            device_id: generate_device_id(),
            last_sync_at: None,
            last_sync_status: None,
        }
    }
}

impl WebDavConfig {
    pub fn ensure_device_id(&mut self) {
        if self.device_id.trim().is_empty() {
            self.device_id = generate_device_id();
        }
    }

    pub fn configured(&self) -> bool {
        !self.url.trim().is_empty()
    }

    pub fn effective_password(&self) -> String {
        std::env::var("LUMEN_WEBDAV_PASSWORD")
            .ok()
            .filter(|value| !value.is_empty())
            .map_or_else(|| self.password.clone(), |value| value)
    }
}

fn generate_device_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u128, |duration| duration.as_nanos());
    let mut hasher = DefaultHasher::new();
    nanos.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    now_secs().hash(&mut hasher);
    format!("device-{:016x}", hasher.finish())
}
