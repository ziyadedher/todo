#![warn(clippy::pedantic)]

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::{DateTime, Days, Local, NaiveDate};
use clap::{Parser, Subcommand};
use colored::Colorize;
use human_panic::setup_panic;
use serde::{Deserialize, Serialize};
use todo::asana::{execute_authorization_flow, Client, Credentials, DataRequest};

const ASANA_USER_TASK_LIST_GID: &str = "1199118625430768";

/// Todo is a simple Asana helper script that pulls data from Asana and shows it in CLI settings
#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the cache file
    #[arg(long, default_value = "~/.cache/todo/todo.json")]
    cache_path: PathBuf,

    /// Path to the configuration file
    #[arg(long, default_value = "~/.config/todo/todo.toml")]
    config_path: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print out a summary of current TODO tasks
    Summary {
        #[arg(long, default_value = "false")]
        use_cache: bool,
    },

    /// Pull and cache information about TODO tasks, without printing anything
    Update,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Cache {
    credentials: Option<Credentials>,
    tasks: Option<Vec<Task>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Config {}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Task {
    gid: String,
    #[serde(with = "todo::asana::serde_formats::datetime")]
    created_at: DateTime<Local>,
    #[serde(with = "todo::asana::serde_formats::optional_date")]
    due_on: Option<NaiveDate>,
    name: String,
}

impl<'a> DataRequest<'a> for Task {
    type RequestData = String;
    type ResponseData = Vec<Task>;

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

    fn params() -> &'a [(&'a str, &'a str)] {
        &[("completed_since", "now")]
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_panic!();
    env_logger::init();

    log::debug!("Parsing command line arguments...");
    let args = Args::parse();
    log::trace!("Parsed command line arguments: {args:#?}");

    let cache = load_cache(&expand_homedir(&args.cache_path)?)?;
    let _config = load_config(&expand_homedir(&args.config_path)?)?;

    let credentials = if let Some(credentials) = &cache.credentials {
        credentials.clone()
    } else {
        let credentials = execute_authorization_flow().await?;
        let cache = cache.clone();
        save_cache(
            &expand_homedir(&args.cache_path)?,
            &Cache {
                credentials: Some(credentials.clone()),
                ..cache
            },
        )?;
        credentials
    };
    let mut client = Client::new(credentials)?;

    // TODO: eventually pull this out and get the tasks list automatically.

    match args.command {
        Command::Summary { use_cache } => {
            fn task_or_tasks(num: usize) -> String {
                if num == 1 {
                    "1 task".to_string()
                } else {
                    format!("{num} tasks")
                }
            }

            let tasks = if let (true, Some(tasks)) = (use_cache, cache.tasks) {
                tasks
            } else {
                client
                    .get::<Task>(&ASANA_USER_TASK_LIST_GID.to_string())
                    .await?
            };

            let today = Local::now().date_naive();
            let num_overdue = tasks
                .iter()
                .filter(|t| t.due_on.is_some_and(|d| d < today))
                .count();
            let num_due_today = tasks
                .iter()
                .filter(|t| t.due_on.is_some_and(|d| d == today))
                .count();
            let num_due_week = tasks
                .iter()
                .filter(|t| {
                    t.due_on
                        .is_some_and(|d| d <= today.checked_add_days(Days::new(7)).unwrap())
                })
                .count()
                - num_due_today
                - num_overdue;

            let mut string = String::new();
            string.push_str(&match (num_overdue, num_due_today) {
                (0, 0) => "Nice! Everything done for now!".green().bold().to_string(),
                (o, 0) => format!("You have {} overdue.", task_or_tasks(o))
                    .red()
                    .bold()
                    .to_string(),
                (0, t) => format!("You have {} due today.", task_or_tasks(t))
                    .yellow()
                    .bold()
                    .to_string(),
                (o, t) => format!("You have {} overdue or due today", task_or_tasks(o + t))
                    .red()
                    .bold()
                    .to_string(),
            });

            string.push_str(&match num_due_week {
                0 => String::new(),
                w => format!(" You have another {} due within a week.", task_or_tasks(w))
                    .blue()
                    .to_string(),
            });

            println!(
                "{string} {}",
                format!("(https://app.asana.com/0/{ASANA_USER_TASK_LIST_GID}/list)").dimmed()
            );
        }

        Command::Update => {
            let tasks = client
                .get::<Task>(&ASANA_USER_TASK_LIST_GID.to_string())
                .await?;
            save_cache(
                &expand_homedir(&args.cache_path)?,
                &Cache {
                    tasks: Some(tasks),
                    ..cache
                },
            )?;
        }
    }

    Ok(())
}
