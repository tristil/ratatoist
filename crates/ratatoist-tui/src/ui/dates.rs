use ratatui::style::Style;

use super::theme::Theme;
use ratatoist_core::api::models::Due;

pub struct FormattedDue {
    pub text: String,
    pub style: Style,
}

pub fn format_due(due: &Due, theme: &Theme) -> FormattedDue {
    let today = today_str();
    let date_str = &due.date;

    let days_away = days_between(&today, date_str);

    if days_away < 0 {
        return FormattedDue {
            text: display_label(due, days_away),
            style: theme.due_overdue(),
        };
    }

    if days_away == 0 {
        return FormattedDue {
            text: display_label(due, days_away),
            style: theme.due_today(),
        };
    }

    if days_away <= 6 {
        return FormattedDue {
            text: display_label(due, days_away),
            style: theme.due_upcoming(),
        };
    }

    FormattedDue {
        text: display_label(due, days_away),
        style: theme.due_future(),
    }
}

fn display_label(due: &Due, days_away: i64) -> String {
    // For recurring tasks, show the next-occurrence date (today / tomorrow /
    // weekday / short date) rather than the recurrence grammar ("every day").
    // The ↻ icon already indicates recurrence; the date is what the user
    // actually needs to see. For non-recurring tasks the `string` is the
    // user's literal input (e.g. "10 Apr", "next friday") — keep that.
    if !due.is_recurring
        && let Some(s) = &due.string
        && !s.is_empty()
    {
        return s.clone();
    }

    match days_away {
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        -1 => "yesterday".to_string(),
        -6..=-2 | 2..=6 => weekday_name(&due.date).to_string(),
        _ => format_short_date(&due.date),
    }
}

pub fn today_str() -> String {
    chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

/// Current local hour (0..=23). Used by the "hide evening tasks until 5 PM"
/// filter on the Today and All views.
pub fn current_local_hour() -> u32 {
    use chrono::Timelike;
    chrono::Local::now().hour()
}

/// True when a task carries the `evening` label and the current local hour
/// is before 17:00. Kept as a standalone predicate so tests can pass a
/// specific hour without touching the clock.
pub fn evening_task_hidden(labels: &[String], current_hour: u32) -> bool {
    current_hour < 17 && labels.iter().any(|l| l == "evening")
}

/// Format a `YYYY-MM-DD` date as a section header for the Upcoming view,
/// matching Todoist's style: `"15 Apr · Today · Wednesday"`,
/// `"16 Apr · Tomorrow · Thursday"`, or `"17 Apr · Friday"` for dates farther
/// out. Dates in the past render as `"1 Apr · Overdue · Tuesday"`.
pub fn format_upcoming_header(date_str: &str) -> String {
    let today = today_str();
    let days_away = days_between(&today, date_str);
    let (_, m, d) = parse_date(date_str).unwrap_or((0, 0, 0));
    let months = [
        "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = months.get(m as usize).unwrap_or(&"???");
    let weekday = weekday_name(date_str);

    let relative = match days_away {
        0 => Some("Today"),
        1 => Some("Tomorrow"),
        n if n < 0 => Some("Overdue"),
        _ => None,
    };

    match relative {
        Some(label) => format!("{d} {month} · {label} · {weekday}"),
        None => format!("{d} {month} · {weekday}"),
    }
}

fn weekday_name(date_str: &str) -> &'static str {
    let Some((y, m, d)) = parse_date(date_str) else {
        return "";
    };
    // Zeller-style weekday from civil days; Sunday = 0..Saturday = 6 in the
    // days_from_civil epoch (1970-01-01 is Thursday, days = 0).
    let days = days_from_civil(y, m, d);
    let wd = ((days % 7) + 7 + 4) % 7; // shift so Sunday = 0
    match wd {
        0 => "Sunday",
        1 => "Monday",
        2 => "Tuesday",
        3 => "Wednesday",
        4 => "Thursday",
        5 => "Friday",
        6 => "Saturday",
        _ => "",
    }
}

pub fn offset_days_str(days: i64) -> String {
    let today = chrono::Local::now().date_naive();
    (today + chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

fn parse_date(s: &str) -> Option<(i32, u32, u32)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

fn days_between(a: &str, b: &str) -> i64 {
    let da = parse_date(a).map(|(y, m, d)| days_from_civil(y, m, d));
    let db = parse_date(b).map(|(y, m, d)| days_from_civil(y, m, d));
    match (da, db) {
        (Some(a), Some(b)) => b - a,
        _ => 999,
    }
}

fn format_short_date(date_str: &str) -> String {
    let Some((_, m, d)) = parse_date(date_str) else {
        return date_str.to_string();
    };
    let months = [
        "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = months.get(m as usize).unwrap_or(&"???");
    format!("{month} {d}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn due(date: &str, recurring: bool, string: Option<&str>) -> Due {
        Due {
            date: date.to_string(),
            is_recurring: recurring,
            timezone: None,
            string: string.map(|s| s.to_string()),
            datetime: None,
            lang: None,
        }
    }

    #[test]
    fn evening_label_hidden_before_5pm_and_visible_after() {
        let evening = vec!["evening".to_string()];
        let other = vec!["work".to_string()];
        let none: Vec<String> = vec![];

        // Before 17:00 — evening hidden, others visible.
        assert!(evening_task_hidden(&evening, 9));
        assert!(evening_task_hidden(&evening, 16));
        assert!(!evening_task_hidden(&other, 9));
        assert!(!evening_task_hidden(&none, 9));

        // At/after 17:00 — nothing hidden by this rule.
        assert!(!evening_task_hidden(&evening, 17));
        assert!(!evening_task_hidden(&evening, 20));

        // Exact lowercase only — "Evening" is not matched.
        let capitalized = vec!["Evening".to_string()];
        assert!(!evening_task_hidden(&capitalized, 9));
    }

    #[test]
    fn recurring_today_shows_today_not_recurrence_string() {
        let today = today_str();
        let d = due(&today, true, Some("every day"));
        assert_eq!(display_label(&d, 0), "today");
    }

    #[test]
    fn recurring_tomorrow_shows_tomorrow() {
        let d = due(&offset_days_str(1), true, Some("every day"));
        assert_eq!(display_label(&d, 1), "tomorrow");
    }

    #[test]
    fn recurring_midweek_shows_weekday() {
        let d = due(&offset_days_str(3), true, Some("every Tue"));
        let label = display_label(&d, 3);
        // Should be a weekday name, not "every Tue".
        assert!(
            matches!(
                label.as_str(),
                "Sunday" | "Monday" | "Tuesday" | "Wednesday" | "Thursday" | "Friday" | "Saturday"
            ),
            "got {label:?}"
        );
    }

    #[test]
    fn non_recurring_keeps_user_string() {
        // User typed "10 Apr" — keep that verbatim.
        let d = due("2026-04-10", false, Some("10 Apr"));
        assert_eq!(display_label(&d, -5), "10 Apr");
    }

    #[test]
    fn non_recurring_without_string_computes_label() {
        let d = due("2026-04-10", false, None);
        assert_eq!(display_label(&d, -5), "Friday");
    }
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400) as u64;
    let m = m as u64;
    let d = d as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}
