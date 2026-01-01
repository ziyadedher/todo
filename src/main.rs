#![warn(clippy::pedantic)]

use std::{
    env,
    path::{Path, PathBuf},
};

use chrono::{Local, NaiveDate};
use clap::{Parser, Subcommand};
use dialoguer::{theme::ColorfulTheme, FuzzySelect, Select};

use todo::{
    asana::{ask_for_pat, execute_authorization_flow, Client, Credentials},
    cache::Cache,
    commands::install::InstallIntegration,
    commands::status::StatusFormat,
    context::{AppContext, GroupedTasks},
    task::{Project, UserTask, UserTaskList, UserTaskListRequest, Workspace},
};

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

        /// Force ANSI color styling in output
        #[arg(long)]
        force_styling: bool,
    },

    /// Install shell/system integrations
    Install {
        #[command(subcommand)]
        integration: InstallIntegration,
    },
}

#[derive(Debug, Subcommand)]
enum FocusCommand {
    /// Run the focus routine
    Run,
    /// Print out an overview of the focus day
    Overview,
}

fn expand_homedir(path: &Path) -> anyhow::Result<PathBuf> {
    Ok(path
        .to_string_lossy()
        .replace('~', &env::var("HOME")?)
        .into())
}

/// Refresh cache with fresh data from Asana.
async fn refresh_cache(
    cache: &mut Cache,
    cache_path: &Path,
    client: &mut Client,
    user_task_list_gid: &str,
    focus_project_gid: Option<&str>,
) -> anyhow::Result<()> {
    log::info!("Refreshing cache...");
    let tasks = client
        .get::<UserTask>(&user_task_list_gid.to_string())
        .await?;

    let focus_day = if let Some(focus_gid) = focus_project_gid {
        Some(todo::commands::get_focus_day(Local::now().date_naive(), client, focus_gid).await?)
    } else {
        None
    };

    cache.tasks = Some(tasks);
    cache.focus_day = focus_day;
    cache.last_updated = Some(Local::now());
    todo::cache::save(cache_path, cache)?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    human_panic::setup_panic!();
    env_logger::init();

    log::debug!("Parsing command line arguments...");
    let args = Args::parse();
    log::trace!("Parsed command line arguments: {args:#?}");

    let cache_path = expand_homedir(&args.cache_path)?;
    let config_path = expand_homedir(&args.config_path)?;

    let mut cache = todo::cache::load(&cache_path)?;
    let mut config = todo::config::load(&config_path)?;

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
                    console::style("Warning: cache has not been updated in more than 3 minutes, is the update command in the background? See the README.md")
                        .red()
                );
            }
        } else {
            log::warn!("Cache has never been updated, letting the user know...");
            eprintln!(
                "{}",
                console::style(
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
            todo::cache::save(&cache_path, &cache)?;
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
        todo::cache::save(&cache_path, &cache)?;
        creds
    };

    let mut client = Client::new(creds)?;

    // Resolve workspace GID
    let workspace_gid = if let Some(ref gid) = config.workspace_gid {
        log::debug!("Using configured workspace GID: {gid}");
        gid.clone()
    } else {
        log::info!("No workspace configured, fetching workspaces...");
        let workspaces = client.get::<Workspace>(&()).await?;
        if workspaces.is_empty() {
            anyhow::bail!("No workspaces found for this user");
        }
        let workspace = if workspaces.len() == 1 {
            log::info!("Found single workspace: {}", workspaces[0].name);
            &workspaces[0]
        } else {
            println!("Multiple workspaces found. Please select one:");
            let workspace_names: Vec<&str> = workspaces.iter().map(|w| w.name.as_str()).collect();
            let selection = Select::with_theme(&ColorfulTheme::default())
                .items(&workspace_names)
                .default(0)
                .interact()?;
            &workspaces[selection]
        };
        // Save to config
        config.workspace_gid = Some(workspace.gid.clone());
        todo::config::save(&config_path, &config)?;
        log::debug!("Saved workspace to config: {}", workspace.name);
        workspace.gid.clone()
    };

    // Resolve focus project GID (optional)
    let focus_project_gid = if let Some(ref gid) = config.focus_project_gid {
        log::debug!("Using configured focus project GID: {gid}");
        Some(gid.clone())
    } else {
        log::info!("No focus project configured, fetching projects...");
        let all_projects = client.get::<Project>(&workspace_gid).await?;

        // Filter to projects with "focus" in the name (case insensitive)
        let focus_projects: Vec<_> = all_projects
            .into_iter()
            .filter(|p| p.name.to_lowercase().contains("focus"))
            .collect();

        if focus_projects.is_empty() {
            log::warn!("No projects with 'focus' in name found. Focus features will be disabled.");
            None
        } else {
            println!("Select a project for daily focus tracking (type to search):");
            let mut project_names: Vec<String> =
                focus_projects.iter().map(|p| p.name.clone()).collect();
            project_names.push("(Skip - disable focus features)".to_string());
            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .items(&project_names)
                .default(0)
                .interact()?;
            if selection == focus_projects.len() {
                // User chose to skip
                None
            } else {
                let project = &focus_projects[selection];
                config.focus_project_gid = Some(project.gid.clone());
                todo::config::save(&config_path, &config)?;
                log::debug!("Saved focus project to config: {}", project.name);
                Some(project.gid.clone())
            }
        }
    };

    log::info!("Getting user task list...");
    let user_task_list =
        if let (Some(user_task_list), true) = (cache.user_task_list.clone(), args.use_cache) {
            log::debug!("Using cached user task list...");
            user_task_list
        } else {
            let request = UserTaskListRequest {
                user_gid: "me".to_string(),
                workspace_gid: workspace_gid.clone(),
            };
            let user_task_list = client.get::<UserTaskList>(&request).await?;
            log::debug!("Saving new user task list to cache...");
            cache.user_task_list = Some(user_task_list.clone());
            todo::cache::save(&cache_path, &cache)?;
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
        todo::cache::save(&cache_path, &cache)?;
        tasks
    };
    log::debug!("Got {} tasks", tasks.len());
    log::trace!("Tasks: {tasks:#?}");

    let now = Local::now();
    let today = now.date_naive();

    log::info!("Grouping tasks...");
    let grouped = GroupedTasks::from_tasks(&tasks, today);
    log::debug!(
        "Grouped tasks: {} overdue, {} due today, {} due this week",
        grouped.overdue.len(),
        grouped.due_today.len(),
        grouped.due_this_week.len()
    );

    let mut ctx = AppContext::new(cache, config, client, args.use_cache);

    match args.command {
        Command::Summary => {
            todo::commands::summary::run(&mut ctx, &grouped).await?;
        }
        Command::List => {
            todo::commands::list::run(&mut ctx, &grouped)?;
        }
        Command::Focus {
            date,
            force_eod,
            command,
        } => {
            if ctx.config.focus_project_gid.is_none() {
                anyhow::bail!("Focus project not configured. Set focus_project_gid in config.");
            }
            match command {
                Some(FocusCommand::Overview) => {
                    todo::commands::focus::run_overview(&mut ctx, date).await?;
                }
                Some(FocusCommand::Run) | None => {
                    todo::commands::focus::run(&mut ctx, date, force_eod).await?;
                }
            }
        }
        Command::Update => {
            if args.use_cache {
                anyhow::bail!("Cannot use --use-cache with update command");
            }
            // Cache refresh happens below after the match
        }
        Command::Status {
            format,
            force_styling,
        } => {
            todo::commands::status::run(&mut ctx, &grouped, &format, force_styling).await?;
        }
        Command::Install { integration } => {
            todo::commands::install::run(&mut ctx, &integration);
        }
    }

    // Refresh cache after commands that fetch/modify Asana data (unless using cache)
    if !args.use_cache {
        refresh_cache(
            &mut ctx.cache,
            &cache_path,
            &mut ctx.client,
            &user_task_list.gid,
            focus_project_gid.as_deref(),
        )
        .await?;
    }

    Ok(())
}
