//! Application configuration types.

use std::fs;
use std::path::Path;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

/// Application configuration loaded from config file.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// tmux integration settings.
    pub tmux: TmuxConfig,
    /// Menu bar integration settings.
    pub menubar: MenubarConfig,
    /// Notification settings.
    pub notifications: NotificationsConfig,
    /// Terminal behavior settings.
    pub terminal: TerminalConfig,
}

/// Load configuration from disk.
///
/// # Errors
///
/// Returns an error if the config file cannot be read or parsed.
pub fn load(path: &Path) -> anyhow::Result<Config> {
    log::debug!(
        "Checking if configuration file exists at {}...",
        path.display()
    );
    if !path.exists() {
        log::warn!(
            "Could not find configuration at {}, so creating and using an empty configuration...",
            path.display()
        );
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("could not create path to configuration file")?;
        }
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .context("could not create configuration file")?;
    }

    log::debug!("Loading configuration from {}...", path.display());
    let config: Config =
        toml::from_str(&fs::read_to_string(path).context("could not read configuration file")?)
            .context("could not deserialize configuration file")?;
    log::trace!("Loaded configuration: {config:#?}");
    Ok(config)
}

/// tmux integration configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct TmuxConfig {
    /// Whether tmux integration is enabled.
    pub enabled: bool,
}

impl Default for TmuxConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Menu bar integration configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct MenubarConfig {
    /// Whether menu bar integration is enabled.
    pub enabled: bool,
    /// Refresh interval in seconds.
    pub refresh_seconds: u32,
}

impl Default for MenubarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            refresh_seconds: 60,
        }
    }
}

/// Notification configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct NotificationsConfig {
    /// Whether notifications are enabled.
    pub enabled: bool,
    /// Morning notification time (HH:MM format).
    pub morning_time: String,
    /// Evening notification time (HH:MM format).
    pub evening_time: String,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            morning_time: "09:00".to_string(),
            evening_time: "20:00".to_string(),
        }
    }
}

/// Terminal behavior configuration.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct TerminalConfig {
    /// Whether to block terminal until focus is acknowledged.
    pub blocking: bool,
}
