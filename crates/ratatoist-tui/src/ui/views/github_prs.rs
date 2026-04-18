use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use crate::app::App;

/// Format an ISO-8601 UTC timestamp as a rough relative age ("3h ago", "2d").
/// Returns None if parsing fails.
fn relative_time(iso: &str) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(iso).ok()?;
    let now = chrono::Utc::now();
    let secs = (now - dt.with_timezone(&chrono::Utc)).num_seconds();
    Some(match secs {
        s if s < 60 => format!("{s}s ago"),
        s if s < 3_600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3_600),
        s if s < 2_592_000 => format!("{}d ago", s / 86_400),
        s => format!("{}mo ago", s / 2_592_000),
    })
}

pub fn render(frame: &mut Frame, app: &App, area: Rect, is_active: bool) {
    let theme = app.theme();

    let prs = app.active_org_prs();

    if app.github_prs_loading && prs.is_empty() {
        let lines = vec![
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "Fetching pull requests…",
                theme.muted_text(),
            ))),
        ];
        frame.render_widget(List::new(lines), area);
        return;
    }

    if let Some(err) = &app.github_prs_error {
        let lines = vec![
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "Failed to fetch pull requests:",
                theme.due_overdue().add_modifier(Modifier::BOLD),
            ))),
            ListItem::new(Line::from(Span::styled(
                err.clone(),
                theme.muted_text(),
            ))),
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "Press r to retry.",
                theme.muted_text(),
            ))),
        ];
        frame.render_widget(List::new(lines), area);
        return;
    }

    if prs.is_empty() {
        let lines = vec![
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "No open pull requests in this org.",
                theme.muted_text(),
            ))),
        ];
        frame.render_widget(List::new(lines), area);
        return;
    }

    let mut items: Vec<ListItem> = Vec::new();
    let mut current_repo: Option<String> = None;
    let mut visual_selected: Option<usize> = None;

    for (pr_idx, pr) in prs.iter().enumerate() {
        if current_repo.as_deref() != Some(pr.repo_full_name.as_str()) {
            if !items.is_empty() {
                items.push(ListItem::new(Line::default()));
            }
            items.push(ListItem::new(Line::from(Span::styled(
                format!("  {}", pr.repo_full_name),
                theme.muted_text().add_modifier(Modifier::BOLD),
            ))));
            current_repo = Some(pr.repo_full_name.clone());
        }

        let mut spans = vec![Span::styled("  ", theme.muted_text())];
        if pr.is_draft {
            spans.push(Span::styled("◌ ", theme.muted_text()));
        } else {
            spans.push(Span::styled("● ", theme.success()));
        }
        spans.push(crate::ui::check_status_span(pr.check_status, theme));
        spans.push(Span::styled(
            format!("#{}  ", pr.number),
            theme.muted_text(),
        ));
        spans.push(Span::styled(&pr.title, theme.normal_text()));
        // Author is usually @me for this list, but still show it for clarity.
        if !pr.author_login.is_empty() {
            spans.push(Span::styled(
                format!("  @{}", pr.author_login),
                theme.muted_text(),
            ));
        }
        if let Some(rel) = relative_time(&pr.updated_at) {
            spans.push(Span::styled(format!("  · {rel}"), theme.muted_text()));
        }

        if pr_idx == app.selected_pr {
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
