use std::sync::Mutex;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, DOCK_ITEMS, InputMode, Pane, ProjectNavItem, VimState};

/// Move focus forward (Tab / Right) from `Pane::Tasks` to the next
/// visible pane. Normally this lands on StatsDock, but when the user has
/// opted out via `show_stats=false` we wrap back to Projects instead so
/// focus never targets a hidden pane.
fn advance_into_stats_or_wrap(app: &mut App) {
    if app.show_stats {
        app.dock_focus = Some(0);
        app.active_pane = Pane::StatsDock;
    } else {
        app.active_pane = Pane::Projects;
    }
}

/// Mirror of `advance_into_stats_or_wrap` for backwards focus motion
/// (BackTab / Left) out of `Pane::Projects`. When Stats is hidden we
/// wrap around to Tasks.
fn retreat_into_stats_or_wrap(app: &mut App) {
    if app.show_stats {
        app.dock_focus = Some(DOCK_ITEMS.len() - 1);
        app.active_pane = Pane::StatsDock;
    } else {
        app.active_pane = Pane::Tasks;
    }
}

pub enum KeyAction {
    Quit,
    ProjectChanged,
    OpenDetail,
    CloseDetail,
    ToggleSettings,
    ToggleHelp,
    ToggleMode,
    ToggleCollapse,
    ToggleFolderCollapse,
    OpenAllFolds,
    CloseAllFolds,
    CompleteTask,
    CompleteTaskById(String),
    OpenDetailById(String),
    #[allow(dead_code)]
    OpenPriorityPicker,
    SelectPriority,
    StarProject,
    CycleFilter,
    CycleSort,
    StartInput,
    StartCommentInput,
    StartFieldEdit,
    SubmitInput,
    SubmitForm,
    FormFieldUp,
    FormFieldDown,
    FormEditField,
    FormEscNormal,
    CancelInput,
    DetailFieldUp,
    DetailFieldDown,
    OpenThemePicker,
    SelectTheme,
    CloseThemePicker,
    AllViewSelected,
    RefreshAllSources,
    TodayViewSelected,
    UpcomingViewSelected,
    RefreshGithubPrs,
    OpenSelectedPrInBrowser,
    JiraCardsViewSelected,
    RefreshJiraCards,
    OpenSelectedJiraCardInBrowser,
    AgendaViewSelected,
    RefreshAgenda,
    OpenSelectedAgendaEventInBrowser,
    ToggleOverdueSection,
    Consumed,
    None,
}

static PENDING_Z: Mutex<bool> = Mutex::new(false);

fn take_pending_z() -> bool {
    let mut pending = PENDING_Z.lock().unwrap();
    let was = *pending;
    *pending = false;
    was
}

fn set_pending_z() {
    *PENDING_Z.lock().unwrap() = true;
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> KeyAction {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return KeyAction::Quit;
    }

    if app.show_help {
        return match key.code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => KeyAction::ToggleHelp,
            _ => KeyAction::Consumed,
        };
    }

    if app.show_priority_picker {
        return handle_priority_picker(app, key);
    }

    if let Some(form) = &app.task_form {
        if form.editing {
            return handle_input(app, key);
        }
        return handle_form_nav(app, key);
    }

    if app.show_input {
        return handle_input(app, key);
    }

    if app.show_theme_picker {
        return handle_theme_picker(app, key);
    }

    if matches!(app.active_pane, Pane::Settings) {
        return handle_settings(app, key);
    }

    if matches!(app.active_pane, Pane::Detail) {
        return handle_detail(app, key);
    }

    if app.dock_focus.is_some() {
        return handle_dock_nav(app, key);
    }

    match app.input_mode {
        InputMode::Vim(state) => handle_vim(app, key, state),
        InputMode::Standard => handle_standard(app, key),
    }
}

fn handle_dock_nav(app: &mut App, key: KeyEvent) -> KeyAction {
    let focus = app.dock_focus.unwrap_or(0);

    match key.code {
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => {
            if focus + 1 >= DOCK_ITEMS.len() {
                app.dock_focus = None;
                app.active_pane = Pane::Projects;
            } else {
                app.dock_focus = Some(focus + 1);
            }
            KeyAction::Consumed
        }
        KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab => {
            if focus == 0 {
                app.dock_focus = None;
                app.active_pane = Pane::Tasks;
            } else {
                app.dock_focus = Some(focus - 1);
            }
            KeyAction::Consumed
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.dock_focus = Some((focus + 1) % DOCK_ITEMS.len());
            KeyAction::Consumed
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.dock_focus = Some(if focus == 0 {
                DOCK_ITEMS.len() - 1
            } else {
                focus - 1
            });
            KeyAction::Consumed
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            let item = DOCK_ITEMS[focus];
            app.dock_filter = if app.dock_filter == Some(item) {
                None
            } else {
                Some(item)
            };
            app.dock_focus = None;
            app.active_pane = Pane::Tasks;
            let visible_len = app.visible_tasks().len();
            app.selected_task = app.selected_task.min(visible_len.saturating_sub(1));
            KeyAction::Consumed
        }
        KeyCode::Esc => {
            app.dock_focus = None;
            app.dock_filter = None;
            app.active_pane = Pane::Projects;
            let visible_len = app.visible_tasks().len();
            app.selected_task = app.selected_task.min(visible_len.saturating_sub(1));
            KeyAction::Consumed
        }
        _ => KeyAction::Consumed,
    }
}

fn handle_input(app: &mut App, key: KeyEvent) -> KeyAction {
    let in_form = app.task_form.is_some();

    match key.code {
        KeyCode::Esc => {
            if in_form {
                let on_content = app
                    .task_form
                    .as_ref()
                    .map(|f| f.active_field == 0)
                    .unwrap_or(false);
                if on_content {
                    KeyAction::CancelInput
                } else {
                    KeyAction::FormEscNormal
                }
            } else if matches!(app.input_mode, InputMode::Standard) {
                KeyAction::CancelInput
            } else {
                KeyAction::SubmitInput
            }
        }
        KeyCode::Enter => KeyAction::SubmitInput,
        KeyCode::Backspace => {
            app.input_buffer.pop();
            KeyAction::Consumed
        }
        KeyCode::Char(c) => {
            app.input_buffer.push(c);
            KeyAction::Consumed
        }
        _ => KeyAction::Consumed,
    }
}

fn handle_form_nav(app: &mut App, key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Char('q') => KeyAction::CancelInput,
        KeyCode::Esc => {
            if let Some(form) = &mut app.task_form {
                if form.active_field == 0 {
                    return KeyAction::CancelInput;
                }
                form.active_field = 0;
                form.editing = true;
                app.input_buffer = form.content.clone();
                app.show_input = true;
                if let InputMode::Vim(_) = app.input_mode {
                    app.input_mode = InputMode::Vim(VimState::Insert);
                }
            }
            KeyAction::Consumed
        }
        KeyCode::Char('j') | KeyCode::Down => KeyAction::FormFieldDown,
        KeyCode::Char('k') | KeyCode::Up => KeyAction::FormFieldUp,
        KeyCode::Enter | KeyCode::Char('i') | KeyCode::Char(' ') => KeyAction::FormEditField,
        KeyCode::Tab => KeyAction::SubmitForm,
        _ => KeyAction::Consumed,
    }
}

fn handle_theme_picker(app: &mut App, key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => KeyAction::CloseThemePicker,
        KeyCode::Char('j') | KeyCode::Down => {
            app.theme_selection = (app.theme_selection + 1) % app.themes.len().max(1);
            KeyAction::Consumed
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.themes.is_empty() {
                return KeyAction::Consumed;
            }
            app.theme_selection = app
                .theme_selection
                .checked_sub(1)
                .unwrap_or(app.themes.len() - 1);
            KeyAction::Consumed
        }
        KeyCode::Enter | KeyCode::Char(' ') => KeyAction::SelectTheme,
        _ => KeyAction::Consumed,
    }
}

fn handle_priority_picker(app: &mut App, key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.show_priority_picker = false;
            KeyAction::Consumed
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.priority_selection = match app.priority_selection {
                4 => 3,
                3 => 2,
                2 => 1,
                _ => 4,
            };
            KeyAction::Consumed
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.priority_selection = match app.priority_selection {
                1 => 2,
                2 => 3,
                3 => 4,
                _ => 1,
            };
            KeyAction::Consumed
        }
        KeyCode::Char('1') => {
            app.priority_selection = 4;
            KeyAction::SelectPriority
        }
        KeyCode::Char('2') => {
            app.priority_selection = 3;
            KeyAction::SelectPriority
        }
        KeyCode::Char('3') => {
            app.priority_selection = 2;
            KeyAction::SelectPriority
        }
        KeyCode::Char('4') => {
            app.priority_selection = 1;
            KeyAction::SelectPriority
        }
        KeyCode::Enter | KeyCode::Char(' ') => KeyAction::SelectPriority,
        _ => KeyAction::Consumed,
    }
}

fn handle_detail(_app: &mut App, key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab => {
            KeyAction::CloseDetail
        }
        KeyCode::Char('q') => KeyAction::Quit,
        KeyCode::Char('?') => KeyAction::ToggleHelp,
        KeyCode::Char('x') => KeyAction::CompleteTask,
        KeyCode::Char('c') => KeyAction::StartCommentInput,
        KeyCode::Char('i') | KeyCode::Enter => KeyAction::StartFieldEdit,
        KeyCode::Char('j') | KeyCode::Down => KeyAction::DetailFieldDown,
        KeyCode::Char('k') | KeyCode::Up => KeyAction::DetailFieldUp,
        _ => KeyAction::None,
    }
}

fn handle_settings(app: &mut App, key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => KeyAction::ToggleSettings,

        KeyCode::Char('j') | KeyCode::Down => {
            app.settings_selection = (app.settings_selection + 1) % settings_item_count();
            KeyAction::Consumed
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.settings_selection == 0 {
                app.settings_selection = settings_item_count() - 1;
            } else {
                app.settings_selection -= 1;
            }
            KeyAction::Consumed
        }

        KeyCode::Enter | KeyCode::Char(' ') => {
            match app.settings_selection {
                0 => return KeyAction::ToggleMode,
                1 => return KeyAction::OpenThemePicker,
                2 => {
                    app.cycle_idle_timeout();
                    return KeyAction::Consumed;
                }
                _ => {}
            }
            KeyAction::Consumed
        }

        _ => KeyAction::None,
    }
}

fn settings_item_count() -> usize {
    3
}

fn handle_vim(app: &mut App, key: KeyEvent, state: VimState) -> KeyAction {
    match state {
        VimState::Normal => handle_vim_normal(app, key),
        VimState::Visual => handle_vim_visual(app, key),
        VimState::Insert => handle_vim_insert(app, key),
    }
}

fn handle_vim_normal(app: &mut App, key: KeyEvent) -> KeyAction {
    if take_pending_z() {
        return match key.code {
            KeyCode::Char('a') if matches!(app.active_pane, Pane::Tasks) => {
                KeyAction::ToggleCollapse
            }
            KeyCode::Char('R') => KeyAction::OpenAllFolds,
            KeyCode::Char('M') => KeyAction::CloseAllFolds,
            _ => KeyAction::Consumed,
        };
    }

    match key.code {
        KeyCode::Char('q') => KeyAction::Quit,
        KeyCode::Char('?') => KeyAction::ToggleHelp,
        KeyCode::Char(',') => KeyAction::ToggleSettings,

        KeyCode::Char('z') => {
            set_pending_z();
            KeyAction::Consumed
        }

        KeyCode::Char('x') if matches!(app.active_pane, Pane::Tasks) && app.all_view_active => {
            // The All view's `visible_tasks()` is project-scoped, so we can't
            // rely on `selected_task` + `visible_tasks()` lookup here. Resolve
            // the task's ID directly from the raw `self.tasks` index stored in
            // `AllViewItem::Task(i)` and complete by ID.
            let items = app.all_view_items();
            if let Some(crate::app::AllViewItem::Task(i)) = items.get(app.selected_all_item)
                && let Some(task) = app.tasks.get(*i)
            {
                KeyAction::CompleteTaskById(task.id.clone())
            } else {
                KeyAction::Consumed
            }
        }
        KeyCode::Char('x') if matches!(app.active_pane, Pane::Tasks) => KeyAction::CompleteTask,
        KeyCode::Char('a') if matches!(app.active_pane, Pane::Tasks) => KeyAction::StartInput,
        KeyCode::Char('f') if matches!(app.active_pane, Pane::Tasks) => KeyAction::CycleFilter,
        KeyCode::Char('o') if matches!(app.active_pane, Pane::Tasks) => KeyAction::CycleSort,
        KeyCode::Char('r')
            if matches!(app.active_pane, Pane::Tasks) && app.all_view_active =>
        {
            KeyAction::RefreshAllSources
        }
        KeyCode::Char('r')
            if matches!(app.active_pane, Pane::Tasks) && app.is_pr_view_active() =>
        {
            KeyAction::RefreshGithubPrs
        }
        KeyCode::Char('r')
            if matches!(app.active_pane, Pane::Tasks) && app.jira_cards_view_active =>
        {
            KeyAction::RefreshJiraCards
        }
        KeyCode::Char('r')
            if matches!(app.active_pane, Pane::Tasks) && app.agenda_view_active =>
        {
            KeyAction::RefreshAgenda
        }
        KeyCode::Char('s') if matches!(app.active_pane, Pane::Projects) => KeyAction::StarProject,

        KeyCode::Char('j') | KeyCode::Down => move_in_pane(app, 1),
        KeyCode::Char('k') | KeyCode::Up => move_in_pane(app, -1),

        KeyCode::Char('g') => jump_to_edge(app, true),
        KeyCode::Char('G') => jump_to_edge(app, false),

        KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => {
            match app.active_pane {
                Pane::Projects => app.active_pane = Pane::Tasks,
                Pane::Tasks => advance_into_stats_or_wrap(app),
                _ => {}
            }
            KeyAction::Consumed
        }
        KeyCode::Char('h')
            if matches!(app.active_pane, Pane::Projects) && app.active_pr_org.is_some() =>
        {
            // Cursor is on a PR org row in the sidebar — hide that org.
            // Persists to ui_settings.json so it stays hidden across restarts.
            if let Some(owner) = app.active_pr_org.clone() {
                app.hide_pr_org(owner);
            }
            KeyAction::Consumed
        }
        KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab => {
            match app.active_pane {
                Pane::Tasks => app.active_pane = Pane::Projects,
                Pane::Projects => retreat_into_stats_or_wrap(app),
                _ => {}
            }
            KeyAction::Consumed
        }

        KeyCode::Enter => match app.active_pane {
            Pane::Projects => {
                app.active_pane = Pane::Tasks;
                KeyAction::Consumed
            }
            Pane::Tasks if app.all_view_active => {
                // All-view items are indexed into raw `self.tasks`,
                // `self.github_prs`, and `self.jira_cards`. For tasks, route
                // through `OpenDetailById` since `visible_tasks()` (used by
                // the regular `OpenDetail` path) is project-scoped on the All
                // view. For PRs, `selected_pr` becomes a raw index —
                // `open_selected_pr_in_browser` is All-view-aware.
                let items = app.all_view_items();
                match items.get(app.selected_all_item) {
                    Some(crate::app::AllViewItem::Task(i)) => match app.tasks.get(*i) {
                        Some(task) => KeyAction::OpenDetailById(task.id.clone()),
                        None => KeyAction::Consumed,
                    },
                    Some(crate::app::AllViewItem::PullRequest(i)) => {
                        app.selected_pr = *i;
                        KeyAction::OpenSelectedPrInBrowser
                    }
                    Some(crate::app::AllViewItem::JiraCard(i)) => {
                        app.selected_jira_card = *i;
                        KeyAction::OpenSelectedJiraCardInBrowser
                    }
                    Some(crate::app::AllViewItem::AgendaEvent(i)) => {
                        app.selected_agenda_item = *i;
                        KeyAction::OpenSelectedAgendaEventInBrowser
                    }
                    None => KeyAction::Consumed,
                }
            }
            Pane::Tasks if app.is_pr_view_active() => KeyAction::OpenSelectedPrInBrowser,
            Pane::Tasks if app.jira_cards_view_active => KeyAction::OpenSelectedJiraCardInBrowser,
            Pane::Tasks if app.agenda_view_active => KeyAction::OpenSelectedAgendaEventInBrowser,
            Pane::Tasks => KeyAction::OpenDetail,
            _ => KeyAction::Consumed,
        },

        KeyCode::Char(' ') if matches!(app.active_pane, Pane::Tasks) && app.today_view_active => {
            KeyAction::ToggleOverdueSection
        }
        KeyCode::Char(' ') if matches!(app.active_pane, Pane::Tasks) => KeyAction::ToggleCollapse,
        KeyCode::Char(' ') if matches!(app.active_pane, Pane::Projects) => {
            KeyAction::ToggleFolderCollapse
        }

        KeyCode::Esc => {
            if matches!(app.active_pane, Pane::Tasks) {
                if app.dock_filter.is_some() {
                    app.dock_filter = None;
                    let visible_len = app.visible_tasks().len();
                    app.selected_task = app.selected_task.min(visible_len.saturating_sub(1));
                } else {
                    app.active_pane = Pane::Projects;
                }
                KeyAction::Consumed
            } else {
                KeyAction::None
            }
        }

        _ => KeyAction::None,
    }
}

fn handle_vim_visual(_app: &mut App, key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Esc => KeyAction::Consumed,
        _ => KeyAction::None,
    }
}

fn handle_vim_insert(_app: &mut App, key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Esc => KeyAction::CancelInput,
        KeyCode::Enter => KeyAction::SubmitInput,
        _ => KeyAction::Consumed,
    }
}

fn handle_standard(app: &mut App, key: KeyEvent) -> KeyAction {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('a') if matches!(app.active_pane, Pane::Tasks) => KeyAction::StartInput,
            KeyCode::Char('x')
                if matches!(app.active_pane, Pane::Tasks) && app.all_view_active =>
            {
                let items = app.all_view_items();
                if let Some(crate::app::AllViewItem::Task(i)) = items.get(app.selected_all_item) {
                    app.selected_task = *i;
                    KeyAction::CompleteTask
                } else {
                    KeyAction::Consumed
                }
            }
            KeyCode::Char('x') if matches!(app.active_pane, Pane::Tasks) => KeyAction::CompleteTask,
            _ => KeyAction::None,
        };
    }

    match key.code {
        KeyCode::Char('q') => KeyAction::Quit,
        KeyCode::Char('?') => KeyAction::ToggleHelp,
        KeyCode::Char(',') => KeyAction::ToggleSettings,
        KeyCode::Char('f') if matches!(app.active_pane, Pane::Tasks) => KeyAction::CycleFilter,

        KeyCode::Down => move_in_pane(app, 1),
        KeyCode::Up => move_in_pane(app, -1),

        KeyCode::Home => jump_to_edge(app, true),
        KeyCode::End => jump_to_edge(app, false),

        KeyCode::Right | KeyCode::Tab => {
            match app.active_pane {
                Pane::Projects => app.active_pane = Pane::Tasks,
                Pane::Tasks => advance_into_stats_or_wrap(app),
                _ => {}
            }
            KeyAction::Consumed
        }
        KeyCode::Left | KeyCode::BackTab => {
            match app.active_pane {
                Pane::Tasks => app.active_pane = Pane::Projects,
                Pane::Projects => retreat_into_stats_or_wrap(app),
                _ => {}
            }
            KeyAction::Consumed
        }

        KeyCode::Enter => match app.active_pane {
            Pane::Projects => {
                app.active_pane = Pane::Tasks;
                KeyAction::Consumed
            }
            Pane::Tasks if app.all_view_active => {
                // All-view items are indexed into raw `self.tasks`,
                // `self.github_prs`, and `self.jira_cards`. For tasks, route
                // through `OpenDetailById` since `visible_tasks()` (used by
                // the regular `OpenDetail` path) is project-scoped on the All
                // view. For PRs, `selected_pr` becomes a raw index —
                // `open_selected_pr_in_browser` is All-view-aware.
                let items = app.all_view_items();
                match items.get(app.selected_all_item) {
                    Some(crate::app::AllViewItem::Task(i)) => match app.tasks.get(*i) {
                        Some(task) => KeyAction::OpenDetailById(task.id.clone()),
                        None => KeyAction::Consumed,
                    },
                    Some(crate::app::AllViewItem::PullRequest(i)) => {
                        app.selected_pr = *i;
                        KeyAction::OpenSelectedPrInBrowser
                    }
                    Some(crate::app::AllViewItem::JiraCard(i)) => {
                        app.selected_jira_card = *i;
                        KeyAction::OpenSelectedJiraCardInBrowser
                    }
                    Some(crate::app::AllViewItem::AgendaEvent(i)) => {
                        app.selected_agenda_item = *i;
                        KeyAction::OpenSelectedAgendaEventInBrowser
                    }
                    None => KeyAction::Consumed,
                }
            }
            Pane::Tasks if app.is_pr_view_active() => KeyAction::OpenSelectedPrInBrowser,
            Pane::Tasks if app.jira_cards_view_active => KeyAction::OpenSelectedJiraCardInBrowser,
            Pane::Tasks if app.agenda_view_active => KeyAction::OpenSelectedAgendaEventInBrowser,
            Pane::Tasks => KeyAction::OpenDetail,
            _ => KeyAction::Consumed,
        },

        KeyCode::Esc => {
            if matches!(app.active_pane, Pane::Tasks) {
                if app.dock_filter.is_some() {
                    app.dock_filter = None;
                    let visible_len = app.visible_tasks().len();
                    app.selected_task = app.selected_task.min(visible_len.saturating_sub(1));
                } else {
                    app.active_pane = Pane::Projects;
                }
                KeyAction::Consumed
            } else {
                KeyAction::None
            }
        }

        _ => KeyAction::None,
    }
}

fn move_in_pane(app: &mut App, delta: i32) -> KeyAction {
    match app.active_pane {
        Pane::Projects => {
            let nav = app.visible_nav_items();
            if nav.is_empty() {
                return KeyAction::Consumed;
            }
            let pos = nav
                .iter()
                .position(|item| match item {
                    ProjectNavItem::Project(i) => {
                        !app.on_virtual_view()
                            && app.folder_cursor.is_none()
                            && *i == app.selected_project
                    }
                    ProjectNavItem::Folder(fi) => app.folder_cursor == Some(*fi),
                    ProjectNavItem::AllView => {
                        app.all_view_active && app.folder_cursor.is_none()
                    }
                    ProjectNavItem::TodayView => {
                        app.today_view_active && app.folder_cursor.is_none()
                    }
                    ProjectNavItem::UpcomingView => {
                        app.upcoming_view_active && app.folder_cursor.is_none()
                    }
                    ProjectNavItem::GithubPrsView(owner) => {
                        app.active_pr_org.as_deref() == Some(owner.as_str())
                            && app.folder_cursor.is_none()
                    }
                    ProjectNavItem::JiraCardsView => {
                        app.jira_cards_view_active && app.folder_cursor.is_none()
                    }
                    ProjectNavItem::AgendaView => {
                        app.agenda_view_active && app.folder_cursor.is_none()
                    }
                })
                .unwrap_or(0) as i32;
            let next_pos = pos + delta;
            if next_pos >= nav.len() as i32 {
                // Past the last sidebar entry: descend into StatsDock if
                // the user has it enabled, otherwise stay put — nothing
                // below on screen to focus.
                if app.show_stats {
                    app.dock_focus = Some(0);
                    app.active_pane = Pane::StatsDock;
                }
                return KeyAction::Consumed;
            }
            if next_pos < 0 {
                return KeyAction::Consumed;
            }
            match &nav[next_pos as usize] {
                ProjectNavItem::Project(i) => {
                    let i = *i;
                    app.folder_cursor = None;
                    app.selected_project = i;
                    KeyAction::ProjectChanged
                }
                ProjectNavItem::Folder(fi) => {
                    app.folder_cursor = Some(*fi);
                    KeyAction::Consumed
                }
                ProjectNavItem::AllView => {
                    app.folder_cursor = None;
                    KeyAction::AllViewSelected
                }
                ProjectNavItem::TodayView => {
                    app.folder_cursor = None;
                    KeyAction::TodayViewSelected
                }
                ProjectNavItem::UpcomingView => {
                    app.folder_cursor = None;
                    KeyAction::UpcomingViewSelected
                }
                ProjectNavItem::GithubPrsView(owner) => {
                    let owner = owner.clone();
                    app.folder_cursor = None;
                    app.activate_github_prs_view(owner);
                    KeyAction::Consumed
                }
                ProjectNavItem::JiraCardsView => {
                    app.folder_cursor = None;
                    KeyAction::JiraCardsViewSelected
                }
                ProjectNavItem::AgendaView => {
                    app.folder_cursor = None;
                    KeyAction::AgendaViewSelected
                }
            }
        }
        Pane::Tasks => {
            if app.all_view_active {
                let len = app.all_view_items().len();
                if len == 0 {
                    return KeyAction::Consumed;
                }
                let current = app.selected_all_item as i32;
                app.selected_all_item = (current + delta).rem_euclid(len as i32) as usize;
                return KeyAction::Consumed;
            }
            if app.jira_cards_view_active {
                let len = app.jira_cards.len();
                if len == 0 {
                    return KeyAction::Consumed;
                }
                let current = app.selected_jira_card as i32;
                app.selected_jira_card = (current + delta).rem_euclid(len as i32) as usize;
                return KeyAction::Consumed;
            }
            if app.agenda_view_active {
                let len = app.agenda_events.len();
                if len == 0 {
                    return KeyAction::Consumed;
                }
                let current = app.selected_agenda_item as i32;
                app.selected_agenda_item = (current + delta).rem_euclid(len as i32) as usize;
                return KeyAction::Consumed;
            }
            if app.is_pr_view_active() {
                let len = app.active_org_prs().len();
                if len == 0 {
                    return KeyAction::Consumed;
                }
                let current = app.selected_pr as i32;
                app.selected_pr = (current + delta).rem_euclid(len as i32) as usize;
                return KeyAction::Consumed;
            }
            let visible = app.visible_tasks();
            let visible_len = visible.len();
            if visible_len == 0 {
                return KeyAction::Consumed;
            }
            let current = app.selected_task as i32;
            let mut next = (current + delta).rem_euclid(visible_len as i32) as usize;
            // Skip context rows (dimmed active parents shown in Done filter).
            for _ in 0..visible_len {
                if !app.is_context_task(visible[next]) {
                    break;
                }
                next = ((next as i32) + delta).rem_euclid(visible_len as i32) as usize;
            }
            app.selected_task = next;
            KeyAction::Consumed
        }
        _ => KeyAction::Consumed,
    }
}

fn jump_to_edge(app: &mut App, top: bool) -> KeyAction {
    match app.active_pane {
        Pane::Projects => {
            let nav = app.visible_nav_items();
            let item = if top { nav.first() } else { nav.last() };
            match item {
                Some(ProjectNavItem::Project(i)) => {
                    let i = *i;
                    app.folder_cursor = None;
                    if app.selected_project != i {
                        app.selected_project = i;
                        return KeyAction::ProjectChanged;
                    }
                }
                Some(ProjectNavItem::Folder(fi)) => {
                    app.folder_cursor = Some(*fi);
                }
                Some(ProjectNavItem::AllView) => {
                    app.folder_cursor = None;
                    return KeyAction::AllViewSelected;
                }
                Some(ProjectNavItem::TodayView) => {
                    app.folder_cursor = None;
                    return KeyAction::TodayViewSelected;
                }
                Some(ProjectNavItem::UpcomingView) => {
                    app.folder_cursor = None;
                    return KeyAction::UpcomingViewSelected;
                }
                Some(ProjectNavItem::GithubPrsView(owner)) => {
                    let owner = owner.clone();
                    app.folder_cursor = None;
                    app.activate_github_prs_view(owner);
                    return KeyAction::Consumed;
                }
                Some(ProjectNavItem::JiraCardsView) => {
                    app.folder_cursor = None;
                    return KeyAction::JiraCardsViewSelected;
                }
                Some(ProjectNavItem::AgendaView) => {
                    app.folder_cursor = None;
                    return KeyAction::AgendaViewSelected;
                }
                None => {}
            }
            KeyAction::Consumed
        }
        Pane::Tasks => {
            let visible_len = app.visible_tasks().len();
            app.selected_task = if top {
                0
            } else {
                visible_len.saturating_sub(1)
            };
            KeyAction::Consumed
        }
        _ => KeyAction::Consumed,
    }
}
