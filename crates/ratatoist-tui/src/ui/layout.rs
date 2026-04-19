use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};

use crate::app::{App, DOCK_ITEMS, DockItem, Pane, SortMode, TaskFilter};

const STATS_HEIGHT: u16 = 4;
/// Pomodoro box reserved height (top border + 1 body line + bottom
/// border). Does not grow — see `pomodoro.spec.md`.
const POMODORO_HEIGHT: u16 = 3;
/// Each rendered star takes one glyph + one separator space. Used to
/// compute how many stars fit on a sidebar row before wrapping.
const STAR_CELL_WIDTH: u16 = 2;
/// Inner horizontal padding the star-jar block subtracts from its own
/// width (left border + left pad + right pad + right border).
const STAR_JAR_H_PADDING: u16 = 4;
/// Borders (top + bottom) added on top of the variable body height.
const STAR_JAR_BORDER_ROWS: u16 = 2;
/// Minimum rows the project list is allowed to keep when the star jar
/// grows. Defines the point at which we start collapsing five yellow
/// stars into one purple star to buy more vertical space.
const STAR_JAR_PROJECT_FLOOR: u16 = 3;
/// Ratio used when collapse is active: five completions → one purple star.
const STAR_COLLAPSE_GROUP: u64 = 5;

/// Kind of star rendered in the jar. Regular yellow for a single
/// completion, purple for a collapsed group of five.
#[derive(Copy, Clone)]
enum StarKind {
    Yellow,
    Purple,
}

/// Pre-computed rendering of the star jar for the current frame. Built
/// once in [`render`] so the block's `Constraint::Length` and the body
/// contents agree on height. See
/// [`star-jar.spec.md`](../../../specifications/star-jar.spec.md).
struct StarJarPlan {
    /// Rows the body (excluding borders) will occupy.
    body_rows: u16,
    /// One row per `Vec`, each holding the glyphs for that row in
    /// left-to-right order. Empty outer vec ↔ count was zero.
    rows: Vec<Vec<StarKind>>,
}

/// Plan the star-jar rendering for `count` completions given the sidebar
/// width and the vertical budget the block may occupy. When the naive
/// all-yellow rendering would exceed the budget, switch to collapsed
/// mode (5 yellow → 1 purple). If collapse still overflows, the extra
/// rows are kept in the plan and the caller will clip them — see the
/// spec for the "figure it out when we hit it" overflow note.
fn plan_star_jar(sidebar_width: u16, max_body_rows: u16, count: u64) -> StarJarPlan {
    if count == 0 {
        return StarJarPlan {
            body_rows: 1,
            rows: vec![Vec::new()],
        };
    }

    let inner_w = sidebar_width.saturating_sub(STAR_JAR_H_PADDING);
    let per_row = (inner_w / STAR_CELL_WIDTH).max(1) as u64;
    let naive_rows = count.div_ceil(per_row);
    let max = max_body_rows.max(1) as u64;

    let kinds: Vec<StarKind> = if naive_rows <= max {
        std::iter::repeat_n(StarKind::Yellow, count as usize).collect()
    } else {
        let purples = count / STAR_COLLAPSE_GROUP;
        let yellows = count % STAR_COLLAPSE_GROUP;
        let mut v: Vec<StarKind> =
            std::iter::repeat_n(StarKind::Purple, purples as usize).collect();
        v.extend(std::iter::repeat_n(StarKind::Yellow, yellows as usize));
        v
    };

    let per_row_usize = per_row as usize;
    let rows: Vec<Vec<StarKind>> = kinds
        .chunks(per_row_usize)
        .map(|c| c.to_vec())
        .collect();
    let body_rows = (rows.len() as u16).max(1);
    StarJarPlan { body_rows, rows }
}
use crate::ui::theme::Theme;

use super::keyhints;
use super::statusbar;
use super::views;

pub fn render(frame: &mut Frame, app: &App) {
    let theme = app.theme();
    let area = frame.area();

    let [main_area, status_area, hints_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let [left_area, right_area] =
        Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)])
            .areas(main_area);

    let projects_active = matches!(app.active_pane, Pane::Projects);
    let stats_active = matches!(app.active_pane, Pane::StatsDock);
    let settings_active = matches!(app.active_pane, Pane::Settings);

    // Budget the jar may occupy before collapsing. Reserve Stats (if
    // shown), the jar's own borders, optional settings block, and a floor
    // of project rows; whatever's left becomes the max body height for the
    // jar. When Stats is hidden via `show_stats=false`, those four rows
    // go to the project list (and, if the jar needs to grow, to the jar).
    let stats_rows: u16 = if app.show_stats { STATS_HEIGHT } else { 0 };
    let settings_rows: u16 = if app.show_settings { 5 } else { 0 };
    // Pomodoro box always renders at its fixed height, directly above
    // the Star Jar. Its rows come out of the project list's budget
    // alongside Stats and Settings.
    let max_body = left_area
        .height
        .saturating_sub(stats_rows)
        .saturating_sub(POMODORO_HEIGHT)
        .saturating_sub(STAR_JAR_BORDER_ROWS)
        .saturating_sub(settings_rows)
        .saturating_sub(STAR_JAR_PROJECT_FLOOR)
        .max(1);
    let plan = plan_star_jar(left_area.width, max_body, app.star_count);
    let jar_height = plan.body_rows.saturating_add(STAR_JAR_BORDER_ROWS);

    // Build the left-column constraints dynamically: the Stats and
    // Settings slots are elided entirely when their feature flag is off,
    // so the corresponding pane can never capture focus of a block that
    // isn't on screen. The Pomodoro box is always present.
    let mut constraints: Vec<Constraint> = vec![Constraint::Min(1)];
    if app.show_stats {
        constraints.push(Constraint::Length(STATS_HEIGHT));
    }
    constraints.push(Constraint::Length(POMODORO_HEIGHT));
    constraints.push(Constraint::Length(jar_height));
    if app.show_settings {
        constraints.push(Constraint::Length(settings_rows));
    }
    let areas = Layout::vertical(constraints).split(left_area);
    let mut idx = 0;
    let projects_area = areas[idx];
    idx += 1;
    let stats_area = if app.show_stats {
        let a = areas[idx];
        idx += 1;
        Some(a)
    } else {
        None
    };
    let pomodoro_area = areas[idx];
    idx += 1;
    let star_area = areas[idx];
    idx += 1;
    let settings_area = if app.show_settings {
        Some(areas[idx])
    } else {
        None
    };

    render_projects_block(frame, app, projects_area, projects_active);
    if let Some(stats_area) = stats_area {
        render_stats_block(frame, app, stats_area, stats_active);
    }
    render_pomodoro_block(frame, app, pomodoro_area);
    render_star_jar_block(frame, app, star_area, &plan);
    if let Some(settings_area) = settings_area {
        views::settings::render(frame, app, settings_area, settings_active);
    }

    if matches!(app.active_pane, Pane::Detail) {
        if let Some(task) = app.selected_task() {
            let task = task.clone();
            let comments = app.comments.clone();
            views::detail::render(
                frame,
                &task,
                &comments,
                &app.user_names,
                app.current_user_id.as_deref(),
                right_area,
                app.detail_scroll,
                app.detail_field,
                theme,
            );
        }
    } else {
        let tasks_active = matches!(app.active_pane, Pane::Tasks);
        render_tasks_block(frame, app, right_area, tasks_active);
    }

    // Toaster floats over the bottom-right of the right-hand pane. Last
    // in render order so it overlays whatever's below it. Invisible
    // when no pomodoro is running — see `pomodoro.spec.md`.
    render_pomodoro_toaster(frame, app, right_area);

    statusbar::render(frame, app, status_area);
    keyhints::render(frame, app, hints_area);
}

fn render_projects_block(frame: &mut Frame, app: &App, area: Rect, active: bool) {
    let theme = app.theme();

    let block = Block::default()
        .title(" Projects ")
        .title_style(if active {
            theme.active_title()
        } else {
            theme.title()
        })
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(if active {
            theme.active_border()
        } else {
            theme.inactive_border()
        })
        .padding(Padding::horizontal(1))
        .style(theme.base_bg());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    views::projects::render(frame, app, inner, active);
}

fn dock_filter_color(filter: DockItem, theme: &Theme) -> Color {
    match filter {
        DockItem::DueOverdue => theme.red,
        DockItem::DueToday => theme.yellow,
        DockItem::DueWeek => theme.cyan,
        DockItem::Priority(4) => theme.red,
        DockItem::Priority(3) => theme.yellow,
        DockItem::Priority(2) => theme.maroon,
        DockItem::Priority(_) => theme.subtle,
    }
}

fn render_tasks_block(frame: &mut Frame, app: &App, area: Rect, active: bool) {
    let theme = app.theme();

    let (title, title_style, border_style) = if let Some(filter) = app.dock_filter {
        let color = dock_filter_color(filter, theme);
        let s = Style::default().fg(color);
        (format!(" ◈ {} ", filter.hint()), s, s)
    } else {
        (
            format!(" {} ", app.selected_project_name()),
            if active {
                theme.active_title()
            } else {
                theme.title()
            },
            if active {
                theme.active_border()
            } else {
                theme.inactive_border()
            },
        )
    };

    let block = Block::default()
        .title(title)
        .title_style(title_style)
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(border_style)
        .padding(Padding::horizontal(1))
        .style(theme.base_bg());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.all_view_active {
        views::all::render(frame, app, inner, active);
    } else if app.is_pr_view_active() {
        let [hint_area, prs_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
        render_external_hint_row(frame, app, hint_area, app.github_prs_fetched_at, app.github_prs_loading);
        views::github_prs::render(frame, app, prs_area, active);
    } else if app.jira_cards_view_active {
        let [hint_area, cards_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
        render_external_hint_row(frame, app, hint_area, app.jira_cards_fetched_at, app.jira_cards_loading);
        views::jira_cards::render(frame, app, cards_area, active);
    } else if app.agenda_view_active {
        let [hint_area, events_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
        render_external_hint_row(frame, app, hint_area, app.agenda_fetched_at, app.agenda_loading);
        views::agenda::render(frame, app, events_area, active);
    } else if app.dock_filter.is_some() {
        let [filter_area, banner_area, tasks_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(inner);
        render_filter_row(frame, app, filter_area);
        render_filter_banner(frame, app, banner_area);
        views::tasks::render(frame, app, tasks_area, active);
    } else {
        let [filter_area, tasks_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
        render_filter_row(frame, app, filter_area);
        views::tasks::render(frame, app, tasks_area, active);
    }
}

fn render_external_hint_row(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    fetched_at: Option<chrono::DateTime<chrono::Local>>,
    loading: bool,
) {
    let theme = app.theme();
    let fetched = fetched_at
        .map(|at| at.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "—".to_string());
    let loading_label = if loading { "  refreshing…" } else { "" };
    let line = Line::from(vec![
        Span::styled("Enter ", theme.key_hint()),
        Span::styled("open  ", theme.muted_text()),
        Span::styled("r ", theme.key_hint()),
        Span::styled("refresh  ", theme.muted_text()),
        Span::styled(
            format!("· fetched {fetched}{loading_label}"),
            theme.muted_text(),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_filter_banner(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let Some(filter) = app.dock_filter else {
        return;
    };
    let color = dock_filter_color(filter, theme);
    let banner = Style::default()
        .fg(theme.base)
        .bg(color)
        .add_modifier(Modifier::BOLD);
    let hint = Style::default().fg(color).bg(theme.surface);
    let line = Line::from(vec![
        Span::styled(format!(" ◈ {}  ", filter.hint()), banner),
        Span::styled("Esc: clear", hint),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme.surface)),
        area,
    );
}

fn render_filter_row(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();

    let style_for = |f: TaskFilter| {
        if app.task_filter == f {
            theme.active_title()
        } else {
            theme.muted_text()
        }
    };

    let mut spans = vec![
        Span::styled("Active", style_for(TaskFilter::Active)),
        Span::styled("  ", theme.muted_text()),
        Span::styled("Done", style_for(TaskFilter::Done)),
        Span::styled("  ", theme.muted_text()),
        Span::styled("Both", style_for(TaskFilter::Both)),
    ];

    if app.sort_mode != SortMode::Default {
        spans.push(Span::styled(
            format!("   ⟳ {}", app.sort_mode.label()),
            theme.due_upcoming(),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Preferred toaster width (outer, including borders). Clamped down
/// when the right pane is narrower than this.
const TOASTER_WIDTH: u16 = 40;

/// Truncate a task title to fit the toaster's inner width, appending
/// `…` when characters had to drop. Operates on `chars` so multi-byte
/// content (emoji, accents) doesn't produce a panic-y byte boundary.
fn truncate_for_toaster(title: &str, inner_width: u16) -> String {
    let width = inner_width as usize;
    let char_count = title.chars().count();
    if char_count <= width {
        return title.to_string();
    }
    if width <= 1 {
        return "…".to_string();
    }
    let mut out: String = title.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Floating "toaster" block rendered in the bottom-right of the right
/// pane while a pomodoro is running. Top row is the MM:SS countdown;
/// subsequent rows list titles of tasks completed during the session,
/// newest first. Width is `TOASTER_WIDTH` clamped to `right_area.width`;
/// height is `1 (countdown) + tasks.len() + 2 (borders)`, clamped so
/// the toaster never exceeds the pane. Vanishes when no pomodoro runs.
/// See [`pomodoro.spec.md`](../../../specifications/pomodoro.spec.md).
fn render_pomodoro_toaster(frame: &mut Frame, app: &App, pane: Rect) {
    let Some(remaining) = app.pomodoro_remaining() else {
        return;
    };
    if pane.width < 6 || pane.height < 3 {
        return;
    }

    let theme = app.theme();
    let width = TOASTER_WIDTH.min(pane.width);
    let task_rows = app.pomodoro_session_tasks.len() as u16;
    let desired_height = 1u16.saturating_add(task_rows).saturating_add(2);
    let height = desired_height.min(pane.height);
    // Anchor to the bottom-right of the pane.
    let x = pane.x.saturating_add(pane.width.saturating_sub(width));
    let y = pane.y.saturating_add(pane.height.saturating_sub(height));
    let area = Rect {
        x,
        y,
        width,
        height,
    };

    // Clear whatever's behind the toaster so rendered borders don't
    // composite over the tasks list.
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" 🍅 session ")
        .title_style(theme.title())
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(theme.inactive_border())
        .padding(Padding::horizontal(1))
        .style(theme.base_bg());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let total = remaining.as_secs();
    let countdown = format!("🍅 {:02}:{:02}", total / 60, total % 60);

    let mut lines: Vec<Line> = Vec::with_capacity(1 + app.pomodoro_session_tasks.len());
    lines.push(Line::from(Span::styled(countdown, theme.due_today())));
    // Inner width minus the `✓ ` prefix (two cells).
    let title_width = inner.width.saturating_sub(2);
    for title in &app.pomodoro_session_tasks {
        lines.push(Line::from(vec![
            Span::styled("✓ ", theme.success()),
            Span::styled(
                truncate_for_toaster(title, title_width),
                theme.normal_text(),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Passive counter block just above the Star Jar. One 🍅 per completed
/// pomodoro today; `—` when empty. Fixed 3-row height regardless of
/// count — overflow clips per the spec. See
/// [`pomodoro.spec.md`](../../../specifications/pomodoro.spec.md).
fn render_pomodoro_block(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let block = Block::default()
        .title(" Pomodoros ")
        .title_style(theme.title())
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(theme.inactive_border())
        .padding(Padding::horizontal(1))
        .style(theme.base_bg());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let line = if app.tomato_count == 0 {
        Line::from(Span::styled("—", theme.muted_text()))
    } else {
        let mut spans: Vec<Span> = Vec::with_capacity(app.tomato_count as usize * 2);
        for i in 0..app.tomato_count {
            if i > 0 {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::raw("🍅"));
        }
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(line), inner);
}

/// Passive counter block at the bottom of the left sidebar. One yellow
/// ★ per task completion, collapsing to purple ★ (×5) when rows run out.
/// No focus, no keybinding. See
/// [`star-jar.spec.md`](../../../specifications/star-jar.spec.md).
fn render_star_jar_block(frame: &mut Frame, app: &App, area: Rect, plan: &StarJarPlan) {
    let theme = app.theme();
    let block = Block::default()
        .title(" Star jar ")
        .title_style(theme.title())
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(theme.inactive_border())
        .padding(Padding::horizontal(1))
        .style(theme.base_bg());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let yellow = Style::default().fg(Color::Yellow);
    let purple = Style::default().fg(Color::Magenta);
    let lines: Vec<Line> = if app.star_count == 0 {
        vec![Line::from(Span::styled("—", theme.muted_text()))]
    } else {
        plan.rows
            .iter()
            .map(|row| {
                let mut spans: Vec<Span> = Vec::with_capacity(row.len() * 2);
                for (i, kind) in row.iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::raw(" "));
                    }
                    let style = match kind {
                        StarKind::Yellow => yellow,
                        StarKind::Purple => purple,
                    };
                    spans.push(Span::styled("★", style));
                }
                Line::from(spans)
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_stats_block(frame: &mut Frame, app: &App, area: Rect, active: bool) {
    let theme = app.theme();
    let stats = app.overview_stats();

    let title = if let Some(idx) = app.dock_focus {
        let hint = DOCK_ITEMS[idx].hint();
        format!(" Stats → {hint} ")
    } else {
        " Stats ".to_string()
    };

    let block = Block::default()
        .title(title)
        .title_style(if active {
            theme.active_title()
        } else {
            theme.muted_text()
        })
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(if active {
            theme.active_border()
        } else {
            theme.inactive_border()
        })
        .padding(Padding::horizontal(1))
        .style(theme.base_bg());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [due_area, prio_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    let dock_style = |item: DockItem, idx: usize, base: ratatui::style::Style| {
        if app.dock_focus == Some(idx) {
            theme.dock_focused_item()
        } else if app.dock_filter == Some(item) {
            theme.active_title()
        } else {
            base
        }
    };

    let overdue_base = if stats.overdue > 0 {
        theme.due_overdue()
    } else {
        theme.muted_text()
    };

    let due_line = Line::from(vec![
        Span::styled("Due  ", theme.muted_text()),
        Span::styled(
            format!("▲ {}  ", stats.overdue),
            dock_style(DockItem::DueOverdue, 0, overdue_base),
        ),
        Span::styled(
            format!("◆ {}  ", stats.due_today),
            dock_style(DockItem::DueToday, 1, theme.due_today()),
        ),
        Span::styled(
            format!("◇ {}", stats.due_week),
            dock_style(DockItem::DueWeek, 2, theme.due_upcoming()),
        ),
    ]);

    let p = &stats.by_priority;
    let prio_line = Line::from(vec![
        Span::styled("P    ", theme.muted_text()),
        Span::styled(
            format!("● {}  ", p[4]),
            dock_style(DockItem::Priority(4), 3, theme.priority_style(4)),
        ),
        Span::styled(
            format!("● {}  ", p[3]),
            dock_style(DockItem::Priority(3), 4, theme.priority_style(3)),
        ),
        Span::styled(
            format!("● {}  ", p[2]),
            dock_style(DockItem::Priority(2), 5, theme.priority_style(2)),
        ),
        Span::styled(
            format!("─ {}", p[1]),
            dock_style(DockItem::Priority(1), 6, theme.muted_text()),
        ),
    ]);

    frame.render_widget(Paragraph::new(due_line), due_area);
    frame.render_widget(Paragraph::new(prio_line), prio_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_kind(rows: &[Vec<StarKind>], kind: fn(&StarKind) -> bool) -> usize {
        rows.iter().flatten().filter(|k| kind(k)).count()
    }

    #[test]
    fn star_jar_plan_wraps_individual_stars_within_budget() {
        // 30-wide sidebar → inner 26 → per_row = 13 stars. With max_body
        // = 5 the budget is 5 * 13 = 65 yellow stars before collapse.
        let plan = plan_star_jar(30, 5, 7);
        assert_eq!(plan.body_rows, 1, "7 stars fit on one row");
        assert_eq!(plan.rows.len(), 1);
        assert_eq!(plan.rows[0].len(), 7);
        assert!(matches!(plan.rows[0][0], StarKind::Yellow));

        let plan = plan_star_jar(30, 5, 15);
        assert_eq!(plan.body_rows, 2, "15 stars wrap to second row");
        assert_eq!(plan.rows[0].len(), 13);
        assert_eq!(plan.rows[1].len(), 2);
    }

    #[test]
    fn star_jar_plan_collapses_to_purple_when_yellows_overflow() {
        // Budget of 1 row * 13 per_row = 13 yellows max. 27 yellows would
        // need 3 rows — overflow — so collapse to 5 purple + 2 yellow = 7
        // glyphs, fits in one row.
        let plan = plan_star_jar(30, 1, 27);
        assert_eq!(plan.body_rows, 1);
        assert_eq!(
            count_kind(&plan.rows, |k| matches!(k, StarKind::Purple)),
            5
        );
        assert_eq!(
            count_kind(&plan.rows, |k| matches!(k, StarKind::Yellow)),
            2
        );
    }

    #[test]
    fn star_jar_plan_empty_count_keeps_single_body_row() {
        let plan = plan_star_jar(30, 3, 0);
        assert_eq!(plan.body_rows, 1);
        assert_eq!(plan.rows.len(), 1);
        assert!(plan.rows[0].is_empty());
    }

    #[test]
    fn truncate_for_toaster_shortens_with_ellipsis() {
        assert_eq!(truncate_for_toaster("short", 30), "short");
        assert_eq!(
            truncate_for_toaster("this title is too long to fit", 10),
            "this titl…"
        );
        // Degenerate widths don't panic.
        assert_eq!(truncate_for_toaster("anything", 1), "…");
        assert_eq!(truncate_for_toaster("anything", 0), "…");
    }
}
