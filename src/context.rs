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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn make_task(gid: &str, name: &str, due_on: Option<NaiveDate>) -> UserTask {
        UserTask {
            gid: gid.to_string(),
            name: name.to_string(),
            due_on,
            created_at: Local::now(),
        }
    }

    #[test]
    fn groups_overdue_tasks() {
        let today = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let tasks = vec![
            make_task(
                "1",
                "Yesterday",
                Some(NaiveDate::from_ymd_opt(2024, 6, 14).unwrap()),
            ),
            make_task(
                "2",
                "Last week",
                Some(NaiveDate::from_ymd_opt(2024, 6, 8).unwrap()),
            ),
        ];

        let grouped = GroupedTasks::from_tasks(&tasks, today);

        assert_eq!(grouped.overdue.len(), 2);
        assert!(grouped.due_today.is_empty());
        assert!(grouped.due_this_week.is_empty());
        // Should be sorted by due date (oldest first)
        assert_eq!(grouped.overdue[0].name, "Last week");
        assert_eq!(grouped.overdue[1].name, "Yesterday");
    }

    #[test]
    fn groups_due_today_tasks() {
        let today = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let tasks = vec![
            make_task("1", "Today task", Some(today)),
            make_task("2", "Another today", Some(today)),
        ];

        let grouped = GroupedTasks::from_tasks(&tasks, today);

        assert!(grouped.overdue.is_empty());
        assert_eq!(grouped.due_today.len(), 2);
        assert!(grouped.due_this_week.is_empty());
    }

    #[test]
    fn groups_due_this_week_tasks() {
        let today = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let tasks = vec![
            make_task(
                "1",
                "Tomorrow",
                Some(NaiveDate::from_ymd_opt(2024, 6, 16).unwrap()),
            ),
            make_task(
                "2",
                "In 5 days",
                Some(NaiveDate::from_ymd_opt(2024, 6, 20).unwrap()),
            ),
            make_task(
                "3",
                "In 7 days",
                Some(NaiveDate::from_ymd_opt(2024, 6, 22).unwrap()),
            ),
        ];

        let grouped = GroupedTasks::from_tasks(&tasks, today);

        assert!(grouped.overdue.is_empty());
        assert!(grouped.due_today.is_empty());
        assert_eq!(grouped.due_this_week.len(), 3);
        // Should be sorted by due date
        assert_eq!(grouped.due_this_week[0].name, "Tomorrow");
        assert_eq!(grouped.due_this_week[1].name, "In 5 days");
        assert_eq!(grouped.due_this_week[2].name, "In 7 days");
    }

    #[test]
    fn excludes_tasks_beyond_week() {
        let today = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let tasks = vec![
            make_task(
                "1",
                "In 8 days",
                Some(NaiveDate::from_ymd_opt(2024, 6, 23).unwrap()),
            ),
            make_task(
                "2",
                "Next month",
                Some(NaiveDate::from_ymd_opt(2024, 7, 15).unwrap()),
            ),
        ];

        let grouped = GroupedTasks::from_tasks(&tasks, today);

        assert!(grouped.overdue.is_empty());
        assert!(grouped.due_today.is_empty());
        assert!(grouped.due_this_week.is_empty());
    }

    #[test]
    fn excludes_tasks_without_due_date() {
        let today = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let tasks = vec![
            make_task("1", "No due date", None),
            make_task("2", "Has due date", Some(today)),
        ];

        let grouped = GroupedTasks::from_tasks(&tasks, today);

        assert!(grouped.overdue.is_empty());
        assert_eq!(grouped.due_today.len(), 1);
        assert_eq!(grouped.due_today[0].name, "Has due date");
        assert!(grouped.due_this_week.is_empty());
    }

    #[test]
    fn handles_mixed_tasks() {
        let today = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let tasks = vec![
            make_task(
                "1",
                "Overdue",
                Some(NaiveDate::from_ymd_opt(2024, 6, 10).unwrap()),
            ),
            make_task("2", "Today", Some(today)),
            make_task(
                "3",
                "This week",
                Some(NaiveDate::from_ymd_opt(2024, 6, 18).unwrap()),
            ),
            make_task("4", "No date", None),
            make_task(
                "5",
                "Far future",
                Some(NaiveDate::from_ymd_opt(2024, 12, 25).unwrap()),
            ),
        ];

        let grouped = GroupedTasks::from_tasks(&tasks, today);

        assert_eq!(grouped.overdue.len(), 1);
        assert_eq!(grouped.due_today.len(), 1);
        assert_eq!(grouped.due_this_week.len(), 1);
    }
}
