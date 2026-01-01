//! List command handler.

use std::fmt::Write;

use console::style;

use crate::context::{AppContext, GroupedTasks};

fn task_or_tasks(num: usize) -> String {
    if num == 1 {
        "1 task".to_string()
    } else {
        format!("{num} tasks")
    }
}

/// Run the list command.
///
/// # Errors
///
/// Returns an error if terminal I/O fails.
///
/// # Panics
///
/// Panics if tasks are missing due dates (should not happen after filtering).
pub fn run(ctx: &mut AppContext, grouped: &GroupedTasks) -> anyhow::Result<()> {
    log::info!("Producing a list of tasks...");
    let mut string = String::new();

    if !grouped.overdue.is_empty() {
        let _ = writeln!(
            string,
            "{} {}",
            style(task_or_tasks(grouped.overdue.len())).red().bold(),
            style("overdue:").bold()
        );
        for task in &grouped.overdue {
            let _ = writeln!(
                string,
                "- ({}) {}",
                style(task.due_on.unwrap().to_string()).red(),
                task.name
            );
        }
        string.push('\n');
    }

    if !grouped.due_today.is_empty() {
        let _ = writeln!(
            string,
            "{} {}",
            style(task_or_tasks(grouped.due_today.len())).yellow(),
            style("due today:").bold()
        );
        for task in &grouped.due_today {
            let _ = writeln!(string, "- {}", task.name);
        }
        string.push('\n');
    }

    if !grouped.due_this_week.is_empty() {
        let _ = writeln!(
            string,
            "{} {}",
            style(task_or_tasks(grouped.due_this_week.len())).blue(),
            style("due within a week:").bold()
        );
        for task in &grouped.due_this_week {
            let _ = writeln!(
                string,
                "- ({}) {}",
                style(task.due_on.unwrap().to_string()).blue(),
                task.name
            );
        }
    }

    if string.is_empty() {
        ctx.term.write_line(&format!(
            "{}",
            style("Nice! Everything done for now!").green().bold()
        ))?;
    } else {
        ctx.term.write_str(string.trim())?;
    }
    Ok(())
}
