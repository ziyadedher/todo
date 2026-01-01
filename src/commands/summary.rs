//! Summary command handler.

use anyhow::Result;
use console::style;

use crate::context::{AppContext, GroupedTasks};
use crate::focus::is_evening;

use super::get_focus_day;

fn task_or_tasks(num: usize) -> String {
    if num == 1 {
        "1 task".to_string()
    } else {
        format!("{num} tasks")
    }
}

/// Run the summary command.
///
/// # Errors
///
/// Returns an error if Asana API requests or terminal I/O fails.
pub async fn run(ctx: &mut AppContext, grouped: &GroupedTasks<'_>) -> Result<()> {
    log::info!("Producing a summary of tasks...");

    let mut task_summary = String::new();
    task_summary.push_str(&match (grouped.overdue.len(), grouped.due_today.len()) {
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

    task_summary.push_str(&match grouped.due_this_week.len() {
        0 => String::new(),
        w => style(format!(
            " You have another {} due within a week.",
            task_or_tasks(w)
        ))
        .blue()
        .to_string(),
    });

    // Get user task list GID for the link
    let user_task_list_gid = ctx
        .cache
        .user_task_list
        .as_ref()
        .map_or("list", |u| u.gid.as_str());

    ctx.term.write_line(&format!(
        "{task_summary} {}",
        style(format!(
            "(https://app.asana.com/0/{user_task_list_gid}/list)"
        ))
        .dim()
    ))?;

    // Check focus status (only if focus project is configured)
    if let Some(ref focus_project_gid) = ctx.config.focus_project_gid {
        log::info!("Checking for focus...");
        let focus_day = if let (Some(focus_day), true) = (&ctx.cache.focus_day, ctx.use_cache) {
            focus_day.clone()
        } else {
            log::info!("No focus day in cache, fetching from Asana...");
            get_focus_day(ctx.now.date_naive(), &mut ctx.client, focus_project_gid).await?
        };

        if focus_day.date == ctx.now.date_naive() {
            let missing_morning = !focus_day.is_morning_done();
            let missing_evening = is_evening(&ctx.now) && !focus_day.is_evening_done();

            let focus_message = match (missing_morning, missing_evening) {
                (true, true) => Some("Don't forget your focus for the day!"),
                (true, false) => Some("Time for your morning reflection."),
                (false, true) => Some("Time for your evening reflection."),
                (false, false) => None,
            };

            if let Some(message) = focus_message {
                ctx.term.write_line(&format!(
                    "{} {}",
                    style(message).yellow(),
                    style("(run `todo focus` to fill out focus data)").dim()
                ))?;
            }
        }
    }

    Ok(())
}
