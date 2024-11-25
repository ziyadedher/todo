#![warn(clippy::pedantic)]

use std::{
    collections::HashMap,
    env,
    fmt::Display,
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
struct Config {}

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

        string.push_str(&format!(
            "🧠 {} {}",
            style(format!(
                "Focus Day: {}",
                style(self.date.weekday().to_string()).blue()
            ))
            .bold(),
            style(format!("({})", self.date.format("%Y-%m-%d"))).dim(),
        ));
        string.push_str(&format!(
            "\n\n{}",
            if self.diary.is_empty() {
                style("no diary entry — yet.").dim()
            } else {
                style(self.diary.as_str())
            },
        ));
        string.push_str(&format!("\n\n{}\n", style("❤️ Statistics").bold().cyan()));

        for stat in self.stats.stats() {
            let line = format!(
                "{name}: {value}",
                name = style(stat.name().to_string()).bold(),
                value = style(stat.value().map_or("-".to_string(), |v| v.to_string()))
            );
            string.push_str(&format!(
                "   {}\n",
                if stat.value().is_some() {
                    style(line)
                } else {
                    style(line).dim()
                }
            ));
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
            gid => anyhow::bail!("unknown focus day stat gid: {}", gid),
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
    log::trace!("Sections: {sections:#?}", sections = sections);

    log::info!("Constructing focus weeks...");
    let focus_weeks = sections
        .into_iter()
        .filter(|s| s.name.starts_with("Daily Focuses"))
        .filter_map(|s| match s.try_into() {
            Ok(s) => Some(s),
            Err(err) => {
                log::warn!("Could not parse focus section name: {}", err);
                None
            }
        })
        .collect::<Vec<FocusWeek>>();
    log::debug!("Constructed {} focus weeks", focus_weeks.len());
    log::trace!("Focus weeks: {focus_weeks:#?}", focus_weeks = focus_weeks);

    log::info!("Finding current focus week...");
    let current_week =
        if let Some(current_week) = focus_weeks.iter().find(|w| w.from <= day && w.to >= day) {
            log::debug!(
                "Found current focus week: {current_week}",
                current_week = current_week
            );
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
            log::debug!(
                "Created current focus week: {current_week}",
                current_week = current_week
            );
            current_week
        };
    log::debug!(
        "Got current focus week: {current_week}",
        current_week = current_week
    );

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
                log::warn!("Could not parse focus task name: {}", err);
                None
            }
        })
        .collect::<Vec<FocusDay>>();
    log::debug!("Constructed {} focus days", focus_days.len());
    log::trace!("Focus days: {focus_days:#?}", focus_days = focus_days);

    log::info!("Finding current focus day...");
    let current_day = if let Some(current_day) = focus_days.iter().find(|d| d.date == day) {
        log::debug!(
            "Found current focus day: {current_day}",
            current_day = current_day
        );
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
        log::debug!(
            "Created current focus day: {current_day}",
            current_day = current_day
        );

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
    log::debug!(
        "Got current focus day: {current_day}",
        current_day = current_day
    );

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
    let _config = load_config(&config_path)?;

    if args.use_cache {
        log::debug!("Using cache, ensuring that we've updated recently...");
        if let Some(last_updated) = cache.last_updated {
            log::debug!(
                "Cache last updated at {last_updated}, checking if we should update...",
                last_updated = last_updated
            );
            if Local::now() - last_updated < chrono::Duration::minutes(3) {
                log::debug!("Cache is recent enough, we're good.");
            } else {
                log::warn!("Cache is not recent enough, letting the user know...");
                term.write_line(
                    &style("Warning: cache has not been updated in more than 3 minutes, is the update command in the background? See the README.md")
                        .red()
                        .to_string(),
                )?;
            }
        } else {
            log::warn!("Cache has never been updated, letting the user know...");
            term.write_line(
                &style(
                    "Warning: cache has never been updated, is caching working? See the README.md",
                )
                .red()
                .to_string(),
            )?;
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
        let tasks = client
            .get::<UserTask>(&user_task_list.gid.to_string())
            .await?;

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
            if let Some(focus_day) = &cache.focus_day {
                if focus_day.date == today {
                    let missing_morning = focus_day.stats.sleep.value().is_none()
                        || focus_day.stats.energy.value().is_none();
                    let missing_evening = now.hour() >= START_HOUR_FOR_EOD
                        && focus_day.stats.stats().iter().any(|s| match s {
                            FocusDayStat::Sleep(_) | FocusDayStat::Energy(_) => false,
                            _ => s.value().is_none(),
                        });

                    if missing_morning || missing_evening {
                        let mut focus_summary = String::new();

                        if missing_morning && missing_evening {
                            focus_summary.push_str(
                                &style("Don't forget your focus for the day!")
                                    .yellow()
                                    .to_string(),
                            );
                        } else if missing_morning {
                            focus_summary.push_str(
                                &style("🌅 Don't forget to fill out your morning focus!")
                                    .yellow()
                                    .to_string(),
                            );
                        } else if missing_evening {
                            focus_summary.push_str(
                                &style("🌙 Time for your evening focus reflection!")
                                    .yellow()
                                    .to_string(),
                            );
                        }

                        term.write_line(&format!(
                            "{focus_summary} {}",
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
                string.push_str(&format!(
                    "{} {}\n",
                    style(task_or_tasks(overdue_tasks.len())).red().bold(),
                    style("overdue:").bold()
                ));
                for task in overdue_tasks {
                    string.push_str(&format!(
                        "- ({}) {}\n",
                        style(task.due_on.unwrap().to_string()).red(),
                        task.name
                    ));
                }
                string.push('\n');
            }

            if !due_today_tasks.is_empty() {
                string.push_str(&format!(
                    "{} {}\n",
                    style(task_or_tasks(due_today_tasks.len())).yellow(),
                    style("due today:").bold()
                ));
                for task in due_today_tasks {
                    string.push_str(&format!("- {}\n", task.name));
                }
                string.push('\n');
            }

            if !due_week_tasks.is_empty() {
                string.push_str(&format!(
                    "{} {}\n",
                    style(task_or_tasks(due_week_tasks.len())).blue(),
                    style("due within a week:").bold()
                ));
                for task in due_week_tasks {
                    string.push_str(&format!(
                        "- ({}) {}\n",
                        style(task.due_on.unwrap().to_string()).blue(),
                        task.name
                    ));
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
                log::info!("Using date from command line: {}", date);
                date
            } else {
                log::info!("Using today's date: {}", today);
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
                    log::trace!(
                        "Calculated unfilled stats: {unfilled_stats_at_this_time:#?}",
                        unfilled_stats_at_this_time = unfilled_stats_at_this_time
                    );

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
                        log::debug!(
                            "Updated focus day stats: {new_stats:#?}",
                            new_stats = new_stats
                        );
                    }

                    log::info!("Updating focus day diary...");
                    println!("{}", style("Have anything to say?").bold().magenta());
                    let new_diary_entry = Input::<String>::with_theme(&ColorfulTheme::default())
                        .with_prompt("diary")
                        .with_initial_text(focus_day.diary.clone())
                        .allow_empty(true)
                        .interact_text()?;
                    log::debug!(
                        "Updated focus day diary: {new_diary_entry}",
                        new_diary_entry = new_diary_entry
                    );
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
            log::info!("Updating cache...");
            let tasks = client
                .get::<UserTask>(&user_task_list.gid.to_string())
                .await?;
            let focus_day = get_focus_day(Local::now().date_naive(), &mut client).await?;

            cache.tasks = Some(tasks.clone());
            cache.focus_day = Some(focus_day);
            cache.last_updated = Some(Local::now());
            save_cache(&cache_path, &cache)?;
        }
    }

    Ok(())
}
