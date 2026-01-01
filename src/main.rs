#![warn(clippy::pedantic)]

use std::{
    collections::HashMap,
    env,
    fmt::{Display, Write as _},
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::{DateTime, Datelike, Days, Local, NaiveDate, Timelike, Weekday};
use clap::{Parser, Subcommand};
use console::{style, Term};
use dialoguer::{theme::ColorfulTheme, Input};
use futures::future::join_all;
use human_panic::setup_panic;
use regex::Regex;
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};

use todo::asana::{
    ask_for_pat, execute_authorization_flow, Client, Credentials, DataRequest, DataWrapper,
};

const ASANA_WORKSPACE_GID: &str = "1199118829113557";
const ASANA_FOCUS_PROJECT_GID: &str = "1200179899177794";

const FOCUS_WEEK_PATTERN: &str =
    r"^Daily Focuses \((?<from>\d{4}-\d{2}-\d{2}) to (?<to>\d{4}-\d{2}-\d{2})\)$";
const FOCUS_DAY_PATTERN: &str = r"^Daily Focus for \w+ \((?<date>\d{4}-\d{2}-\d{2})\)$";

/// The hour of the day at which the end of day is considered to be starting.
const START_HOUR_FOR_EOD: u32 = 20;

/// Todo is a simple Asana helper script that pulls data from Asana and shows it in CLI settings
#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the cache file
    #[arg(long, default_value = "~/.cache/todo/cache.json")]
    cache_path: PathBuf,

    /// Path to the configuration file
    #[arg(long, default_value = "~/.config/todo/config.toml")]
    config_path: PathBuf,

    /// If set, uses the discouraged PAT flow (instead of OAuth)
    #[arg(long)]
    use_pat: bool,

    /// If set, uses the cache instead of pulling from Asana
    #[arg(long)]
    use_cache: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print out a summary of current todo tasks
    Summary,

    /// Print out a list of todo tasks ordered by due date
    List,

    /// Manage the Focus project
    Focus {
        /// The date to focus on
        #[arg(long)]
        date: Option<NaiveDate>,

        /// If set, forces the end of day to be considered to be starting
        #[arg(long, default_value = "false")]
        force_eod: bool,

        #[command(subcommand)]
        command: Option<FocusCommand>,
    },

    /// Pull and cache information about todo task and focus, without printing anything
    Update,

    /// Output machine-readable status for integrations (tmux, menubar, etc.)
    Status {
        /// Output format
        #[arg(long, default_value = "short")]
        format: StatusFormat,

        /// Color mode
        #[arg(long, default_value = "always")]
        color: ColorMode,
    },

    /// Install shell/system integrations
    Install {
        #[command(subcommand)]
        integration: InstallIntegration,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum StatusFormat {
    /// Short one-line format for shell prompts and status bars
    Short,
    /// JSON format for programmatic use
    Json,
    /// xbar/SwiftBar format for macOS menu bar
    Xbar,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum ColorMode {
    /// Always use ANSI colors
    Always,
    /// Never use colors
    Never,
    /// Use tmux color syntax (#[fg=red])
    Tmux,
}

#[derive(Debug, Subcommand)]
enum InstallIntegration {
    /// Print zsh prompt configuration
    Zsh,
    /// Print tmux configuration snippet (including Dracula theme)
    Tmux,
    /// Install xbar/SwiftBar plugin (macOS only)
    Xbar,
    /// Install launchd notification agents (macOS only)
    Notifications,
    /// Show all available integrations and their status
    Show,
}

/// User choice when blocking terminal prompt is shown.
#[derive(Clone, Copy)]
enum BlockingChoice {
    RunFocus,
    Skip,
}

#[derive(Debug, Subcommand)]
enum FocusCommand {
    /// Run the focus routine
    Run,
    /// Print out an overview of the focus day
    Overview,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Cache {
    creds: Option<Credentials>,
    user_task_list: Option<UserTaskList>,
    tasks: Option<Vec<UserTask>>,
    focus_day: Option<FocusDay>,
    last_updated: Option<DateTime<Local>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct Config {
    tmux: TmuxConfig,
    menubar: MenubarConfig,
    notifications: NotificationsConfig,
    terminal: TerminalConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct TmuxConfig {
    enabled: bool,
}

impl Default for TmuxConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct MenubarConfig {
    enabled: bool,
    refresh_seconds: u32,
}

impl Default for MenubarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            refresh_seconds: 60,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct NotificationsConfig {
    enabled: bool,
    morning_time: String,
    evening_time: String,
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct TerminalConfig {
    blocking: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct UserTask {
    gid: String,
    #[serde(with = "todo::asana::serde_formats::datetime")]
    created_at: DateTime<Local>,
    #[serde(with = "todo::asana::serde_formats::optional_date")]
    due_on: Option<NaiveDate>,
    name: String,
}

impl<'a> DataRequest<'a> for UserTask {
    type RequestData = String;
    type ResponseData = Vec<Self>;

    fn segments(request_data: &'a Self::RequestData) -> Vec<String> {
        vec![
            "user_task_lists".to_string(),
            request_data.clone(),
            "tasks".to_string(),
        ]
    }

    fn fields() -> &'a [&'a str] {
        &["this.gid", "this.created_at", "this.due_on", "this.name"]
    }

    fn params() -> Vec<(&'a str, String)> {
        vec![("completed_since", "now".to_string())]
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct UserTaskList {
    gid: String,
}

impl<'a> DataRequest<'a> for UserTaskList {
    type RequestData = String;
    type ResponseData = Self;

    fn segments(user_gid: &'a Self::RequestData) -> Vec<String> {
        vec![
            "users".to_string(),
            user_gid.clone(),
            "user_task_list".to_string(),
        ]
    }

    fn fields() -> &'a [&'a str] {
        &["this.gid"]
    }

    fn params() -> Vec<(&'a str, String)> {
        vec![("workspace", ASANA_WORKSPACE_GID.to_string())]
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Section {
    gid: String,
    name: String,
}

impl<'a> DataRequest<'a> for Section {
    type RequestData = String;
    type ResponseData = Vec<Self>;

    fn segments(request_data: &'a Self::RequestData) -> Vec<String> {
        vec![
            "projects".to_string(),
            request_data.clone(),
            "sections".to_string(),
        ]
    }

    fn fields() -> &'a [&'a str] {
        &["this.gid", "this.name"]
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CreateSectionRequest {
    name: String,
    insert_before: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FocusTask {
    gid: String,
    name: String,
    notes: String,
    custom_fields: Option<Vec<FocusTaskCustomField>>,
}

impl<'a> DataRequest<'a> for FocusTask {
    type RequestData = String;
    type ResponseData = Vec<Self>;

    fn segments(request_data: &'a Self::RequestData) -> Vec<String> {
        vec![
            "sections".to_string(),
            request_data.clone(),
            "tasks".to_string(),
        ]
    }

    fn fields() -> &'a [&'a str] {
        &[
            "this.gid",
            "this.name",
            "this.notes",
            "this.custom_fields",
            "this.custom_fields.gid",
            "this.custom_fields.number_value",
        ]
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FocusTaskCustomField {
    gid: String,
    number_value: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct UpdateFocusTaskCustomFieldsRequest {
    notes: String,
    custom_fields: HashMap<String, u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CreateSectionTaskRequest {
    name: String,
    projects: Vec<String>,
    memberships: Vec<CreateSectionTaskRequestMembership>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CreateSectionTaskRequestMembership {
    project: String,
    section: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AddTaskToSectionRequest {
    task: String,
    insert_after: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FocusTaskSubtask {
    gid: String,
    name: String,
    completed: bool,
}

impl DataRequest<'_> for FocusTaskSubtask {
    type RequestData = String;
    type ResponseData = Vec<Self>;

    fn segments(request_data: &Self::RequestData) -> Vec<String> {
        vec![
            "tasks".to_string(),
            request_data.clone(),
            "subtasks".to_string(),
        ]
    }

    fn fields() -> &'static [&'static str] {
        &["this.gid", "this.name", "this.completed"]
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CreateSubtaskRequest {
    name: String,
    assignee: String,
    #[serde(with = "todo::asana::serde_formats::optional_date")]
    due_on: Option<NaiveDate>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FocusWeek {
    section: Section,
    from: NaiveDate,
    to: NaiveDate,
}

impl TryFrom<Section> for FocusWeek {
    type Error = anyhow::Error;

    fn try_from(section: Section) -> Result<Self, Self::Error> {
        let captures = Regex::new(FOCUS_WEEK_PATTERN)
            .context("unable to parse focus section pattern")?
            .captures(&section.name)
            .context(section.name.clone())?;
        Ok(Self {
            section: section.clone(),
            from: NaiveDate::parse_from_str(&captures["from"], "%Y-%m-%d")
                .context(section.name.clone())?,
            to: NaiveDate::parse_from_str(&captures["to"], "%Y-%m-%d")
                .context(section.name.clone())?,
        })
    }
}

impl Display for FocusWeek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Focus Week ({from} to {to})",
            from = self.from.format("%Y-%m-%d"),
            to = self.to.format("%Y-%m-%d")
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FocusDay {
    task: FocusTask,
    date: NaiveDate,
    stats: FocusDayStats,
    diary: String,
    subtasks: Option<Vec<FocusTaskSubtask>>,
}

impl FocusDay {
    fn to_full_string(&self) -> String {
        let mut string = String::new();

        let _ = write!(
            string,
            "🧠 {} {}",
            style(format!(
                "Focus Day: {}",
                style(self.date.weekday().to_string()).blue()
            ))
            .bold(),
            style(format!("({})", self.date.format("%Y-%m-%d"))).dim(),
        );
        let _ = write!(
            string,
            "\n\n{}",
            if self.diary.is_empty() {
                style("no diary entry — yet.").dim()
            } else {
                style(self.diary.as_str())
            },
        );
        let _ = writeln!(string, "\n\n{}", style("❤️ Statistics").bold().cyan());

        for stat in self.stats.stats() {
            let line = format!(
                "{name}: {value}",
                name = style(stat.name().to_string()).bold(),
                value = style(stat.value().map_or("-".to_string(), |v| v.to_string()))
            );
            let _ = writeln!(
                string,
                "   {}",
                if stat.value().is_some() {
                    style(line)
                } else {
                    style(line).dim()
                }
            );
        }
        string
    }

    async fn load_subtasks(&mut self, client: &mut Client) -> anyhow::Result<&[FocusTaskSubtask]> {
        let subtasks = client.get::<FocusTaskSubtask>(&self.task.gid).await?;
        self.subtasks = Some(subtasks);
        Ok(self.subtasks.as_ref().unwrap())
    }
}

impl TryFrom<FocusTask> for FocusDay {
    type Error = anyhow::Error;

    fn try_from(task: FocusTask) -> Result<Self, Self::Error> {
        let captures = Regex::new(FOCUS_DAY_PATTERN)
            .context("unable to parse focus section pattern")?
            .captures(&task.name)
            .context(task.name.clone())?;
        Ok(Self {
            task: task.clone(),
            date: NaiveDate::parse_from_str(&captures["date"], "%Y-%m-%d")
                .context(task.name.clone())?,
            stats: task
                .custom_fields
                .context("could not find custom fields")?
                .try_into()?,
            diary: task.notes,
            subtasks: None,
        })
    }
}

impl Display for FocusDay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Focus Day ({date}) (stats: {stats})",
            date = self.date.format("%Y-%m-%d"),
            stats = self.stats
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
struct FocusDayStats {
    sleep: FocusDayStat,
    energy: FocusDayStat,
    flow: FocusDayStat,
    hydration: FocusDayStat,
    health: FocusDayStat,
    satisfaction: FocusDayStat,
    stress: FocusDayStat,
}

impl FocusDayStats {
    fn stats(&self) -> Vec<&FocusDayStat> {
        vec![
            &self.sleep,
            &self.energy,
            &self.flow,
            &self.hydration,
            &self.health,
            &self.satisfaction,
            &self.stress,
        ]
    }

    fn set_stat(&mut self, stat: FocusDayStat) {
        match stat {
            FocusDayStat::Sleep(_) => self.sleep = stat,
            FocusDayStat::Energy(_) => self.energy = stat,
            FocusDayStat::Flow(_) => self.flow = stat,
            FocusDayStat::Hydration(_) => self.hydration = stat,
            FocusDayStat::Health(_) => self.health = stat,
            FocusDayStat::Satisfaction(_) => self.satisfaction = stat,
            FocusDayStat::Stress(_) => self.stress = stat,
        }
    }
}

impl Default for FocusDayStats {
    fn default() -> Self {
        Self {
            sleep: FocusDayStat::Sleep(None),
            energy: FocusDayStat::Energy(None),
            flow: FocusDayStat::Flow(None),
            hydration: FocusDayStat::Hydration(None),
            health: FocusDayStat::Health(None),
            satisfaction: FocusDayStat::Satisfaction(None),
            stress: FocusDayStat::Stress(None),
        }
    }
}

impl TryFrom<Vec<FocusTaskCustomField>> for FocusDayStats {
    type Error = anyhow::Error;

    fn try_from(custom_fields: Vec<FocusTaskCustomField>) -> Result<Self, Self::Error> {
        let mut stats = Self::default();
        for custom_field in custom_fields {
            let stat = FocusDayStat::try_from(custom_field)?;
            match stat {
                FocusDayStat::Sleep(_) => stats.sleep = stat,
                FocusDayStat::Energy(_) => stats.energy = stat,
                FocusDayStat::Flow(_) => stats.flow = stat,
                FocusDayStat::Hydration(_) => stats.hydration = stat,
                FocusDayStat::Health(_) => stats.health = stat,
                FocusDayStat::Satisfaction(_) => stats.satisfaction = stat,
                FocusDayStat::Stress(_) => stats.stress = stat,
            }
        }
        Ok(stats)
    }
}

impl Display for FocusDayStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{sleep}, {energy}, {flow}, {hydration}, {health}, {satisfaction}, {stress}",
            sleep = self.sleep,
            energy = self.energy,
            flow = self.flow,
            hydration = self.hydration,
            health = self.health,
            satisfaction = self.satisfaction,
            stress = self.stress,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
enum FocusDayStat {
    Sleep(Option<u32>),
    Energy(Option<u32>),
    Flow(Option<u32>),
    Hydration(Option<u32>),
    Health(Option<u32>),
    Satisfaction(Option<u32>),
    Stress(Option<u32>),
}

impl FocusDayStat {
    fn name(&self) -> &'static str {
        match self {
            Self::Sleep(_) => "sleep",
            Self::Energy(_) => "energy",
            Self::Flow(_) => "flow",
            Self::Hydration(_) => "hydration",
            Self::Health(_) => "health",
            Self::Satisfaction(_) => "satisfaction",
            Self::Stress(_) => "stress",
        }
    }

    fn value(&self) -> Option<u32> {
        match self {
            Self::Sleep(value)
            | Self::Energy(value)
            | Self::Flow(value)
            | Self::Hydration(value)
            | Self::Health(value)
            | Self::Satisfaction(value)
            | Self::Stress(value) => *value,
        }
    }

    fn set_value(&mut self, value: Option<u32>) {
        match self {
            Self::Sleep(_) => *self = Self::Sleep(value),
            Self::Energy(_) => *self = Self::Energy(value),
            Self::Flow(_) => *self = Self::Flow(value),
            Self::Hydration(_) => *self = Self::Hydration(value),
            Self::Health(_) => *self = Self::Health(value),
            Self::Satisfaction(_) => *self = Self::Satisfaction(value),
            Self::Stress(_) => *self = Self::Stress(value),
        }
    }

    fn field_gid(&self) -> &'static str {
        match self {
            Self::Sleep(_) => "1204172638538713",
            Self::Energy(_) => "1204172638540767",
            Self::Flow(_) => "1204172638540769",
            Self::Hydration(_) => "1204172638540771",
            Self::Health(_) => "1204172638540773",
            Self::Satisfaction(_) => "1204172638540775",
            Self::Stress(_) => "1204172638540777",
        }
    }
}

impl TryFrom<FocusTaskCustomField> for FocusDayStat {
    type Error = anyhow::Error;

    fn try_from(custom_field: FocusTaskCustomField) -> Result<Self, Self::Error> {
        Ok(match custom_field.gid.as_str() {
            "1204172638538713" => Self::Sleep(custom_field.number_value),
            "1204172638540767" => Self::Energy(custom_field.number_value),
            "1204172638540769" => Self::Flow(custom_field.number_value),
            "1204172638540771" => Self::Hydration(custom_field.number_value),
            "1204172638540773" => Self::Health(custom_field.number_value),
            "1204172638540775" => Self::Satisfaction(custom_field.number_value),
            "1204172638540777" => Self::Stress(custom_field.number_value),
            gid => anyhow::bail!("unknown focus day stat gid: {gid}"),
        })
    }
}

impl Display for FocusDayStat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{name}={value}",
            name = self.name(),
            value = self.value().map_or("-".to_string(), |v| v.to_string())
        )
    }
}

/// Represents the current focus status for integrations
#[derive(Clone, Debug, Serialize)]
struct FocusStatus {
    /// Whether morning reflection (sleep/energy) is complete
    morning_done: bool,
    /// Whether evening reflection is complete (only relevant after `START_HOUR_FOR_EOD`)
    evening_done: bool,
    /// Whether it's currently evening time
    is_evening: bool,
    /// Number of overdue tasks
    overdue_count: usize,
    /// Number of tasks due today
    due_today_count: usize,
}

impl FocusStatus {
    fn new(
        focus_day: &FocusDay,
        now: DateTime<Local>,
        overdue_count: usize,
        due_today_count: usize,
    ) -> Self {
        let today = now.date_naive();
        let is_evening = now.hour() >= START_HOUR_FOR_EOD;

        // Morning is done if sleep AND energy are filled
        let morning_done = focus_day.date == today
            && focus_day.stats.sleep.value().is_some()
            && focus_day.stats.energy.value().is_some();

        // Evening is done if all non-morning stats are filled
        let evening_done = focus_day.date == today
            && focus_day.stats.stats().iter().all(|s| match s {
                FocusDayStat::Sleep(_) | FocusDayStat::Energy(_) => true,
                _ => s.value().is_some(),
            });

        Self {
            morning_done,
            evening_done,
            is_evening,
            overdue_count,
            due_today_count,
        }
    }

    fn to_short_string(&self, color_mode: &ColorMode) -> String {
        // Color helper based on mode
        let colorize = |text: &str, color: &str| -> String {
            match color_mode {
                ColorMode::Always => {
                    let styled = match color {
                        "red" => style(text).red(),
                        "yellow" => style(text).yellow(),
                        "green" => style(text).green(),
                        _ => style(text),
                    };
                    styled.force_styling(true).to_string()
                }
                ColorMode::Never => text.to_string(),
                ColorMode::Tmux => {
                    let fg = match color {
                        "red" => "red",
                        "yellow" => "yellow",
                        "green" => "green",
                        _ => "default",
                    };
                    format!("#[fg={fg}]{text}#[fg=default]")
                }
            }
        };

        let mut parts = Vec::new();

        // Focus status (yellow)
        if !self.morning_done {
            parts.push(colorize("focus:am", "yellow"));
        } else if self.is_evening && !self.evening_done {
            parts.push(colorize("focus:pm", "yellow"));
        }

        // Task counts
        if self.overdue_count > 0 {
            parts.push(colorize(&format!("!{}", self.overdue_count), "red"));
        }
        if self.due_today_count > 0 {
            parts.push(colorize(&format!("+{}", self.due_today_count), "yellow"));
        }

        if parts.is_empty() {
            colorize("✓", "green")
        } else {
            parts.join(" ")
        }
    }

    fn to_xbar_string(&self, config: &Config) -> String {
        if !config.menubar.enabled {
            return String::new();
        }

        let icon = if !self.morning_done {
            "☀️"
        } else if self.is_evening && !self.evening_done {
            "🌙"
        } else {
            "✓"
        };

        let mut output = String::new();
        let _ = writeln!(output, "{icon}\n---");

        // Status section
        if self.morning_done {
            output.push_str("Morning: ✓ Done\n");
        } else {
            output.push_str("Morning: ⏳ Pending | shell=todo | param1=focus | terminal=true\n");
        }

        if self.is_evening {
            if self.evening_done {
                output.push_str("Evening: ✓ Done\n");
            } else {
                output
                    .push_str("Evening: ⏳ Pending | shell=todo | param1=focus | terminal=true\n");
            }
        }

        output.push_str("---\n");

        // Task counts
        match (self.overdue_count, self.due_today_count) {
            (0, 0) => output.push_str("✓ No urgent tasks\n"),
            (o, 0) => {
                let _ = writeln!(output, "🔴 {o} overdue");
            }
            (0, t) => {
                let _ = writeln!(output, "🟡 {t} due today");
            }
            (o, t) => {
                let _ = writeln!(output, "🔴 {o} overdue");
                let _ = writeln!(output, "🟡 {t} due today");
            }
        }

        output.push_str("---\n");
        output.push_str("Run Focus | shell=todo | param1=focus | terminal=true\n");
        output.push_str("Refresh | refresh=true\n");

        output
    }
}

/// Parse a time string like "09:00" or "9" into (hour, minute).
fn parse_time_string(time: &str) -> (u32, u32) {
    let parts: Vec<&str> = time.split(':').collect();
    let hour = parts.first().and_then(|h| h.parse().ok()).unwrap_or(0);
    let minute = parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0);
    (hour, minute)
}

fn expand_homedir(path: &Path) -> anyhow::Result<PathBuf> {
    Ok(path
        .to_string_lossy()
        .replace('~', &env::var("HOME")?)
        .into())
}

fn load_cache(path: &Path) -> anyhow::Result<Cache> {
    log::debug!("Checking if cache file exists at {}...", path.display());
    if !path.exists() {
        log::warn!(
            "Could not find cache at {}, so creating and using an empty cache...",
            path.display()
        );
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("could not create path to cache file")?;
        }
        save_cache(path, &Cache::default())?;
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
            save_cache(path, &Cache::default())?;
            load_cache(path)
        }
    }
}

fn save_cache(path: &Path, cache: &Cache) -> anyhow::Result<()> {
    log::debug!("Saving cache to {}...", path.display());
    fs::write(
        path,
        serde_json::to_string_pretty(cache).context("could not serialize cache")?,
    )
    .context("could not write to cache file")?;
    log::trace!("Saved cache: {cache:#?}");

    Ok(())
}

/// Refresh cache with fresh data from Asana.
async fn refresh_cache(
    cache: &mut Cache,
    cache_path: &Path,
    client: &mut Client,
    user_task_list_gid: &str,
) -> anyhow::Result<()> {
    log::info!("Refreshing cache...");
    let tasks = client
        .get::<UserTask>(&user_task_list_gid.to_string())
        .await?;
    let focus_day = get_focus_day(Local::now().date_naive(), client).await?;

    cache.tasks = Some(tasks);
    cache.focus_day = Some(focus_day);
    cache.last_updated = Some(Local::now());
    save_cache(cache_path, cache)?;
    Ok(())
}

fn load_config(path: &Path) -> anyhow::Result<Config> {
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

#[allow(clippy::too_many_lines)]
async fn get_focus_day(day: NaiveDate, client: &mut Client) -> anyhow::Result<FocusDay> {
    log::info!("Getting focus sections...");
    let sections = client
        .get::<Section>(&ASANA_FOCUS_PROJECT_GID.to_string())
        .await?;
    log::debug!("Got {} sections", sections.len());
    log::trace!("Sections: {sections:#?}");

    log::info!("Constructing focus weeks...");
    let focus_weeks = sections
        .into_iter()
        .filter(|s| s.name.starts_with("Daily Focuses"))
        .filter_map(|s| match s.try_into() {
            Ok(s) => Some(s),
            Err(err) => {
                log::warn!("Could not parse focus section name: {err}");
                None
            }
        })
        .collect::<Vec<FocusWeek>>();
    log::debug!("Constructed {} focus weeks", focus_weeks.len());
    log::trace!("Focus weeks: {focus_weeks:#?}");

    log::info!("Finding current focus week...");
    let current_week =
        if let Some(current_week) = focus_weeks.iter().find(|w| w.from <= day && w.to >= day) {
            log::debug!("Found current focus week: {current_week}");
            current_week.clone()
        } else {
            log::warn!("Could not find current focus week, so creating it...");
            let week = day.week(Weekday::Mon);
            let current_week: FocusWeek = client
                .mutate_request(
                    Method::POST,
                    &format!(
                        "https://app.asana.com/api/1.0/projects/{ASANA_FOCUS_PROJECT_GID}/sections"
                    )
                    .parse()
                    .context("issue parsing focus week creation request url")?,
                    DataWrapper {
                        data: CreateSectionRequest {
                            name: format!(
                                "Daily Focuses ({from} to {to})",
                                from = week.first_day().format("%Y-%m-%d"),
                                to = week.last_day().format("%Y-%m-%d")
                            ),
                            insert_before: focus_weeks
                                .first()
                                .context("unable to get any focus weeks")?
                                .section
                                .gid
                                .clone(),
                        },
                    },
                )
                .await
                .context("issue creating focus week")?
                .json::<DataWrapper<Section>>()
                .await
                .context("unable to parse focus week creation response")?
                .data
                .try_into()?;
            log::debug!("Created current focus week: {current_week}");
            current_week
        };
    log::debug!("Got current focus week: {current_week}");

    log::info!("Getting tasks in current focus week...");
    let tasks = client.get::<FocusTask>(&current_week.section.gid).await?;
    log::debug!("Got {} tasks", tasks.len());

    log::info!("Constructing focus days...");
    let focus_days = tasks
        .into_iter()
        .filter(|t| t.name.starts_with("Daily Focus for"))
        .filter_map(|t| match t.try_into() {
            Ok(t) => Some(t),
            Err(err) => {
                log::warn!("Could not parse focus task name: {err}");
                None
            }
        })
        .collect::<Vec<FocusDay>>();
    log::debug!("Constructed {} focus days", focus_days.len());
    log::trace!("Focus days: {focus_days:#?}");

    log::info!("Finding current focus day...");
    let current_day = if let Some(current_day) = focus_days.iter().find(|d| d.date == day) {
        log::debug!("Found current focus day: {current_day}");
        current_day.clone()
    } else {
        log::warn!("Could not find current focus day, so creating it...");
        let current_day: FocusDay = client
            .mutate_request(
                Method::POST,
                &"https://app.asana.com/api/1.0/tasks"
                    .to_string()
                    .parse()
                    .context("issue parsing focus day creation request url")?,
                DataWrapper {
                    data: CreateSectionTaskRequest {
                        name: format!(
                            "Daily Focus for {day} ({date})",
                            day = day.weekday(),
                            date = day.format("%Y-%m-%d")
                        ),
                        projects: vec![ASANA_FOCUS_PROJECT_GID.to_string()],
                        memberships: vec![CreateSectionTaskRequestMembership {
                            project: ASANA_FOCUS_PROJECT_GID.to_string(),
                            section: current_week.section.gid.clone(),
                        }],
                    },
                },
            )
            .await
            .context("issue creating focus day")?
            .json::<DataWrapper<FocusTask>>()
            .await
            .context("unable to parse focus day creation response")?
            .data
            .try_into()?;
        log::debug!("Created current focus day: {current_day}");

        if let Some(previous_closest_day) = focus_days
            .iter()
            .filter(|d| d.date < day)
            .max_by_key(|d| d.date)
        {
            log::debug!("Ordering the created focus day correctly...");
            client
                .mutate_request(
                    Method::POST,
                    &format!(
                        "https://app.asana.com/api/1.0/sections/{section_gid}/addTask",
                        section_gid = current_week.section.gid
                    )
                    .parse()
                    .context("issue parsing focus day ordering request url")?,
                    DataWrapper {
                        data: AddTaskToSectionRequest {
                            task: current_day.task.gid.clone(),
                            insert_after: previous_closest_day.task.gid.clone(),
                        },
                    },
                )
                .await
                .context("issue ordering focus day")?;
        }

        current_day
    };
    log::debug!("Got current focus day: {current_day}");

    Ok(current_day)
}

fn task_or_tasks(num: usize) -> String {
    if num == 1 {
        "1 task".to_string()
    } else {
        format!("{num} tasks")
    }
}

#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_panic!();
    env_logger::init();

    let term = Term::stdout();

    log::debug!("Parsing command line arguments...");
    let args = Args::parse();
    log::trace!("Parsed command line arguments: {args:#?}");

    let cache_path = expand_homedir(&args.cache_path)?;
    let config_path = expand_homedir(&args.config_path)?;

    let mut cache = load_cache(&cache_path)?;
    let config = load_config(&config_path)?;

    // Skip cache warnings for machine-readable commands
    let is_machine_readable = matches!(args.command, Command::Status { .. });

    if args.use_cache && !is_machine_readable {
        log::debug!("Using cache, ensuring that we've updated recently...");
        if let Some(last_updated) = cache.last_updated {
            log::debug!("Cache last updated at {last_updated}, checking if we should update...");
            if Local::now() - last_updated < chrono::Duration::minutes(3) {
                log::debug!("Cache is recent enough, we're good.");
            } else {
                log::warn!("Cache is not recent enough, letting the user know...");
                eprintln!(
                    "{}",
                    style("Warning: cache has not been updated in more than 3 minutes, is the update command in the background? See the README.md")
                        .red()
                );
            }
        } else {
            log::warn!("Cache has never been updated, letting the user know...");
            eprintln!(
                "{}",
                style(
                    "Warning: cache has never been updated, is caching working? See the README.md",
                )
                .red()
            );
        }
    }

    let creds = if args.use_pat {
        if let Some(Credentials::PersonalAccessToken(pat)) = &cache.creds {
            Credentials::PersonalAccessToken(pat.clone())
        } else {
            let creds = ask_for_pat()?;
            cache.creds = Some(creds.clone());
            save_cache(&cache_path, &cache)?;
            creds
        }
    } else if let Some(Credentials::OAuth2 {
        access_token,
        refresh_token,
    }) = &cache.creds
    {
        Credentials::OAuth2 {
            access_token: access_token.clone(),
            refresh_token: refresh_token.clone(),
        }
    } else {
        let creds = execute_authorization_flow().await?;
        cache.creds = Some(creds.clone());
        save_cache(&cache_path, &cache)?;
        creds
    };

    let mut client = Client::new(creds)?;

    log::info!("Getting user task list..");
    let user_task_list =
        if let (Some(user_task_list), true) = (cache.user_task_list.clone(), args.use_cache) {
            log::debug!("Using cached user task list...");
            user_task_list
        } else {
            let user_task_list = client.get::<UserTaskList>(&"me".to_string()).await?;
            log::debug!("Saving new user task list to cache...");
            cache.user_task_list = Some(user_task_list.clone());
            save_cache(&cache_path, &cache)?;
            user_task_list
        };
    log::debug!("Got user task list: {user_task_list:#?}");

    log::info!("Getting tasks...");
    let tasks = if let (Some(tasks), true) = (cache.tasks.clone(), args.use_cache) {
        log::debug!("Using cached tasks...");
        tasks
    } else {
        log::debug!("Getting tasks from Asana...");
        let tasks = client.get::<UserTask>(&user_task_list.gid.clone()).await?;

        log::debug!("Saving new tasks to cache...");
        cache.tasks = Some(tasks.clone());
        save_cache(&cache_path, &cache)?;
        tasks
    };
    log::debug!("Got {} tasks", tasks.len());
    log::trace!("Tasks: {tasks:#?}");

    let now = Local::now();
    let today = now.date_naive();

    log::info!("Grouping tasks...");
    let overdue_tasks = {
        let mut tasks: Vec<_> = tasks
            .iter()
            .filter(|t| t.due_on.is_some_and(|d| d < today))
            .collect();
        tasks.sort_by_key(|t| t.due_on.unwrap());
        tasks
    };
    let due_today_tasks = {
        let mut tasks: Vec<_> = tasks
            .iter()
            .filter(|t| t.due_on.is_some_and(|d| d == today))
            .collect();
        tasks.sort_by_key(|t| t.due_on.unwrap());
        tasks
    };
    let due_week_tasks = {
        let mut tasks: Vec<_> = tasks
            .iter()
            .filter(|t| {
                t.due_on.is_some_and(|d| {
                    d > today && d <= today.checked_add_days(Days::new(7)).unwrap()
                })
            })
            .collect();
        tasks.sort_by_key(|t| t.due_on.unwrap());
        tasks
    };
    log::debug!(
        "Grouped tasks: {overdue_tasks} overdue, {due_today_tasks} due today, {due_week_tasks} due this week",
        overdue_tasks = overdue_tasks.len(),
        due_today_tasks = due_today_tasks.len(),
        due_week_tasks = due_week_tasks.len()
    );

    match args.command {
        Command::Summary => {
            log::info!("Producing a summary of tasks...");
            let mut task_summary = String::new();
            task_summary.push_str(&match (overdue_tasks.len(), due_today_tasks.len()) {
                (0, 0) => style("Nice! Everything done for now!")
                    .green()
                    .bold()
                    .to_string(),
                (o, 0) => style(format!("You have {} overdue.", task_or_tasks(o)))
                    .red()
                    .bold()
                    .to_string(),
                (0, t) => style(format!("You have {} due today.", task_or_tasks(t)))
                    .yellow()
                    .bold()
                    .to_string(),
                (o, t) => style(format!(
                    "You have {} overdue or due today",
                    task_or_tasks(o + t)
                ))
                .red()
                .bold()
                .to_string(),
            });

            task_summary.push_str(&match due_week_tasks.len() {
                0 => String::new(),
                w => style(format!(
                    " You have another {} due within a week.",
                    task_or_tasks(w)
                ))
                .blue()
                .to_string(),
            });

            term.write_line(&format!(
                "{task_summary} {}",
                style(format!(
                    "(https://app.asana.com/0/{user_task_list_gid}/list)",
                    user_task_list_gid = user_task_list.gid
                ))
                .dim()
            ))?;

            log::info!("Checking for focus...");
            let focus_day = if let (Some(focus_day), true) = (&cache.focus_day, args.use_cache) {
                focus_day.clone()
            } else {
                log::info!("No focus day in cache, fetching from Asana...");
                get_focus_day(today, &mut client).await?
            };

            if focus_day.date == today {
                let missing_morning = focus_day.stats.sleep.value().is_none()
                    || focus_day.stats.energy.value().is_none();
                let missing_evening = now.hour() >= START_HOUR_FOR_EOD
                    && focus_day.stats.stats().iter().any(|s| match s {
                        FocusDayStat::Sleep(_) | FocusDayStat::Energy(_) => false,
                        _ => s.value().is_none(),
                    });

                if missing_morning || missing_evening {
                    let focus_message = if missing_morning && missing_evening {
                        "Don't forget your focus for the day!"
                    } else if missing_morning {
                        "Time for your morning reflection."
                    } else {
                        "Time for your evening reflection."
                    };

                    if config.terminal.blocking {
                        // Blocking mode: require user to acknowledge
                        use dialoguer::Select;

                        println!();
                        println!("{}", style(format!("⚠️  {focus_message}")).yellow().bold());
                        println!();

                        let choices = [
                            (BlockingChoice::RunFocus, "Run focus now"),
                            (BlockingChoice::Skip, "Skip for now"),
                        ];

                        let selection = Select::with_theme(&ColorfulTheme::default())
                            .with_prompt("What would you like to do?")
                            .items(choices.map(|(_, s)| s))
                            .default(0)
                            .interact()?;

                        if matches!(choices[selection].0, BlockingChoice::RunFocus) {
                            // User chose to run focus - we'll handle this by telling them to run the command
                            // (We can't easily re-enter the Focus command flow from here)
                            println!();
                            println!("{}", style("Great! Running `todo focus`...").green());
                            println!();

                            // Execute focus flow inline
                            let date = today;
                            let force_eod = false;

                            log::info!("Running focus from blocking prompt...");

                            log::debug!("Calculating unfilled stats...");
                            let focus_day_for_run = focus_day.clone();
                            let unfilled_stats_at_this_time: Vec<&FocusDayStat> = focus_day_for_run
                                .stats
                                .stats()
                                .into_iter()
                                .filter(|s| match s {
                                    FocusDayStat::Sleep(_) | FocusDayStat::Energy(_) => {
                                        s.value().is_none()
                                    }
                                    _ => {
                                        s.value().is_none()
                                            && (force_eod
                                                || date < today
                                                || now.hour() >= START_HOUR_FOR_EOD)
                                    }
                                })
                                .collect::<Vec<_>>();

                            let mut new_stats = focus_day_for_run.stats.clone();
                            if unfilled_stats_at_this_time.is_empty() {
                                println!("{}\n", style("All caught up on stats!").bold().green());
                            } else {
                                log::info!("Updating focus day stats...");
                                println!("{}", style("Time to fill out some stats!").bold().cyan());
                                for stat in unfilled_stats_at_this_time {
                                    let mut new_stat = stat.clone();
                                    let value = Input::<u32>::with_theme(&ColorfulTheme::default())
                                        .with_prompt(format!(
                                            "{} {}",
                                            stat.name(),
                                            style("(0-9)").dim()
                                        ))
                                        .validate_with(|i: &u32| {
                                            if *i > 9 {
                                                Err("value must be between 0 and 9".to_string())
                                            } else {
                                                Ok(())
                                            }
                                        })
                                        .interact_text()?;
                                    new_stat.set_value(Some(value));
                                    new_stats.set_stat(new_stat);
                                }
                                println!();
                            }

                            // Sync the stats
                            if new_stats != focus_day_for_run.stats {
                                term.write_str(&style("Syncing focus data...").dim().to_string())?;
                                let url: Url = format!(
                                    "https://app.asana.com/api/1.0/tasks/{task_gid}",
                                    task_gid = focus_day_for_run.task.gid
                                )
                                .parse()
                                .context("issue parsing focus day update request url")?;

                                let custom_fields: HashMap<String, u32> = new_stats
                                    .stats()
                                    .into_iter()
                                    .filter_map(|s| {
                                        s.value().map(|v| (s.field_gid().to_string(), v))
                                    })
                                    .collect();

                                client
                                    .mutate_request(
                                        Method::PUT,
                                        &url,
                                        DataWrapper {
                                            data: UpdateFocusTaskCustomFieldsRequest {
                                                notes: focus_day_for_run.diary.clone(),
                                                custom_fields,
                                            },
                                        },
                                    )
                                    .await?;
                                term.clear_line()?;
                                println!("{}", style("Focus data synced!").green());
                            }
                        }
                    } else {
                        // Non-blocking mode: just show the message
                        term.write_line(&format!(
                            "{} {}",
                            style(focus_message).yellow(),
                            style("(run `todo focus` to fill out focus data)").dim()
                        ))?;
                    }
                }
            }
        }

        Command::List => {
            log::info!("Producing a list of tasks...");
            let mut string = String::new();

            if !overdue_tasks.is_empty() {
                let _ = writeln!(
                    string,
                    "{} {}",
                    style(task_or_tasks(overdue_tasks.len())).red().bold(),
                    style("overdue:").bold()
                );
                for task in overdue_tasks {
                    let _ = writeln!(
                        string,
                        "- ({}) {}",
                        style(task.due_on.unwrap().to_string()).red(),
                        task.name
                    );
                }
                string.push('\n');
            }

            if !due_today_tasks.is_empty() {
                let _ = writeln!(
                    string,
                    "{} {}",
                    style(task_or_tasks(due_today_tasks.len())).yellow(),
                    style("due today:").bold()
                );
                for task in due_today_tasks {
                    let _ = writeln!(string, "- {}", task.name);
                }
                string.push('\n');
            }

            if !due_week_tasks.is_empty() {
                let _ = writeln!(
                    string,
                    "{} {}",
                    style(task_or_tasks(due_week_tasks.len())).blue(),
                    style("due within a week:").bold()
                );
                for task in due_week_tasks {
                    let _ = writeln!(
                        string,
                        "- ({}) {}",
                        style(task.due_on.unwrap().to_string()).blue(),
                        task.name
                    );
                }
            }

            if string.is_empty() {
                string.push_str(
                    &style("Nice! Everything done for now!")
                        .green()
                        .bold()
                        .to_string(),
                );
            } else {
                println!("{}", string.trim());
            }
        }

        Command::Focus {
            date,
            force_eod,
            command,
        } => {
            log::info!("Managing focus...");

            let date = if let Some(date) = date {
                log::info!("Using date from command line: {date}");
                date
            } else {
                log::info!("Using today's date: {today}");
                today
            };

            term.write_str(&style("Loading focus day...").dim().to_string())?;
            let mut focus_day = get_focus_day(date, &mut client).await?;
            term.clear_line()?;

            match command {
                Some(FocusCommand::Run) | None => {
                    log::info!("Running focus...");

                    log::debug!("Calculating unfilled stats...");
                    let unfilled_stats_at_this_time: Vec<&FocusDayStat> = focus_day
                        .stats
                        .stats()
                        .into_iter()
                        .filter(|s| match s {
                            FocusDayStat::Sleep(_) | FocusDayStat::Energy(_) => s.value().is_none(),
                            _ => {
                                s.value().is_none()
                                    && (force_eod
                                        || date < today
                                        || now.hour() >= START_HOUR_FOR_EOD)
                            }
                        })
                        .collect::<Vec<_>>();
                    log::trace!("Calculated unfilled stats: {unfilled_stats_at_this_time:#?}");

                    let mut new_stats = focus_day.stats.clone();
                    if unfilled_stats_at_this_time.is_empty() {
                        println!("{}\n", style("All caught up on stats!").bold().green());
                    } else {
                        log::info!("Updating focus day stats...");
                        println!("{}", style("Time to fill out some stats!").bold().cyan());
                        for stat in unfilled_stats_at_this_time {
                            let mut new_stat = stat.clone();
                            let value = Input::<u32>::with_theme(&ColorfulTheme::default())
                                .with_prompt(format!("{} {}", stat.name(), style("(0-9)").dim()))
                                .validate_with(|i: &u32| {
                                    if *i > 9 {
                                        Err("value must be between 0 and 9".to_string())
                                    } else {
                                        Ok(())
                                    }
                                })
                                .interact_text()?;
                            new_stat.set_value(Some(value));
                            new_stats.set_stat(new_stat);
                        }
                        println!();
                        log::debug!("Updated focus day stats: {new_stats:#?}");
                    }

                    log::info!("Updating focus day diary...");
                    println!("{}", style("Have anything to say?").bold().magenta());
                    let new_diary_entry = Input::<String>::with_theme(&ColorfulTheme::default())
                        .with_prompt("diary")
                        .with_initial_text(focus_day.diary.clone())
                        .allow_empty(true)
                        .interact_text()?;
                    log::debug!("Updated focus day diary: {new_diary_entry}");
                    println!();

                    let sync_task = tokio::spawn({
                        let client = client.clone();
                        let focus_day = focus_day.clone();
                        let url: Url = format!(
                            "https://app.asana.com/api/1.0/tasks/{task_gid}",
                            task_gid = focus_day.task.gid
                        )
                        .parse()
                        .context("issue parsing focus day update request url")?;
                        let custom_fields = new_stats
                            .stats()
                            .into_iter()
                            .filter_map(|s| {
                                if s.value().is_some() {
                                    Some((s.field_gid().to_string(), s.value().unwrap()))
                                } else {
                                    None
                                }
                            })
                            .collect();

                        async move {
                            log::info!(
                                "Deciding if there are any changes to focus data to sync..."
                            );
                            if new_stats == focus_day.stats && new_diary_entry == focus_day.diary {
                                log::info!("No changes to focus data to sync");
                                return Ok::<bool, anyhow::Error>(false);
                            }

                            log::info!("Sending new focus data...");
                            client
                                .mutate_request(
                                    Method::PUT,
                                    &url,
                                    DataWrapper {
                                        data: UpdateFocusTaskCustomFieldsRequest {
                                            notes: new_diary_entry,
                                            custom_fields,
                                        },
                                    },
                                )
                                .await?;
                            log::debug!("Sent new focus data");
                            Ok(true)
                        }
                    });

                    log::info!("Loading subtasks for the focus day...");
                    term.write_str(&style("Loading subtasks...").dim().to_string())?;
                    focus_day.load_subtasks(&mut client).await?;
                    term.clear_line()?;
                    log::debug!(
                        "Loaded {} subtasks",
                        focus_day.subtasks.as_ref().map_or(0, Vec::len)
                    );

                    let mut subtasks = focus_day.subtasks.clone().unwrap_or_default();

                    log::info!("Asking for tasks to add to focus day...");
                    println!("{}", style("Any tasks to do today?").bold().red());
                    let mut subtask_tasks: Vec<tokio::task::JoinHandle<anyhow::Result<()>>> =
                        Vec::new();
                    let task_gid = focus_day.task.gid.clone();
                    loop {
                        for subtask in &subtasks {
                            println!("- {}", subtask.name);
                        }

                        let subtask_name = Input::<String>::with_theme(&ColorfulTheme::default())
                            .with_prompt("new task")
                            .allow_empty(true)
                            .interact_text()?;
                        if subtask_name.is_empty() {
                            break;
                        }

                        subtasks.push(FocusTaskSubtask {
                            gid: "new".to_string(),
                            name: subtask_name.clone(),
                            completed: false,
                        });

                        let subtask_task = tokio::spawn({
                            let client = client.clone();
                            let task_gid = task_gid.clone();
                            let url: Url =
                                format!("https://app.asana.com/api/1.0/tasks/{task_gid}/subtasks")
                                    .parse()
                                    .context("issue parsing subtask creation request url")?;

                            async move {
                                log::info!("Creating subtask...");
                                client
                                    .mutate_request(
                                        Method::POST,
                                        &url,
                                        DataWrapper {
                                            data: CreateSubtaskRequest {
                                                name: subtask_name,
                                                assignee: "me".to_string(),
                                                due_on: Some(today),
                                            },
                                        },
                                    )
                                    .await?;
                                log::debug!("Created subtask");
                                Ok::<(), anyhow::Error>(())
                            }
                        });
                        subtask_tasks.push(subtask_task);

                        term.clear_last_lines(subtasks.len())?;
                    }

                    if !sync_task.is_finished() {
                        term.write_str(
                            &style("Waiting for focus data to sync...").dim().to_string(),
                        )?;
                        sync_task.await??;
                        term.clear_line()?;
                    }
                    if subtask_tasks.iter().any(|t| !t.is_finished()) {
                        term.write_str(
                            &style("Waiting for subtasks to sync...").dim().to_string(),
                        )?;
                        for res in join_all(subtask_tasks).await {
                            res??;
                        }
                        term.clear_line()?;
                    }
                }
                Some(FocusCommand::Overview) => {
                    print!(
                        "{}",
                        get_focus_day(date, &mut client).await?.to_full_string()
                    );
                }
            }
        }

        Command::Update => {
            refresh_cache(&mut cache, &cache_path, &mut client, &user_task_list.gid).await?;
        }

        Command::Status { format, color } => {
            log::info!("Generating status output...");

            // Get focus day from cache or fetch
            let focus_day = if let (Some(focus_day), true) = (&cache.focus_day, args.use_cache) {
                focus_day.clone()
            } else {
                get_focus_day(today, &mut client).await?
            };

            let status =
                FocusStatus::new(&focus_day, now, overdue_tasks.len(), due_today_tasks.len());

            match format {
                StatusFormat::Short => {
                    if config.tmux.enabled {
                        print!("{}", status.to_short_string(&color));
                    }
                }
                StatusFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string(&status).context("failed to serialize status")?
                    );
                }
                StatusFormat::Xbar => {
                    print!("{}", status.to_xbar_string(&config));
                }
            }
        }

        Command::Install { integration } => {
            match integration {
                InstallIntegration::Show => {
                    println!("{}", style("Available integrations:").bold());
                    println!();

                    println!("  {}", style("zsh").cyan());
                    println!("    Show focus status in your shell prompt");
                    println!("    Run: todo install zsh");
                    println!();

                    println!(
                        "  {} - {}",
                        style("tmux").cyan(),
                        if config.tmux.enabled {
                            style("enabled").green()
                        } else {
                            style("disabled").dim()
                        }
                    );
                    println!("    Status bar integration (standard + Dracula theme)");
                    println!("    Run: todo install tmux");
                    println!();

                    println!(
                        "  {} - {}",
                        style("xbar").cyan(),
                        if config.menubar.enabled {
                            style("enabled").green()
                        } else {
                            style("disabled").dim()
                        }
                    );
                    println!("    macOS menu bar widget (requires xbar/SwiftBar)");
                    println!("    Run: todo install xbar");
                    println!();

                    println!(
                        "  {} - {}",
                        style("notifications").cyan(),
                        if config.notifications.enabled {
                            style("enabled").green()
                        } else {
                            style("disabled").dim()
                        }
                    );
                    println!(
                        "    Scheduled notifications at {} and {}",
                        config.notifications.morning_time, config.notifications.evening_time
                    );
                    println!("    Run: todo install notifications");
                    println!();

                    println!(
                        "  {} - {}",
                        style("terminal blocking").cyan(),
                        if config.terminal.blocking {
                            style("enabled").green()
                        } else {
                            style("disabled").dim()
                        }
                    );
                    println!("    Block new terminal sessions until focus is acknowledged");
                    println!(
                        "    Set terminal.blocking = true in {}",
                        args.config_path.display()
                    );
                }

                InstallIntegration::Zsh => {
                    println!("{}", style("Zsh Prompt Integration").bold().cyan());
                    println!();
                    println!("Add this to your ~/.zshrc:");
                    println!();
                    println!(
                        "{}",
                        style(
                            r"# Todo focus status in prompt
export TODO_PROMPT='%F{magenta}$(todo --use-cache status --format short)%f'"
                        )
                        .dim()
                    );
                    println!();
                    println!(
                        "Then add {} to your PROMPT, for example:",
                        style("${TODO_PROMPT}").cyan()
                    );
                    println!(
                        "{}",
                        style(r#"export PROMPT="${TODO_PROMPT} ${PROMPT}""#).dim()
                    );
                    println!();
                    println!("{}", style("Status format:").bold());
                    println!("  focus:am  = morning focus pending");
                    println!("  focus:pm  = evening focus pending");
                    println!("  !N        = N overdue tasks");
                    println!("  +N        = N tasks due today");
                    println!("  ✓         = all clear");
                }

                InstallIntegration::Tmux => {
                    println!("{}", style("tmux Integration").bold().cyan());
                    println!();
                    println!("{}", style("Option 1: Standard tmux").bold());
                    println!("Add to ~/.tmux.conf:");
                    println!(
                        "{}",
                        style(
                            r"set -g status-right '#(todo --use-cache status --format short --color tmux) | %H:%M'"
                        )
                        .dim()
                    );
                    println!();
                    println!("{}", style("Option 2: Dracula theme").bold());
                    println!();
                    println!("1. Create the script:");
                    println!(
                        "{}",
                        style("   mkdir -p ~/.tmux/plugins/tmux/scripts").dim()
                    );
                    println!(
                        "{}",
                        style(
                            r"   echo '#!/bin/bash
todo --use-cache status --format short --color never' > ~/.tmux/plugins/tmux/scripts/todo.sh"
                        )
                        .dim()
                    );
                    println!(
                        "{}",
                        style("   chmod +x ~/.tmux/plugins/tmux/scripts/todo.sh").dim()
                    );
                    println!();
                    println!("2. Add to your @dracula-plugins in ~/.tmux.conf:");
                    println!(
                        "{}",
                        style(r#"   set -g @dracula-plugins "custom:todo.sh git cpu-usage ...""#)
                            .dim()
                    );
                    println!();
                    println!(
                        "3. Reload: {}",
                        style("tmux source-file ~/.tmux.conf").dim()
                    );
                    println!();
                    println!("{}", style("Status format:").bold());
                    println!("  focus:am = morning focus pending");
                    println!("  focus:pm = evening focus pending");
                    println!("  !N       = N overdue tasks");
                    println!("  +N       = N due today");
                    println!("  ✓        = all clear");
                }

                InstallIntegration::Xbar => {
                    #[cfg(target_os = "macos")]
                    {
                        let plugin_dir = expand_homedir(Path::new(
                            "~/Library/Application Support/xbar/plugins",
                        ))?;
                        let plugin_path =
                            plugin_dir.join(format!("todo.{}s.sh", config.menubar.refresh_seconds));

                        if !plugin_dir.exists() {
                            println!(
                                "{}",
                                style("xbar/SwiftBar not found. Install it from:").yellow()
                            );
                            println!("  https://xbarapp.com/ or https://swiftbar.app/");
                            return Ok(());
                        }

                        let script = format!(
                            r#"#!/bin/bash
# Todo Focus Status for xbar/SwiftBar
# Refresh every {} seconds

todo --use-cache status --format xbar
"#,
                            config.menubar.refresh_seconds
                        );

                        fs::write(&plugin_path, script)?;
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            fs::set_permissions(&plugin_path, fs::Permissions::from_mode(0o755))?;
                        }

                        println!("{}", style("xbar plugin installed!").green().bold());
                        println!("Plugin location: {}", plugin_path.display());
                        println!();
                        println!("Restart xbar/SwiftBar to load the plugin.");
                    }

                    #[cfg(not(target_os = "macos"))]
                    {
                        println!(
                            "{}",
                            style("xbar/SwiftBar is only available on macOS.").yellow()
                        );
                    }
                }

                InstallIntegration::Notifications => {
                    #[cfg(target_os = "macos")]
                    {
                        println!("{}", style("macOS Notifications").bold().cyan());
                        println!();
                        println!(
                            "This will create launchd agents for morning ({}) and evening ({}) reminders.",
                            config.notifications.morning_time, config.notifications.evening_time
                        );
                        println!();

                        let launch_agents_dir =
                            expand_homedir(Path::new("~/Library/LaunchAgents"))?;
                        fs::create_dir_all(&launch_agents_dir)?;

                        let (morning_hour, morning_minute) =
                            parse_time_string(&config.notifications.morning_time);
                        let (evening_hour, evening_minute) =
                            parse_time_string(&config.notifications.evening_time);

                        // Morning plist
                        let morning_plist = format!(
                            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.todo.morning-reminder</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/bin/osascript</string>
        <string>-e</string>
        <string>display notification "Time for your morning focus!" with title "Todo" sound name "default"</string>
    </array>
    <key>StartCalendarInterval</key>
    <dict>
        <key>Hour</key>
        <integer>{morning_hour}</integer>
        <key>Minute</key>
        <integer>{morning_minute}</integer>
    </dict>
</dict>
</plist>"#
                        );

                        // Evening plist
                        let evening_plist = format!(
                            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.todo.evening-reminder</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/bin/osascript</string>
        <string>-e</string>
        <string>display notification "Time for your evening reflection!" with title "Todo" sound name "default"</string>
    </array>
    <key>StartCalendarInterval</key>
    <dict>
        <key>Hour</key>
        <integer>{evening_hour}</integer>
        <key>Minute</key>
        <integer>{evening_minute}</integer>
    </dict>
</dict>
</plist>"#
                        );

                        let morning_path =
                            launch_agents_dir.join("com.todo.morning-reminder.plist");
                        let evening_path =
                            launch_agents_dir.join("com.todo.evening-reminder.plist");

                        fs::write(&morning_path, morning_plist)?;
                        fs::write(&evening_path, evening_plist)?;

                        println!("{}", style("Notification agents installed!").green().bold());
                        println!();
                        println!("Load them with:");
                        println!(
                            "{}",
                            style(format!("  launchctl load {}", morning_path.display())).dim()
                        );
                        println!(
                            "{}",
                            style(format!("  launchctl load {}", evening_path.display())).dim()
                        );
                        println!();
                        println!(
                            "To uninstall, use 'launchctl unload' and delete the plist files."
                        );
                    }

                    #[cfg(target_os = "linux")]
                    {
                        println!("{}", style("Linux Notifications").bold().cyan());
                        println!();
                        println!("For Linux, you can use systemd user timers or cron.");
                        println!();
                        println!("Example crontab entries (run 'crontab -e' to edit):");
                        println!();

                        let (morning_hour, morning_minute) =
                            parse_time_string(&config.notifications.morning_time);
                        let (evening_hour, evening_minute) =
                            parse_time_string(&config.notifications.evening_time);

                        println!(
                            "{}",
                            style(format!(
                                "{morning_minute} {morning_hour} * * * notify-send 'Todo' 'Time for your morning focus!'"
                            ))
                            .dim()
                        );
                        println!(
                            "{}",
                            style(format!(
                                "{evening_minute} {evening_hour} * * * notify-send 'Todo' 'Time for your evening reflection!'"
                            ))
                            .dim()
                        );
                        println!();
                        println!(
                            "{}",
                            style("Note: Requires 'libnotify' (notify-send) to be installed.")
                                .yellow()
                        );
                    }

                    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
                    {
                        println!(
                            "{}",
                            style("Notifications not yet supported on this platform.").yellow()
                        );
                    }
                }
            }
        }
    }

    if !args.use_cache {
        refresh_cache(&mut cache, &cache_path, &mut client, &user_task_list.gid).await?;
    }

    Ok(())
}
