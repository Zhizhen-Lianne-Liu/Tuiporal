use crate::app::{App, ConnectionStatus};
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let state = &app.namespace_list_state;

    // Show error message if there is one
    if let Some(error) = &state.error {
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "⚠ An error occurred:",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(error, Style::default().fg(Color::White))),
            Line::from(""),
            Line::from(Span::styled(
                "Press 'r' to retry or 'ESC' to go back",
                Style::default().fg(Color::Yellow),
            )),
        ];
        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Namespaces - Error")
                    .style(Style::default().fg(Color::Red)),
            );
        frame.render_widget(paragraph, area);
        return;
    }

    // Show loading indicator
    if state.loading {
        let spinner = app.spinner();
        let loading_text = format!("{} Loading namespaces...", spinner);
        let paragraph = Paragraph::new(loading_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Namespaces")
                    .style(Style::default().fg(Color::Yellow)),
            )
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(paragraph, area);
        return;
    }

    // Show connection status if not connected
    if !matches!(app.connection_status, ConnectionStatus::Connected) {
        let (status_text, color) = match &app.connection_status {
            ConnectionStatus::Disconnected => ("Not connected to Temporal".to_string(), Color::Red),
            ConnectionStatus::Connecting => {
                let spinner = app.spinner();
                (format!("{} Connecting to Temporal...", spinner), Color::Yellow)
            },
            ConnectionStatus::Error(e) => (format!("Connection error: {}", e), Color::Red),
            ConnectionStatus::Connected => (String::new(), Color::White),
        };
        let paragraph = Paragraph::new(status_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Namespaces")
                    .style(Style::default().fg(color)),
            )
            .style(Style::default().fg(color));
        frame.render_widget(paragraph, area);
        return;
    }

    // Show empty message if no namespaces
    if state.items.is_empty() {
        let lines = vec![
            Line::from("No namespaces found"),
            Line::from(""),
            Line::from(Span::styled(
                "Press 'r' to refresh",
                Style::default().fg(Color::Yellow),
            )),
        ];
        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Namespaces")
                .style(Style::default().fg(Color::White)),
        );
        frame.render_widget(paragraph, area);
        return;
    }

    // Build the table
    let header = Row::new(vec![
        Cell::from("Namespace"),
        Cell::from("Description"),
        Cell::from("State"),
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = state
        .items
        .iter()
        .map(|ns_response| {
            let (name, description, state_str) = if let Some(info) = &ns_response.namespace_info {
                let name = info.name.clone();
                let description = info.description.clone();
                let state = get_namespace_state(info.state);
                (name, description, state)
            } else {
                ("Unknown".to_string(), "".to_string(), "Unknown".to_string())
            };

            // Highlight current namespace
            let style = if name == app.current_namespace {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(name).style(style),
                Cell::from(description),
                Cell::from(state_str),
            ])
        })
        .collect();

    let widths = [
        Constraint::Percentage(30),
        Constraint::Percentage(50),
        Constraint::Percentage(20),
    ];

    let title = format!(
        "Namespaces ({} items) - Current: {}",
        state.items.len(),
        app.current_namespace
    );

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(Style::default().fg(Color::White)),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_stateful_widget(table, area, &mut state.table_state.clone());
}

fn get_namespace_state(state: i32) -> String {
    // Namespace state enum values
    match state {
        0 => "Unspecified".to_string(),
        1 => "Registered".to_string(),
        2 => "Deprecated".to_string(),
        3 => "Deleted".to_string(),
        _ => format!("Unknown({})", state),
    }
}
