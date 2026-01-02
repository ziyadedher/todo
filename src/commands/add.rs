//! Add command for creating new tasks.

use anyhow::Context as _;
use chrono::NaiveDate;
use console::style;
use dialoguer::{theme::ColorfulTheme, Input};
use reqwest::{Method, Url};

use crate::asana::DataWrapper;
use crate::context::AppContext;
use crate::task::CreateTaskRequest;
use crate::utils::parse_flexible_date;

/// Run the add command.
///
/// Creates a new task in the user's Asana task list.
///
/// Two modes:
/// - **CLI mode**: `todo add "Task name" --due tomorrow --description "Notes"`
/// - **Interactive mode**: `todo add` prompts for name, due date, and description
///
/// # Errors
///
/// Returns an error if the task cannot be created or if in cache-only mode.
pub async fn run(
    ctx: &mut AppContext,
    name: Option<String>,
    due: Option<String>,
    description: Option<String>,
) -> anyhow::Result<()> {
    if ctx.use_cache {
        anyhow::bail!("Cannot add tasks in cache-only mode. Run without --use-cache.");
    }

    let workspace_gid = ctx
        .config
        .workspace_gid
        .clone()
        .context("Workspace not configured. Run a command without --use-cache first.")?;

    // Determine if we're in interactive mode (no name provided)
    let interactive_mode = name.is_none();

    // Get task name (prompt if not provided)
    let task_name = if let Some(n) = name {
        n
    } else {
        Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("Task name")
            .interact_text()?
    };

    if task_name.trim().is_empty() {
        anyhow::bail!("Task name cannot be empty");
    }

    // Parse due date if provided, or prompt in interactive mode
    let due_date: Option<NaiveDate> = if let Some(d) = due {
        Some(parse_flexible_date(&d)?)
    } else if interactive_mode {
        // Interactive mode: ask for optional due date
        let due_input: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Due date (optional, e.g., tomorrow, next friday)")
            .allow_empty(true)
            .interact_text()?;

        if due_input.trim().is_empty() {
            None
        } else {
            Some(parse_flexible_date(&due_input)?)
        }
    } else {
        None
    };

    // Get description if in interactive mode and not provided
    let notes: Option<String> = if let Some(d) = description {
        if d.trim().is_empty() {
            None
        } else {
            Some(d)
        }
    } else if interactive_mode {
        // Interactive mode: ask for optional description
        let desc_input: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Description (optional)")
            .allow_empty(true)
            .interact_text()?;

        if desc_input.trim().is_empty() {
            None
        } else {
            Some(desc_input)
        }
    } else {
        None
    };

    // Create the task via Asana API
    let url: Url = "https://app.asana.com/api/1.0/tasks".parse()?;
    let body = DataWrapper {
        data: CreateTaskRequest {
            name: task_name.clone(),
            assignee: "me".to_string(),
            workspace: workspace_gid,
            due_on: due_date,
            notes,
        },
    };

    let response = ctx
        .client
        .mutate_request(Method::POST, &url, body)
        .await
        .context("Failed to create task")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Asana API error ({status}): {body}");
    }

    // Print success message
    let due_str = if let Some(d) = due_date {
        format!(" (due {})", d.format("%b %d, %Y"))
    } else {
        String::new()
    };

    ctx.term.write_line(&format!(
        "{} Created task: {}{}",
        style("✔").green().bold(),
        style(&task_name).cyan(),
        style(&due_str).dim()
    ))?;

    Ok(())
}
