use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use crate::app::{AllViewItem, App};
use crate::ui::dates;
use crate::ui::theme::Theme;

/// Rough relative-time formatter for PR timestamps (duplicated from
/// github_prs.rs to avoid a public export for a 10-line helper).
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
    let all_items = app.all_view_items();

    if all_items.is_empty() {
        let lines = vec![
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "No items — agenda events, tasks due today, open PRs, and Jira cards appear here.",
                theme.muted_text(),
            ))),
        ];
        frame.render_widget(List::new(lines), area);
        return;
    }

    let mut items: Vec<ListItem> = Vec::new();
    let mut visual_selected: Option<usize> = None;

    #[derive(PartialEq)]
    enum Section {
        Agenda,
        Task,
        Pr,
        Jira,
    }
    let mut last_section: Option<Section> = None;

    for (idx, item) in all_items.iter().enumerate() {
        match item {
            AllViewItem::AgendaEvent(event_idx) => {
                if last_section.as_ref() != Some(&Section::Agenda) {
                    if !items.is_empty() {
                        items.push(ListItem::new(Line::default()));
                    }
                    items.push(ListItem::new(Line::from(Span::styled(
                        "▸ Agenda",
                        theme.success().add_modifier(Modifier::BOLD),
                    ))));
                    last_section = Some(Section::Agenda);
                }
                if idx == app.selected_all_item {
                    visual_selected = Some(items.len());
                }
                items.push(build_agenda_row(app, *event_idx, theme));
            }
            AllViewItem::Task(task_idx) => {
                if last_section.as_ref() != Some(&Section::Task) {
                    if !items.is_empty() {
                        items.push(ListItem::new(Line::default()));
                    }
                    items.push(ListItem::new(Line::from(Span::styled(
                        "▸ Today",
                        theme.due_today().add_modifier(Modifier::BOLD),
                    ))));
                    last_section = Some(Section::Task);
                }
                if idx == app.selected_all_item {
                    visual_selected = Some(items.len());
                }
                items.push(build_task_row(app, *task_idx, theme));
            }
            AllViewItem::PullRequest(pr_idx) => {
                if last_section.as_ref() != Some(&Section::Pr) {
                    if !items.is_empty() {
                        items.push(ListItem::new(Line::default()));
                    }
                    items.push(ListItem::new(Line::from(Span::styled(
                        "▸ Pull Requests",
                        theme.priority_style(3).add_modifier(Modifier::BOLD),
                    ))));
                    last_section = Some(Section::Pr);
                }
                if idx == app.selected_all_item {
                    visual_selected = Some(items.len());
                }
                items.push(build_pr_row(app, *pr_idx, theme));
            }
            AllViewItem::JiraCard(card_idx) => {
                if last_section.as_ref() != Some(&Section::Jira) {
                    if !items.is_empty() {
                        items.push(ListItem::new(Line::default()));
                    }
                    items.push(ListItem::new(Line::from(Span::styled(
                        "▸ Jira Cards",
                        theme.key_hint().add_modifier(Modifier::BOLD),
                    ))));
                    last_section = Some(Section::Jira);
                }
                if idx == app.selected_all_item {
                    visual_selected = Some(items.len());
                }
                items.push(build_jira_row(app, *card_idx, theme));
            }
        }
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

fn build_task_row<'a>(app: &'a App, task_idx: usize, theme: &Theme) -> ListItem<'a> {
    let Some(task) = app.tasks.get(task_idx) else {
        return ListItem::new(Line::default());
    };
    let mut spans: Vec<Span<'a>> = Vec::new();
    spans.push(Span::styled("  ", theme.muted_text()));
    spans.push(Span::styled(
        Theme::priority_dot(task.priority),
        theme.priority_style(task.priority),
    ));
    spans.push(Span::styled(&task.content, theme.normal_text()));

    if task.due.as_ref().is_some_and(|d| d.is_recurring) {
        spans.push(Span::styled("  ↻", theme.muted_text()));
    }
    if let Some(due) = &task.due {
        let formatted = dates::format_due(due, theme);
        spans.push(Span::styled(
            format!("  {}", formatted.text),
            formatted.style,
        ));
    }
    // Show source project.
    let project_name = app
        .projects
        .iter()
        .find(|p| p.id == task.project_id)
        .map(|p| p.name.as_str())
        .unwrap_or("");
    if !project_name.is_empty() {
        spans.push(Span::styled(
            format!("  {project_name}"),
            theme.muted_text(),
        ));
    }
    ListItem::new(Line::from(spans))
}

fn build_pr_row<'a>(app: &'a App, pr_idx: usize, theme: &Theme) -> ListItem<'a> {
    let Some(pr) = app.github_prs.get(pr_idx) else {
        return ListItem::new(Line::default());
    };
    let mut spans: Vec<Span<'a>> = Vec::new();
    spans.push(Span::styled("  ", theme.muted_text()));
    if pr.is_draft {
        spans.push(Span::styled("◌ ", theme.muted_text()));
    } else {
        spans.push(Span::styled("● ", theme.success()));
    }
    spans.push(crate::ui::check_status_span(pr.check_status, theme));
    spans.push(Span::styled(
        format!("{}/#{} ", pr.repo_full_name, pr.number),
        theme.muted_text(),
    ));
    spans.push(Span::styled(&pr.title, theme.normal_text()));
    if let Some(rel) = relative_time(&pr.updated_at) {
        spans.push(Span::styled(format!("  · {rel}"), theme.muted_text()));
    }
    ListItem::new(Line::from(spans))
}

fn build_agenda_row<'a>(app: &'a App, event_idx: usize, theme: &Theme) -> ListItem<'a> {
    let Some(event) = app.agenda_events.get(event_idx) else {
        return ListItem::new(Line::default());
    };
    let time = agenda_time_label(event);
    let mut spans: Vec<Span<'a>> = Vec::new();
    spans.push(Span::styled("  ", theme.muted_text()));
    spans.push(Span::styled(
        format!("{:<8}", time),
        theme.key_hint(),
    ));
    spans.push(Span::styled(&event.summary, theme.normal_text()));
    if !event.location.is_empty() {
        spans.push(Span::styled(
            format!("  · {}", event.location),
            theme.muted_text(),
        ));
    }
    ListItem::new(Line::from(spans))
}

/// Compact time label for an agenda row: `"all day"` for all-day events,
/// `"9"` or `"9:30"` for timed events parsed as local time.
fn agenda_time_label(event: &crate::app::CalendarEvent) -> String {
    if event.all_day {
        return "all day".to_string();
    }
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&event.start) else {
        return event.start.clone();
    };
    let local = dt.with_timezone(&chrono::Local);
    let hour = local.format("%H").to_string();
    let minute = local.format("%M").to_string();
    if minute == "00" {
        hour
    } else {
        format!("{hour}:{minute}")
    }
}

fn build_jira_row<'a>(app: &'a App, card_idx: usize, theme: &Theme) -> ListItem<'a> {
    let Some(card) = app.jira_cards.get(card_idx) else {
        return ListItem::new(Line::default());
    };
    let mut spans: Vec<Span<'a>> = Vec::new();
    spans.push(Span::styled("  ", theme.muted_text()));
    spans.push(Span::styled(
        issue_type_glyph(&card.issue_type),
        theme.muted_text(),
    ));
    spans.push(Span::styled(
        format!("{:<12}", card.key),
        theme.key_hint(),
    ));
    spans.push(Span::styled(&card.summary, theme.normal_text()));
    if !card.status.is_empty() {
        spans.push(Span::styled(
            format!("  · {}", card.status),
            theme.muted_text(),
        ));
    }
    ListItem::new(Line::from(spans))
}
