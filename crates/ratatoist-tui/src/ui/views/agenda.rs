use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use crate::app::{App, CalendarEvent};

/// Format an event's start→end as a short time range, honoring local time.
/// All-day events return `"all day"`; events whose start parse fails fall
/// back to the raw string (shouldn't happen — GCal always returns valid
/// RFC3339 for timed events).
fn format_time_range(event: &CalendarEvent) -> String {
    if event.all_day {
        return "all day".to_string();
    }

    let Some(start) = parse_local_time(&event.start) else {
        return event.start.clone();
    };
    let Some(end) = parse_local_time(&event.end) else {
        return start;
    };
    // Drop `:00` minutes for readability (09:00 → 9, 09:30 stays 9:30).
    format!("{start}–{end}")
}

/// Parse an RFC3339 timestamp and format as a compact local time:
/// `"9"` or `"9:30"` (no AM/PM, 24h clock, minutes only when non-zero).
fn parse_local_time(iso: &str) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(iso).ok()?;
    let local = dt.with_timezone(&chrono::Local);
    let hour = local.format("%H").to_string();
    let minute = local.format("%M").to_string();
    if minute == "00" {
        Some(hour)
    } else {
        Some(format!("{hour}:{minute}"))
    }
}

pub fn render(frame: &mut Frame, app: &App, area: Rect, is_active: bool) {
    let theme = app.theme();

    if app.agenda_loading && app.agenda_events.is_empty() {
        let lines = vec![
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "Fetching today's agenda…",
                theme.muted_text(),
            ))),
        ];
        frame.render_widget(List::new(lines), area);
        return;
    }

    if let Some(err) = &app.agenda_error {
        let lines = vec![
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "Failed to fetch calendar events:",
                theme.due_overdue().add_modifier(Modifier::BOLD),
            ))),
            ListItem::new(Line::from(Span::styled(err.clone(), theme.muted_text()))),
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "Press r to retry. If unauthenticated, run `gws auth login`.",
                theme.muted_text(),
            ))),
        ];
        frame.render_widget(List::new(lines), area);
        return;
    }

    let visible_indices = app.visible_agenda_event_indices();
    if visible_indices.is_empty() {
        let lines = vec![
            ListItem::new(Line::default()),
            ListItem::new(Line::from(Span::styled(
                "No events scheduled for today.",
                theme.muted_text(),
            ))),
        ];
        frame.render_widget(List::new(lines), area);
        return;
    }

    // Pad the time column to the widest label so summaries line up.
    let time_col_width = visible_indices
        .iter()
        .filter_map(|i| app.agenda_events.get(*i))
        .map(|e| format_time_range(e).chars().count())
        .max()
        .unwrap_or(8)
        + 2;

    // Only surface the calendar-name chip when more than one calendar is
    // represented in today's list. If every event is from the same
    // calendar, the chip is redundant noise.
    let distinct_calendars: std::collections::HashSet<&str> = visible_indices
        .iter()
        .filter_map(|i| app.agenda_events.get(*i))
        .map(|e| e.calendar_name.as_str())
        .filter(|n| !n.is_empty())
        .collect();
    let show_calendar_chip = distinct_calendars.len() > 1;

    let mut items: Vec<ListItem> = Vec::new();
    let mut visual_selected: Option<usize> = None;

    for raw_idx in &visible_indices {
        let Some(event) = app.agenda_events.get(*raw_idx) else {
            continue;
        };
        let mut spans = vec![Span::styled("  ", theme.muted_text())];
        let time = format_time_range(event);
        let pad = time_col_width.saturating_sub(time.chars().count());
        spans.push(Span::styled(time, theme.key_hint()));
        spans.push(Span::styled(" ".repeat(pad), theme.muted_text()));
        spans.push(Span::styled(&event.summary, theme.normal_text()));
        if show_calendar_chip && !event.calendar_name.is_empty() {
            spans.push(Span::styled(
                format!("  · {}", event.calendar_name),
                theme.muted_text(),
            ));
        }
        if !event.location.is_empty() {
            spans.push(Span::styled(
                format!("  · {}", event.location),
                theme.muted_text(),
            ));
        }

        if *raw_idx == app.selected_agenda_item {
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
