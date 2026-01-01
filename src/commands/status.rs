//! Status command handler.

use anyhow::{Context as _, Result};

use crate::context::{AppContext, GroupedTasks};
use crate::focus::FocusStatus;

use super::get_focus_day;

/// Status output format.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum StatusFormat {
    /// Short one-line format for shell prompts and status bars.
    Short,
    /// JSON format for programmatic use.
    Json,
    /// xbar/SwiftBar format for macOS menu bar.
    Xbar,
}

/// Run the status command.
///
/// # Errors
///
/// Returns an error if Asana API requests or JSON serialization fails.
pub async fn run(
    ctx: &mut AppContext,
    grouped: &GroupedTasks<'_>,
    format: &StatusFormat,
    force_styling: bool,
) -> Result<()> {
    log::info!("Generating status output...");

    // Get focus day from cache or fetch
    let focus_day = if let (Some(focus_day), true) = (&ctx.cache.focus_day, ctx.use_cache) {
        focus_day.clone()
    } else {
        get_focus_day(ctx.today, &mut ctx.client).await?
    };

    let status = FocusStatus::new(
        &focus_day,
        ctx.now,
        grouped.overdue.len(),
        grouped.due_today.len(),
    );

    match format {
        StatusFormat::Short => {
            if ctx.config.tmux.enabled {
                print!("{}", status.to_short_string(force_styling));
            }
        }
        StatusFormat::Json => {
            println!(
                "{}",
                serde_json::to_string(&status).context("failed to serialize status")?
            );
        }
        StatusFormat::Xbar => {
            print!("{}", status.to_xbar_string(&ctx.config));
        }
    }

    Ok(())
}
