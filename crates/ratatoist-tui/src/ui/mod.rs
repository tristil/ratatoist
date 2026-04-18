pub mod components;
pub mod dates;
pub mod keyhints;
pub mod layout;
pub mod setup;
pub mod splash;
pub mod statusbar;
pub mod theme;
pub mod views;

pub const LOGO: &str = r#"
░█▀▄░█▀█░▀█▀░█▀█░▀█▀░█▀█░▀█▀░█▀▀░▀█▀
░█▀▄░█▀█░░█░░█▀█░░█░░█░█░░█░░▀▀█░░█░
░▀░▀░▀░▀░░▀░░▀░▀░░▀░░▀▀▀░▀▀▀░▀▀▀░░▀░
"#;

use ratatui::Frame;
use ratatui::text::Span;

use crate::app::{App, CheckStatus};
use crate::ui::theme::Theme;

/// Glyph + color for a PR's CI rollup state. Returned as a span already
/// styled by the theme so render sites can just push it. Always emits a
/// two-column-wide span (glyph + trailing space) so row alignment stays
/// consistent whether the status is known or not.
pub fn check_status_span<'a>(status: Option<CheckStatus>, theme: &Theme) -> Span<'a> {
    match status {
        Some(CheckStatus::Success) => Span::styled("✓ ", theme.success()),
        Some(CheckStatus::Failure) => Span::styled("✗ ", theme.due_overdue()),
        Some(CheckStatus::Pending) => Span::styled("· ", theme.muted_text()),
        None => Span::styled("  ", theme.muted_text()),
    }
}

pub fn draw(frame: &mut Frame, app: &App) {
    layout::render(frame, app);

    if app.show_theme_picker {
        components::theme_picker::render(frame, app);
    } else if app.show_priority_picker {
        components::priority_picker::render(frame, app.priority_selection, app.theme());
    } else if let Some(form) = &app.task_form {
        components::task_form::render(frame, app, form);
    } else if app.show_input {
        components::input_popup::render(frame, app);
    }

    if app.show_help {
        components::cheatsheet::render(frame, &app.input_mode, app.theme());
    }

    if let Some(error) = &app.error {
        components::error_popup::render(frame, error, app.theme());
    }
}
