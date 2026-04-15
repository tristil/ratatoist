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
    if let Some(s) = &due.string
        && !s.is_empty()
    {
        return s.clone();
    }

    match days_away {
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        -1 => "yesterday".to_string(),
        _ => format_short_date(&due.date),
    }
}

pub fn today_str() -> String {
    chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
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
