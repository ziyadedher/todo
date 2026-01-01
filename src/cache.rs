//! Application cache for storing data between runs.

use std::fs;
use std::path::Path;

use anyhow::Context as _;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::asana::Credentials;
use crate::focus::FocusDay;
use crate::task::{UserTask, UserTaskList};

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
