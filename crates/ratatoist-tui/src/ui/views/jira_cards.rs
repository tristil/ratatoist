use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use crate::app::App;

fn issue_type_glyph(issue_type: &str) -> &'static str {
    match issue_type {
        "Bug" => "✦ ",
        "Story" => "◈ ",
        "Task" => "☐ ",
        "Epic" => "▼ ",
        "Sub-task" | "Subtask" => "⤷ ",
        "" => "  ",
        _ => "• ",
    }
}

pub fn render(frame: &mut Frame, app: &App, area: Rect, is_active: bool) {
    let theme = app.theme();

    if app.jira_cards_loading && app.jira_cards.is_empty() {
        let lines = vec![
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "Fetching Jira cards…",
                theme.muted_text(),
            ))),
        ];
        frame.render_widget(List::new(lines), area);
        return;
    }

    if let Some(err) = &app.jira_cards_error {
        let lines = vec![
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "Failed to fetch Jira cards:",
                theme.due_overdue().add_modifier(Modifier::BOLD),
            ))),
            ListItem::new(Line::from(Span::styled(err.clone(), theme.muted_text()))),
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "Press r to retry. If unauthenticated, run `acli jira auth login`.",
                theme.muted_text(),
            ))),
        ];
        frame.render_widget(List::new(lines), area);
        return;
    }

    if app.jira_cards.is_empty() {
        let lines = vec![
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "No open Jira cards assigned to you.",
                theme.muted_text(),
            ))),
        ];
        frame.render_widget(List::new(lines), area);
        return;
    }

    let mut items: Vec<ListItem> = Vec::new();
    let mut current_project: Option<String> = None;
    let mut visual_selected: Option<usize> = None;

    for (idx, card) in app.jira_cards.iter().enumerate() {
        if current_project.as_deref() != Some(card.project_key.as_str()) {
            if !items.is_empty() {
                items.push(ListItem::new(Line::default()));
            }
            items.push(ListItem::new(Line::from(Span::styled(
                format!("  {}", card.project_key),
                theme.muted_text().add_modifier(Modifier::BOLD),
            ))));
            current_project = Some(card.project_key.clone());
        }

        let mut spans = vec![Span::styled("  ", theme.muted_text())];
        spans.push(Span::styled(
            issue_type_glyph(&card.issue_type),
            theme.muted_text(),
        ));
        spans.push(Span::styled(
            format!("{:<10}", card.key),
            theme.key_hint(),
        ));
        spans.push(Span::styled(&card.summary, theme.normal_text()));
        if !card.status.is_empty() {
            spans.push(Span::styled(
                format!("  · {}", card.status),
                theme.muted_text(),
            ));
        }
        if !card.priority.is_empty() && card.priority != "Medium" {
            spans.push(Span::styled(
                format!("  · {}", card.priority),
                theme.muted_text(),
            ));
        }

        if idx == app.selected_jira_card {
            visual_selected = Some(items.len());
        }
        items.push(ListItem::new(Line::from(spans)));
    }

    let highlight_style = if is_active {
        theme.selected_item()
    } else {
        theme.subtle_text()
    };

    let list = List::new(items).highlight_style(highlight_style);
    let mut state = ListState::default().with_selected(visual_selected);
    frame.render_stateful_widget(list, area, &mut state);
}
