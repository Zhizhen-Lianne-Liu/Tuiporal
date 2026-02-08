pub mod screens;

use crate::app::{App, Screen};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};

pub fn render(app: &App, frame: &mut Frame) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),     // Content
            Constraint::Length(1),  // Footer
        ])
        .split(frame.area());

    // Render header with tabs
    render_header(app, frame, chunks[0]);

    // Render content based on current screen
    match app.current_screen {
        Screen::Workflows => screens::workflows::render(app, frame, chunks[1]),
        Screen::Namespaces => screens::namespaces::render(app, frame, chunks[1]),
        Screen::WorkflowDetail => screens::workflow_detail::render(app, frame, chunks[1]),
        Screen::Help => screens::help::render(app, frame, chunks[1]),
    }

    // Render footer
    render_footer(app, frame, chunks[2]);
}

fn render_header(app: &App, frame: &mut Frame, area: ratatui::layout::Rect) {
    let titles = vec!["Workflows (1)", "Namespaces (2)", "Help (?)"];
    let index = match app.current_screen {
        Screen::Workflows => 0,
        Screen::Namespaces => 1,
        Screen::WorkflowDetail => 0, // Keep Workflows highlighted when in detail view
        Screen::Help => 2,
    };

    // Build title with connection status indicator
    let (status_icon, status_color) = match &app.connection_status {
        crate::app::ConnectionStatus::Connected => ("●", Color::Green),
        crate::app::ConnectionStatus::Connecting => (app.spinner(), Color::Yellow),
        crate::app::ConnectionStatus::Disconnected => ("●", Color::Red),
        crate::app::ConnectionStatus::Error(_) => ("●", Color::Red),
    };

    let title = format!("Tuiporal {} | ns: {}", status_icon, app.current_namespace);

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(Span::styled(title, Style::default().fg(status_color))))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .select(index);

    frame.render_widget(tabs, area);
}

fn render_footer(app: &App, frame: &mut Frame, area: ratatui::layout::Rect) {
    let help_text = match app.current_screen {
        Screen::Workflows => {
            if app.workflow_list_state.input_mode {
                Line::from(vec![
                    Span::styled("Type to search | ", Style::default().fg(Color::White)),
                    Span::styled("Enter", Style::default().fg(Color::Yellow)),
                    Span::raw(" confirm | "),
                    Span::styled("ESC", Style::default().fg(Color::Yellow)),
                    Span::raw(" cancel"),
                ])
            } else {
                let mut help_spans = vec![
                    Span::styled("↑/k", Style::default().fg(Color::Yellow)),
                    Span::raw("/"),
                    Span::styled("↓/j", Style::default().fg(Color::Yellow)),
                    Span::raw(" nav | "),
                    Span::styled("Enter", Style::default().fg(Color::Yellow)),
                    Span::raw(" view | "),
                    Span::styled("/", Style::default().fg(Color::Yellow)),
                    Span::raw(" search | "),
                    Span::styled("f", Style::default().fg(Color::Yellow)),
                    Span::raw(" filter | "),
                    Span::styled("c", Style::default().fg(Color::Yellow)),
                    Span::raw(" clear | "),
                    Span::styled("a", Style::default().fg(Color::Yellow)),
                    Span::raw(" auto | "),
                ];

                // Add pagination hints if applicable
                if app.workflow_list_state.has_prev_page() || app.workflow_list_state.has_next_page() {
                    if app.workflow_list_state.has_prev_page() {
                        help_spans.push(Span::styled("←/p", Style::default().fg(Color::Yellow)));
                        help_spans.push(Span::raw(" prev | "));
                    }
                    if app.workflow_list_state.has_next_page() {
                        help_spans.push(Span::styled("→/n", Style::default().fg(Color::Yellow)));
                        help_spans.push(Span::raw(" next | "));
                    }
                }

                help_spans.push(Span::styled("r", Style::default().fg(Color::Yellow)));
                help_spans.push(Span::raw(" refresh | "));
                help_spans.push(Span::styled("?", Style::default().fg(Color::Yellow)));
                help_spans.push(Span::raw(" help | "));
                help_spans.push(Span::styled("q", Style::default().fg(Color::Yellow)));
                help_spans.push(Span::raw(" quit"));

                Line::from(help_spans)
            }
        }
        Screen::Namespaces => Line::from(vec![
            Span::styled("↑/k", Style::default().fg(Color::Yellow)),
            Span::raw("/"),
            Span::styled("↓/j", Style::default().fg(Color::Yellow)),
            Span::raw(" nav | "),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(" switch | "),
            Span::styled("r", Style::default().fg(Color::Yellow)),
            Span::raw(" refresh | "),
            Span::styled("?", Style::default().fg(Color::Yellow)),
            Span::raw(" help | "),
            Span::styled("ESC", Style::default().fg(Color::Yellow)),
            Span::raw(" back | "),
            Span::styled("q", Style::default().fg(Color::Yellow)),
            Span::raw(" quit"),
        ]),
        Screen::WorkflowDetail => {
            if app.workflow_detail_state.show_event_detail {
                Line::from(vec![
                    Span::styled("↑/k", Style::default().fg(Color::Yellow)),
                    Span::raw("/"),
                    Span::styled("↓/j", Style::default().fg(Color::Yellow)),
                    Span::raw(" scroll | "),
                    Span::styled("PgUp/PgDn", Style::default().fg(Color::Yellow)),
                    Span::raw(" page | "),
                    Span::styled("ESC/q", Style::default().fg(Color::Yellow)),
                    Span::raw(" close"),
                ])
            } else if app.workflow_detail_state.show_dialog.is_some() {
                Line::from(vec![
                    Span::styled("Type input | ", Style::default().fg(Color::White)),
                    Span::styled("Enter", Style::default().fg(Color::Yellow)),
                    Span::raw(" confirm | "),
                    Span::styled("ESC", Style::default().fg(Color::Yellow)),
                    Span::raw(" cancel"),
                ])
            } else if app.workflow_detail_state.success_message.is_some() {
                Line::from(vec![
                    Span::raw("Press any key to continue"),
                ])
            } else {
                Line::from(vec![
                    Span::styled("↑/k", Style::default().fg(Color::Yellow)),
                    Span::raw("/"),
                    Span::styled("↓/j", Style::default().fg(Color::Yellow)),
                    Span::raw(" nav | "),
                    Span::styled("Enter", Style::default().fg(Color::Yellow)),
                    Span::raw(" view | "),
                    Span::styled("t", Style::default().fg(Color::Yellow)),
                    Span::raw(" terminate | "),
                    Span::styled("x", Style::default().fg(Color::Yellow)),
                    Span::raw(" cancel | "),
                    Span::styled("s", Style::default().fg(Color::Yellow)),
                    Span::raw(" signal | "),
                    Span::styled("?", Style::default().fg(Color::Yellow)),
                    Span::raw(" help | "),
                    Span::styled("ESC", Style::default().fg(Color::Yellow)),
                    Span::raw(" back | "),
                    Span::styled("q", Style::default().fg(Color::Yellow)),
                    Span::raw(" quit"),
                ])
            }
        }
        Screen::Help => Line::from(vec![
            Span::styled("↑/k", Style::default().fg(Color::Yellow)),
            Span::raw("/"),
            Span::styled("↓/j", Style::default().fg(Color::Yellow)),
            Span::raw(" scroll | "),
            Span::styled("PgUp/PgDn", Style::default().fg(Color::Yellow)),
            Span::raw(" page | "),
            Span::styled("?", Style::default().fg(Color::Yellow)),
            Span::raw(" close | "),
            Span::styled("ESC", Style::default().fg(Color::Yellow)),
            Span::raw(" back | "),
            Span::styled("q", Style::default().fg(Color::Yellow)),
            Span::raw(" quit"),
        ]),
    };

    let footer = Paragraph::new(help_text);
    frame.render_widget(footer, area);
}
