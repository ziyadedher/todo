//! Update command handler.
//!
//! Note: The update command is handled directly in main.rs via `refresh_cache`.
//! This module is a placeholder for consistency with the commands structure.

use anyhow::Result;

use crate::context::AppContext;

/// Run the update command.
///
/// This is a placeholder - the actual update logic is in main.rs.
///
/// # Errors
///
/// This function currently never returns an error.
pub fn run(_ctx: &mut AppContext) -> Result<()> {
    // Update is handled directly in main.rs via refresh_cache
    Ok(())
}
