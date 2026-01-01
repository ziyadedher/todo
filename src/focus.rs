//! Focus day types and related functionality.

use std::collections::HashMap;
use std::fmt::{Display, Write as _};

use anyhow::Context as _;
use chrono::{DateTime, Datelike, Local, NaiveDate, Timelike as _};
use console::style;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::asana::{Client, DataRequest};

/// Regex pattern for focus week section names.
pub const FOCUS_WEEK_PATTERN: &str =
    r"^Daily Focuses \((?<from>\d{4}-\d{2}-\d{2}) to (?<to>\d{4}-\d{2}-\d{2})\)$";

/// Regex pattern for focus day task names.
pub const FOCUS_DAY_PATTERN: &str = r"^Daily Focus for \w+ \((?<date>\d{4}-\d{2}-\d{2})\)$";

/// The hour of the day at which the end of day is considered to be starting.
pub const START_HOUR_FOR_EOD: u32 = 20;

/// A section in an Asana project.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Section {
    /// Section GID.
    pub gid: String,
    /// Section name.
    pub name: String,
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

/// Request to create a new section.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateSectionRequest {
    /// Section name.
    pub name: String,
    /// GID of section to insert before.
    pub insert_before: String,
}

/// A focus task in Asana.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FocusTask {
    /// Task GID.
    pub gid: String,
    /// Task name.
    pub name: String,
    /// Task notes/diary.
    pub notes: String,
    /// Custom fields (stats).
    pub custom_fields: Option<Vec<FocusTaskCustomField>>,
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

/// A custom field on a focus task.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FocusTaskCustomField {
    /// Field GID.
    pub gid: String,
    /// Field value.
    pub number_value: Option<u32>,
}

/// Request to update focus task custom fields.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdateFocusTaskCustomFieldsRequest {
    /// Updated notes.
    pub notes: String,
    /// Updated custom fields.
    pub custom_fields: HashMap<String, u32>,
}

/// Request to create a task in a section.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateSectionTaskRequest {
    /// Task name.
    pub name: String,
    /// Project GIDs.
    pub projects: Vec<String>,
    /// Section memberships.
    pub memberships: Vec<CreateSectionTaskRequestMembership>,
}

/// Section membership for task creation.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateSectionTaskRequestMembership {
    /// Project GID.
    pub project: String,
    /// Section GID.
    pub section: String,
}

/// Request to add a task to a section.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AddTaskToSectionRequest {
    /// Task GID.
    pub task: String,
    /// GID of task to insert after.
    pub insert_after: String,
}

/// A subtask of a focus task.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FocusTaskSubtask {
    /// Subtask GID.
    pub gid: String,
    /// Subtask name.
    pub name: String,
    /// Whether the subtask is completed.
    pub completed: bool,
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

/// Request to create a subtask.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateSubtaskRequest {
    /// Subtask name.
    pub name: String,
    /// Assignee.
    pub assignee: String,
    /// Due date.
    #[serde(with = "crate::asana::serde_formats::optional_date")]
    pub due_on: Option<NaiveDate>,
}

/// A week of focus days.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FocusWeek {
    /// The section representing this week.
    pub section: Section,
    /// Start date of the week.
    pub from: NaiveDate,
    /// End date of the week.
    pub to: NaiveDate,
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

/// A single focus day with stats and diary.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FocusDay {
    /// The underlying Asana task.
    pub task: FocusTask,
    /// Date of the focus day.
    pub date: NaiveDate,
    /// Focus stats.
    pub stats: FocusDayStats,
    /// Diary entry.
    pub diary: String,
    /// Subtasks for the day.
    pub subtasks: Option<Vec<FocusTaskSubtask>>,
}

impl FocusDay {
    /// Check if the morning routine is done for the given date.
    #[must_use]
    pub fn is_morning_done(&self) -> bool {
        self.stats.sleep.value().is_some() && self.stats.energy.value().is_some()
    }

    /// Check if the evening routine is done for the given date.
    #[must_use]
    pub fn is_evening_done(&self) -> bool {
        self.stats.stats().iter().all(|s| match s {
            FocusDayStat::Sleep(_) | FocusDayStat::Energy(_) => true,
            _ => s.value().is_some(),
        })
    }

    /// Render the focus day as a full string.
    #[must_use]
    pub fn to_full_string(&self) -> String {
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

    /// Load subtasks for this focus day.
    ///
    /// # Errors
    ///
    /// Returns an error if the Asana API request fails.
    ///
    /// # Panics
    ///
    /// This function does not panic under normal operation.
    pub async fn load_subtasks(
        &mut self,
        client: &mut Client,
    ) -> anyhow::Result<&[FocusTaskSubtask]> {
        let subtasks = client.get::<FocusTaskSubtask>(&self.task.gid).await?;
        self.subtasks = Some(subtasks);
        // SAFETY: We just set subtasks to Some above
        Ok(self.subtasks.as_ref().expect("subtasks should be set"))
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

/// Statistics for a focus day.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct FocusDayStats {
    /// Sleep quality.
    pub sleep: FocusDayStat,
    /// Energy level.
    pub energy: FocusDayStat,
    /// Flow state.
    pub flow: FocusDayStat,
    /// Hydration level.
    pub hydration: FocusDayStat,
    /// Health level.
    pub health: FocusDayStat,
    /// Satisfaction level.
    pub satisfaction: FocusDayStat,
    /// Stress level.
    pub stress: FocusDayStat,
}

impl FocusDayStats {
    /// Get all stats as a vector.
    #[must_use]
    pub fn stats(&self) -> Vec<&FocusDayStat> {
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

    /// Set a stat value.
    pub fn set_stat(&mut self, stat: FocusDayStat) {
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

/// A single focus day stat.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum FocusDayStat {
    /// Sleep quality (0-9).
    Sleep(Option<u32>),
    /// Energy level (0-9).
    Energy(Option<u32>),
    /// Flow state (0-9).
    Flow(Option<u32>),
    /// Hydration level (0-9).
    Hydration(Option<u32>),
    /// Health level (0-9).
    Health(Option<u32>),
    /// Satisfaction level (0-9).
    Satisfaction(Option<u32>),
    /// Stress level (0-9).
    Stress(Option<u32>),
}

impl FocusDayStat {
    /// Get the stat name.
    #[must_use]
    pub fn name(&self) -> &'static str {
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

    /// Get the stat value.
    #[must_use]
    pub fn value(&self) -> Option<u32> {
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

    /// Set the stat value.
    pub fn set_value(&mut self, value: Option<u32>) {
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

    /// Get the Asana field GID for this stat.
    #[must_use]
    pub fn field_gid(&self) -> &'static str {
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

/// Check if the current time is in the evening (after EOD start hour).
#[must_use]
pub fn is_evening(now: &DateTime<Local>) -> bool {
    now.hour() >= START_HOUR_FOR_EOD
}
