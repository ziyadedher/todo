//! Status command handler.

use crate::{
    config::Config,
    focus::{is_evening, FocusDay},
};
use anyhow::{Context as _, Result};
use chrono::{DateTime, Local};
use console::style;
use serde::Serialize;
use std::fmt::Write;

use crate::context::{AppContext, GroupedTasks};

use super::get_focus_day;

/// Represents the current focus status for integrations.
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    /// Whether morning reflection (sleep/energy) is complete.
    pub morning_done: bool,
    /// Whether evening reflection is complete.
    pub evening_done: bool,
    /// Whether it's currently evening time.
    pub is_evening: bool,
    /// Number of overdue tasks.
    pub overdue_count: usize,
    /// Number of tasks due today.
    pub due_today_count: usize,
}

impl Status {
    /// Create a new status from a focus day.
    #[must_use]
    pub fn new(
        focus_day: Option<&FocusDay>,
        now: DateTime<Local>,
        overdue_count: usize,
        due_today_count: usize,
    ) -> Self {
        let is_evening = is_evening(&now);

        let (morning_done, evening_done) = if let Some(focus_day) = focus_day {
            let today = now.date_naive();
            let morning = focus_day.date == today && focus_day.is_morning_done();
            let evening = focus_day.date == today && focus_day.is_evening_done();
            (morning, evening)
        } else {
            // No focus day, consider focus done (don't show focus prompts)
            (true, true)
        };

        Self {
            morning_done,
            evening_done,
            is_evening,
            overdue_count,
            due_today_count,
        }
    }

    /// Render as a short string for status bars.
    #[must_use]
    pub fn to_short_string(&self, force_styling: bool) -> String {
        let mut parts = Vec::new();

        if !self.morning_done {
            parts.push(
                style("focus:am")
                    .yellow()
                    .force_styling(force_styling)
                    .to_string(),
            );
        } else if self.is_evening && !self.evening_done {
            parts.push(
                style("focus:pm")
                    .yellow()
                    .force_styling(force_styling)
                    .to_string(),
            );
        }

        if self.overdue_count > 0 {
            parts.push(
                style(format!("!{}", self.overdue_count))
                    .red()
                    .force_styling(force_styling)
                    .to_string(),
            );
        }
        if self.due_today_count > 0 {
            parts.push(
                style(format!("+{}", self.due_today_count))
                    .yellow()
                    .force_styling(force_styling)
                    .to_string(),
            );
        }

        if parts.is_empty() {
            style("✓").green().force_styling(force_styling).to_string()
        } else {
            parts.join(" ")
        }
    }

    /// Render as xbar format.
    #[must_use]
    pub fn to_xbar_string(&self, config: &Config) -> String {
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

/// Status output format.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum StatusFormat {
    /// Short one-line format for shell prompts and status bars.
    Short,
    /// JSON format for programmatic use.
    Json,
    /// xbar/SwiftBar format for macOS menu bar.
    Xbar,
}

/// Run the status command.
///
/// # Errors
///
/// Returns an error if Asana API requests or JSON serialization fails.
pub async fn run(
    ctx: &mut AppContext,
    grouped: &GroupedTasks<'_>,
    format: &StatusFormat,
    force_styling: bool,
) -> Result<()> {
    log::info!("Generating status output...");

    // Get focus day from cache or fetch (if focus project is configured)
    let focus_day = if let Some(ref focus_project_gid) = ctx.config.focus_project_gid {
        if let (Some(focus_day), true) = (&ctx.cache.focus_day, ctx.use_cache) {
            Some(focus_day.clone())
        } else {
            Some(get_focus_day(ctx.now.date_naive(), &mut ctx.client, focus_project_gid).await?)
        }
    } else {
        None
    };

    let status = Status::new(
        focus_day.as_ref(),
        ctx.now,
        grouped.overdue.len(),
        grouped.due_today.len(),
    );

    match format {
        StatusFormat::Short => {
            if ctx.config.tmux.enabled {
                ctx.term.write_str(&status.to_short_string(force_styling))?;
            }
        }
        StatusFormat::Json => {
            ctx.term.write_line(
                &serde_json::to_string(&status).context("failed to serialize status")?,
            )?;
        }
        StatusFormat::Xbar => {
            ctx.term.write_str(&status.to_xbar_string(&ctx.config))?;
        }
    }

    Ok(())
}
