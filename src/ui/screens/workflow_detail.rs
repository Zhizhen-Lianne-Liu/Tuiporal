use crate::app::{App, WorkflowOperation};
use crate::generated::temporal::api::enums::v1::WorkflowExecutionStatus;
use chrono::{DateTime, Utc};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect, Alignment},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Clear},
    Frame,
};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let state = &app.workflow_detail_state;

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
                "Press 'ESC' to go back",
                Style::default().fg(Color::Yellow),
            )),
        ];
        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Workflow Detail - Error")
                    .style(Style::default().fg(Color::Red)),
            );
        frame.render_widget(paragraph, area);
        return;
    }

    // Show loading indicator
    if state.loading {
        let spinner = app.spinner();
        let loading_text = format!("{} Loading workflow details...", spinner);
        let paragraph = Paragraph::new(loading_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Workflow Detail")
                    .style(Style::default().fg(Color::Yellow)),
            )
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(paragraph, area);
        return;
    }

    // Show message if no workflow loaded
    if state.workflow.is_none() {
        let paragraph = Paragraph::new("No workflow loaded")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Workflow Detail")
                    .style(Style::default().fg(Color::White)),
            );
        frame.render_widget(paragraph, area);
        return;
    }

    // Split the area into sections
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),  // Metadata section
            Constraint::Min(0),      // Event history table
        ])
        .split(area);

    // Render metadata section
    render_workflow_metadata(app, frame, chunks[0]);

    // Render event history table
    render_event_history(app, frame, chunks[1]);

    // Render dialog overlay if needed
    if state.show_dialog.is_some() {
        render_operation_dialog(app, frame, area);
    }

    // Render success message overlay if needed
    if state.success_message.is_some() {
        render_success_message(app, frame, area);
    }

    // Render event detail modal if needed
    if state.show_event_detail {
        render_event_detail_modal(app, frame, area);
    }
}

fn render_workflow_metadata(app: &App, frame: &mut Frame, area: Rect) {
    let state = &app.workflow_detail_state;
    let workflow = state.workflow.as_ref().unwrap();

    let workflow_id = workflow
        .execution
        .as_ref()
        .map(|e| e.workflow_id.as_str())
        .unwrap_or("Unknown");

    let run_id = workflow
        .execution
        .as_ref()
        .map(|e| e.run_id.as_str())
        .unwrap_or("Unknown");

    let workflow_type = workflow
        .r#type
        .as_ref()
        .map(|t| t.name.as_str())
        .unwrap_or("Unknown");

    let status = WorkflowExecutionStatus::try_from(workflow.status)
        .unwrap_or(WorkflowExecutionStatus::Unspecified);
    let status_str = match status {
        WorkflowExecutionStatus::Running => "Running",
        WorkflowExecutionStatus::Completed => "Completed",
        WorkflowExecutionStatus::Failed => "Failed",
        WorkflowExecutionStatus::Canceled => "Canceled",
        WorkflowExecutionStatus::Terminated => "Terminated",
        WorkflowExecutionStatus::ContinuedAsNew => "Continued As New",
        WorkflowExecutionStatus::TimedOut => "Timed Out",
        _ => "Unknown",
    };
    let status_color = match status {
        WorkflowExecutionStatus::Running => Color::Yellow,
        WorkflowExecutionStatus::Completed => Color::Green,
        WorkflowExecutionStatus::Failed => Color::Red,
        WorkflowExecutionStatus::Canceled => Color::Magenta,
        WorkflowExecutionStatus::Terminated => Color::Red,
        WorkflowExecutionStatus::TimedOut => Color::Red,
        _ => Color::White,
    };

    let start_time = workflow
        .start_time
        .as_ref()
        .and_then(|t| {
            let seconds = t.seconds as i64;
            let nanos = t.nanos as u32;
            DateTime::from_timestamp(seconds, nanos)
                .map(|dt: DateTime<Utc>| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        })
        .unwrap_or_else(|| "Unknown".to_string());

    let close_time = workflow
        .close_time
        .as_ref()
        .and_then(|t| {
            let seconds = t.seconds as i64;
            let nanos = t.nanos as u32;
            DateTime::from_timestamp(seconds, nanos)
                .map(|dt: DateTime<Utc>| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        })
        .unwrap_or_else(|| "N/A".to_string());

    let lines = vec![
        Line::from(vec![
            Span::styled("Workflow ID: ", Style::default().fg(Color::Cyan)),
            Span::raw(workflow_id),
        ]),
        Line::from(vec![
            Span::styled("Run ID: ", Style::default().fg(Color::Cyan)),
            Span::raw(run_id),
        ]),
        Line::from(vec![
            Span::styled("Type: ", Style::default().fg(Color::Cyan)),
            Span::raw(workflow_type),
        ]),
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Cyan)),
            Span::styled(status_str, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Start Time: ", Style::default().fg(Color::Cyan)),
            Span::raw(start_time),
        ]),
        Line::from(vec![
            Span::styled("Close Time: ", Style::default().fg(Color::Cyan)),
            Span::raw(close_time),
        ]),
    ];

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Workflow Metadata")
                .style(Style::default().fg(Color::White)),
        );

    frame.render_widget(paragraph, area);
}

fn render_event_history(app: &App, frame: &mut Frame, area: Rect) {
    let state = &app.workflow_detail_state;

    if state.history.is_empty() {
        let paragraph = Paragraph::new("No history events found")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Event History")
                    .style(Style::default().fg(Color::White)),
            );
        frame.render_widget(paragraph, area);
        return;
    }

    // Build the table
    let header = Row::new(vec![
        Cell::from("Event ID"),
        Cell::from("Event Type"),
        Cell::from("Timestamp"),
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = state
        .history
        .iter()
        .map(|event| {
            let event_id = event.event_id.to_string();
            let event_type = get_event_type_name(event.event_type);
            let timestamp = event
                .event_time
                .as_ref()
                .and_then(|t| {
                    let seconds = t.seconds as i64;
                    let nanos = t.nanos as u32;
                    DateTime::from_timestamp(seconds, nanos)
                        .map(|dt: DateTime<Utc>| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                })
                .unwrap_or_else(|| "Unknown".to_string());

            Row::new(vec![
                Cell::from(event_id),
                Cell::from(event_type),
                Cell::from(timestamp),
            ])
        })
        .collect();

    let widths = [
        Constraint::Percentage(15),
        Constraint::Percentage(40),
        Constraint::Percentage(45),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Event History ({} events)", state.history.len()))
                .style(Style::default().fg(Color::White)),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_stateful_widget(table, area, &mut state.table_state.clone());
}

fn get_event_type_name(event_type: i32) -> String {
    // Map event type enum to human-readable names
    // This is a simplified version - you can expand this based on the proto definitions
    match event_type {
        1 => "WorkflowExecutionStarted".to_string(),
        2 => "WorkflowExecutionCompleted".to_string(),
        3 => "WorkflowExecutionFailed".to_string(),
        4 => "WorkflowExecutionTimedOut".to_string(),
        5 => "WorkflowTaskScheduled".to_string(),
        6 => "WorkflowTaskStarted".to_string(),
        7 => "WorkflowTaskCompleted".to_string(),
        8 => "WorkflowTaskTimedOut".to_string(),
        9 => "WorkflowTaskFailed".to_string(),
        10 => "ActivityTaskScheduled".to_string(),
        11 => "ActivityTaskStarted".to_string(),
        12 => "ActivityTaskCompleted".to_string(),
        13 => "ActivityTaskFailed".to_string(),
        14 => "ActivityTaskTimedOut".to_string(),
        15 => "ActivityTaskCancelRequested".to_string(),
        16 => "ActivityTaskCanceled".to_string(),
        17 => "TimerStarted".to_string(),
        18 => "TimerFired".to_string(),
        19 => "TimerCanceled".to_string(),
        20 => "WorkflowExecutionCancelRequested".to_string(),
        21 => "WorkflowExecutionCanceled".to_string(),
        22 => "RequestCancelExternalWorkflowExecutionInitiated".to_string(),
        23 => "RequestCancelExternalWorkflowExecutionFailed".to_string(),
        24 => "ExternalWorkflowExecutionCancelRequested".to_string(),
        25 => "MarkerRecorded".to_string(),
        26 => "WorkflowExecutionSignaled".to_string(),
        27 => "WorkflowExecutionTerminated".to_string(),
        28 => "WorkflowExecutionContinuedAsNew".to_string(),
        29 => "StartChildWorkflowExecutionInitiated".to_string(),
        30 => "StartChildWorkflowExecutionFailed".to_string(),
        31 => "ChildWorkflowExecutionStarted".to_string(),
        32 => "ChildWorkflowExecutionCompleted".to_string(),
        33 => "ChildWorkflowExecutionFailed".to_string(),
        34 => "ChildWorkflowExecutionCanceled".to_string(),
        35 => "ChildWorkflowExecutionTimedOut".to_string(),
        36 => "ChildWorkflowExecutionTerminated".to_string(),
        37 => "SignalExternalWorkflowExecutionInitiated".to_string(),
        38 => "SignalExternalWorkflowExecutionFailed".to_string(),
        39 => "ExternalWorkflowExecutionSignaled".to_string(),
        40 => "UpsertWorkflowSearchAttributes".to_string(),
        _ => format!("Unknown({})", event_type),
    }
}

fn render_operation_dialog(app: &App, frame: &mut Frame, area: Rect) {
    let state = &app.workflow_detail_state;
    let operation = state.show_dialog.as_ref().unwrap();

    // Create a centered dialog area
    let dialog_width = 60;
    let dialog_height = 8;
    let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    // Clear the area
    frame.render_widget(Clear, dialog_area);

    // Build dialog content based on operation type
    let (title, prompt, show_input) = match operation {
        WorkflowOperation::Terminate => (
            "Terminate Workflow",
            "Enter termination reason (or leave empty):",
            true,
        ),
        WorkflowOperation::Cancel => (
            "Cancel Workflow",
            "Are you sure you want to cancel this workflow?",
            false,
        ),
        WorkflowOperation::Signal => (
            "Signal Workflow",
            "Enter signal name:",
            true,
        ),
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(prompt, Style::default().fg(Color::White))),
        Line::from(""),
    ];

    if show_input {
        lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::styled(&state.dialog_input, Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" confirm | "),
        Span::styled("ESC", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw(" cancel"),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(Style::default().fg(Color::Yellow)),
        )
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, dialog_area);
}

fn render_success_message(app: &App, frame: &mut Frame, area: Rect) {
    let state = &app.workflow_detail_state;
    let message = state.success_message.as_ref().unwrap();

    // Create a centered message area
    let msg_width = 60;
    let msg_height = 5;
    let msg_x = (area.width.saturating_sub(msg_width)) / 2;
    let msg_y = (area.height.saturating_sub(msg_height)) / 2;
    let msg_area = Rect::new(msg_x, msg_y, msg_width, msg_height);

    // Clear the area
    frame.render_widget(Clear, msg_area);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(message, Style::default().fg(Color::White))),
        Line::from(""),
    ];

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Success")
                .style(Style::default().fg(Color::Green)),
        )
        .alignment(Alignment::Center);

    frame.render_widget(paragraph, msg_area);
}

fn render_event_detail_modal(app: &App, frame: &mut Frame, area: Rect) {
    let state = &app.workflow_detail_state;

    // Get the selected event
    let event = match state.selected_event() {
        Some(e) => e,
        None => return,
    };

    // Create a large modal area (80% of screen)
    let modal_width = (area.width * 4) / 5;
    let modal_height = (area.height * 4) / 5;
    let modal_x = (area.width.saturating_sub(modal_width)) / 2;
    let modal_y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(modal_x, modal_y, modal_width, modal_height);

    // Clear the area
    frame.render_widget(Clear, modal_area);

    // Build event details
    let mut lines = vec![];

    // Event ID and Type
    lines.push(Line::from(vec![
        Span::styled("Event ID: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(event.event_id.to_string()),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Event Type: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(get_event_type_name(event.event_type)),
    ]));

    // Timestamp
    if let Some(event_time) = &event.event_time {
        let timestamp = DateTime::from_timestamp(event_time.seconds as i64, event_time.nanos as u32)
            .map(|dt: DateTime<Utc>| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        lines.push(Line::from(vec![
            Span::styled("Timestamp: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(timestamp),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Event Attributes:",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Format event attributes based on type
    lines.extend(format_event_attributes(event));

    let total_lines = lines.len() as u16;
    let scroll_offset = state.event_detail_scroll_offset;

    // Calculate if we can scroll more
    let visible_lines = modal_area.height.saturating_sub(2); // Subtract borders
    let max_scroll = total_lines.saturating_sub(visible_lines);
    let can_scroll_down = scroll_offset < max_scroll;
    let can_scroll_up = scroll_offset > 0;

    // Add scroll indicators to title
    let mut title = "Event Details".to_string();
    if can_scroll_up || can_scroll_down {
        title.push_str(" | ");
        if can_scroll_up {
            title.push_str("↑ ");
        }
        title.push_str(&format!("({}/{})", scroll_offset + visible_lines.min(total_lines), total_lines));
        if can_scroll_down {
            title.push_str(" ↓");
        }
    }
    title.push_str(" | ESC/q to close");

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(Style::default().fg(Color::Cyan)),
        )
        .scroll((scroll_offset, 0))
        .wrap(ratatui::widgets::Wrap { trim: false });

    frame.render_widget(paragraph, modal_area);
}

fn format_event_attributes(event: &crate::generated::temporal::api::history::v1::HistoryEvent) -> Vec<Line<'static>> {
    let mut lines = vec![];

    // Use prost's reflection capabilities to format the event
    // For now, we'll show a simplified version with the most common attributes

    if let Some(attrs) = &event.attributes {
        // This is a oneof field - we need to handle each variant
        // For simplicity, we'll use debug formatting
        let debug_str = format!("{:?}", attrs);

        // Split into lines and format nicely
        for (i, line) in debug_str.lines().enumerate() {
            if i < 50 { // Limit to 50 lines to avoid overwhelming the display
                lines.push(Line::from(Span::raw(line.to_string())));
            }
        }

        if debug_str.lines().count() > 50 {
            lines.push(Line::from(Span::styled(
                "... (output truncated)",
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "No attributes available",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines
}
