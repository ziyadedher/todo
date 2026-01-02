//! Install command handler.

#[cfg(target_os = "macos")]
use std::fs;
use std::path::{Path, PathBuf};

use console::style;

use crate::context::AppContext;

/// Install integration type.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum InstallIntegration {
    /// Show all available integrations and their status
    Show,
    /// Print zsh prompt configuration
    Zsh,
    /// Print tmux configuration snippet (including Dracula theme)
    Tmux,
    /// Install xbar/SwiftBar plugin (macOS only)
    Xbar,
    /// Install launchd notification agents (macOS only)
    Notifications,
}

#[allow(dead_code)] // Used in macOS-specific code
fn expand_homedir(path: &Path) -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(path.to_string_lossy().replace('~', &home)))
}

fn parse_time_string(time: &str) -> (u32, u32) {
    let parts: Vec<&str> = time.split(':').collect();
    let hour = parts.first().and_then(|h| h.parse().ok()).unwrap_or(0);
    let minute = parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0);
    (hour, minute)
}

/// Run the install command.
#[allow(clippy::too_many_lines)]
pub fn run(ctx: &mut AppContext, integration: &InstallIntegration) {
    match integration {
        InstallIntegration::Show => {
            println!("{}", style("Available integrations:").bold());
            println!();

            println!("  {}", style("zsh").cyan());
            println!("    Show focus status in your shell prompt");
            println!("    Run: todo install zsh");
            println!();

            println!(
                "  {} - {}",
                style("tmux").cyan(),
                if ctx.config.tmux.enabled {
                    style("enabled").green()
                } else {
                    style("disabled").dim()
                }
            );
            println!("    Status bar integration (standard + Dracula theme)");
            println!("    Run: todo install tmux");
            println!();

            println!(
                "  {} - {}",
                style("xbar").cyan(),
                if ctx.config.menubar.enabled {
                    style("enabled").green()
                } else {
                    style("disabled").dim()
                }
            );
            println!("    macOS menu bar widget (requires xbar/SwiftBar)");
            println!("    Run: todo install xbar");
            println!();

            println!(
                "  {} - {}",
                style("notifications").cyan(),
                if ctx.config.notifications.enabled {
                    style("enabled").green()
                } else {
                    style("disabled").dim()
                }
            );
            println!(
                "    Scheduled notifications at {} and {}",
                ctx.config.notifications.morning_time, ctx.config.notifications.evening_time
            );
            println!("    Run: todo install notifications");
            println!();

            println!(
                "  {} - {}",
                style("terminal blocking").cyan(),
                if ctx.config.terminal.blocking {
                    style("enabled").green()
                } else {
                    style("disabled").dim()
                }
            );
            println!("    Block new terminal sessions until focus is acknowledged");
            println!("    Set terminal.blocking = true in config");
        }

        InstallIntegration::Zsh => {
            println!("{}", style("Zsh Prompt Integration").bold().cyan());
            println!();
            println!("Add this to your ~/.zshrc:");
            println!();
            println!(
                "{}",
                style(
                    r"# Todo focus status in prompt
export TODO_PROMPT='%F{magenta}$(todo --use-cache status --format short)%f'"
                )
                .dim()
            );
            println!();
            println!(
                "Then add {} to your PROMPT, for example:",
                style("${TODO_PROMPT}").cyan()
            );
            println!(
                "{}",
                style(r#"export PROMPT="${TODO_PROMPT} ${PROMPT}""#).dim()
            );
            println!();
            println!("{}", style("Status format:").bold());
            println!("  focus:am  = morning focus pending");
            println!("  focus:pm  = evening focus pending");
            println!("  !N        = N overdue tasks");
            println!("  +N        = N tasks due today");
            println!("  ✔         = all clear");
        }

        InstallIntegration::Tmux => {
            println!("{}", style("tmux Integration").bold().cyan());
            println!();
            println!("{}", style("Option 1: Standard tmux").bold());
            println!("Add to ~/.tmux.conf:");
            println!(
                "{}",
                style(r"set -g status-right '#(todo --use-cache status --format short) | %H:%M'")
                    .dim()
            );
            println!();
            println!("{}", style("Option 2: Dracula theme").bold());
            println!();
            println!("1. Create the script:");
            println!(
                "{}",
                style("   mkdir -p ~/.tmux/plugins/tmux/scripts").dim()
            );
            println!(
                "{}",
                style(
                    r"   echo '#!/bin/bash
todo --use-cache status --format short' > ~/.tmux/plugins/tmux/scripts/todo.sh"
                )
                .dim()
            );
            println!(
                "{}",
                style("   chmod +x ~/.tmux/plugins/tmux/scripts/todo.sh").dim()
            );
            println!();
            println!("2. Add to your @dracula-plugins in ~/.tmux.conf:");
            println!(
                "{}",
                style(r#"   set -g @dracula-plugins "custom:todo.sh git cpu-usage ...""#).dim()
            );
            println!();
            println!(
                "3. Reload: {}",
                style("tmux source-file ~/.tmux.conf").dim()
            );
            println!();
            println!("{}", style("Status format:").bold());
            println!("  focus:am = morning focus pending");
            println!("  focus:pm = evening focus pending");
            println!("  !N       = N overdue tasks");
            println!("  +N       = N due today");
            println!("  ✔        = all clear");
        }

        InstallIntegration::Xbar => {
            #[cfg(target_os = "macos")]
            {
                if let Some(plugin_dir) =
                    expand_homedir(Path::new("~/Library/Application Support/xbar/plugins"))
                {
                    let plugin_path =
                        plugin_dir.join(format!("todo.{}s.sh", ctx.config.menubar.refresh_seconds));

                    if !plugin_dir.exists() {
                        println!(
                            "{}",
                            style("xbar/SwiftBar not found. Install it from:").yellow()
                        );
                        println!("  https://xbarapp.com/ or https://swiftbar.app/");
                        return;
                    }

                    let script = format!(
                        r#"#!/bin/bash
# Todo Focus Status for xbar/SwiftBar
# Refresh every {} seconds

todo --use-cache status --format xbar
"#,
                        ctx.config.menubar.refresh_seconds
                    );

                    if fs::write(&plugin_path, script).is_ok() {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            let _ = fs::set_permissions(
                                &plugin_path,
                                fs::Permissions::from_mode(0o755),
                            );
                        }

                        println!("{}", style("xbar plugin installed!").green().bold());
                        println!("Plugin location: {}", plugin_path.display());
                        println!();
                        println!("Restart xbar/SwiftBar to load the plugin.");
                    } else {
                        println!("{}", style("Failed to write plugin file.").red());
                    }
                }
            }

            #[cfg(not(target_os = "macos"))]
            {
                println!(
                    "{}",
                    style("xbar/SwiftBar is only available on macOS.").yellow()
                );
            }
        }

        InstallIntegration::Notifications => {
            #[cfg(target_os = "macos")]
            {
                println!("{}", style("macOS Notifications").bold().cyan());
                println!();
                println!(
                    "This will create launchd agents for morning ({}) and evening ({}) reminders.",
                    ctx.config.notifications.morning_time, ctx.config.notifications.evening_time
                );
                println!();

                if let Some(launch_agents_dir) = expand_homedir(Path::new("~/Library/LaunchAgents"))
                {
                    let _ = fs::create_dir_all(&launch_agents_dir);

                    let (morning_hour, morning_minute) =
                        parse_time_string(&ctx.config.notifications.morning_time);
                    let (evening_hour, evening_minute) =
                        parse_time_string(&ctx.config.notifications.evening_time);

                    let morning_plist = format!(
                        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.todo.morning-reminder</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/bin/osascript</string>
        <string>-e</string>
        <string>display notification "Time for your morning focus!" with title "Todo" sound name "default"</string>
    </array>
    <key>StartCalendarInterval</key>
    <dict>
        <key>Hour</key>
        <integer>{morning_hour}</integer>
        <key>Minute</key>
        <integer>{morning_minute}</integer>
    </dict>
</dict>
</plist>"#
                    );

                    let evening_plist = format!(
                        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.todo.evening-reminder</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/bin/osascript</string>
        <string>-e</string>
        <string>display notification "Time for your evening reflection!" with title "Todo" sound name "default"</string>
    </array>
    <key>StartCalendarInterval</key>
    <dict>
        <key>Hour</key>
        <integer>{evening_hour}</integer>
        <key>Minute</key>
        <integer>{evening_minute}</integer>
    </dict>
</dict>
</plist>"#
                    );

                    let morning_path = launch_agents_dir.join("com.todo.morning-reminder.plist");
                    let evening_path = launch_agents_dir.join("com.todo.evening-reminder.plist");

                    let _ = fs::write(&morning_path, morning_plist);
                    let _ = fs::write(&evening_path, evening_plist);

                    println!("{}", style("Notification agents installed!").green().bold());
                    println!();
                    println!("Load them with:");
                    println!(
                        "{}",
                        style(format!("  launchctl load {}", morning_path.display())).dim()
                    );
                    println!(
                        "{}",
                        style(format!("  launchctl load {}", evening_path.display())).dim()
                    );
                    println!();
                    println!("To uninstall, use 'launchctl unload' and delete the plist files.");
                }
            }

            #[cfg(target_os = "linux")]
            {
                println!("{}", style("Linux Notifications").bold().cyan());
                println!();
                println!("For Linux, you can use systemd user timers or cron.");
                println!();
                println!("Example crontab entries (run 'crontab -e' to edit):");
                println!();

                let (morning_hour, morning_minute) =
                    parse_time_string(&ctx.config.notifications.morning_time);
                let (evening_hour, evening_minute) =
                    parse_time_string(&ctx.config.notifications.evening_time);

                println!(
                    "{}",
                    style(format!(
                        "{morning_minute} {morning_hour} * * * notify-send 'Todo' 'Time for your morning focus!'"
                    ))
                    .dim()
                );
                println!(
                    "{}",
                    style(format!(
                        "{evening_minute} {evening_hour} * * * notify-send 'Todo' 'Time for your evening reflection!'"
                    ))
                    .dim()
                );
                println!();
                println!(
                    "{}",
                    style("Note: Requires 'libnotify' (notify-send) to be installed.").yellow()
                );
            }

            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            {
                println!(
                    "{}",
                    style("Notifications not yet supported on this platform.").yellow()
                );
            }
        }
    }
}
