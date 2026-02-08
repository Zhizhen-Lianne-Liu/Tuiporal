use crate::app::App;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Tuiporal - Temporal TUI Client",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Global Navigation",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  1", Style::default().fg(Color::Yellow)),
            Span::raw("         → Switch to Workflows screen"),
        ]),
        Line::from(vec![
            Span::styled("  2", Style::default().fg(Color::Yellow)),
            Span::raw("         → Switch to Namespaces screen"),
        ]),
        Line::from(vec![
            Span::styled("  3", Style::default().fg(Color::Yellow)),
            Span::raw("         → Switch to Workflow Detail screen"),
        ]),
        Line::from(vec![
            Span::styled("  4", Style::default().fg(Color::Yellow)),
            Span::raw("         → Switch to Settings screen"),
        ]),
        Line::from(vec![
            Span::styled("  ?", Style::default().fg(Color::Yellow)),
            Span::raw("         → Show this help screen"),
        ]),
        Line::from(vec![
            Span::styled("  q/ESC", Style::default().fg(Color::Yellow)),
            Span::raw("     → Quit or go back"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Workflows Screen",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  ↑/k, ↓/j", Style::default().fg(Color::Yellow)),
            Span::raw("  → Navigate up/down"),
        ]),
        Line::from(vec![
            Span::styled("  Enter", Style::default().fg(Color::Yellow)),
            Span::raw("      → View workflow details"),
        ]),
        Line::from(vec![
            Span::styled("  /", Style::default().fg(Color::Yellow)),
            Span::raw("         → Search workflows (Temporal visibility query)"),
        ]),
        Line::from(vec![
            Span::styled("  f", Style::default().fg(Color::Yellow)),
            Span::raw("         → Cycle through status filters (Running/Completed/Failed/etc)"),
        ]),
        Line::from(vec![
            Span::styled("  c", Style::default().fg(Color::Yellow)),
            Span::raw("         → Clear search and filters"),
        ]),
        Line::from(vec![
            Span::styled("  r", Style::default().fg(Color::Yellow)),
            Span::raw("         → Refresh workflow list"),
        ]),
        Line::from(vec![
            Span::styled("  a", Style::default().fg(Color::Yellow)),
            Span::raw("         → Toggle auto-refresh (5s interval)"),
        ]),
        Line::from(vec![
            Span::styled("  →/n", Style::default().fg(Color::Yellow)),
            Span::raw("       → Next page (if available)"),
        ]),
        Line::from(vec![
            Span::styled("  ←/p", Style::default().fg(Color::Yellow)),
            Span::raw("       → Previous page (if available)"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Workflow Detail Screen",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  ↑/k, ↓/j", Style::default().fg(Color::Yellow)),
            Span::raw("  → Navigate event history"),
        ]),
        Line::from(vec![
            Span::styled("  Enter", Style::default().fg(Color::Yellow)),
            Span::raw("      → View event details"),
        ]),
        Line::from(vec![
            Span::styled("  t", Style::default().fg(Color::Yellow)),
            Span::raw("         → Terminate workflow"),
        ]),
        Line::from(vec![
            Span::styled("  x", Style::default().fg(Color::Yellow)),
            Span::raw("         → Cancel workflow"),
        ]),
        Line::from(vec![
            Span::styled("  s", Style::default().fg(Color::Yellow)),
            Span::raw("         → Signal workflow"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Namespaces Screen",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  ↑/k, ↓/j", Style::default().fg(Color::Yellow)),
            Span::raw("  → Navigate namespaces"),
        ]),
        Line::from(vec![
            Span::styled("  Enter", Style::default().fg(Color::Yellow)),
            Span::raw("      → Switch to selected namespace"),
        ]),
        Line::from(vec![
            Span::styled("  r", Style::default().fg(Color::Yellow)),
            Span::raw("         → Refresh namespace list"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Tips",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::raw("  • Use "),
            Span::styled("Temporal visibility queries", Style::default().fg(Color::Yellow)),
            Span::raw(" in search (e.g., WorkflowType='MyWorkflow')"),
        ]),
        Line::from(vec![
            Span::raw("  • Press "),
            Span::styled("a", Style::default().fg(Color::Yellow)),
            Span::raw(" to enable auto-refresh for real-time monitoring"),
        ]),
        Line::from(vec![
            Span::raw("  • Filters and searches can be combined for precise results"),
        ]),
        Line::from(vec![
            Span::raw("  • Current namespace is shown in the Settings screen"),
        ]),
    ];

    let total_lines = lines.len() as u16;
    let scroll_offset = app.help_state.scroll_offset;

    // Calculate if we can scroll more
    let visible_lines = area.height.saturating_sub(2); // Subtract borders
    let max_scroll = total_lines.saturating_sub(visible_lines);
    let can_scroll_down = scroll_offset < max_scroll;
    let can_scroll_up = scroll_offset > 0;

    // Add scroll indicators to title
    let mut title = "Help - Press ? or ESC to close".to_string();
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

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(Style::default().fg(Color::White)),
        )
        .scroll((scroll_offset, 0));

    frame.render_widget(paragraph, area);
}
