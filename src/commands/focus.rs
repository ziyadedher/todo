//! Focus command handler.

use anyhow::{Context as _, Result};
use chrono::{Datelike, NaiveDate, Timelike, Weekday};
use console::style;
use dialoguer::{theme::ColorfulTheme, Input};
use futures::future::join_all;
use reqwest::{Method, Url};

use crate::asana::{Client, DataWrapper};
use crate::context::AppContext;
use crate::focus::{
    AddTaskToSectionRequest, CreateSectionRequest, CreateSectionTaskRequest,
    CreateSectionTaskRequestMembership, CreateSubtaskRequest, FocusDay, FocusDayStat, FocusTask,
    FocusTaskSubtask, FocusWeek, Section, UpdateFocusTaskCustomFieldsRequest,
    ASANA_FOCUS_PROJECT_GID, START_HOUR_FOR_EOD,
};

/// Get the focus day for a given date, creating it if necessary.
///
/// # Errors
///
/// Returns an error if the Asana API requests fail.
#[allow(clippy::too_many_lines)]
pub async fn get_focus_day(day: NaiveDate, client: &mut Client) -> Result<FocusDay> {
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

/// Show focus day overview.
///
/// # Errors
///
/// Returns an error if Asana API requests fail or terminal I/O fails.
pub async fn run_overview(ctx: &mut AppContext, date: Option<NaiveDate>) -> Result<()> {
    let date = date.unwrap_or(ctx.today);

    ctx.term
        .write_str(&style("Loading focus day...").dim().to_string())?;
    let focus_day = get_focus_day(date, &mut ctx.client).await?;
    ctx.term.clear_line()?;

    print!("{}", focus_day.to_full_string());
    Ok(())
}

/// Run the focus command.
///
/// # Errors
///
/// Returns an error if Asana API requests fail or terminal I/O fails.
#[allow(clippy::too_many_lines)]
pub async fn run(ctx: &mut AppContext, date: Option<NaiveDate>, force_eod: bool) -> Result<()> {
    log::info!("Managing focus...");

    let date = date.unwrap_or(ctx.today);
    log::info!("Using date: {date}");

    ctx.term
        .write_str(&style("Loading focus day...").dim().to_string())?;
    let mut focus_day = get_focus_day(date, &mut ctx.client).await?;
    ctx.term.clear_line()?;

    // Run focus routine
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
                    && (force_eod || date < ctx.today || ctx.now.hour() >= START_HOUR_FOR_EOD)
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
        let client = ctx.client.clone();
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
            .filter_map(|s| s.value().map(|v| (s.field_gid().to_string(), v)))
            .collect();

        async move {
            log::info!("Deciding if there are any changes to focus data to sync...");
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
    ctx.term
        .write_str(&style("Loading subtasks...").dim().to_string())?;
    focus_day.load_subtasks(&mut ctx.client).await?;
    ctx.term.clear_line()?;
    log::debug!(
        "Loaded {} subtasks",
        focus_day.subtasks.as_ref().map_or(0, Vec::len)
    );

    let mut subtasks = focus_day.subtasks.clone().unwrap_or_default();

    log::info!("Asking for tasks to add to focus day...");
    println!("{}", style("Any tasks to do today?").bold().red());
    let mut subtask_tasks: Vec<tokio::task::JoinHandle<Result<()>>> = Vec::new();
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
            let client = ctx.client.clone();
            let task_gid = task_gid.clone();
            let today = ctx.today;
            let url: Url = format!("https://app.asana.com/api/1.0/tasks/{task_gid}/subtasks")
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

        ctx.term.clear_last_lines(subtasks.len())?;
    }

    if !sync_task.is_finished() {
        ctx.term
            .write_str(&style("Waiting for focus data to sync...").dim().to_string())?;
        sync_task.await??;
        ctx.term.clear_line()?;
    }
    if subtask_tasks.iter().any(|t| !t.is_finished()) {
        ctx.term
            .write_str(&style("Waiting for subtasks to sync...").dim().to_string())?;
        for res in join_all(subtask_tasks).await {
            res??;
        }
        ctx.term.clear_line()?;
    }

    Ok(())
}
