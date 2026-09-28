//! Human- and agent-friendly entry points into an execution.
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "tuiporal",
    about = "Browse Temporal workflows in your terminal"
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Open a workflow directly (optionally in an iTerm split next to the caller).
    Show {
        #[arg(long)]
        workflow_id: String,
        /// Omit to show the latest run of this workflow ID.
        #[arg(long)]
        run_id: Option<String>,
        /// Open a new pane beside this iTerm session (macOS/iTerm2 only).
        #[arg(long)]
        split: bool,
    },
}

#[derive(Clone, Debug)]
pub struct WorkflowTarget {
    pub workflow_id: String,
    pub run_id: Option<String>,
}

impl WorkflowTarget {
    pub fn validate(&self) -> Result<()> {
        if self.workflow_id.is_empty() || self.workflow_id.chars().any(char::is_control) {
            bail!("--workflow-id must be nonempty and contain no control characters");
        }
        if self
            .run_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.chars().any(char::is_control))
        {
            bail!("--run-id must be nonempty and contain no control characters");
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
pub fn open_in_split(target: &WorkflowTarget) -> Result<()> {
    use std::process::Command as Process;
    // iTerm supplies the UUID of the session that invoked this command. Do not
    // split the currently selected tab: the user may be looking elsewhere.
    let session = std::env::var("ITERM_SESSION_ID").context(
        "--split needs iTerm2 (ITERM_SESSION_ID is not set); omit --split in other terminals",
    )?;
    let session_id = session.rsplit(':').next().unwrap_or("");
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-')
    {
        bail!("Invalid iTerm session ID");
    }
    let exe = std::env::current_exe()?;
    let cwd = std::env::current_dir()?;
    let mut command = format!("cd {} && ", shell_quote(&cwd.to_string_lossy()));
    if let Some(home) = std::env::var_os("HOME") {
        command.push_str(&format!("HOME={} ", shell_quote(&home.to_string_lossy())));
    }
    command.push_str(&format!(
        "{} show --workflow-id {}",
        shell_quote(&exe.to_string_lossy()),
        shell_quote(&target.workflow_id)
    ));
    if let Some(id) = &target.run_id {
        command.push_str(&format!(" --run-id {}", shell_quote(id)));
    }

    // The command is an AppleScript *argument*, never interpolated into source.
    let script = r#"
on run argv
    set callerId to item 1 of argv
    set launchCommand to item 2 of argv
    tell application "iTerm"
        repeat with w in windows
            repeat with t in tabs of w
                repeat with s in sessions of t
                    if (id of s as text) is callerId then
                        tell s to set newPane to split vertically with default profile
                        delay 1
                        if (contents of newPane) contains "Would you like to update?" then
                            tell newPane to write text "n"
                            delay 1
                        end if
                        tell newPane to write text launchCommand
                        return id of newPane
                    end if
                end repeat
            end repeat
        end repeat
    end tell
    error "Could not find the calling iTerm session; run without --split"
end run
"#;
    let output = Process::new("osascript")
        .arg("-e")
        .arg(script)
        .arg(session_id)
        .arg(command)
        .output()
        .context("Could not invoke iTerm2")?;
    if !output.status.success() {
        bail!(
            "iTerm could not open the split: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    println!(
        "Opened Tuiporal beside this iTerm session (pane {}).",
        String::from_utf8_lossy(&output.stdout).trim()
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn open_in_split(_target: &WorkflowTarget) -> Result<()> {
    bail!("--split currently supports iTerm2 on macOS only; use `tuiporal show --workflow-id ID` in a terminal")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_direct_open_with_split() {
        let args = Args::try_parse_from([
            "tuiporal",
            "show",
            "--workflow-id",
            "order-1",
            "--run-id",
            "run-2",
            "--split",
        ])
        .unwrap();
        assert!(
            matches!(args.command, Some(Command::Show { workflow_id, run_id: Some(_), split: true }) if workflow_id == "order-1")
        );
    }

    #[test]
    fn rejects_control_characters_in_workflow_ids() {
        assert!(WorkflowTarget {
            workflow_id: "hello\nworld".into(),
            run_id: None
        }
        .validate()
        .is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn shell_argument_escapes_quotes_without_evaluating_metacharacters() {
        assert_eq!(
            shell_quote("a' ; touch /tmp/nope"),
            "'a'\\'' ; touch /tmp/nope'"
        );
    }
}
