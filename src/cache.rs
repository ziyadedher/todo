//! Application cache for storing data between runs.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::Context as _;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::asana::Credentials;
use crate::focus::FocusDay;
use crate::task::{UserTask, UserTaskList};

/// Maximum age for an auth lock before it's considered stale.
const AUTH_LOCK_MAX_AGE: Duration = Duration::from_secs(300); // 5 minutes

/// Cached application data.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Cache {
    /// Stored credentials.
    pub creds: Option<Credentials>,
    /// User's task list reference.
    pub user_task_list: Option<UserTaskList>,
    /// Cached tasks.
    pub tasks: Option<Vec<UserTask>>,
    /// Cached focus day.
    pub focus_day: Option<FocusDay>,
    /// Last time the cache was updated.
    pub last_updated: Option<DateTime<Local>>,
}

/// Load cache from disk.
///
/// # Errors
///
/// Returns an error if the cache file cannot be read or parsed.
pub fn load(path: &Path) -> anyhow::Result<Cache> {
    log::debug!("Checking if cache file exists at {}...", path.display());
    if !path.exists() {
        log::warn!(
            "Could not find cache at {}, so creating and using an empty cache...",
            path.display()
        );
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("could not create path to cache file")?;
        }
        save(path, &Cache::default())?;
    }

    log::debug!("Loading cache from {}...", path.display());
    let cache =
        serde_json::from_str(&fs::read_to_string(path).context("could not read cache file")?);
    match cache {
        Ok(cache) => {
            log::trace!("Loaded cache: {cache:#?}");
            Ok(cache)
        }
        Err(err) => {
            log::warn!(
                "Could not deserialize cache file at {}, wiping it and trying again...",
                path.display()
            );
            log::debug!("Cache deserialization error: {err}");
            save(path, &Cache::default())?;
            load(path)
        }
    }
}

/// Save cache to disk.
///
/// # Errors
///
/// Returns an error if the cache cannot be serialized or written.
pub fn save(path: &Path, cache: &Cache) -> anyhow::Result<()> {
    log::debug!("Saving cache to {}...", path.display());
    fs::write(
        path,
        serde_json::to_string_pretty(cache).context("could not serialize cache")?,
    )
    .context("could not write to cache file")?;
    log::trace!("Saved cache: {cache:#?}");
    Ok(())
}

/// Guard that holds an auth lock and releases it when dropped.
pub struct AuthLockGuard {
    lock_path: std::path::PathBuf,
}

impl Drop for AuthLockGuard {
    fn drop(&mut self) {
        if let Err(e) = fs::remove_file(&self.lock_path) {
            log::warn!("Failed to remove auth lock file: {e}");
        }
    }
}

/// Error returned when acquiring auth lock fails.
#[derive(Debug, thiserror::Error)]
pub enum AuthLockError {
    /// Another auth flow is already in progress.
    #[error("another authentication flow is already in progress")]
    AlreadyLocked,
    /// I/O error while managing lock.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Get the path to the auth lock file based on the cache path.
fn get_auth_lock_path(cache_path: &Path) -> std::path::PathBuf {
    cache_path.with_file_name("auth.lock")
}

/// Check if an auth lock exists and is still valid (not stale).
fn is_lock_valid(lock_path: &Path) -> bool {
    if !lock_path.exists() {
        return false;
    }

    // Read the lock file to get the timestamp
    let Ok(mut file) = File::open(lock_path) else {
        return false;
    };

    let mut contents = String::new();
    if file.read_to_string(&mut contents).is_err() {
        return false;
    }

    // Parse the timestamp from the lock file
    let timestamp: u64 = match contents.trim().parse() {
        Ok(t) => t,
        Err(_) => return false,
    };

    // Check if the lock is still within the max age
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    now.saturating_sub(timestamp) < AUTH_LOCK_MAX_AGE.as_secs()
}

/// Attempt to acquire an auth lock.
///
/// Returns a guard that will release the lock when dropped.
///
/// # Errors
///
/// Returns `AuthLockError::AlreadyLocked` if another auth flow is in progress.
pub fn acquire_auth_lock(cache_path: &Path) -> Result<AuthLockGuard, AuthLockError> {
    let lock_path = get_auth_lock_path(cache_path);
    log::debug!("Attempting to acquire auth lock at {}", lock_path.display());

    // Check if there's a valid existing lock
    if is_lock_valid(&lock_path) {
        log::warn!("Auth lock already held by another process");
        return Err(AuthLockError::AlreadyLocked);
    }

    // Remove stale lock if it exists
    if lock_path.exists() {
        log::debug!("Removing stale auth lock");
        let _ = fs::remove_file(&lock_path);
    }

    // Create the lock file with current timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)?;

    file.write_all(timestamp.to_string().as_bytes())?;

    log::debug!("Auth lock acquired successfully");
    Ok(AuthLockGuard { lock_path })
}

/// Check if an auth flow is currently in progress.
#[must_use]
pub fn is_auth_in_progress(cache_path: &Path) -> bool {
    is_lock_valid(&get_auth_lock_path(cache_path))
}
