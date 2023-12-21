#![warn(clippy::pedantic)]
#![warn(clippy::cargo)]

use std::{collections::HashMap, env, fs, path::PathBuf};

use anyhow::Context;
use chrono::{DateTime, Days, Local, NaiveDate};
use clap::{Parser, Subcommand};
use human_panic::setup_panic;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use todo::asana::{Client, Credentials, DataRequest};

const ASANA_USER_TASK_LIST_GID: &str = "1199118625430768";

/// Todo is a simple Asana helper script that pulls data from Asana and shows it in CLI settings
#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(long, default_value = "~/.config/todo/creds.toml")]
    config_path: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print out a summary of current TODO tasks
    Summary,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Config {
    credentials: Option<Credentials>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Task {
    gid: String,
    #[serde(with = "todo::asana::datetime_format")]
    created_at: DateTime<Local>,
    #[serde(with = "todo::asana::optional_date_format")]
    due_on: Option<NaiveDate>,
    name: String,
}

impl<'a> DataRequest<'a> for Task {
    type RequestData = String;
    type ResponseData = Vec<Task>;

    fn endpoint(request_data: Self::RequestData, base_url: &Url) -> Url {
        base_url
            .join(&format!("user_task_lists/{request_data}/tasks"))
            .context("could not create endpoint")
            .unwrap()
    }

    fn fields() -> &'static [&'static str] {
        &["this.gid", "this.created_at", "this.due_on", "this.name"]
    }

    fn other_params() -> HashMap<String, String> {
        [("completed_since".to_string(), "now".to_string())].into()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_panic!();
    env_logger::init();

    log::debug!("Parsing command line arguments...");
    let args = Args::parse();
    let config_path: PathBuf = args
        .config_path
        .to_string_lossy()
        .replace("~", &env::var("HOME")?)
        .into();
    log::trace!("Parsed command line arguments: {args:#?}");

    log::debug!(
        "Checking if configuration file exists at {}...",
        config_path.display()
    );
    if !config_path.exists() {
        log::warn!(
            "Could not find configuration at {}, so creating and using an empty configuration...",
            config_path.display()
        );
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).context("could not create path to configuration file")?;
        }
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&config_path)
            .context("could not create configuration file")?;
    }

    log::debug!("Loading configuration from {}...", config_path.display());
    let config: Config = toml::from_str(
        &fs::read_to_string(&config_path).context("could not read credentials file")?,
    )
    .context("could not deserialize configuration file")?;
    log::trace!("Loaded configuration: {config:#?}");

    let mut client = if let Some(credentials) = config.credentials {
        Client::new_from_credentials(credentials)?
    } else {
        let client = Client::new().await?;
        log::info!("Saving new access token to {}...", config_path.display());
        let config = Config {
            credentials: Some(client.credentials().clone()),
        };
        fs::write(
            &config_path,
            toml::to_string_pretty(&config).context("could not serialize configuration")?,
        )
        .context("could not write to configuration file")?;
        client
    };

    // TODO: eventually pull this out and get the tasks list automatically.
    let tasks = client
        .get::<Task>(ASANA_USER_TASK_LIST_GID.to_string())
        .await?;

    match args.command {
        Command::Summary => {
            fn task_or_tasks(num: usize) -> String {
                if num == 1 {
                    "1 task".to_string()
                } else {
                    format!("{num} tasks")
                }
            }

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
                (0, 0) => "Nice! Everything done for now!".to_string(),
                (o, 0) => format!("You have {} overdue.", task_or_tasks(o)),
                (0, t) => format!("You have {} due today.", task_or_tasks(t)),
                (o, t) => format!("You have {} overdue or due today", task_or_tasks(o + t)),
            });

            string.push_str(&match num_due_week {
                0 => String::new(),
                w => format!(" You have another {} due within a week.", task_or_tasks(w)),
            });

            println!("{string} (https://app.asana.com/0/{ASANA_USER_TASK_LIST_GID}/list)");
        }
    }

    Ok(())
}
