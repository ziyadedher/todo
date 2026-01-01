//! Application context shared across commands.

use chrono::{DateTime, Days, Local, NaiveDate};
use console::Term;

use crate::asana::Client;
use crate::cache::Cache;
use crate::config::Config;
use crate::task::UserTask;

/// Shared application context passed to all commands.
pub struct AppContext {
    /// Application cache.
    pub cache: Cache,
    /// Application configuration.
    pub config: Config,
    /// Asana API client.
    pub client: Client,
    /// Terminal for output.
    pub term: Term,
    /// Current time.
    pub now: DateTime<Local>,
    /// Whether to use cached data.
    pub use_cache: bool,
}

impl AppContext {
    /// Create a new application context.
    #[must_use]
    pub fn new(cache: Cache, config: Config, client: Client, use_cache: bool) -> Self {
        let now = Local::now();
        Self {
            cache,
            config,
            client,
            term: Term::stdout(),
            now,
            use_cache,
        }
    }
}

/// Grouped tasks by due date.
pub struct GroupedTasks<'a> {
    /// Tasks that are overdue.
    pub overdue: Vec<&'a UserTask>,
    /// Tasks due today.
    pub due_today: Vec<&'a UserTask>,
    /// Tasks due within the next week.
    pub due_this_week: Vec<&'a UserTask>,
}

impl<'a> GroupedTasks<'a> {
    /// Group tasks by their due date relative to today.
    ///
    /// # Panics
    ///
    /// Panics if date arithmetic overflows (adding 7 days to today).
    #[must_use]
    pub fn from_tasks(tasks: &'a [UserTask], today: NaiveDate) -> Self {
        let mut overdue: Vec<_> = tasks
            .iter()
            .filter(|t| t.due_on.is_some_and(|d| d < today))
            .collect();
        overdue.sort_by_key(|t| t.due_on.expect("filtered to have due_on"));

        let mut due_today: Vec<_> = tasks
            .iter()
            .filter(|t| t.due_on.is_some_and(|d| d == today))
            .collect();
        due_today.sort_by_key(|t| t.due_on.expect("filtered to have due_on"));

        let week_end = today
            .checked_add_days(Days::new(7))
            .expect("date arithmetic overflow");
        let mut due_this_week: Vec<_> = tasks
            .iter()
            .filter(|t| t.due_on.is_some_and(|d| d > today && d <= week_end))
            .collect();
        due_this_week.sort_by_key(|t| t.due_on.expect("filtered to have due_on"));

        Self {
            overdue,
            due_today,
            due_this_week,
        }
    }
}
