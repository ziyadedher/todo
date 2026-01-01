//! Complete command for marking tasks as done.

use anyhow::Context as _;
use chrono::Datelike as _;
use console::style;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use futures::future::join_all;
use reqwest::{Method, Url};
use serde::Serialize;
use tokio::task::JoinHandle;

use crate::asana::{Client, DataWrapper};
use crate::context::AppContext;
use crate::task::UserTask;

/// Request body for completing a task.
#[derive(Serialize)]
struct CompleteTaskRequest {
    completed: bool,
}

/// Run the complete command.
///
/// Shows a list of incomplete tasks and lets the user select tasks to mark as complete.
/// Completions happen in the background, allowing rapid selection of multiple tasks.
///
/// # Errors
///
/// Returns an error if the task cannot be completed or if there are no tasks.
///
/// # Panics
///
/// Panics if cached tasks don't have due dates (invariant enforced at cache time).
pub async fn run(ctx: &mut AppContext) -> anyhow::Result<()> {
    if ctx.use_cache {
        anyhow::bail!("Cannot complete tasks in cache-only mode. Run without --use-cache.");
    }

    let tasks = ctx
        .cache
        .tasks
        .clone()
        .context("No tasks found. Run 'todo update' first.")?;

    if tasks.is_empty() {
        ctx.term
            .write_line(&style("No incomplete tasks found!").green().to_string())?;
        return Ok(());
    }

    // Track which tasks have been completed (by index)
    let mut completed_indices: Vec<usize> = Vec::new();
    let mut completion_tasks: Vec<JoinHandle<anyhow::Result<()>>> = Vec::new();

    loop {
        // Build display list, excluding already-completed tasks
        let available: Vec<(usize, &UserTask)> = tasks
            .iter()
            .enumerate()
            .filter(|(i, _)| !completed_indices.contains(i))
            .collect();

        if available.is_empty() {
            ctx.term
                .write_line(&style("All tasks completed!").green().to_string())?;
            break;
        }
        // Sort by due date (earliest first, None at the end)
        let mut sorted_available: Vec<(usize, &UserTask)> = available;
        sorted_available.sort_by(|(_, a), (_, b)| match (a.due_on, b.due_on) {
            (Some(da), Some(db)) => da.cmp(&db),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });

        let today = chrono::Local::now().date_naive();
        let current_year = today.year();

        // FuzzySelect doesn't handle ANSI codes well, so use plain text
        let display_items: Vec<String> = sorted_available
            .iter()
            .map(|(_, t)| {
                let due_str = if let Some(d) = t.due_on {
                    if d.year() == current_year {
                        d.format("%b %d").to_string()
                    } else {
                        d.format("%b %d, %Y").to_string()
                    }
                } else {
                    "no due".to_string()
                };
                format!("{due_str} | {}", t.name)
            })
            .collect();

        // Update available to use sorted order for selection mapping
        let available = sorted_available;

        let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
            .with_prompt("Select a task to complete (ESC to finish)")
            .items(&display_items)
            .default(0)
            .interact_opt()?;

        let Some(selected_idx) = selection else {
            break;
        };

        let (original_idx, task) = available[selected_idx];
        completed_indices.push(original_idx);

        // Spawn background task for completion
        let task_gid = task.gid.clone();
        let client = ctx.client.clone();
        completion_tasks.push(tokio::spawn(async move {
            complete_task(&client, &task_gid).await
        }));
    }

    // Wait for all background completions
    if completion_tasks.iter().any(|t| !t.is_finished()) {
        ctx.term
            .write_str(&style("Waiting for tasks to complete...").dim().to_string())?;
        for res in join_all(completion_tasks).await {
            res??;
        }
        ctx.term.clear_line()?;
    }

    Ok(())
}

/// Mark a specific task as complete via the Asana API.
async fn complete_task(client: &Client, task_gid: &str) -> anyhow::Result<()> {
    let url: Url = format!("https://app.asana.com/api/1.0/tasks/{task_gid}").parse()?;
    let body = DataWrapper {
        data: CompleteTaskRequest { completed: true },
    };

    let response = client
        .mutate_request(Method::PUT, &url, body)
        .await
        .context("Failed to complete task")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Asana API error ({status}): {body}");
    }

    Ok(())
}
