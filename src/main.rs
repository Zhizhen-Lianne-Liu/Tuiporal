mod app;
mod cli;
mod config;
mod events;
#[allow(dead_code)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/temporal.rs"));
}
mod temporal;
mod ui;

use anyhow::Result;
use app::App;
use clap::Parser;
use crossterm::{
    event::DisableMouseCapture,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::fs::OpenOptions;
use std::io;
use std::sync::Mutex;

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli::Args::parse();
    let target = match args.command {
        Some(cli::Command::Show {
            workflow_id,
            run_id,
            split,
        }) => {
            let target = cli::WorkflowTarget {
                workflow_id,
                run_id,
            };
            target.validate()?;
            if split {
                cli::open_in_split(&target)?;
                return Ok(());
            }
            Some(target)
        }
        None => None,
    };

    // Keep logs out of the TUI. Opt in to a log file for diagnostics.
    if let Some(path) = std::env::var_os("TUIPORAL_LOG") {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        tracing_subscriber::fmt()
            .with_writer(Mutex::new(file))
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::ERROR)
            .with_writer(std::io::sink)
            .init();
    }

    // Connect before altering terminal state, so startup errors leave it usable.
    let app = App::new(target).await?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // We do not use mouse events. Let the terminal handle dragging/selection
    // so text in the TUI can be copied with the usual terminal shortcut.
    execute!(stdout, DisableMouseCapture, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the application
    let res = app.run(&mut terminal).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}
