use crate::app::{App, ConnectionStatus, WorkflowFilter};
use crate::generated::temporal::api::{
    enums::v1::WorkflowExecutionStatus, workflow::v1::WorkflowExecutionInfo,
};
use chrono::{DateTime, Utc};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let state = &app.workflow_list_state;

    // Split area if we need to show search/filter bar
    let show_search_bar = state.input_mode || state.active_filter.is_some() || !state.query.is_empty();
    let (search_area, table_area) = if show_search_bar {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };

    // Render search/filter bar if needed
    if let Some(search_rect) = search_area {
        render_search_bar(app, frame, search_rect);
    }

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
                    .title("Workflows - Error")
                    .style(Style::default().fg(Color::Red)),
            );
        frame.render_widget(paragraph, table_area);
        return;
    }

    // Show loading indicator
    if state.loading {
        let spinner = app.spinner();
        let loading_text = format!("{} Loading workflows...", spinner);
        let paragraph = Paragraph::new(loading_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Workflows")
                    .style(Style::default().fg(Color::Yellow)),
            )
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(paragraph, table_area);
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
                    .title("Workflows")
                    .style(Style::default().fg(color)),
            )
            .style(Style::default().fg(color));
        frame.render_widget(paragraph, table_area);
        return;
    }

    // Show empty message if no workflows
    if state.items.is_empty() {
        let lines = vec![
            Line::from("No workflows found"),
            Line::from(""),
            Line::from(Span::styled("Press 'r' to refresh", Style::default().fg(Color::Yellow))),
        ];
        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Workflows")
                    .style(Style::default().fg(Color::White)),
            );
        frame.render_widget(paragraph, table_area);
        return;
    }

    // Build the table
    let header = Row::new(vec![
        Cell::from("Workflow ID"),
        Cell::from("Type"),
        Cell::from("Status"),
        Cell::from("Start Time"),
    ])
    .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = state
        .items
        .iter()
        .map(|workflow| {
            let workflow_id = get_workflow_id(workflow);
            let workflow_type = get_workflow_type(workflow);
            let status = get_workflow_status(workflow);
            let start_time = get_workflow_start_time(workflow);

            let status_style = match status.0 {
                WorkflowExecutionStatus::Running => Style::default().fg(Color::Yellow),
                WorkflowExecutionStatus::Completed => Style::default().fg(Color::Green),
                WorkflowExecutionStatus::Failed => Style::default().fg(Color::Red),
                WorkflowExecutionStatus::Canceled => Style::default().fg(Color::Magenta),
                WorkflowExecutionStatus::Terminated => Style::default().fg(Color::Red),
                WorkflowExecutionStatus::TimedOut => Style::default().fg(Color::Red),
                _ => Style::default().fg(Color::White),
            };

            Row::new(vec![
                Cell::from(workflow_id),
                Cell::from(workflow_type),
                Cell::from(status.1).style(status_style),
                Cell::from(start_time),
            ])
        })
        .collect();

    let widths = [
        Constraint::Percentage(30),
        Constraint::Percentage(25),
        Constraint::Percentage(15),
        Constraint::Percentage(30),
    ];

    // Build title with pagination info and auto-refresh status
    let mut title = format!("Workflows ({} items)", state.items.len());
    if state.current_page > 1 || state.has_next_page() {
        title = format!("{} - Page {}", title, state.current_page);
        if state.has_next_page() {
            title = format!("{} [→]", title);
        }
    }
    if state.auto_refresh_enabled {
        title = format!("{} [Auto: {}s]", title, state.auto_refresh_interval_secs);
    }

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

    frame.render_stateful_widget(table, table_area, &mut state.table_state.clone());
}

fn render_search_bar(app: &App, frame: &mut Frame, area: Rect) {
    let state = &app.workflow_list_state;

    let mut spans = vec![];

    // Show filter if active
    if let Some(filter) = &state.active_filter {
        let filter_text = match filter {
            WorkflowFilter::All => "All",
            WorkflowFilter::Running => "Running",
            WorkflowFilter::Completed => "Completed",
            WorkflowFilter::Failed => "Failed",
            WorkflowFilter::Canceled => "Canceled",
        };
        spans.push(Span::styled("Filter: ", Style::default().fg(Color::Cyan)));
        spans.push(Span::styled(
            filter_text,
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" | "));
    }

    // Show search query
    if state.input_mode {
        spans.push(Span::styled("Search: ", Style::default().fg(Color::Cyan)));
        spans.push(Span::styled(
            &state.query,
            Style::default().fg(Color::White),
        ));
        spans.push(Span::styled("_", Style::default().fg(Color::Yellow))); // cursor
    } else if !state.query.is_empty() {
        spans.push(Span::styled("Query: ", Style::default().fg(Color::Cyan)));
        spans.push(Span::styled(
            &state.query,
            Style::default().fg(Color::White),
        ));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::White)),
    );

    frame.render_widget(paragraph, area);
}

fn get_workflow_id(workflow: &WorkflowExecutionInfo) -> String {
    workflow
        .execution
        .as_ref()
        .and_then(|e| Some(e.workflow_id.clone()))
        .unwrap_or_else(|| "Unknown".to_string())
}

fn get_workflow_type(workflow: &WorkflowExecutionInfo) -> String {
    workflow
        .r#type
        .as_ref()
        .and_then(|t| Some(t.name.clone()))
        .unwrap_or_else(|| "Unknown".to_string())
}

fn get_workflow_status(workflow: &WorkflowExecutionInfo) -> (WorkflowExecutionStatus, String) {
    let status = WorkflowExecutionStatus::try_from(workflow.status).unwrap_or(WorkflowExecutionStatus::Unspecified);
    let status_str = match status {
        WorkflowExecutionStatus::Running => "Running",
        WorkflowExecutionStatus::Completed => "Completed",
        WorkflowExecutionStatus::Failed => "Failed",
        WorkflowExecutionStatus::Canceled => "Canceled",
        WorkflowExecutionStatus::Terminated => "Terminated",
        WorkflowExecutionStatus::ContinuedAsNew => "Continued",
        WorkflowExecutionStatus::TimedOut => "Timed Out",
        _ => "Unknown",
    };
    (status, status_str.to_string())
}

fn get_workflow_start_time(workflow: &WorkflowExecutionInfo) -> String {
    workflow
        .start_time
        .as_ref()
        .and_then(|t| {
            let seconds = t.seconds as i64;
            let nanos = t.nanos as u32;
            DateTime::from_timestamp(seconds, nanos).map(|dt: DateTime<Utc>| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        })
        .unwrap_or_else(|| "Unknown".to_string())
}
