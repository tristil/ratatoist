use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::Local;

use anyhow::Result;
use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use ratatoist_core::api::client::TodoistClient;
use ratatoist_core::api::models::{Comment, Folder, Label, Project, Section, Task, Workspace};
use ratatoist_core::api::sync::{SyncCommand, SyncRequest, SyncResponse};
use ratatoist_core::sync_state::SyncState;

use crate::keys::{self, KeyAction};
use crate::ui;

static CMD_COUNTER: AtomicU64 = AtomicU64::new(0);

fn new_uuid() -> String {
    let ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let c = CMD_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{ns:08x}-{c:016x}-4000-8000-000000000000")
}

fn new_temp_id() -> String {
    format!("tmp_{}", CMD_COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Projects,
    Tasks,
    Detail,
    Settings,
    StatsDock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Standard,
    Vim(VimState),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimState {
    Normal,
    #[allow(dead_code)] // Reserved for visual mode selection.
    Visual,
    Insert,
}

impl InputMode {
    pub fn label(&self) -> &'static str {
        match self {
            InputMode::Standard => "STANDARD",
            InputMode::Vim(VimState::Normal) => "NORMAL",
            InputMode::Vim(VimState::Visual) => "VISUAL",
            InputMode::Vim(VimState::Insert) => "INSERT",
        }
    }
}

pub struct OverviewStats {
    pub due_today: u32,
    pub due_week: u32,
    pub overdue: u32,
    pub by_priority: [u32; 5],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFilter {
    Active,
    Done,
    Both,
}

impl TaskFilter {
    pub fn next(self) -> Self {
        match self {
            TaskFilter::Active => TaskFilter::Done,
            TaskFilter::Done => TaskFilter::Both,
            TaskFilter::Both => TaskFilter::Active,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockItem {
    DueOverdue,
    DueToday,
    DueWeek,
    Priority(u8),
}

pub const DOCK_ITEMS: [DockItem; 7] = [
    DockItem::DueOverdue,
    DockItem::DueToday,
    DockItem::DueWeek,
    DockItem::Priority(4),
    DockItem::Priority(3),
    DockItem::Priority(2),
    DockItem::Priority(1),
];

impl DockItem {
    pub fn hint(self) -> &'static str {
        match self {
            DockItem::DueOverdue => "overdue",
            DockItem::DueToday => "due today",
            DockItem::DueWeek => "due this week",
            DockItem::Priority(4) => "urgent (P1)",
            DockItem::Priority(3) => "high (P2)",
            DockItem::Priority(2) => "medium (P3)",
            DockItem::Priority(1) => "no priority",
            DockItem::Priority(_) => "by priority",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Default,
    Priority,
    DueDate,
    Created,
}

impl SortMode {
    pub fn label(&self) -> &'static str {
        match self {
            SortMode::Default => "order",
            SortMode::Priority => "priority",
            SortMode::DueDate => "due",
            SortMode::Created => "created",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            SortMode::Default => SortMode::Priority,
            SortMode::Priority => SortMode::DueDate,
            SortMode::DueDate => SortMode::Created,
            SortMode::Created => SortMode::Default,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppError {
    pub title: String,
    pub message: String,
    pub suggestion: Option<String>,
    pub recoverable: bool,
}

impl AppError {
    fn from_api(err: &anyhow::Error, context: &str) -> Self {
        let raw = format!("{err:#}");
        let (title, message, suggestion) = parse_api_error(&raw, context);
        Self {
            title,
            message,
            suggestion,
            recoverable: true,
        }
    }
}

fn parse_api_error(raw: &str, context: &str) -> (String, String, Option<String>) {
    if let Some(json_start) = raw.find('{')
        && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw[json_start..])
    {
        let error_msg = parsed["error"]
            .as_str()
            .unwrap_or("Unknown error")
            .to_string();
        let error_tag = parsed["error_tag"].as_str().unwrap_or("");

        let suggestion = match error_tag {
            "INVALID_DATE_FORMAT" | "BAD_REQUEST" => Some(
                "Try natural language like \"tomorrow\", \"next monday\", or \"Feb 28\""
                    .to_string(),
            ),
            "NOT_FOUND" => Some("The item may have been deleted. Try refreshing.".to_string()),
            "FORBIDDEN" => Some("You don't have permission for this action.".to_string()),
            "UNAUTHORIZED" => {
                Some("Your API token may have expired. Check your config.".to_string())
            }
            _ => None,
        };

        return (format!("{context} failed"), error_msg, suggestion);
    }

    (format!("{context} failed"), raw.to_string(), None)
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UserRecord {
    pub id: String,
    pub full_name: String,
    pub email: String,
    pub display: String,
}

impl UserRecord {
    pub fn new(id: String, full_name: Option<String>, email: Option<String>) -> Self {
        let name = full_name.unwrap_or_default();
        let mail = email.unwrap_or_default();
        let display = match (name.is_empty(), mail.is_empty()) {
            (false, false) => format!("{name} - {mail}"),
            (false, true) => name.clone(),
            (true, false) => mail.clone(),
            _ => id.clone(),
        };
        Self {
            id,
            full_name: name,
            email: mail,
            display,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskForm {
    pub content: String,
    pub priority: u8,
    pub due_string: String,
    pub project_id: String,
    pub active_field: usize,
    pub editing: bool,
}

impl TaskForm {
    pub fn new(project_id: String) -> Self {
        Self {
            content: String::new(),
            priority: 1,
            due_string: String::new(),
            project_id,
            active_field: 0,
            editing: true,
        }
    }

    pub fn field_count() -> usize {
        4
    }
}

/// Build the `item_add` sync command args from a completed task form.
///
/// The Todoist Sync API takes a `due` object (`{ "string": "tomorrow" }`), not
/// the REST API's top-level `due_string` shorthand — sending `due_string` at
/// the top level is silently dropped by the server, which is how the add-task
/// modal was losing the date the user typed in.
pub(crate) fn build_item_add_args(form: &TaskForm, project_id: &str) -> serde_json::Value {
    let mut args = serde_json::json!({
        "content": form.content,
        "project_id": project_id,
    });
    if !form.due_string.is_empty() {
        args["due"] = serde_json::json!({ "string": form.due_string });
    }
    if form.priority > 1 {
        args["priority"] = serde_json::Value::Number(serde_json::Number::from(form.priority));
    }
    args
}

// Tracks what was in local state before an optimistic mutation so we can
// revert if the server rejects the command.
pub enum OptimisticOp {
    TaskAdded {
        temp_id: String,
    },
    #[allow(dead_code)] // Used once delete task (d) is wired up.
    TaskRemoved {
        snapshot: Task,
    },
    TaskUpdated {
        task_id: String,
        before: Task,
    },
    CommentAdded {
        temp_id: String,
        task_id: String,
    },
    ProjectUpdated {
        project_id: String,
        before: Project,
    },
}

pub enum ProjectEntry {
    PersonalHeader,
    WorkspaceHeader(usize),
    FolderHeader(usize),
    Project(usize),
    Separator,
    AllView,
    AgendaView,
    TodayView,
    UpcomingView,
    /// One entry per GitHub owner (user or org) that has at least one open PR.
    /// The string is the owner login (`"cxrlos"`, `"appfolio"`, etc.).
    GithubPrsView(String),
    JiraCardsView,
}

pub enum ProjectNavItem {
    Folder(usize),
    Project(usize),
    AllView,
    AgendaView,
    TodayView,
    UpcomingView,
    GithubPrsView(String),
    JiraCardsView,
}

/// A tagged item in the All view. The usize is an index into the source
/// collection (visible_tasks() for tasks, github_prs for PRs, jira_cards
/// for Jira cards, agenda_events for calendar events) at the time the vec
/// was built.
#[derive(Debug, Clone, Copy)]
pub enum AllViewItem {
    Task(usize),
    PullRequest(usize),
    JiraCard(usize),
    AgendaEvent(usize),
}

/// Check whether a CLI tool is on PATH and responds to `--version`. Runs once
/// at startup; cheap and synchronous.
fn binary_available(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check whether `gws` is configured to read the user's Google Calendar.
/// Runs `gws auth status` (JSON by default) and verifies the granted scope
/// list includes the calendar scope. Returns false if the binary is absent,
/// the user hasn't run `gws auth login`, or calendar access wasn't granted —
/// any of which would make the Agenda view a dead end. Runs once at startup.
fn gws_calendar_configured() -> bool {
    let Ok(output) = std::process::Command::new("gws")
        .args(["auth", "status"])
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    // `gws auth status` emits one line of log chatter to stdout before the
    // JSON document; find the first `{` and parse from there.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(json_start) = stdout.find('{') else {
        return false;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&stdout[json_start..]) else {
        return false;
    };
    val.get("scopes")
        .and_then(|s| s.as_array())
        .map(|scopes| {
            scopes.iter().filter_map(|s| s.as_str()).any(|s| {
                s == "https://www.googleapis.com/auth/calendar"
                    || s == "https://www.googleapis.com/auth/calendar.readonly"
                    || s == "https://www.googleapis.com/auth/calendar.events"
                    || s == "https://www.googleapis.com/auth/calendar.events.readonly"
            })
        })
        .unwrap_or(false)
}

/// A pull request row as returned by `gh search prs --json ...`. Subset of the
/// gh schema — only what we render. `node_id` and `check_status` are
/// populated by a follow-up GraphQL call after the search; `check_status`
/// stays `None` if that call fails or if the PR has no checks configured.
#[derive(Debug, Clone, Default)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub repo_full_name: String,
    pub author_login: String,
    pub updated_at: String,
    pub is_draft: bool,
    pub node_id: String,
    pub check_status: Option<CheckStatus>,
}

/// CI rollup state for the most recent commit on a PR. Mapped from
/// GitHub's `statusCheckRollup.state` values (SUCCESS, FAILURE, ERROR,
/// PENDING, EXPECTED). `Expected` is folded into `Pending` — from the
/// user's perspective both mean "waiting on a check to report."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Success,
    Failure,
    Pending,
}

/// A Jira work item as returned by `acli jira workitem search --json`.
#[derive(Debug, Clone, Default)]
pub struct JiraCard {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub priority: String,
    pub issue_type: String,
    /// Project key parsed from the issue key ("PROJ-123" → "PROJ"). Used for
    /// grouping in the view.
    pub project_key: String,
}

/// A Google Calendar event as returned by
/// `gws calendar events list`. Subset of the Calendar API event resource —
/// only fields the agenda view renders.
#[derive(Debug, Clone, Default)]
pub struct CalendarEvent {
    pub summary: String,
    /// RFC 3339 start timestamp for timed events, or `YYYY-MM-DD` for
    /// all-day events. Sort key for the agenda list.
    pub start: String,
    /// Same shape as `start`. Empty for events the API didn't return an end
    /// for (rare; defensive default).
    pub end: String,
    /// True when the event has a `start.date` (all-day) rather than a
    /// `start.dateTime`. Controls how we render the time column.
    pub all_day: bool,
    pub location: String,
    /// `htmlLink` from the event — opens the event in Google Calendar web.
    /// Used by Enter-to-open-in-browser on agenda rows.
    pub html_link: String,
    /// Display name of the calendar this event came from (the
    /// `calendarList` entry's `summaryOverride` or `summary`). Surfaced in
    /// the agenda row when multiple calendars contribute to today's list.
    pub calendar_name: String,
}

enum BgResult {
    SyncDelta(Box<SyncResponse>),
    CommandResults(Box<SyncResponse>),
    CompletedTasks {
        project_id: String,
        records: Result<Vec<Task>>,
    },
    WebSocketConnected,
    WebSocketEvent,
    WebSocketDisconnected,
    GithubPrsFetched(Result<Vec<PullRequest>>),
    JiraCardsFetched(Result<Vec<JiraCard>>),
    AgendaFetched(Result<Vec<CalendarEvent>>),
    Comments {
        task_id: String,
        comments: Result<Vec<Comment>>,
        fetch_seq: u64,
    },
}

pub struct App {
    pub projects: Vec<Project>,
    pub workspaces: Vec<Workspace>,
    pub folders: Vec<Folder>,
    pub tasks: Vec<Task>,
    pub labels: Vec<Label>,
    pub sections: Vec<Section>,
    pub selected_project: usize,
    pub selected_task: usize,
    pub active_pane: Pane,
    pub running: bool,
    pub error: Option<AppError>,
    pub input_mode: InputMode,
    pub show_settings: bool,
    pub show_help: bool,
    pub show_input: bool,
    pub input_buffer: String,
    pub settings_selection: usize,
    pub collapsed: HashSet<String>,
    pub detail_scroll: u16,
    pub sort_mode: SortMode,
    pub comments: Vec<Comment>,
    pub comment_input: bool,
    pub detail_field: usize,
    pub show_priority_picker: bool,
    pub priority_selection: u8,
    pub editing_field: bool,
    pub task_form: Option<TaskForm>,
    pub current_user_id: Option<String>,
    pub user_names: HashMap<String, UserRecord>,
    pub task_filter: TaskFilter,
    pub dock_focus: Option<usize>,
    pub dock_filter: Option<DockItem>,
    pub themes: Vec<crate::ui::theme::Theme>,
    pub theme_idx: usize,
    pub show_theme_picker: bool,
    pub theme_selection: usize,
    pub websocket_connected: bool,
    pub sync_token: String,
    pub completed_cache: HashMap<String, Vec<Task>>,
    pub comments_by_task: HashMap<String, Vec<Comment>>,
    pub idle_timeout_secs: u64,
    pub idle_forcer: bool,
    pub ephemeral: bool,
    pub last_sync_at: Option<chrono::DateTime<Local>>,
    pub collapsed_folders: HashSet<String>,
    pub folder_cursor: Option<usize>,
    pub current_user_name: Option<String>,
    pub today_view_active: bool,
    pub upcoming_view_active: bool,
    /// Which GitHub owner's PRs are currently showing. `Some("cxrlos")` etc.
    /// Replaces the older single-view bool; every entry in the sidebar binds
    /// to one specific owner, and `None` means no PR view is focused.
    pub active_pr_org: Option<String>,
    pub github_prs: Vec<PullRequest>,
    pub github_prs_loading: bool,
    pub github_prs_error: Option<String>,
    pub github_prs_fetched_at: Option<chrono::DateTime<Local>>,
    pub selected_pr: usize,
    pub gh_available: bool,
    /// Owners hidden from the PR sidebar by the user via `h`. Persisted in
    /// `ui_settings.json`.
    pub hidden_pr_orgs: HashSet<String>,
    pub jira_cards_view_active: bool,
    pub jira_cards: Vec<JiraCard>,
    pub jira_cards_loading: bool,
    pub jira_cards_error: Option<String>,
    pub jira_cards_fetched_at: Option<chrono::DateTime<Local>>,
    pub selected_jira_card: usize,
    pub acli_available: bool,
    pub agenda_view_active: bool,
    pub agenda_events: Vec<CalendarEvent>,
    pub agenda_loading: bool,
    pub agenda_error: Option<String>,
    pub agenda_fetched_at: Option<chrono::DateTime<Local>>,
    pub selected_agenda_item: usize,
    /// True if `gws` is on PATH AND `gws auth status` reports a granted
    /// scope that lets us read the user's calendar. Gates the Agenda sidebar
    /// entry and all agenda fetches — when false, the entry stays hidden so
    /// users who installed `gws` without authorizing calendar access don't
    /// see a view that can't load. Probed once at startup.
    pub gws_available: bool,
    /// Whether the StatsDock block is rendered in the left sidebar. Off by
    /// default — the dashboard is enough for most users and the block
    /// eats four rows. Persisted in `ui_settings.json` under `show_stats`;
    /// also gates Tab / j-past-end focus transitions so a hidden block
    /// doesn't capture focus.
    pub show_stats: bool,
    /// Running tally of task completions for the local day. Incremented on
    /// every `item_close` enqueue (non-recurring or recurring), not
    /// decremented on `item_reopen` — the jar is a one-way record of effort.
    /// Persisted in `ui_settings.json` alongside `star_date`; see
    /// [`star-jar.spec.md`](../../../specifications/star-jar.spec.md).
    pub star_count: u64,
    /// Wall-clock instant the current 25-minute pomodoro began, or `None`
    /// when no pomodoro is running. Session-scoped — deliberately NOT
    /// persisted; closing the app abandons the timer. See
    /// [`pomodoro.spec.md`](../../../specifications/pomodoro.spec.md).
    pub pomodoro_started_at: Option<Instant>,
    /// Count of pomodoros completed today. Incremented by
    /// `maybe_award_tomato` when the running timer reaches 25:00, not
    /// by cancellation. Persisted in `ui_settings.json`; resets lazily
    /// at local midnight (same pattern as the Star Jar).
    pub tomato_count: u64,
    /// Local date (YYYY-MM-DD) of the stored `tomato_count`. When a read
    /// observes this no longer matches today, the count resets to zero.
    pub tomato_date: String,
    /// Local date (YYYY-MM-DD) of the stored `star_count`. When a read
    /// observes this no longer matches today, the count resets to zero.
    pub star_date: String,
    pub all_view_active: bool,
    pub selected_all_item: usize,
    /// When the detail pane is open, this is the ID of the task being
    /// displayed. Set by `open_detail` / `open_detail_for`, cleared on
    /// `CloseDetail`. `selected_task()` prefers this over the
    /// `selected_task` index so detail opened from the All view (where
    /// `visible_tasks()` is project-scoped) still resolves correctly.
    pub detail_task_id: Option<String>,
    pub overdue_section_collapsed: bool,
    last_activity: Instant,
    pending_ws_sync: bool,
    /// Background poll interval for `gh search prs` (seconds). GitHub allows
    /// 5000 authed REST requests/hour, well above any reasonable polling
    /// cadence for a personal tool, so this is a comfort knob, not a safety
    /// knob. Default 300s = 5 minutes.
    pub github_prs_poll_interval_secs: u64,
    /// Set when we'd have polled PRs but the user is idle; fired on the next
    /// keystroke (same pattern as `pending_ws_sync`).
    pending_pr_poll: bool,
    /// Background poll interval for `acli jira workitem search` (seconds).
    /// Jira Cloud's per-user rate-limit budget easily accommodates a 10s
    /// cadence for one user. Default 10s.
    pub jira_cards_poll_interval_secs: u64,
    pending_jira_poll: bool,
    /// Background poll interval for `gws calendar events list` (seconds).
    /// Calendar events change slowly compared to PRs or tasks, and Google
    /// Calendar's per-user quota is generous. Default 300s = 5 min.
    pub agenda_poll_interval_secs: u64,
    pending_agenda_poll: bool,
    comments_fetch_seq: u64,
    websocket_url: Option<String>,
    pending_commands: Vec<SyncCommand>,
    temp_id_pending: HashMap<String, OptimisticOp>,
    /// Task IDs for recurring completes whose sync response hasn't arrived
    /// yet. We filter these out of visible_tasks so the user sees them
    /// disappear from Today instantly (like non-recurring completes do)
    /// instead of waiting for the server round-trip. Prevents double-tap `x`
    /// from advancing the series twice.
    pending_close_recurring: HashSet<String>,
    bg_tx: mpsc::Sender<BgResult>,
    bg_rx: mpsc::Receiver<BgResult>,
    client: Arc<TodoistClient>,
}

fn load_theme_idx(themes: &[crate::ui::theme::Theme]) -> usize {
    let path = ratatoist_core::config::Config::config_dir().join("ui_settings.json");
    if let Ok(src) = std::fs::read_to_string(&path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&src)
        && let Some(name) = val["theme"].as_str()
        && let Some(idx) = themes.iter().position(|t| t.name == name)
    {
        return idx;
    }
    0
}

/// Owners (organizations or personal logins) that the user has hidden from
/// the Pull Requests sidebar. Persisted in `ui_settings.json` under the
/// `hidden_pr_orgs` key. Order is preserved as a `Vec` for deterministic
/// JSON output but lookups treat it as a set.
fn load_hidden_pr_orgs() -> HashSet<String> {
    let path = ratatoist_core::config::Config::config_dir().join("ui_settings.json");
    if let Ok(src) = std::fs::read_to_string(&path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&src)
        && let Some(arr) = val["hidden_pr_orgs"].as_array()
    {
        return arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    HashSet::new()
}

/// Default background poll interval for `gh search prs`. GitHub's search
/// endpoint rate-limits authenticated users to 30 req/min, so 10 seconds
/// (6/min) is comfortably under and keeps the PR view near-real-time.
const DEFAULT_GITHUB_PRS_POLL_INTERVAL_SECS: u64 = 20;

fn load_github_prs_poll_interval_secs() -> u64 {
    let path = ratatoist_core::config::Config::config_dir().join("ui_settings.json");
    if let Ok(src) = std::fs::read_to_string(&path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&src)
        && let Some(secs) = val["github_prs_poll_interval_secs"].as_u64()
    {
        // Clamp to at least 5s to avoid accidentally DoS'ing the search
        // endpoint if the user sets a tiny value by mistake.
        return secs.max(5);
    }
    DEFAULT_GITHUB_PRS_POLL_INTERVAL_SECS
}

/// Default background poll interval for `acli jira workitem search`.
const DEFAULT_JIRA_CARDS_POLL_INTERVAL_SECS: u64 = 10;

fn load_jira_cards_poll_interval_secs() -> u64 {
    let path = ratatoist_core::config::Config::config_dir().join("ui_settings.json");
    if let Ok(src) = std::fs::read_to_string(&path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&src)
        && let Some(secs) = val["jira_cards_poll_interval_secs"].as_u64()
    {
        return secs.max(5);
    }
    DEFAULT_JIRA_CARDS_POLL_INTERVAL_SECS
}

/// Default background poll interval for `gws calendar events list`.
/// Calendar events rarely change minute-to-minute, so a slower cadence is
/// fine and friendlier to the Calendar API quota.
const DEFAULT_AGENDA_POLL_INTERVAL_SECS: u64 = 300;

fn load_agenda_poll_interval_secs() -> u64 {
    let path = ratatoist_core::config::Config::config_dir().join("ui_settings.json");
    if let Ok(src) = std::fs::read_to_string(&path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&src)
        && let Some(secs) = val["agenda_poll_interval_secs"].as_u64()
    {
        return secs.max(30);
    }
    DEFAULT_AGENDA_POLL_INTERVAL_SECS
}

/// Whether the StatsDock block should render. Defaults to `false` when
/// absent (explicit opt-in) so new users / new installs start with the
/// leaner sidebar.
fn load_show_stats() -> bool {
    let path = ratatoist_core::config::Config::config_dir().join("ui_settings.json");
    if let Ok(src) = std::fs::read_to_string(&path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&src)
        && let Some(b) = val["show_stats"].as_bool()
    {
        return b;
    }
    false
}

/// Length of a single pomodoro. Fixed; see `pomodoro.spec.md`.
pub const POMODORO_DURATION: Duration = Duration::from_secs(25 * 60);

/// Load today's tomato counter from disk. Mirrors `load_star_jar` —
/// returns `(count, today)` with the count zeroed when the persisted date
/// doesn't match today's local date, so callers never see stale values.
fn load_tomato_jar() -> (u64, String) {
    let today = crate::ui::dates::today_str();
    let path = ratatoist_core::config::Config::config_dir().join("ui_settings.json");
    if let Ok(src) = std::fs::read_to_string(&path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&src)
        && let Some(date) = val["tomato_date"].as_str()
        && date == today
        && let Some(count) = val["tomato_count"].as_u64()
    {
        return (count, today);
    }
    (0, today)
}

/// Load today's star jar from disk. Returns `(count, date)` where `date` is
/// always today's local `YYYY-MM-DD`: if the persisted entry is from a
/// previous day (or no entry exists), the count is zero and the date is
/// today. Callers can trust the returned date without re-checking.
fn load_star_jar() -> (u64, String) {
    let today = crate::ui::dates::today_str();
    let path = ratatoist_core::config::Config::config_dir().join("ui_settings.json");
    if let Ok(src) = std::fs::read_to_string(&path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&src)
        && let Some(date) = val["star_date"].as_str()
        && date == today
        && let Some(count) = val["star_count"].as_u64()
    {
        return (count, today);
    }
    (0, today)
}

fn load_idle_timeout_secs() -> u64 {
    let path = ratatoist_core::config::Config::config_dir().join("ui_settings.json");
    if let Ok(src) = std::fs::read_to_string(&path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&src)
    {
        if let Some(secs) = val["idle_timeout_secs"].as_u64() {
            return secs;
        }
        if let Some(mins) = val["idle_timeout_mins"].as_u64() {
            return mins * 60;
        }
    }
    300
}

impl App {
    pub fn theme(&self) -> &crate::ui::theme::Theme {
        &self.themes[self.theme_idx]
    }

    pub fn cycle_task_filter(&mut self) {
        self.task_filter = self.task_filter.next();
        if matches!(self.task_filter, TaskFilter::Done | TaskFilter::Both)
            && let Some(pid) = self
                .projects
                .get(self.selected_project)
                .map(|p| p.id.clone())
            && !self.completed_cache.contains_key(&pid)
        {
            self.spawn_completed_tasks_fetch(pid);
        }
        let visible_len = self.visible_tasks().len();
        if visible_len == 0 {
            self.selected_task = 0;
        } else if self.selected_task >= visible_len {
            self.selected_task = visible_len - 1;
        }
    }

    pub fn sync_age_label(&self) -> String {
        match self.last_sync_at {
            Some(at) => at.format("%Y-%m-%d %H:%M").to_string(),
            None => "--".to_string(),
        }
    }

    pub fn is_idle(&self) -> bool {
        self.idle_timeout_secs > 0
            && self.last_activity.elapsed() >= Duration::from_secs(self.idle_timeout_secs)
    }

    pub fn cycle_idle_timeout(&mut self) {
        const OPTIONS: &[u64] = &[60, 120, 300, 600, 900, 1800];
        const DEBUG_OPTIONS: &[u64] = &[5, 60, 120, 300, 600, 900, 1800];
        let options = if self.idle_forcer {
            DEBUG_OPTIONS
        } else {
            OPTIONS
        };
        let pos = options
            .iter()
            .position(|&v| v == self.idle_timeout_secs)
            .unwrap_or(2);
        self.idle_timeout_secs = options[(pos + 1) % options.len()];
        self.save_ui_settings();
    }

    pub fn save_ui_settings(&self) {
        if self.ephemeral {
            return;
        }
        let dir = ratatoist_core::config::Config::config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("ui_settings.json");
        let name = &self.themes[self.theme_idx].name;
        let mut hidden: Vec<&String> = self.hidden_pr_orgs.iter().collect();
        hidden.sort();
        let json = serde_json::json!({
            "theme": name,
            "idle_timeout_secs": self.idle_timeout_secs,
            "hidden_pr_orgs": hidden,
            "github_prs_poll_interval_secs": self.github_prs_poll_interval_secs,
            "jira_cards_poll_interval_secs": self.jira_cards_poll_interval_secs,
            "agenda_poll_interval_secs": self.agenda_poll_interval_secs,
            "show_stats": self.show_stats,
            "star_count": self.star_count,
            "star_date": self.star_date,
            "tomato_count": self.tomato_count,
            "tomato_date": self.tomato_date,
        });
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&json).unwrap_or_default(),
        );
    }

    /// Hide a GitHub owner from the PR sidebar and persist the change. If the
    /// hidden owner was the active view, switch back to the Inbox project so
    /// the user isn't stranded on a now-invisible row.
    pub fn hide_pr_org(&mut self, owner: String) {
        if owner.is_empty() {
            return;
        }
        let was_active = self.active_pr_org.as_deref() == Some(owner.as_str());
        self.hidden_pr_orgs.insert(owner);
        self.save_ui_settings();
        if was_active {
            self.active_pr_org = None;
            // Drop focus to whichever project is currently selected.
            self.active_pane = Pane::Projects;
        }
    }

    /// Roll the star jar forward if the local date has changed since it was
    /// last observed. Called from any read path (render, increment) so the
    /// jar resets lazily at midnight without a background timer.
    fn roll_star_jar_if_new_day(&mut self) {
        let today = crate::ui::dates::today_str();
        if self.star_date != today {
            self.star_date = today;
            self.star_count = 0;
        }
    }

    /// Add one star to today's jar and persist. Rolls the jar first so a
    /// completion that crosses midnight ticks from zero on the new day,
    /// not from yesterday's final count.
    pub fn increment_star_jar(&mut self) {
        self.roll_star_jar_if_new_day();
        self.star_count = self.star_count.saturating_add(1);
        self.save_ui_settings();
    }

    /// Roll the tomato counter forward on a day change. Analogous to
    /// `roll_star_jar_if_new_day` — read on every render tick so the box
    /// empties lazily at local midnight without a background timer.
    fn roll_tomato_jar_if_new_day(&mut self) {
        let today = crate::ui::dates::today_str();
        if self.tomato_date != today {
            self.tomato_date = today;
            self.tomato_count = 0;
        }
    }

    /// `p` key binding — start a pomodoro if none is running, cancel the
    /// current one otherwise. Cancellation awards nothing; completion is
    /// driven by `maybe_award_tomato` in the main loop. See
    /// `pomodoro.spec.md` for the full rationale.
    pub fn toggle_pomodoro(&mut self) {
        if self.pomodoro_started_at.is_some() {
            self.pomodoro_started_at = None;
        } else {
            self.pomodoro_started_at = Some(Instant::now());
        }
    }

    /// Remaining time on the active pomodoro, or `None` when no pomodoro
    /// is running. Clamps to zero once the full duration has elapsed so
    /// the status-bar countdown holds at `00:00` for the frame between
    /// elapse and `maybe_award_tomato` clearing the state.
    pub fn pomodoro_remaining(&self) -> Option<Duration> {
        let start = self.pomodoro_started_at?;
        Some(POMODORO_DURATION.saturating_sub(start.elapsed()))
    }

    /// If a pomodoro has fully elapsed, increment the tomato count and
    /// clear the running state. Called every main-loop tick; cheap (a
    /// couple of compares in the common no-op case).
    pub fn maybe_award_tomato(&mut self) {
        let Some(start) = self.pomodoro_started_at else {
            return;
        };
        if start.elapsed() < POMODORO_DURATION {
            return;
        }
        self.pomodoro_started_at = None;
        self.roll_tomato_jar_if_new_day();
        self.tomato_count = self.tomato_count.saturating_add(1);
        self.save_ui_settings();
    }

    pub fn new(client: TodoistClient, idle_forcer: bool, ephemeral: bool) -> Self {
        let (bg_tx, bg_rx) = mpsc::channel(64);
        let mut themes = crate::ui::theme::Theme::builtin();
        let user_themes_dir = ratatoist_core::config::Config::config_dir().join("themes");
        themes.extend(crate::ui::theme::Theme::load_user_themes(&user_themes_dir));
        let theme_idx = load_theme_idx(&themes);
        let config_dir = ratatoist_core::config::Config::config_dir();
        let sync_token = if ephemeral {
            "*".to_string()
        } else {
            SyncState::load(&config_dir).sync_token
        };
        let idle_timeout_secs = load_idle_timeout_secs();
        let (star_count, star_date) = load_star_jar();
        let (tomato_count, tomato_date) = load_tomato_jar();
        let show_stats = load_show_stats();

        Self {
            projects: Vec::new(),
            workspaces: Vec::new(),
            folders: Vec::new(),
            tasks: Vec::new(),
            labels: Vec::new(),
            sections: Vec::new(),
            selected_project: 0,
            selected_task: 0,
            active_pane: Pane::Projects,
            running: true,
            error: None,
            input_mode: InputMode::Vim(VimState::Normal),
            show_settings: false,
            show_help: false,
            show_input: false,
            input_buffer: String::new(),
            settings_selection: 0,
            collapsed: HashSet::new(),
            detail_scroll: 0,
            sort_mode: SortMode::Default,
            comments: Vec::new(),
            comment_input: false,
            detail_field: 0,
            show_priority_picker: false,
            priority_selection: 1,
            editing_field: false,
            task_form: None,
            task_filter: TaskFilter::Active,
            dock_focus: None,
            dock_filter: None,
            current_user_id: None,
            user_names: HashMap::new(),
            themes,
            theme_idx,
            show_theme_picker: false,
            theme_selection: theme_idx,
            websocket_connected: false,
            sync_token,
            completed_cache: HashMap::new(),
            comments_by_task: HashMap::new(),
            idle_timeout_secs,
            idle_forcer,
            ephemeral,
            last_sync_at: None,
            collapsed_folders: HashSet::new(),
            folder_cursor: None,
            current_user_name: None,
            today_view_active: false,
            upcoming_view_active: false,
            active_pr_org: None,
            github_prs: Vec::new(),
            github_prs_loading: false,
            github_prs_error: None,
            github_prs_fetched_at: None,
            selected_pr: 0,
            gh_available: binary_available("gh"),
            hidden_pr_orgs: load_hidden_pr_orgs(),
            jira_cards_view_active: false,
            jira_cards: Vec::new(),
            jira_cards_loading: false,
            jira_cards_error: None,
            jira_cards_fetched_at: None,
            selected_jira_card: 0,
            acli_available: binary_available("acli"),
            agenda_view_active: false,
            agenda_events: Vec::new(),
            agenda_loading: false,
            agenda_error: None,
            agenda_fetched_at: None,
            selected_agenda_item: 0,
            gws_available: gws_calendar_configured(),
            show_stats,
            star_count,
            star_date,
            pomodoro_started_at: None,
            tomato_count,
            tomato_date,
            // Start on the All view — it's the primary landing page (first
            // sidebar entry) and surfaces Today's tasks, open PRs, and Jira
            // cards in one place.
            all_view_active: true,
            selected_all_item: 0,
            detail_task_id: None,
            overdue_section_collapsed: false,
            last_activity: Instant::now(),
            pending_ws_sync: false,
            github_prs_poll_interval_secs: load_github_prs_poll_interval_secs(),
            pending_pr_poll: false,
            jira_cards_poll_interval_secs: load_jira_cards_poll_interval_secs(),
            pending_jira_poll: false,
            agenda_poll_interval_secs: load_agenda_poll_interval_secs(),
            pending_agenda_poll: false,
            comments_fetch_seq: 0,
            websocket_url: None,
            pending_commands: Vec::new(),
            temp_id_pending: HashMap::new(),
            pending_close_recurring: HashSet::new(),
            bg_tx,
            bg_rx,
            client: Arc::new(client),
        }
    }

    pub async fn load_with_splash(&mut self, terminal: &mut DefaultTerminal) {
        info!(sync_token = %self.sync_token, "full sync starting");

        terminal
            .draw(|f| ui::splash::render(f, 0.0, "connecting to todoist...", self.theme()))
            .ok();

        let req = SyncRequest {
            sync_token: "*".to_string(),
            resource_types: vec![
                "items".to_string(),
                "projects".to_string(),
                "sections".to_string(),
                "labels".to_string(),
                "notes".to_string(),
                "collaborators".to_string(),
                "workspaces".to_string(),
                "folders".to_string(),
                "user".to_string(),
            ],
            commands: vec![],
        };

        terminal
            .draw(|f| ui::splash::render(f, 0.3, "syncing data...", self.theme()))
            .ok();

        match self.client.sync(&req).await {
            Ok(resp) => {
                terminal
                    .draw(|f| ui::splash::render(f, 0.8, "applying sync...", self.theme()))
                    .ok();
                self.apply_sync_delta(resp);

                terminal
                    .draw(|f| ui::splash::render(f, 1.0, "ready", self.theme()))
                    .ok();

                info!(
                    projects = self.projects.len(),
                    tasks = self.tasks.len(),
                    labels = self.labels.len(),
                    users = self.user_names.len(),
                    "full sync complete"
                );

                if let Some(url) = self.websocket_url.clone() {
                    self.spawn_websocket(url);
                }
            }
            Err(e) => {
                self.set_error(&e, "Initial sync");
            }
        }
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        info!("entering main loop");

        // One-time fetch of open PRs at startup so the per-org sidebar
        // entries can appear once the response lands. If gh isn't on PATH
        // this is a no-op (the sidebar simply won't show any PR entries).
        if self.gh_available && self.github_prs.is_empty() && self.github_prs_fetched_at.is_none() {
            self.spawn_github_prs_fetch();
        }
        if self.acli_available
            && self.jira_cards.is_empty()
            && self.jira_cards_fetched_at.is_none()
        {
            self.spawn_jira_cards_fetch();
        }
        if self.gws_available
            && self.agenda_events.is_empty()
            && self.agenda_fetched_at.is_none()
        {
            self.spawn_agenda_fetch();
        }

        while self.running {
            self.drain_bg_results();
            self.maybe_poll_github_prs();
            self.maybe_poll_jira_cards();
            self.maybe_poll_agenda();
            // Roll the star jar to zero when the local date has changed
            // since the last observed count (e.g. app left running past
            // midnight). Mutates state so render can stay `&App`.
            self.roll_star_jar_if_new_day();
            self.roll_tomato_jar_if_new_day();
            // Completion check for the active pomodoro, if any — cheap
            // enough to run every frame.
            self.maybe_award_tomato();

            terminal.draw(|frame| ui::draw(frame, self))?;

            if event::poll(Duration::from_millis(16))?
                && let Event::Key(key) = event::read()?
            {
                let was_idle = self.is_idle();
                self.last_activity = Instant::now();
                if was_idle && self.pending_ws_sync {
                    self.pending_ws_sync = false;
                    self.spawn_incremental_sync();
                }
                if was_idle && self.pending_pr_poll {
                    self.pending_pr_poll = false;
                    self.spawn_github_prs_fetch();
                }
                if was_idle && self.pending_jira_poll {
                    self.pending_jira_poll = false;
                    self.spawn_jira_cards_fetch();
                }
                if was_idle && self.pending_agenda_poll {
                    self.pending_agenda_poll = false;
                    self.spawn_agenda_fetch();
                }

                if self.error.is_some() {
                    self.handle_error_dismiss();
                    continue;
                }

                let prev_pane = self.active_pane;
                match keys::handle_key(self, key) {
                    KeyAction::Quit => {
                        info!("quit requested");
                        self.running = false;
                    }
                    KeyAction::ProjectChanged => self.switch_to_project_tasks(),
                    KeyAction::AllViewSelected => self.activate_all_view(),
                    KeyAction::TodayViewSelected => self.activate_today_view(),
                    KeyAction::UpcomingViewSelected => self.activate_upcoming_view(),
                    KeyAction::RefreshGithubPrs => self.refresh_github_prs(),
                    KeyAction::OpenSelectedPrInBrowser => self.open_selected_pr_in_browser(),
                    KeyAction::JiraCardsViewSelected => self.activate_jira_cards_view(),
                    KeyAction::RefreshJiraCards => self.refresh_jira_cards(),
                    KeyAction::RefreshAllSources => self.refresh_all_sources(),
                    KeyAction::OpenSelectedJiraCardInBrowser => {
                        self.open_selected_jira_card_in_browser()
                    }
                    KeyAction::AgendaViewSelected => self.activate_agenda_view(),
                    KeyAction::RefreshAgenda => self.refresh_agenda(),
                    KeyAction::OpenSelectedAgendaEventInBrowser => {
                        self.open_selected_event_in_browser()
                    }
                    KeyAction::ToggleOverdueSection => self.toggle_overdue_section(),
                    KeyAction::TogglePomodoro => self.toggle_pomodoro(),
                    KeyAction::OpenDetail => self.open_detail(),
                    KeyAction::CloseDetail => {
                        self.active_pane = Pane::Tasks;
                        self.detail_scroll = 0;
                        self.detail_task_id = None;
                    }
                    KeyAction::ToggleSettings => {
                        self.show_settings = !self.show_settings;
                        self.active_pane = if self.show_settings {
                            Pane::Settings
                        } else {
                            Pane::Projects
                        };
                    }
                    KeyAction::ToggleHelp => self.show_help = !self.show_help,
                    KeyAction::ToggleMode => self.toggle_input_mode(),
                    KeyAction::ToggleCollapse => self.toggle_collapse(),
                    KeyAction::ToggleFolderCollapse => self.toggle_folder_collapse(),
                    KeyAction::OpenAllFolds => self.collapsed.clear(),
                    KeyAction::CloseAllFolds => self.close_all_folds(),
                    KeyAction::CompleteTask => self.complete_selected_task(),
                    KeyAction::CompleteTaskById(id) => self.complete_task_by_id(id),
                    KeyAction::OpenDetailById(id) => self.open_detail_for(id),
                    KeyAction::OpenPriorityPicker => {
                        if let Some(task) = self.selected_task() {
                            self.priority_selection = task.priority;
                            self.show_priority_picker = true;
                        }
                    }
                    KeyAction::SelectPriority => {
                        self.show_priority_picker = false;
                        if let Some(form) = &mut self.task_form {
                            form.priority = self.priority_selection;
                        } else {
                            self.apply_priority(self.priority_selection);
                        }
                    }
                    KeyAction::StarProject => self.star_selected_project(),
                    KeyAction::CycleFilter => self.cycle_task_filter(),
                    KeyAction::CycleSort => {
                        self.sort_mode = self.sort_mode.next();
                        info!(sort = self.sort_mode.label(), "sort mode changed");
                    }
                    KeyAction::StartInput => self.start_input(),
                    KeyAction::StartCommentInput => self.start_comment_input(),
                    KeyAction::StartFieldEdit => self.start_field_edit(),
                    KeyAction::SubmitInput => self.submit_input(),
                    KeyAction::SubmitForm => self.submit_task_form(),
                    KeyAction::FormFieldUp => self.form_field_up(),
                    KeyAction::FormFieldDown => self.form_field_down(),
                    KeyAction::FormEditField => self.form_edit_field(),
                    KeyAction::FormEscNormal => {
                        self.submit_input();
                    }
                    KeyAction::CancelInput => self.cancel_input(),
                    KeyAction::DetailFieldUp => self.move_detail_field(-1),
                    KeyAction::DetailFieldDown => self.move_detail_field(1),
                    KeyAction::OpenThemePicker => {
                        self.theme_selection = self.theme_idx;
                        self.show_theme_picker = true;
                    }
                    KeyAction::SelectTheme => {
                        self.theme_idx = self.theme_selection;
                        self.show_theme_picker = false;
                        self.save_ui_settings();
                    }
                    KeyAction::CloseThemePicker => {
                        self.show_theme_picker = false;
                    }
                    KeyAction::Consumed | KeyAction::None => {}
                }
                if matches!(prev_pane, Pane::Tasks) && !matches!(self.active_pane, Pane::Tasks) {
                    self.dock_filter = None;
                }
            }
        }

        info!("exiting main loop");
        Ok(())
    }

    fn apply_sync_delta(&mut self, resp: SyncResponse) {
        if resp.full_sync {
            if let Some(projects) = resp.projects {
                self.projects = projects
                    .into_iter()
                    .filter(|p| !p.is_deleted.unwrap_or(false))
                    .collect();
                self.sort_projects();
            }
            if let Some(items) = resp.items {
                self.tasks = items.into_iter().filter(|t| !t.is_deleted).collect();
            }
            if let Some(labels) = resp.labels {
                self.labels = labels
                    .into_iter()
                    .filter(|l| !l.is_deleted.unwrap_or(false))
                    .collect();
            }
            if let Some(sections) = resp.sections {
                self.sections = sections
                    .into_iter()
                    .filter(|s| !s.is_deleted.unwrap_or(false))
                    .collect();
            }
            if let Some(notes) = resp.notes {
                self.comments_by_task.clear();
                for note in notes {
                    if !note.is_deleted {
                        let tid = note
                            .item_id
                            .clone()
                            .or_else(|| note.task_id.clone())
                            .unwrap_or_default();
                        self.comments_by_task.entry(tid).or_default().push(note);
                    }
                }
            }
            if let Some(collabs) = resp.collaborators {
                for c in collabs {
                    self.user_names
                        .entry(c.id.clone())
                        .or_insert_with(|| UserRecord::new(c.id, c.name, c.email));
                }
            }
            if let Some(workspaces) = resp.workspaces {
                self.workspaces = workspaces.into_iter().filter(|w| !w.is_deleted).collect();
            }
            if let Some(folders) = resp.folders {
                self.folders = folders.into_iter().filter(|f| !f.is_deleted).collect();
            }
            if let Some(user) = resp.user {
                self.current_user_id = Some(user.id.clone());
                self.websocket_url = user.websocket_url;
                if let Some(name) = &user.full_name {
                    self.current_user_name = Some(name.clone());
                }
                self.user_names
                    .entry(user.id.clone())
                    .or_insert_with(|| UserRecord::new(user.id, user.full_name, user.email));
            }
        } else {
            if let Some(projects) = resp.projects {
                for p in projects {
                    if p.is_deleted.unwrap_or(false) {
                        self.projects.retain(|e| e.id != p.id);
                    } else if let Some(e) = self.projects.iter_mut().find(|e| e.id == p.id) {
                        *e = p;
                    } else {
                        self.projects.push(p);
                    }
                }
                self.sort_projects();
            }
            if let Some(items) = resp.items {
                for item in items {
                    if item.is_deleted {
                        self.tasks.retain(|t| t.id != item.id);
                    } else if let Some(e) = self.tasks.iter_mut().find(|t| t.id == item.id) {
                        *e = item;
                    } else {
                        self.tasks.push(item);
                    }
                }
            }
            if let Some(labels) = resp.labels {
                for l in labels {
                    if l.is_deleted.unwrap_or(false) {
                        self.labels.retain(|e| e.id != l.id);
                    } else if let Some(e) = self.labels.iter_mut().find(|e| e.id == l.id) {
                        *e = l;
                    } else {
                        self.labels.push(l);
                    }
                }
            }
            if let Some(sections) = resp.sections {
                for s in sections {
                    if s.is_deleted.unwrap_or(false) {
                        self.sections.retain(|e| e.id != s.id);
                    } else if let Some(e) = self.sections.iter_mut().find(|e| e.id == s.id) {
                        *e = s;
                    } else {
                        self.sections.push(s);
                    }
                }
            }
            if let Some(notes) = resp.notes {
                let open_task_id = self.selected_task().map(|t| t.id.clone());
                let mut affected_task: Option<String> = None;
                for note in notes {
                    let tid = note
                        .item_id
                        .clone()
                        .or_else(|| note.task_id.clone())
                        .unwrap_or_default();
                    if note.is_deleted {
                        if let Some(list) = self.comments_by_task.get_mut(&tid) {
                            list.retain(|c| c.id != note.id);
                        }
                    } else if let Some(list) = self.comments_by_task.get_mut(&tid) {
                        if let Some(c) = list.iter_mut().find(|c| c.id == note.id) {
                            *c = note;
                        } else {
                            list.push(note);
                        }
                    } else {
                        self.comments_by_task.insert(tid.clone(), vec![note]);
                    }
                    if open_task_id.as_deref() == Some(&tid) {
                        affected_task = Some(tid);
                    }
                }
                if let Some(tid) = affected_task
                    && let Some(updated) = self.comments_by_task.get(&tid)
                {
                    self.comments = updated.clone();
                }
            }
        }

        if !resp.sync_token.is_empty() {
            self.sync_token = resp.sync_token;
            self.save_sync_token();
        }
        self.last_sync_at = Some(Local::now());

        // Keep selection in bounds after any sync.
        let visible_len = self.visible_tasks().len();
        if visible_len == 0 {
            self.selected_task = 0;
        } else if self.selected_task >= visible_len {
            self.selected_task = visible_len - 1;
        }
    }

    fn flush_commands(&mut self) {
        if self.pending_commands.is_empty() {
            return;
        }

        let commands = std::mem::take(&mut self.pending_commands);
        let client = Arc::clone(&self.client);
        let tx = self.bg_tx.clone();
        let sync_token = self.sync_token.clone();

        tokio::spawn(async move {
            let req = SyncRequest {
                sync_token,
                resource_types: vec![],
                commands,
            };
            let result = client.sync(&req).await;
            match result {
                Ok(resp) => {
                    let _ = tx.send(BgResult::CommandResults(Box::new(resp))).await;
                }
                Err(e) => {
                    error!(error = %e, "command flush failed");
                    // Commands stay lost on network failure; next WS-triggered
                    // sync will correct server state.
                }
            }
        });
    }

    fn apply_temp_id_mapping(&mut self, temp_id: &str, real_id: &str) {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == temp_id) {
            t.id = real_id.to_string();
        }
        for c in &mut self.comments {
            if c.id == temp_id {
                c.id = real_id.to_string();
            }
            if c.item_id.as_deref() == Some(temp_id) {
                c.item_id = Some(real_id.to_string());
            }
        }
    }

    fn revert_optimistic(&mut self, op: OptimisticOp) {
        match op {
            OptimisticOp::TaskAdded { temp_id } => {
                self.tasks.retain(|t| t.id != temp_id);
            }
            OptimisticOp::TaskRemoved { snapshot } => {
                self.tasks.push(snapshot);
            }
            OptimisticOp::TaskUpdated { task_id, before } => {
                if let Some(t) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                    *t = before;
                }
            }
            OptimisticOp::CommentAdded { temp_id, task_id } => {
                let current = self.selected_task().map(|t| t.id.clone());
                if current.as_deref() == Some(&task_id) {
                    self.comments.retain(|c| c.id != temp_id);
                }
            }
            OptimisticOp::ProjectUpdated { project_id, before } => {
                if let Some(p) = self.projects.iter_mut().find(|p| p.id == project_id) {
                    *p = before;
                }
                self.sort_projects();
            }
        }
    }

    fn save_sync_token(&self) {
        if self.ephemeral {
            return;
        }
        let config_dir = ratatoist_core::config::Config::config_dir();
        let state = SyncState {
            sync_token: self.sync_token.clone(),
        };
        if let Err(e) = state.save(&config_dir) {
            warn!(error = %e, "failed to persist sync token");
        }
    }

    fn spawn_websocket(&self, url: String) {
        let tx = self.bg_tx.clone();
        tokio::spawn(run_websocket(url, tx));
    }

    fn spawn_incremental_sync(&self) {
        let client = Arc::clone(&self.client);
        let tx = self.bg_tx.clone();
        let sync_token = self.sync_token.clone();

        tokio::spawn(async move {
            let req = SyncRequest {
                sync_token,
                resource_types: vec![
                    "items".to_string(),
                    "projects".to_string(),
                    "sections".to_string(),
                    "labels".to_string(),
                    "notes".to_string(),
                ],
                commands: vec![],
            };
            match client.sync(&req).await {
                Ok(resp) => {
                    let _ = tx.send(BgResult::SyncDelta(Box::new(resp))).await;
                }
                Err(e) => {
                    error!(error = %e, "incremental sync failed");
                }
            }
        });
    }

    fn drain_bg_results(&mut self) {
        while let Ok(result) = self.bg_rx.try_recv() {
            match result {
                BgResult::SyncDelta(resp) => {
                    self.apply_sync_delta(*resp);
                }

                BgResult::CommandResults(resp) => {
                    let mut refresh_comments_for: Option<String> = None;
                    for (uuid, status) in &resp.sync_status {
                        if status.is_err() {
                            if let Some(op) = self.temp_id_pending.remove(uuid) {
                                if let OptimisticOp::TaskUpdated { task_id, .. } = &op {
                                    self.pending_close_recurring.remove(task_id);
                                }
                                self.revert_optimistic(op);
                            }
                            let msg = status
                                .error_message()
                                .unwrap_or("unknown error")
                                .to_string();
                            error!(uuid, error = %msg, "command rejected by server");
                            self.error = Some(AppError {
                                title: "Command failed".to_string(),
                                message: msg,
                                suggestion: None,
                                recoverable: true,
                            });
                        } else if let Some(op) = self.temp_id_pending.remove(uuid) {
                            match &op {
                                OptimisticOp::CommentAdded { task_id, .. } => {
                                    let current = self.selected_task().map(|t| t.id.clone());
                                    if current.as_deref() == Some(task_id.as_str()) {
                                        refresh_comments_for = Some(task_id.clone());
                                    }
                                }
                                OptimisticOp::TaskUpdated { task_id, .. } => {
                                    // Recurring close landed: the items delta
                                    // in this same response carries the new
                                    // due date, so we can drop the pending
                                    // placeholder and let the task reappear
                                    // (now under tomorrow in Upcoming).
                                    self.pending_close_recurring.remove(task_id);
                                }
                                _ => {}
                            }
                        }
                    }
                    for (temp_id, real_id) in &resp.temp_id_mapping {
                        self.apply_temp_id_mapping(temp_id, real_id);
                    }
                    if !resp.sync_token.is_empty() {
                        self.sync_token = resp.sync_token.clone();
                        self.save_sync_token();
                    }
                    if let Some(tid) = refresh_comments_for {
                        self.spawn_comments_fetch(tid);
                    }
                }

                BgResult::CompletedTasks {
                    project_id,
                    records,
                } => match records {
                    Ok(r) => {
                        self.completed_cache.insert(project_id, r);
                    }
                    Err(e) => self.set_error(&e, "Load completed tasks"),
                },

                BgResult::WebSocketConnected => {
                    debug!("websocket connected");
                    self.websocket_connected = true;
                }
                BgResult::WebSocketEvent => {
                    self.websocket_connected = true;
                    if self.is_idle() {
                        self.pending_ws_sync = true;
                    } else {
                        self.spawn_incremental_sync();
                    }
                }
                BgResult::WebSocketDisconnected => {
                    debug!("websocket disconnected");
                    self.websocket_connected = false;
                }

                BgResult::GithubPrsFetched(result) => {
                    self.github_prs_loading = false;
                    self.github_prs_fetched_at = Some(Local::now());
                    match result {
                        Ok(prs) => {
                            self.github_prs = prs;
                            self.selected_pr = self
                                .selected_pr
                                .min(self.github_prs.len().saturating_sub(1));
                            self.github_prs_error = None;
                        }
                        Err(e) => {
                            error!(error = %e, "github PR fetch failed");
                            self.github_prs_error = Some(e.to_string());
                        }
                    }
                }

                BgResult::JiraCardsFetched(result) => {
                    self.jira_cards_loading = false;
                    self.jira_cards_fetched_at = Some(Local::now());
                    match result {
                        Ok(cards) => {
                            self.jira_cards = cards;
                            self.selected_jira_card = self
                                .selected_jira_card
                                .min(self.jira_cards.len().saturating_sub(1));
                            self.jira_cards_error = None;
                        }
                        Err(e) => {
                            error!(error = %e, "jira cards fetch failed");
                            self.jira_cards_error = Some(e.to_string());
                        }
                    }
                }

                BgResult::AgendaFetched(result) => {
                    self.agenda_loading = false;
                    self.agenda_fetched_at = Some(Local::now());
                    match result {
                        Ok(events) => {
                            self.agenda_events = events;
                            self.selected_agenda_item = self
                                .selected_agenda_item
                                .min(self.agenda_events.len().saturating_sub(1));
                            self.agenda_error = None;
                        }
                        Err(e) => {
                            error!(error = %e, "agenda fetch failed");
                            self.agenda_error = Some(e.to_string());
                        }
                    }
                }

                BgResult::Comments {
                    task_id,
                    comments,
                    fetch_seq,
                } => match comments {
                    Ok(c) => {
                        let count = c.len() as i32;
                        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                            t.note_count = Some(count);
                        }
                        self.comments_by_task.insert(task_id.clone(), c.clone());
                        let current_tid = self.selected_task().map(|t| t.id.clone());
                        if current_tid.as_deref() == Some(&task_id)
                            && fetch_seq == self.comments_fetch_seq
                        {
                            self.comments = c;
                        }
                    }
                    Err(e) => self.set_error(&e, "Load comments"),
                },
            }
        }
    }

    fn open_detail(&mut self) {
        let task_id = {
            let visible = self.visible_tasks();
            match visible.get(self.selected_task) {
                Some(t) => t.id.clone(),
                None => return,
            }
        };
        self.open_detail_for(task_id);
    }

    /// Open the detail pane for a specific task ID. Used by the All view,
    /// where the visible-tasks index isn't a valid handle for the selected
    /// task. Shared by the regular `open_detail` path too, so both flows go
    /// through the same setup.
    fn open_detail_for(&mut self, task_id: String) {
        let task_project_id = match self.tasks.iter().find(|t| t.id == task_id) {
            Some(t) => t.project_id.clone(),
            None => return,
        };

        if self.dock_filter.is_some()
            && let Some(pos) = self.projects.iter().position(|p| p.id == task_project_id)
        {
            self.selected_project = pos;
        }

        self.detail_task_id = Some(task_id.clone());
        self.active_pane = Pane::Detail;
        self.detail_scroll = 0;
        self.detail_field = 0;

        // Serve cached comments immediately, refresh in background.
        if let Some(cached) = self.comments_by_task.get(&task_id) {
            self.comments = cached.clone();
        } else {
            self.comments.clear();
        }
        self.spawn_comments_fetch(task_id);
    }

    fn spawn_comments_fetch(&mut self, task_id: String) {
        self.comments_fetch_seq += 1;
        let fetch_seq = self.comments_fetch_seq;
        let client = Arc::clone(&self.client);
        let tx = self.bg_tx.clone();
        let tid = task_id.clone();

        tokio::spawn(async move {
            let comments = client.get_comments(&tid).await;
            let _ = tx
                .send(BgResult::Comments {
                    task_id: tid,
                    comments,
                    fetch_seq,
                })
                .await;
        });
    }

    fn spawn_completed_tasks_fetch(&self, project_id: String) {
        let client = Arc::clone(&self.client);
        let tx = self.bg_tx.clone();
        let pid = project_id.clone();

        tokio::spawn(async move {
            let records = client.get_completed_tasks(Some(&pid), None).await;
            let _ = tx
                .send(BgResult::CompletedTasks {
                    project_id: pid,
                    records,
                })
                .await;
        });
    }

    pub fn activate_all_view(&mut self) {
        tracing::debug!("all view activated");
        self.all_view_active = true;
        self.today_view_active = false;
        self.upcoming_view_active = false;
        self.active_pr_org = None;
        self.jira_cards_view_active = false;
        self.agenda_view_active = false;
        self.selected_all_item = 0;
    }

    /// Build the combined item list for the All view. Today's calendar
    /// events first (they're time-bound and the most time-sensitive thing
    /// on the list), then Today tasks, then non-hidden PRs, then Jira
    /// cards. Indices point into the respective source collections at
    /// call time.
    pub fn all_view_items(&self) -> Vec<AllViewItem> {
        let mut items = Vec::new();

        // Today's agenda events, in the order the API returned (startTime).
        for (i, _) in self.agenda_events.iter().enumerate() {
            items.push(AllViewItem::AgendaEvent(i));
        }

        // Today tasks (same filter as Today view, minus overdue-collapse).
        let today = crate::ui::dates::today_str();
        let current_hour = crate::ui::dates::current_local_hour();
        let is_weekend = crate::ui::dates::is_weekend_local();
        for (i, t) in self.tasks.iter().enumerate() {
            if t.is_deleted || t.checked || t.parent_id.is_some() {
                continue;
            }
            if self.pending_close_recurring.contains(&t.id) {
                continue;
            }
            if crate::ui::dates::evening_task_hidden(&t.labels, current_hour) {
                continue;
            }
            if crate::ui::dates::work_weekend_hidden(&t.labels, is_weekend) {
                continue;
            }
            let due_today_or_overdue = t
                .due
                .as_ref()
                .is_some_and(|d| d.date.as_str() <= today.as_str());
            if !due_today_or_overdue {
                continue;
            }
            match &t.responsible_uid {
                None => {}
                Some(uid) if self.current_user_id.as_deref() == Some(uid.as_str()) => {}
                _ => continue,
            }
            items.push(AllViewItem::Task(i));
        }

        // Non-hidden PRs.
        for (i, pr) in self.github_prs.iter().enumerate() {
            if let Some((owner, _)) = pr.repo_full_name.split_once('/')
                && self.hidden_pr_orgs.contains(owner)
            {
                continue;
            }
            items.push(AllViewItem::PullRequest(i));
        }

        // Jira cards.
        for (i, _) in self.jira_cards.iter().enumerate() {
            items.push(AllViewItem::JiraCard(i));
        }

        items
    }

    /// Fire a PR fetch if it's time (interval elapsed since last fetch, gh
    /// available, not already loading). If the user is idle, defer until
    /// they're back — mirrors the pattern used for the Todoist WebSocket
    /// sync. Called once per main-loop tick; the cost is a timestamp compare.
    fn maybe_poll_github_prs(&mut self) {
        if !self.gh_available || self.github_prs_loading {
            return;
        }
        let elapsed_secs = self
            .github_prs_fetched_at
            .map(|at| (Local::now() - at).num_seconds())
            .unwrap_or(i64::MAX);
        if elapsed_secs < self.github_prs_poll_interval_secs as i64 {
            return;
        }
        if self.is_idle() {
            // Hold off while idle; fire on the next keystroke so the user
            // always sees fresh data on return but we don't burn requests
            // while they're AFK.
            self.pending_pr_poll = true;
            return;
        }
        self.spawn_github_prs_fetch();
    }

    /// Mirror of `maybe_poll_github_prs` for Jira cards.
    fn maybe_poll_jira_cards(&mut self) {
        if !self.acli_available || self.jira_cards_loading {
            return;
        }
        let elapsed_secs = self
            .jira_cards_fetched_at
            .map(|at| (Local::now() - at).num_seconds())
            .unwrap_or(i64::MAX);
        if elapsed_secs < self.jira_cards_poll_interval_secs as i64 {
            return;
        }
        if self.is_idle() {
            self.pending_jira_poll = true;
            return;
        }
        self.spawn_jira_cards_fetch();
    }

    /// Mirror of `maybe_poll_github_prs` for agenda events.
    fn maybe_poll_agenda(&mut self) {
        if !self.gws_available || self.agenda_loading {
            return;
        }
        let elapsed_secs = self
            .agenda_fetched_at
            .map(|at| (Local::now() - at).num_seconds())
            .unwrap_or(i64::MAX);
        if elapsed_secs < self.agenda_poll_interval_secs as i64 {
            return;
        }
        if self.is_idle() {
            self.pending_agenda_poll = true;
            return;
        }
        self.spawn_agenda_fetch();
    }

    pub fn refresh_all_sources(&mut self) {
        self.spawn_github_prs_fetch();
        self.spawn_jira_cards_fetch();
        self.spawn_agenda_fetch();
    }

    fn switch_to_project_tasks(&mut self) {
        self.today_view_active = false;
        self.upcoming_view_active = false;
        self.active_pr_org = None;
        self.jira_cards_view_active = false;
        self.agenda_view_active = false;
        self.all_view_active = false;
        self.selected_task = 0;
        self.detail_scroll = 0;
    }

    pub fn activate_today_view(&mut self) {
        tracing::debug!("today view activated");
        self.today_view_active = true;
        self.upcoming_view_active = false;
        self.active_pr_org = None;
        self.jira_cards_view_active = false;
        self.agenda_view_active = false;
        self.all_view_active = false;
        self.overdue_section_collapsed = false;
        self.selected_task = 0;
        self.detail_scroll = 0;
    }

    /// Number of tasks that would appear in the Upcoming view (all scheduled
    /// active parent tasks assigned to me or unassigned). Used for the
    /// sidebar badge.
    pub fn upcoming_task_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| {
                if t.is_deleted || t.checked || t.parent_id.is_some() || t.due.is_none() {
                    return false;
                }
                match &t.responsible_uid {
                    None => true,
                    Some(uid) => self.current_user_id.as_deref() == Some(uid.as_str()),
                }
            })
            .count()
    }

    pub fn activate_upcoming_view(&mut self) {
        tracing::debug!("upcoming view activated");
        self.today_view_active = false;
        self.upcoming_view_active = true;
        self.active_pr_org = None;
        self.jira_cards_view_active = false;
        self.agenda_view_active = false;
        self.all_view_active = false;
        self.selected_task = 0;
        self.detail_scroll = 0;
    }

    pub fn activate_github_prs_view(&mut self, owner: String) {
        tracing::debug!(owner = %owner, "github PRs view activated");
        self.today_view_active = false;
        self.upcoming_view_active = false;
        self.active_pr_org = Some(owner);
        self.jira_cards_view_active = false;
        self.agenda_view_active = false;
        self.all_view_active = false;
        self.selected_pr = 0;
        // No fetch here — the initial fetch runs once on App::new().
        // Manual refresh via the `r` key still works.
    }

    pub fn activate_jira_cards_view(&mut self) {
        tracing::debug!("jira cards view activated");
        self.today_view_active = false;
        self.upcoming_view_active = false;
        self.active_pr_org = None;
        self.jira_cards_view_active = true;
        self.agenda_view_active = false;
        self.all_view_active = false;
        self.selected_jira_card = 0;
        self.spawn_jira_cards_fetch();
    }

    pub fn refresh_jira_cards(&mut self) {
        if self.jira_cards_view_active {
            self.spawn_jira_cards_fetch();
        }
    }

    pub fn open_selected_jira_card_in_browser(&mut self) {
        let Some(card) = self.jira_cards.get(self.selected_jira_card) else {
            return;
        };
        let key = card.key.clone();
        tokio::spawn(async move {
            let _ = tokio::process::Command::new("acli")
                .args(["jira", "workitem", "view", &key, "--web"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;
        });
    }

    fn spawn_jira_cards_fetch(&mut self) {
        if self.jira_cards_loading {
            return;
        }
        self.jira_cards_loading = true;
        self.jira_cards_error = None;
        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            let result = fetch_jira_cards().await;
            let _ = tx.send(BgResult::JiraCardsFetched(result)).await;
        });
    }

    pub fn activate_agenda_view(&mut self) {
        tracing::debug!("agenda view activated");
        self.today_view_active = false;
        self.upcoming_view_active = false;
        self.active_pr_org = None;
        self.jira_cards_view_active = false;
        self.agenda_view_active = true;
        self.all_view_active = false;
        self.selected_agenda_item = 0;
        self.spawn_agenda_fetch();
    }

    pub fn refresh_agenda(&mut self) {
        if self.agenda_view_active {
            self.spawn_agenda_fetch();
        }
    }

    pub fn open_selected_event_in_browser(&mut self) {
        // On the All view `selected_agenda_item` is a raw index into
        // `agenda_events`; inside the Agenda view it's also a raw index
        // (no filtering). Single helper for both paths.
        let Some(event) = self.agenda_events.get(self.selected_agenda_item) else {
            return;
        };
        let url = event.html_link.clone();
        if url.is_empty() {
            return;
        }
        // `open` is the macOS default-browser launcher. On other platforms
        // the binary is absent; if this ever ships cross-platform we'd
        // switch to a conditional.
        tokio::spawn(async move {
            let _ = tokio::process::Command::new("open")
                .arg(url)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;
        });
    }

    fn spawn_agenda_fetch(&mut self) {
        if self.agenda_loading {
            return;
        }
        self.agenda_loading = true;
        self.agenda_error = None;
        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            let result = fetch_agenda_events().await;
            let _ = tx.send(BgResult::AgendaFetched(result)).await;
        });
    }

    pub fn open_selected_pr_in_browser(&mut self) {
        // On the All view `selected_pr` is a raw `self.github_prs` index;
        // inside a PR-org view it indexes into the filtered
        // `active_org_prs()` list. Same key-action, two contexts.
        let pr = if self.all_view_active {
            self.github_prs.get(self.selected_pr)
        } else {
            self.active_org_prs().get(self.selected_pr).copied()
        };
        let Some(pr) = pr else {
            return;
        };
        let url = pr.url.clone();
        // `gh pr view <url> --web` opens the PR in the default browser. gh
        // prints "Opening <url>..." to stderr and occasionally an update
        // notice ("A new release of gh is available..."). Both would bleed
        // into the TUI, so we redirect both streams to the null device.
        tokio::spawn(async move {
            let _ = tokio::process::Command::new("gh")
                .args(["pr", "view", &url, "--web"])
                .env("GH_NO_UPDATE_NOTIFIER", "1")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;
        });
    }

    pub fn refresh_github_prs(&mut self) {
        if self.is_pr_view_active() {
            self.spawn_github_prs_fetch();
        }
    }

    fn spawn_github_prs_fetch(&mut self) {
        if self.github_prs_loading {
            return;
        }
        self.github_prs_loading = true;
        self.github_prs_error = None;
        let tx = self.bg_tx.clone();
        tokio::spawn(async move {
            let result = fetch_github_prs().await;
            let _ = tx.send(BgResult::GithubPrsFetched(result)).await;
        });
    }

    pub fn toggle_overdue_section(&mut self) {
        self.overdue_section_collapsed = !self.overdue_section_collapsed;
        // If collapsing, move cursor to first today task (index 0 in the new visible list).
        if self.overdue_section_collapsed {
            self.selected_task = 0;
        }
    }

    fn complete_selected_task(&mut self) {
        self.enqueue_complete_selected();
        self.flush_commands();
    }

    /// Complete a task addressed by its ID, bypassing the `selected_task` +
    /// `visible_tasks()` lookup. Used by the All view, whose item list spans
    /// tasks that aren't in the current project-scoped `visible_tasks()`.
    /// Clamps `selected_all_item` since the completed task is filtered out
    /// of the All view after the optimistic update.
    fn complete_task_by_id(&mut self, task_id: String) {
        self.enqueue_complete_task_by_id(task_id);
        let new_len = self.all_view_items().len();
        if new_len > 0 && self.selected_all_item >= new_len {
            self.selected_all_item = new_len - 1;
        }
        self.flush_commands();
    }

    /// Enqueue the complete/reopen command for the selected task and apply
    /// the optimistic UI update. Split out so tests can inspect the enqueued
    /// command without flushing (which spawns a tokio task and drains the
    /// pending queue).
    fn enqueue_complete_selected(&mut self) {
        let task_id = {
            let visible = self.visible_tasks();
            let Some(task) = visible.get(self.selected_task) else {
                return;
            };
            task.id.clone()
        };

        self.enqueue_complete_task_by_id(task_id);

        let new_len = self.visible_tasks().len();
        if new_len > 0 && self.selected_task >= new_len {
            self.selected_task = new_len - 1;
        }
    }

    /// Core of the complete/reopen flow: apply the optimistic update and
    /// enqueue the sync command for the given task ID. Caller is responsible
    /// for any view-specific selection clamping and for flushing.
    fn enqueue_complete_task_by_id(&mut self, task_id: String) {
        let Some(task) = self.tasks.iter().find(|t| t.id == task_id) else {
            return;
        };
        let was_checked = task.checked;
        let is_recurring = task.due.as_ref().map(|d| d.is_recurring).unwrap_or(false);

        let before = self.tasks.iter().find(|t| t.id == task_id).cloned();

        // For recurring tasks, completing advances the series to the next
        // occurrence — the task should stay in the list with a new due date,
        // not disappear. Skip the optimistic `checked` flip and let the sync
        // response deliver the advanced due date. Non-recurring complete and
        // any reopen still flip optimistically for instant feedback.
        let completing_recurring = !was_checked && is_recurring;
        if completing_recurring {
            // Hide from the visible list immediately; server will deliver the
            // advanced due date shortly. Prevents the user double-tapping `x`
            // and advancing the recurrence by two.
            self.pending_close_recurring.insert(task_id.clone());
        } else if let Some(t) = self.tasks.iter_mut().find(|t| t.id == task_id) {
            t.checked = !was_checked;
        }

        // Earn a star for today whenever we're *closing* a task (either
        // recurring advance or non-recurring complete). Reopens don't
        // subtract — the jar is a one-way record of effort, per
        // star-jar.spec.md.
        if !was_checked {
            self.increment_star_jar();
        }

        // item_close is the command that mirrors the official Todoist UI:
        // recurring tasks advance to the next occurrence, non-recurring tasks
        // close normally. item_complete (which we previously used for
        // recurring) marks the current instance complete without advancing
        // the series — the user sees the task stay put with the old due date.
        let cmd_type = if was_checked {
            "item_reopen"
        } else {
            "item_close"
        };

        let uuid = new_uuid();
        self.pending_commands.push(SyncCommand {
            r#type: cmd_type.to_string(),
            temp_id: None,
            uuid: uuid.clone(),
            args: serde_json::json!({ "id": task_id }),
        });

        if let Some(snapshot) = before {
            self.temp_id_pending.insert(
                uuid,
                OptimisticOp::TaskUpdated {
                    task_id,
                    before: snapshot,
                },
            );
        }
    }

    fn start_input(&mut self) {
        // Virtual views (Today, Upcoming) have no "current project" — fall
        // back to the user's Inbox. Today also pre-fills the due date so the
        // task shows up on Today without extra keystrokes; Upcoming leaves
        // the due date blank since the user hasn't committed to a specific
        // day just by being in the view.
        let inbox_id = || {
            self.projects
                .iter()
                .find(|p| p.is_inbox())
                .map(|p| p.id.clone())
                .unwrap_or_default()
        };
        // Mirror the Today-view defaults when the user's cursor is sitting
        // in the Today section of the All view (i.e. the selected row is a
        // Task — the only section that holds Tasks). See
        // `today-view-add.spec.md`.
        let on_all_view_today_section = self.all_view_active
            && self
                .all_view_items()
                .get(self.selected_all_item)
                .is_some_and(|i| matches!(i, AllViewItem::Task(_)));
        let (project_id, default_due) = if self.today_view_active || on_all_view_today_section {
            (inbox_id(), "today")
        } else if self.upcoming_view_active {
            (inbox_id(), "")
        } else {
            let pid = self
                .projects
                .get(self.selected_project)
                .map(|p| p.id.clone())
                .unwrap_or_default();
            (pid, "")
        };
        let mut form = TaskForm::new(project_id);
        if !default_due.is_empty() {
            form.due_string = default_due.to_string();
        }
        self.task_form = Some(form);
        self.show_input = true;
        self.input_buffer.clear();
        if let InputMode::Vim(_) = self.input_mode {
            self.input_mode = InputMode::Vim(VimState::Insert);
        }
    }

    fn submit_input(&mut self) {
        let content = self.input_buffer.trim().to_string();

        if self.comment_input {
            if !content.is_empty() {
                self.submit_comment(content);
            }
            self.cancel_input();
            return;
        }

        if self.editing_field {
            if !content.is_empty() {
                self.submit_field_edit(content);
            }
            self.cancel_input();
            return;
        }

        if let Some(form) = &self.task_form
            && form.editing
        {
            let field = form.active_field;
            let Some(mut form) = self.task_form.take() else {
                return;
            };
            match field {
                0 => {
                    // Content goes verbatim; the API parses any inline
                    // natural-language dates or priorities.
                    form.content = content;
                }
                2 => form.due_string = content,
                _ => {}
            }
            form.editing = false;
            self.task_form = Some(form);
            self.input_buffer.clear();
            self.show_input = false;
            if let InputMode::Vim(_) = self.input_mode {
                self.input_mode = InputMode::Vim(VimState::Normal);
            }
            return;
        }

        self.cancel_input();
    }

    pub fn submit_task_form(&mut self) {
        let Some(form) = self.task_form.take() else {
            return;
        };

        if form.content.trim().is_empty() {
            self.cancel_input();
            return;
        }

        let project_id = form.project_id.clone();

        let temp_id = new_temp_id();
        let uuid = new_uuid();

        let optimistic = Task {
            id: temp_id.clone(),
            content: form.content.clone(),
            project_id: project_id.clone(),
            priority: form.priority,
            ..Task::default()
        };
        self.tasks.push(optimistic);
        self.temp_id_pending.insert(
            uuid.clone(),
            OptimisticOp::TaskAdded {
                temp_id: temp_id.clone(),
            },
        );

        let args = build_item_add_args(&form, &project_id);

        self.pending_commands.push(SyncCommand {
            r#type: "item_add".to_string(),
            temp_id: Some(temp_id),
            uuid,
            args,
        });

        self.flush_commands();

        self.task_form = None;
        self.show_input = false;
        self.input_buffer.clear();
        if let InputMode::Vim(_) = self.input_mode {
            self.input_mode = InputMode::Vim(VimState::Normal);
        }
    }

    fn submit_comment(&mut self, content: String) {
        let Some(task) = self.selected_task() else {
            return;
        };
        let task_id = task.id.clone();

        let temp_id = new_temp_id();
        let uuid = new_uuid();

        let now = chrono::Utc::now().to_rfc3339();
        let optimistic = Comment {
            id: temp_id.clone(),
            content: content.clone(),
            posted_at: Some(now),
            posted_by_uid: self.current_user_id.clone(),
            task_id: Some(task_id.clone()),
            item_id: Some(task_id.clone()),
            ..Comment::default()
        };
        self.comments.push(optimistic);
        self.comments_fetch_seq += 1;

        self.temp_id_pending.insert(
            uuid.clone(),
            OptimisticOp::CommentAdded {
                temp_id: temp_id.clone(),
                task_id: task_id.clone(),
            },
        );
        self.pending_commands.push(SyncCommand {
            r#type: "note_add".to_string(),
            temp_id: Some(temp_id),
            uuid,
            args: serde_json::json!({ "item_id": task_id, "content": content }),
        });
        self.flush_commands();
    }

    fn submit_field_edit(&mut self, value: String) {
        let (task_id, before) = {
            let Some(task) = self.selected_task() else {
                return;
            };
            (task.id.clone(), task.clone())
        };

        let uuid = new_uuid();
        let args = match self.detail_field {
            0 => {
                if let Some(t) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                    t.content = value.clone();
                }
                serde_json::json!({ "id": task_id, "content": value })
            }
            2 => {
                // Due string: server parses and returns the Due object — no
                // optimistic update possible here. Sync API takes a `due`
                // object, not the REST-style `due_string` shorthand.
                serde_json::json!({ "id": task_id, "due": { "string": value } })
            }
            3 => {
                if let Some(t) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                    t.description = value.clone();
                }
                serde_json::json!({ "id": task_id, "description": value })
            }
            _ => return,
        };

        self.temp_id_pending.insert(
            uuid.clone(),
            OptimisticOp::TaskUpdated {
                task_id: task_id.clone(),
                before,
            },
        );
        self.pending_commands.push(SyncCommand {
            r#type: "item_update".to_string(),
            temp_id: None,
            uuid,
            args,
        });
        self.flush_commands();
    }

    pub fn form_field_up(&mut self) {
        if let Some(form) = &mut self.task_form
            && !form.editing
        {
            let count = TaskForm::field_count();
            form.active_field = if form.active_field == 0 {
                count - 1
            } else {
                form.active_field - 1
            };
        }
    }

    pub fn form_field_down(&mut self) {
        if let Some(form) = &mut self.task_form
            && !form.editing
        {
            form.active_field = (form.active_field + 1) % TaskForm::field_count();
        }
    }

    pub fn form_edit_field(&mut self) {
        if let Some(form) = &mut self.task_form {
            match form.active_field {
                0 => {
                    self.input_buffer = form.content.clone();
                    form.editing = true;
                    self.show_input = true;
                    if let InputMode::Vim(_) = self.input_mode {
                        self.input_mode = InputMode::Vim(VimState::Insert);
                    }
                }
                1 => {
                    self.priority_selection = form.priority;
                    self.show_priority_picker = true;
                }
                2 => {
                    self.input_buffer = form.due_string.clone();
                    form.editing = true;
                    self.show_input = true;
                    if let InputMode::Vim(_) = self.input_mode {
                        self.input_mode = InputMode::Vim(VimState::Insert);
                    }
                }
                3 => {
                    let cur = self
                        .projects
                        .iter()
                        .position(|p| p.id == form.project_id)
                        .unwrap_or(0);
                    let next = (cur + 1) % self.projects.len().max(1);
                    if let Some(p) = self.projects.get(next) {
                        form.project_id = p.id.clone();
                    }
                }
                _ => {}
            }
        }
    }

    fn cancel_input(&mut self) {
        self.show_input = false;
        self.comment_input = false;
        self.editing_field = false;
        self.task_form = None;
        self.input_buffer.clear();
        if let InputMode::Vim(_) = self.input_mode {
            self.input_mode = InputMode::Vim(VimState::Normal);
        }
    }

    fn star_selected_project(&mut self) {
        let Some(project) = self.projects.get(self.selected_project) else {
            return;
        };
        let pid = project.id.clone();
        let before = project.clone();
        let new_fav = !project.is_favorite;

        if let Some(p) = self.projects.iter_mut().find(|p| p.id == pid) {
            p.is_favorite = new_fav;
        }
        self.sort_projects();

        let uuid = new_uuid();
        self.temp_id_pending.insert(
            uuid.clone(),
            OptimisticOp::ProjectUpdated {
                project_id: pid.clone(),
                before,
            },
        );
        self.pending_commands.push(SyncCommand {
            r#type: "project_update".to_string(),
            temp_id: None,
            uuid,
            args: serde_json::json!({ "id": pid, "is_favorite": new_fav }),
        });
        self.flush_commands();
    }

    fn sort_projects(&mut self) {
        let selected_id = self
            .projects
            .get(self.selected_project)
            .map(|p| p.id.clone());
        let source = self.projects.clone();
        let mut ordered: Vec<Project> = Vec::with_capacity(source.len());

        let personal: Vec<Project> = source
            .iter()
            .filter(|p| p.workspace_id.is_none())
            .cloned()
            .collect();
        collect_project_subtree(None, &personal, &mut ordered);

        let workspaces = self.workspaces.clone();
        for ws in &workspaces {
            let ws_projects: Vec<Project> = source
                .iter()
                .filter(|p| p.workspace_id.as_deref() == Some(ws.id.as_str()))
                .cloned()
                .collect();
            if ws_projects.is_empty() {
                continue;
            }

            let no_folder: Vec<Project> = ws_projects
                .iter()
                .filter(|p| p.folder_id.is_none())
                .cloned()
                .collect();
            collect_project_subtree(None, &no_folder, &mut ordered);

            let mut ws_folders: Vec<&Folder> = self
                .folders
                .iter()
                .filter(|f| f.workspace_id == ws.id)
                .collect();
            ws_folders.sort_by_key(|f| f.child_order);

            for folder in ws_folders {
                let in_folder: Vec<Project> = ws_projects
                    .iter()
                    .filter(|p| p.folder_id.as_deref() == Some(folder.id.as_str()))
                    .cloned()
                    .collect();
                collect_project_subtree(None, &in_folder, &mut ordered);
            }
        }

        let ordered_ids: HashSet<String> = ordered.iter().map(|p| p.id.clone()).collect();
        for p in &source {
            if !ordered_ids.contains(&p.id) {
                ordered.push(p.clone());
            }
        }

        self.projects = ordered;
        if let Some(id) = selected_id
            && let Some(pos) = self.projects.iter().position(|p| p.id == id)
        {
            self.selected_project = pos;
        }
    }

    pub fn project_list_entries(&self) -> Vec<ProjectEntry> {
        let mut entries = Vec::new();
        // All is the top-level dashboard row — lives above everything else,
        // independent of the Inbox grouping. Today / Upcoming / PR / Jira
        // views still sit under Inbox as virtual children of the personal
        // project group.
        entries.push(ProjectEntry::AllView);
        let mut in_personal = false;
        let mut last_ws_id: Option<&str> = None;
        let mut last_folder_id: Option<&str> = None;

        for (i, p) in self.projects.iter().enumerate() {
            let ws_id = p.workspace_id.as_deref();
            let folder_id = p.folder_id.as_deref();

            let folder_collapsed = folder_id
                .map(|fid| self.collapsed_folders.contains(fid))
                .unwrap_or(false);

            if ws_id.is_none() {
                if !in_personal {
                    in_personal = true;
                    entries.push(ProjectEntry::PersonalHeader);
                }
            } else {
                if last_ws_id != ws_id {
                    last_ws_id = ws_id;
                    last_folder_id = None;
                    entries.push(ProjectEntry::Separator);
                    if let Some(wi) = self
                        .workspaces
                        .iter()
                        .position(|w| w.id.as_str() == ws_id.unwrap())
                    {
                        entries.push(ProjectEntry::WorkspaceHeader(wi));
                    }
                }
                if last_folder_id != folder_id {
                    last_folder_id = folder_id;
                    if let Some(fid) = folder_id
                        && let Some(fi) = self.folders.iter().position(|f| f.id.as_str() == fid)
                    {
                        entries.push(ProjectEntry::FolderHeader(fi));
                    }
                }
            }

            if !folder_collapsed {
                let is_inbox = self.projects[i].is_inbox();
                entries.push(ProjectEntry::Project(i));
                if is_inbox {
                    entries.push(ProjectEntry::TodayView);
                    entries.push(ProjectEntry::UpcomingView);
                    // One Pull Requests entry per GitHub owner that has open
                    // PRs right now. We only emit entries for owners we've
                    // seen data for, so before the startup fetch completes
                    // (or if it fails), none appear.
                    if self.gh_available {
                        for (owner, _) in self.pr_owners_with_counts() {
                            entries.push(ProjectEntry::GithubPrsView(owner));
                        }
                    }
                    if self.acli_available {
                        entries.push(ProjectEntry::JiraCardsView);
                    }
                    if self.gws_available {
                        entries.push(ProjectEntry::AgendaView);
                    }
                }
            }
        }

        entries
    }

    pub fn project_indent(&self, project: &Project) -> usize {
        let base = if project.folder_id.is_some() { 3 } else { 1 };
        base + self.project_depth(&project.id)
    }

    pub fn project_depth(&self, project_id: &str) -> usize {
        let mut depth = 0;
        let mut current = project_id;
        loop {
            let Some(parent_id) = self
                .projects
                .iter()
                .find(|p| p.id == current)
                .and_then(|p| p.parent_id.as_deref())
            else {
                break;
            };
            depth += 1;
            current = parent_id;
        }
        depth
    }

    pub fn visible_nav_items(&self) -> Vec<ProjectNavItem> {
        self.project_list_entries()
            .into_iter()
            .filter_map(|e| match e {
                ProjectEntry::FolderHeader(fi) => Some(ProjectNavItem::Folder(fi)),
                ProjectEntry::Project(i) => Some(ProjectNavItem::Project(i)),
                ProjectEntry::AllView => Some(ProjectNavItem::AllView),
                ProjectEntry::TodayView => Some(ProjectNavItem::TodayView),
                ProjectEntry::UpcomingView => Some(ProjectNavItem::UpcomingView),
                ProjectEntry::GithubPrsView(owner) => Some(ProjectNavItem::GithubPrsView(owner)),
                ProjectEntry::JiraCardsView => Some(ProjectNavItem::JiraCardsView),
                ProjectEntry::AgendaView => Some(ProjectNavItem::AgendaView),
                _ => None,
            })
            .collect()
    }

    pub fn toggle_folder_collapse(&mut self) {
        let fid = if let Some(fi) = self.folder_cursor {
            self.folders.get(fi).map(|f| f.id.clone())
        } else {
            self.projects
                .get(self.selected_project)
                .and_then(|p| p.folder_id.clone())
        };
        let Some(fid) = fid else {
            return;
        };
        if self.collapsed_folders.contains(&fid) {
            self.collapsed_folders.remove(&fid);
        } else {
            self.collapsed_folders.insert(fid.clone());
        }
        if let Some(fi) = self.folders.iter().position(|f| f.id == fid) {
            self.folder_cursor = Some(fi);
        }
    }

    fn apply_priority(&mut self, new_priority: u8) {
        let (task_id, before, old_priority) = {
            let Some(task) = self.selected_task() else {
                return;
            };
            (task.id.clone(), task.clone(), task.priority)
        };

        if old_priority == new_priority {
            return;
        }

        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == task_id) {
            t.priority = new_priority;
        }

        let uuid = new_uuid();
        self.temp_id_pending.insert(
            uuid.clone(),
            OptimisticOp::TaskUpdated {
                task_id: task_id.clone(),
                before,
            },
        );
        self.pending_commands.push(SyncCommand {
            r#type: "item_update".to_string(),
            temp_id: None,
            uuid,
            args: serde_json::json!({ "id": task_id, "priority": new_priority }),
        });
        self.flush_commands();
    }

    fn start_comment_input(&mut self) {
        self.comment_input = true;
        self.show_input = true;
        self.input_buffer.clear();
        if let InputMode::Vim(_) = self.input_mode {
            self.input_mode = InputMode::Vim(VimState::Insert);
        }
    }

    fn start_field_edit(&mut self) {
        let Some(task) = self.selected_task() else {
            return;
        };

        if self.detail_field == 1 {
            self.priority_selection = task.priority;
            self.show_priority_picker = true;
            return;
        }

        let prefill = match self.detail_field {
            0 => task.content.clone(),
            2 => task
                .due
                .as_ref()
                .and_then(|d| d.string.clone())
                .unwrap_or_default(),
            3 => task.description.clone(),
            _ => return,
        };
        self.editing_field = true;
        self.show_input = true;
        self.input_buffer = prefill;
        if let InputMode::Vim(_) = self.input_mode {
            self.input_mode = InputMode::Vim(VimState::Insert);
        }
    }

    fn move_detail_field(&mut self, delta: i32) {
        let max_fields = 4;
        let current = self.detail_field as i32;
        self.detail_field = (current + delta).rem_euclid(max_fields) as usize;
    }

    fn toggle_collapse(&mut self) {
        let visible = self.visible_tasks();
        let Some(task) = visible.get(self.selected_task) else {
            return;
        };
        let task_id = task.id.clone();
        let parent_id = task.parent_id.clone();

        if self.has_children(&task_id) {
            if self.collapsed.contains(&task_id) {
                self.collapsed.remove(&task_id);
            } else {
                self.collapsed.insert(task_id);
            }
            return;
        }

        if let Some(pid) = parent_id {
            self.collapsed.insert(pid.clone());
            if let Some(pos) = self.visible_tasks().iter().position(|t| t.id == pid) {
                self.selected_task = pos;
            }
        }
    }

    fn close_all_folds(&mut self) {
        let parent_ids: HashSet<String> = self
            .tasks
            .iter()
            .filter_map(|t| t.parent_id.clone())
            .collect();
        for task in &self.tasks {
            if parent_ids.contains(&task.id) {
                self.collapsed.insert(task.id.clone());
            }
        }
    }

    pub fn toggle_input_mode(&mut self) {
        self.input_mode = match self.input_mode {
            InputMode::Vim(_) => InputMode::Standard,
            InputMode::Standard => InputMode::Vim(VimState::Normal),
        };
        info!(mode = self.input_mode.label(), "input mode toggled");
    }

    fn set_error(&mut self, err: &anyhow::Error, context: &str) {
        let app_err = AppError::from_api(err, context);
        error!(context, error = %app_err.message, "app error");
        self.error = Some(app_err);
    }

    fn handle_error_dismiss(&mut self) {
        if let Some(err) = self.error.take() {
            if !err.recoverable {
                info!("unrecoverable error dismissed, exiting");
                self.running = false;
            } else {
                debug!("error dismissed, continuing");
            }
        }
    }

    /// True when any virtual view (Today / Upcoming / Pull Requests) is
    /// currently active and the underlying `selected_project` should be
    /// ignored for display, navigation, or add-task defaults.
    pub fn on_virtual_view(&self) -> bool {
        self.all_view_active
            || self.today_view_active
            || self.upcoming_view_active
            || self.active_pr_org.is_some()
            || self.jira_cards_view_active
            || self.agenda_view_active
    }

    /// Any Pull Requests view (for some specific org) is currently active.
    pub fn is_pr_view_active(&self) -> bool {
        self.active_pr_org.is_some()
    }

    /// PRs filtered to the currently active org, in the order returned by gh.
    pub fn active_org_prs(&self) -> Vec<&PullRequest> {
        let Some(org) = &self.active_pr_org else {
            return Vec::new();
        };
        self.github_prs
            .iter()
            .filter(|pr| pr.repo_full_name.starts_with(&format!("{org}/")))
            .collect()
    }

    /// Unique owners sorted alphabetically, with their open-PR counts. Empty
    /// owners (malformed `repository.nameWithOwner`) and any owners listed in
    /// `hidden_pr_orgs` are dropped — those simply don't appear in the
    /// sidebar.
    pub fn pr_owners_with_counts(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for pr in &self.github_prs {
            if let Some((owner, _)) = pr.repo_full_name.split_once('/') {
                if self.hidden_pr_orgs.contains(owner) {
                    continue;
                }
                *counts.entry(owner.to_string()).or_insert(0) += 1;
            }
        }
        let mut sorted: Vec<_> = counts.into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        sorted
    }

    pub fn selected_project_name(&self) -> std::borrow::Cow<'_, str> {
        if self.all_view_active {
            return "All".into();
        }
        if self.today_view_active {
            return "Today".into();
        }
        if self.upcoming_view_active {
            return "Upcoming".into();
        }
        if let Some(owner) = &self.active_pr_org {
            return format!("Pull Requests · {owner}").into();
        }
        if self.jira_cards_view_active {
            return "Jira Cards".into();
        }
        if self.agenda_view_active {
            return "Agenda".into();
        }
        self.projects
            .get(self.selected_project)
            .map(|p| std::borrow::Cow::Borrowed(p.name.as_str()))
            .unwrap_or(std::borrow::Cow::Borrowed("Tasks"))
    }

    pub fn selected_task(&self) -> Option<&Task> {
        // When the detail pane is open, honor the task ID it was opened
        // with. This is what makes "Enter on a Todoist task in the All
        // view" work correctly, since the All view doesn't populate
        // `visible_tasks()` with that task.
        if let Some(id) = &self.detail_task_id {
            return self.tasks.iter().find(|t| t.id == *id);
        }
        let visible = self.visible_tasks();
        visible.get(self.selected_task).copied()
    }

    pub fn overview_stats(&self) -> OverviewStats {
        let today = crate::ui::dates::today_str();
        let week_end = crate::ui::dates::offset_days_str(7);

        let mut due_today = 0u32;
        let mut due_week = 0u32;
        let mut overdue = 0u32;
        let mut by_priority = [0u32; 5];

        for task in &self.tasks {
            if task.is_deleted {
                continue;
            }
            if !task.checked {
                let p = task.priority as usize;
                if p < by_priority.len() {
                    by_priority[p] += 1;
                }
            }
            if let Some(due) = &task.due {
                if due.date == today && !task.checked {
                    due_today += 1;
                }
                if due.date < today && !task.checked {
                    overdue += 1;
                }
                if due.date >= today && due.date <= week_end {
                    due_week += 1;
                }
            }
        }

        OverviewStats {
            due_today,
            due_week,
            overdue,
            by_priority,
        }
    }

    pub fn has_children(&self, task_id: &str) -> bool {
        self.tasks
            .iter()
            .any(|t| t.parent_id.as_deref() == Some(task_id))
    }

    pub fn is_collapsed(&self, task_id: &str) -> bool {
        self.collapsed.contains(task_id)
    }

    pub fn visible_tasks(&self) -> Vec<&Task> {
        if self.today_view_active {
            let today = crate::ui::dates::today_str();
            let current_hour = crate::ui::dates::current_local_hour();
            let is_weekend = crate::ui::dates::is_weekend_local();
            let mut tasks: Vec<&Task> = self
                .tasks
                .iter()
                .filter(|t| {
                    if t.is_deleted || t.checked || t.parent_id.is_some() {
                        return false;
                    }
                    if self.pending_close_recurring.contains(&t.id) {
                        return false;
                    }
                    if crate::ui::dates::evening_task_hidden(&t.labels, current_hour) {
                        return false;
                    }
                    if crate::ui::dates::work_weekend_hidden(&t.labels, is_weekend) {
                        return false;
                    }
                    let is_today_or_overdue = t
                        .due
                        .as_ref()
                        .is_some_and(|d| d.date.as_str() <= today.as_str());
                    if !is_today_or_overdue {
                        return false;
                    }
                    match &t.responsible_uid {
                        None => true,
                        Some(uid) => self.current_user_id.as_deref() == Some(uid.as_str()),
                    }
                })
                .collect();
            // Overdue tasks first (ascending by date), then today tasks (by child_order).
            tasks.sort_by(|a, b| {
                let a_date = a.due.as_ref().map(|d| d.date.as_str()).unwrap_or("");
                let b_date = b.due.as_ref().map(|d| d.date.as_str()).unwrap_or("");
                a_date.cmp(b_date).then(a.child_order.cmp(&b.child_order))
            });
            if self.overdue_section_collapsed {
                tasks.retain(|t| {
                    t.due
                        .as_ref()
                        .is_some_and(|d| d.date.as_str() == today.as_str())
                });
            }
            return tasks;
        }

        if self.upcoming_view_active {
            // Upcoming is unbounded forward: every active parent task that
            // has any due date (overdue, today, or future). Tasks without a
            // due date are excluded — Upcoming is specifically the scheduled
            // view.
            let mut tasks: Vec<&Task> = self
                .tasks
                .iter()
                .filter(|t| {
                    if t.is_deleted || t.checked || t.parent_id.is_some() {
                        return false;
                    }
                    if self.pending_close_recurring.contains(&t.id) {
                        return false;
                    }
                    if t.due.is_none() {
                        return false;
                    }
                    match &t.responsible_uid {
                        None => true,
                        Some(uid) => self.current_user_id.as_deref() == Some(uid.as_str()),
                    }
                })
                .collect();
            tasks.sort_by(|a, b| {
                let a_date = a.due.as_ref().map(|d| d.date.as_str()).unwrap_or("");
                let b_date = b.due.as_ref().map(|d| d.date.as_str()).unwrap_or("");
                a_date
                    .cmp(b_date)
                    .then(a.project_id.cmp(&b.project_id))
                    .then(a.child_order.cmp(&b.child_order))
            });
            return tasks;
        }

        let today = crate::ui::dates::today_str();
        let week_end = crate::ui::dates::offset_days_str(7);

        let current_project_id = self
            .projects
            .get(self.selected_project)
            .map(|p| p.id.as_str());

        let mut top_level: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| {
                if t.is_deleted || t.parent_id.is_some() {
                    return false;
                }
                if self.pending_close_recurring.contains(&t.id) {
                    return false;
                }
                if let Some(dock) = self.dock_filter {
                    return match dock {
                        DockItem::DueOverdue => {
                            t.due.as_ref().is_some_and(|d| d.date < today) && !t.checked
                        }
                        DockItem::DueToday => t.due.as_ref().is_some_and(|d| d.date == today),
                        DockItem::DueWeek => t
                            .due
                            .as_ref()
                            .is_some_and(|d| d.date >= today && d.date <= week_end),
                        DockItem::Priority(p) => t.priority == p && !t.checked,
                    };
                }
                Some(t.project_id.as_str()) == current_project_id
                    && match self.task_filter {
                        TaskFilter::Active => !t.checked,
                        TaskFilter::Done => t.checked || self.has_completed_descendant(&t.id),
                        TaskFilter::Both => true,
                    }
            })
            .collect();

        match self.sort_mode {
            SortMode::Default => {
                if self.dock_filter.is_none() {
                    let so = |sid: Option<&str>| {
                        sid.and_then(|id| self.sections.iter().find(|s| s.id == id))
                            .and_then(|s| s.section_order)
                            .unwrap_or(i32::MIN)
                    };
                    top_level.sort_by(|a, b| {
                        so(a.section_id.as_deref())
                            .cmp(&so(b.section_id.as_deref()))
                            .then(a.child_order.cmp(&b.child_order))
                    });
                } else {
                    top_level.sort_by_key(|t| t.child_order);
                }
            }
            SortMode::Priority => top_level.sort_by(|a, b| b.priority.cmp(&a.priority)),
            SortMode::DueDate => top_level.sort_by(|a, b| {
                let a_due = a.due.as_ref().map(|d| d.date.as_str()).unwrap_or("9999");
                let b_due = b.due.as_ref().map(|d| d.date.as_str()).unwrap_or("9999");
                a_due.cmp(b_due)
            }),
            SortMode::Created => top_level.sort_by(|a, b| {
                let a_at = a.added_at.as_deref().unwrap_or("");
                let b_at = b.added_at.as_deref().unwrap_or("");
                b_at.cmp(a_at)
            }),
        }

        if self.dock_filter.is_some() {
            return top_level;
        }

        let mut result = Vec::with_capacity(self.tasks.len());
        for task in top_level {
            result.push(task);
            if !self.collapsed.contains(&task.id) {
                if self.task_filter == TaskFilter::Done {
                    self.collect_done_children(&task.id, &mut result);
                } else {
                    self.collect_visible_children(&task.id, &mut result);
                }
            }
        }

        if matches!(self.task_filter, TaskFilter::Done | TaskFilter::Both)
            && let Some(pid) = self
                .projects
                .get(self.selected_project)
                .map(|p| p.id.clone())
        {
            self.append_cached_completed(&pid, &mut result);
        }

        result
    }

    fn collect_done_children<'a>(&'a self, parent_id: &str, result: &mut Vec<&'a Task>) {
        let mut children: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| {
                !t.is_deleted
                    && t.parent_id.as_deref() == Some(parent_id)
                    && (t.checked || self.has_completed_descendant(&t.id))
            })
            .collect();
        children.sort_by_key(|t| t.child_order);
        for child in children {
            result.push(child);
            if !self.collapsed.contains(&child.id) {
                self.collect_done_children(&child.id, result);
            }
        }
    }

    fn has_completed_descendant(&self, task_id: &str) -> bool {
        self.tasks
            .iter()
            .any(|t| !t.is_deleted && t.checked && self.is_descendant_of(&t.id, task_id))
    }

    fn is_descendant_of(&self, task_id: &str, ancestor_id: &str) -> bool {
        let mut current = task_id.to_string();
        loop {
            let parent = self
                .tasks
                .iter()
                .find(|t| t.id == current)
                .and_then(|t| t.parent_id.clone());
            match parent {
                None => return false,
                Some(pid) if pid == ancestor_id => return true,
                Some(pid) => current = pid,
            }
        }
    }

    pub fn is_context_task(&self, task: &Task) -> bool {
        if !(self.task_filter == TaskFilter::Done && self.dock_filter.is_none() && !task.checked) {
            return false;
        }
        if self.has_completed_descendant(&task.id) {
            return true;
        }
        if let Some(pid) = self
            .projects
            .get(self.selected_project)
            .map(|p| p.id.as_str())
            && let Some(cached) = self.completed_cache.get(pid)
        {
            return cached
                .iter()
                .any(|t| self.is_cached_descendant_of(t, &task.id, cached));
        }
        false
    }

    fn collect_visible_children<'a>(&'a self, parent_id: &str, result: &mut Vec<&'a Task>) {
        let mut children: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| !t.is_deleted && t.parent_id.as_deref() == Some(parent_id))
            .collect();
        children.sort_by_key(|t| t.child_order);

        for child in children {
            result.push(child);
            if !self.collapsed.contains(&child.id) {
                self.collect_visible_children(&child.id, result);
            }
        }
    }

    pub fn task_depth(&self, task: &Task) -> usize {
        let mut depth = 0;
        let mut current_parent = task.parent_id.as_deref();
        while let Some(pid) = current_parent {
            depth += 1;
            current_parent = self
                .tasks
                .iter()
                .find(|t| t.id == pid)
                .and_then(|t| t.parent_id.as_deref());
        }
        depth
    }

    /// Appends cached completed tasks for `project_id` into `result`, inserting active parent
    /// tasks as dimmed context rows where needed. Works for both Done and Both filters:
    /// in Both mode, active parents are already in `result` so they're skipped via `already_shown`.
    fn append_cached_completed<'a>(&'a self, project_id: &str, result: &mut Vec<&'a Task>) {
        let cached = match self.completed_cache.get(project_id) {
            Some(c) if !c.is_empty() => c,
            _ => return,
        };

        let already_shown: HashSet<&str> = result.iter().map(|t| t.id.as_str()).collect();
        let cached_ids: HashSet<&str> = cached.iter().map(|t| t.id.as_str()).collect();

        // Roots: cached tasks whose parent is absent from the cached set.
        let mut roots: Vec<&Task> = cached
            .iter()
            .filter(|t| {
                t.parent_id
                    .as_ref()
                    .is_none_or(|pid| !cached_ids.contains(pid.as_str()))
            })
            .collect();
        roots.sort_by_key(|t| t.child_order);

        for root in roots {
            // If this cached root has an active parent not yet shown, add it as a context row.
            if let Some(ref pid) = root.parent_id
                && !already_shown.contains(pid.as_str())
                && let Some(parent) = self.tasks.iter().find(|t| t.id == *pid && !t.is_deleted)
            {
                result.push(parent);
            }
            result.push(root);
            Self::collect_cached_children(&root.id, cached, &mut *result);
        }
    }

    fn collect_cached_children<'a>(
        parent_id: &str,
        cached: &'a [Task],
        result: &mut Vec<&'a Task>,
    ) {
        let mut children: Vec<&Task> = cached
            .iter()
            .filter(|t| t.parent_id.as_deref() == Some(parent_id))
            .collect();
        children.sort_by_key(|t| t.child_order);
        for child in children {
            result.push(child);
            Self::collect_cached_children(&child.id, cached, result);
        }
    }

    /// Returns true if `task` is a descendant of `ancestor_id` within `cached`.
    fn is_cached_descendant_of(&self, task: &Task, ancestor_id: &str, cached: &[Task]) -> bool {
        let mut current_parent = task.parent_id.as_deref();
        while let Some(pid) = current_parent {
            if pid == ancestor_id {
                return true;
            }
            current_parent = cached
                .iter()
                .find(|t| t.id == pid)
                .and_then(|t| t.parent_id.as_deref());
        }
        false
    }
}

fn collect_project_subtree(parent_id: Option<&str>, all: &[Project], out: &mut Vec<Project>) {
    let mut children: Vec<&Project> = all
        .iter()
        .filter(|p| p.parent_id.as_deref() == parent_id)
        .collect();
    children.sort_by(|a, b| {
        let a_pin = a.is_inbox() || a.is_favorite;
        let b_pin = b.is_inbox() || b.is_favorite;
        b_pin.cmp(&a_pin).then(a.child_order.cmp(&b.child_order))
    });
    for child in children {
        out.push(child.clone());
        collect_project_subtree(Some(&child.id), all, out);
    }
}

/// Run two `gh search prs` queries in parallel — one for PRs I authored, one
/// for PRs assigned to me — merge the results, dedup by URL, then make one
/// GraphQL call to fold in CI rollup state. GitHub search's flag-based
/// qualifiers are ANDed, so capturing both relationships needs two calls.
/// Errors from either search query are surfaced; partial success (one query
/// fails, one succeeds) is treated as a failure so the user sees the problem
/// rather than a silently truncated list. The check-status call is
/// best-effort — if it fails the PRs are still returned with
/// `check_status: None`.
async fn fetch_github_prs() -> Result<Vec<PullRequest>> {
    let (authored, assigned) = tokio::try_join!(
        fetch_github_prs_with_flag("--author"),
        fetch_github_prs_with_flag("--assignee"),
    )?;

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut merged: Vec<PullRequest> = Vec::with_capacity(authored.len() + assigned.len());
    for pr in authored.into_iter().chain(assigned.into_iter()) {
        if seen.insert(pr.url.clone()) {
            merged.push(pr);
        }
    }

    match fetch_pr_check_statuses(&merged).await {
        Ok(statuses) => {
            for pr in &mut merged {
                if let Some(s) = statuses.get(&pr.node_id) {
                    pr.check_status = Some(*s);
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "pr check-status fetch failed; rendering without it");
        }
    }

    Ok(merged)
}

/// Batch-fetch `statusCheckRollup.state` for a set of PRs in a single
/// GraphQL request using aliased `node(id:…)` lookups. Returns a map from
/// node ID to `CheckStatus`; PRs without rollup state (no checks
/// configured, or no commits yet) are simply omitted. One API call
/// regardless of PR count.
async fn fetch_pr_check_statuses(
    prs: &[PullRequest],
) -> Result<std::collections::HashMap<String, CheckStatus>> {
    use tokio::process::Command;

    let ids: Vec<&str> = prs
        .iter()
        .map(|pr| pr.node_id.as_str())
        .filter(|id| !id.is_empty())
        .collect();
    if ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    // Build `n0: node(id:"..."){...F} n1: node(id:"..."){...F} ...` so we
    // can correlate each rollup back to its PR by alias index.
    let mut selections = String::new();
    for (i, id) in ids.iter().enumerate() {
        // GitHub node IDs are safe ASCII; still escape quotes defensively.
        let esc = id.replace('\\', "\\\\").replace('"', "\\\"");
        selections.push_str(&format!("n{i}: node(id: \"{esc}\") {{ ...S }} "));
    }
    let query = format!(
        "query {{ {selections} }} \
         fragment S on PullRequest {{ \
           commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state }} }} }} }} \
         }}"
    );

    let output = Command::new("gh")
        .args(["api", "graphql", "-f"])
        .arg(format!("query={query}"))
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to invoke gh api graphql: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow::anyhow!(
            "gh api graphql exited with status {}: {}",
            output.status,
            stderr
        ));
    }

    let raw: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!("graphql JSON parse error: {e}"))?;
    let data = &raw["data"];

    let mut out = std::collections::HashMap::new();
    for (i, id) in ids.iter().enumerate() {
        let state = data[format!("n{i}")]["commits"]["nodes"][0]["commit"]["statusCheckRollup"]
            ["state"]
            .as_str();
        if let Some(status) = state.and_then(parse_check_state) {
            out.insert((*id).to_string(), status);
        }
    }
    Ok(out)
}

/// Map GitHub's `StatusState` enum string to our collapsed `CheckStatus`.
/// `EXPECTED` (required check not yet reported) folds into `Pending` — to
/// the user they're both "waiting."
fn parse_check_state(s: &str) -> Option<CheckStatus> {
    match s {
        "SUCCESS" => Some(CheckStatus::Success),
        "FAILURE" | "ERROR" => Some(CheckStatus::Failure),
        "PENDING" | "EXPECTED" => Some(CheckStatus::Pending),
        _ => None,
    }
}

/// Shell out to `gh search prs <flag> @me --state open --json ...` and parse
/// the JSON array into `PullRequest` records. `flag` is `--author` or
/// `--assignee`; the rest of the query is identical.
async fn fetch_github_prs_with_flag(flag: &str) -> Result<Vec<PullRequest>> {
    use tokio::process::Command;

    let output = Command::new("gh")
        .args([
            "search",
            "prs",
            flag,
            "@me",
            "--state",
            "open",
            // Exclude PRs in archived repos — they can't be merged or closed,
            // so they're permanent noise in this view.
            "--archived=false",
            "--limit",
            "100",
            "--json",
            "id,number,title,url,repository,author,updatedAt,isDraft",
        ])
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to invoke gh: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("gh exited with status {}", output.status)
        } else {
            stderr
        };
        return Err(anyhow::anyhow!("{msg}"));
    }

    let raw: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!("gh JSON parse error: {e}"))?;

    let arr = raw
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("expected JSON array from gh"))?;

    let mut prs = Vec::with_capacity(arr.len());
    for item in arr {
        prs.push(PullRequest {
            number: item["number"].as_u64().unwrap_or(0),
            title: item["title"].as_str().unwrap_or("").to_string(),
            url: item["url"].as_str().unwrap_or("").to_string(),
            repo_full_name: item["repository"]["nameWithOwner"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            author_login: item["author"]["login"].as_str().unwrap_or("").to_string(),
            updated_at: item["updatedAt"].as_str().unwrap_or("").to_string(),
            is_draft: item["isDraft"].as_bool().unwrap_or(false),
            node_id: item["id"].as_str().unwrap_or("").to_string(),
            check_status: None,
        });
    }
    Ok(prs)
}

/// Shell out to `acli jira workitem search` and parse the JSON output into
/// `JiraCard` records. Errors (unauthenticated, missing acli, network) surface
/// as the error message.
async fn fetch_jira_cards() -> Result<Vec<JiraCard>> {
    use tokio::process::Command;

    let output = Command::new("acli")
        .args([
            "jira",
            "workitem",
            "search",
            "--jql",
            "assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC",
            "--fields",
            "key,summary,status,priority,issuetype",
            "--limit",
            "100",
            "--json",
        ])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to invoke acli: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("acli exited with status {}", output.status)
        } else {
            stderr
        };
        return Err(anyhow::anyhow!("{msg}"));
    }

    // The JSON shape is an array of objects with the fields we requested.
    // Nested fields (status.name, priority.name, issuetype.name, assignee) are
    // objects; we extract the name strings.
    let raw: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!("acli JSON parse error: {e}"))?;

    let arr = raw
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("expected JSON array from acli"))?;

    let pick_name = |v: &serde_json::Value| -> String {
        // Fields come back as either `"In Progress"` (string) or
        // `{ "name": "In Progress", ... }` depending on acli version; handle
        // both.
        if let Some(s) = v.as_str() {
            return s.to_string();
        }
        v.get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_string()
    };

    let mut cards = Vec::with_capacity(arr.len());
    for item in arr {
        // Keys may live at the top level or under `fields`.
        let fields = item.get("fields").unwrap_or(item);
        let key = item
            .get("key")
            .and_then(|k| k.as_str())
            .unwrap_or("")
            .to_string();
        let project_key = key.split_once('-').map(|(p, _)| p.to_string()).unwrap_or_default();
        cards.push(JiraCard {
            key,
            project_key,
            summary: fields
                .get("summary")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            status: pick_name(fields.get("status").unwrap_or(&serde_json::Value::Null)),
            priority: pick_name(fields.get("priority").unwrap_or(&serde_json::Value::Null)),
            issue_type: pick_name(fields.get("issuetype").unwrap_or(&serde_json::Value::Null)),
        });
    }
    Ok(cards)
}

/// Fetch today's events blended across every subscribed, visible Google
/// Calendar. Enumerates calendars via `gws calendar calendarList list`,
/// filters to entries where `selected == true && hidden != true`, then
/// fetches events from each one concurrently and merges the results sorted
/// by start time. Returns an error only if enumeration fails or every
/// per-calendar fetch fails; individual calendar failures are logged and
/// dropped so one broken calendar doesn't blank the whole agenda.
/// "Today" is computed in the user's local timezone.
async fn fetch_agenda_events() -> Result<Vec<CalendarEvent>> {
    use chrono::{Local, TimeZone};

    let now = Local::now();
    let today = now.date_naive();
    let tomorrow = today + chrono::Duration::days(1);
    let start_local = Local
        .from_local_datetime(&today.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .ok_or_else(|| anyhow::anyhow!("ambiguous local midnight today"))?;
    let end_local = Local
        .from_local_datetime(&tomorrow.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .ok_or_else(|| anyhow::anyhow!("ambiguous local midnight tomorrow"))?;
    let time_min = start_local.to_rfc3339();
    let time_max = end_local.to_rfc3339();

    let calendars = fetch_subscribed_calendars().await?;
    if calendars.is_empty() {
        return Ok(Vec::new());
    }

    let fetches = calendars.into_iter().map(|(id, name)| {
        let time_min = time_min.clone();
        let time_max = time_max.clone();
        async move {
            let res = fetch_events_for_calendar(&id, &name, &time_min, &time_max).await;
            (name, res)
        }
    });
    let results = futures_util::future::join_all(fetches).await;

    let mut events = Vec::new();
    let mut first_error: Option<anyhow::Error> = None;
    let mut success_count = 0usize;
    for (name, res) in results {
        match res {
            Ok(mut batch) => {
                success_count += 1;
                events.append(&mut batch);
            }
            Err(e) => {
                tracing::warn!(calendar = %name, error = %e, "agenda fetch failed for calendar");
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }
    }

    if success_count == 0
        && let Some(e) = first_error
    {
        return Err(e);
    }

    events.sort_by(|a, b| a.start.cmp(&b.start));
    Ok(events)
}

/// Enumerate the user's subscribed, visible calendars. Returns pairs of
/// `(calendar id, display name)` — the id is used for the events-list
/// request, the name is surfaced in the agenda row so blended events can
/// be attributed back to their source calendar.
async fn fetch_subscribed_calendars() -> Result<Vec<(String, String)>> {
    use tokio::process::Command;

    let output = Command::new("gws")
        .args(["calendar", "calendarList", "list"])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to invoke gws: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("gws calendarList exited with status {}", output.status)
        } else {
            stderr
        };
        return Err(anyhow::anyhow!("{msg}"));
    }

    let raw: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!("gws calendarList JSON parse error: {e}"))?;
    let empty = vec![];
    let items = raw
        .get("items")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);

    let mut calendars = Vec::with_capacity(items.len());
    for item in items {
        // Google omits both flags when they're false, so treat absent as
        // the default: not-selected and not-hidden.
        let selected = item.get("selected").and_then(|v| v.as_bool()) == Some(true);
        let hidden = item.get("hidden").and_then(|v| v.as_bool()) == Some(true);
        if !selected || hidden {
            continue;
        }
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        // `summaryOverride` is the user's local rename in Calendar web;
        // prefer it over the calendar owner's `summary`.
        let name = item
            .get("summaryOverride")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("summary").and_then(|v| v.as_str()))
            .unwrap_or(id)
            .to_string();
        calendars.push((id.to_string(), name));
    }
    Ok(calendars)
}

/// Fetch today's events for a single calendar. `calendar_name` is stamped
/// onto each returned `CalendarEvent` so merged results retain their
/// origin.
async fn fetch_events_for_calendar(
    calendar_id: &str,
    calendar_name: &str,
    time_min: &str,
    time_max: &str,
) -> Result<Vec<CalendarEvent>> {
    use tokio::process::Command;

    let params = serde_json::json!({
        "calendarId": calendar_id,
        "timeMin": time_min,
        "timeMax": time_max,
        "singleEvents": true,
        "orderBy": "startTime",
        "maxResults": 100,
    });

    let output = Command::new("gws")
        .args(["calendar", "events", "list", "--params"])
        .arg(params.to_string())
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to invoke gws: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("gws exited with status {}", output.status)
        } else {
            stderr
        };
        return Err(anyhow::anyhow!("{msg}"));
    }

    let raw: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!("gws JSON parse error: {e}"))?;
    let empty = vec![];
    let items = raw
        .get("items")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);

    let mut events = Vec::with_capacity(items.len());
    for item in items {
        // Skip cancelled instances of recurring events — the API returns
        // them as ghosts with status=cancelled.
        if item.get("status").and_then(|s| s.as_str()) == Some("cancelled") {
            continue;
        }
        let (start, all_day) = extract_time(item.get("start"));
        let (end, _) = extract_time(item.get("end"));
        events.push(CalendarEvent {
            summary: item
                .get("summary")
                .and_then(|s| s.as_str())
                .unwrap_or("(no title)")
                .to_string(),
            start,
            end,
            all_day,
            location: item
                .get("location")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            html_link: item
                .get("htmlLink")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            calendar_name: calendar_name.to_string(),
        });
    }
    Ok(events)
}

/// Pull a start/end timestamp out of a Google Calendar event's
/// `start` / `end` object. Returns the timestamp string plus a flag
/// indicating whether this was an all-day event (`date`) versus a timed
/// event (`dateTime`).
fn extract_time(obj: Option<&serde_json::Value>) -> (String, bool) {
    let Some(obj) = obj else {
        return (String::new(), false);
    };
    if let Some(dt) = obj.get("dateTime").and_then(|v| v.as_str()) {
        return (dt.to_string(), false);
    }
    if let Some(d) = obj.get("date").and_then(|v| v.as_str()) {
        return (d.to_string(), true);
    }
    (String::new(), false)
}

async fn run_websocket(url: String, tx: mpsc::Sender<BgResult>) {
    use futures_util::StreamExt;
    use tokio_tungstenite::connect_async_tls_with_config;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let mut backoff_secs = 5u64;
    loop {
        let connect_result = async {
            let mut req = url.as_str().into_client_request()?;
            req.headers_mut()
                .insert("Origin", "https://app.todoist.com".parse()?);
            connect_async_tls_with_config(req, None, false, None).await
        }
        .await;

        match connect_result {
            Ok((ws_stream, _)) => {
                backoff_secs = 5;
                let _ = tx.send(BgResult::WebSocketConnected).await;

                let (_, mut read) = ws_stream.split();
                while read.next().await.is_some() {
                    let _ = tx.send(BgResult::WebSocketEvent).await;
                }
                let _ = tx.send(BgResult::WebSocketDisconnected).await;
                // Clean disconnect — reconnect quickly without growing backoff.
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            Err(e) => {
                debug!(error = %e, "websocket connection failed, retrying");
            }
        }
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(60);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn test_app() -> App {
        let client = TodoistClient::new("test_token").expect("client");
        let mut app = App::new(client, false, true);
        // Isolate tests from on-disk config (e.g. hidden_pr_orgs in ui_settings.json).
        app.hidden_pr_orgs.clear();
        app.star_count = 0;
        app.star_date = crate::ui::dates::today_str();
        app.tomato_count = 0;
        app.tomato_date = crate::ui::dates::today_str();
        app.pomodoro_started_at = None;
        app
    }

    /// Dispatch a key event through the same routing the main loop uses.
    fn press(app: &mut App, code: KeyCode) {
        let key = KeyEvent::new(code, KeyModifiers::NONE);
        match keys::handle_key(app, key) {
            KeyAction::StartInput => app.start_input(),
            KeyAction::SubmitInput => app.submit_input(),
            KeyAction::SubmitForm => app.submit_task_form(),
            KeyAction::FormFieldUp => app.form_field_up(),
            KeyAction::FormFieldDown => app.form_field_down(),
            KeyAction::FormEditField => app.form_edit_field(),
            KeyAction::FormEscNormal => app.submit_input(),
            KeyAction::CancelInput => app.cancel_input(),
            KeyAction::CompleteTaskById(id) => app.complete_task_by_id(id),
            KeyAction::OpenDetailById(id) => app.open_detail_for(id),
            KeyAction::TogglePomodoro => app.toggle_pomodoro(),
            KeyAction::Consumed | KeyAction::None => {}
            _ => panic!("unexpected key action in test"),
        }
    }

    /// Regression: editing the Due date field inside the add-task modal should
    /// persist the typed value into `form.due_string` so that
    /// `submit_task_form` sends it as `due_string` in the `item_add` command.
    #[test]
    fn add_modal_updates_due_date() {
        let mut app = test_app();

        // Open the add modal (equivalent to pressing `a` on the task list).
        app.active_pane = Pane::Tasks;
        press(&mut app, KeyCode::Char('a'));
        let form = app.task_form.as_ref().expect("form opens");
        assert_eq!(form.active_field, 0);
        assert!(form.editing, "new form starts editing content");

        // Type "Buy milk" and Enter to commit the content field.
        for c in "Buy milk".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);

        let form = app.task_form.as_ref().unwrap();
        assert_eq!(form.content, "Buy milk");
        assert!(!form.editing);

        // j, j → Due date (field 2).
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.task_form.as_ref().unwrap().active_field, 2);

        // Enter edit mode on the Due date field.
        press(&mut app, KeyCode::Enter);
        assert!(
            app.task_form.as_ref().unwrap().editing,
            "Enter on Due date should enter editing mode"
        );

        // Type "tomorrow" and Enter.
        for c in "tomorrow".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);

        let form = app.task_form.as_ref().unwrap();
        assert_eq!(
            form.due_string, "tomorrow",
            "Due date should be saved into the form after editing"
        );
        assert!(!form.editing);
    }

    /// The Todoist Sync API needs a `due` object; the previous code sent
    /// `due_string` at the top level, which the server silently ignored — the
    /// task was created but without the date the user typed.
    #[test]
    fn item_add_args_use_due_object_for_sync_api() {
        let mut form = TaskForm::new("project_1".to_string());
        form.content = "Buy milk".to_string();
        form.due_string = "tomorrow".to_string();

        let args = build_item_add_args(&form, "project_1");

        assert_eq!(args["content"], "Buy milk");
        assert_eq!(args["project_id"], "project_1");
        assert_eq!(
            args["due"],
            serde_json::json!({ "string": "tomorrow" }),
            "Sync API expects a `due` object, not top-level `due_string`"
        );
        assert!(
            args.get("due_string").is_none(),
            "top-level `due_string` is REST-API-only and must not be sent"
        );
    }

    #[test]
    fn item_add_args_omits_due_when_empty() {
        let mut form = TaskForm::new("project_1".to_string());
        form.content = "Buy milk".to_string();

        let args = build_item_add_args(&form, "project_1");

        assert!(args.get("due").is_none());
        assert!(args.get("due_string").is_none());
    }

    /// Adding a task from the Today view should pre-fill the Due date with
    /// "today" and target the Inbox.
    #[test]
    fn add_from_today_view_defaults_to_today() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "inbox_1".to_string(),
            name: "Inbox".to_string(),
            inbox_project: Some(true),
            ..Project::default()
        });
        app.projects.push(Project {
            id: "proj_2".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });
        app.selected_project = 1;
        app.activate_today_view();

        app.start_input();

        let form = app.task_form.as_ref().expect("form opens");
        assert_eq!(form.due_string, "today", "Due date pre-fills to today");
        assert_eq!(form.project_id, "inbox_1", "Today-view adds go to Inbox");
    }

    /// Adding from the Today section of the All view mirrors the Today
    /// view's defaults: due = "today", project = Inbox. Selecting a
    /// non-Task row (Agenda / PR / Jira) keeps the empty default. Covers
    /// the All-view branch in start_input — see today-view-add.spec.md.
    #[test]
    fn add_from_all_view_today_section_defaults_to_today() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "inbox_1".to_string(),
            name: "Inbox".to_string(),
            inbox_project: Some(true),
            ..Project::default()
        });
        app.projects.push(Project {
            id: "proj_2".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });
        app.selected_project = 1;
        app.tasks.push(Task {
            id: "t1".to_string(),
            content: "Plan week".to_string(),
            project_id: "proj_2".to_string(),
            due: Some(Due {
                date: crate::ui::dates::today_str(),
                is_recurring: false,
                ..Due::default()
            }),
            ..Task::default()
        });
        app.agenda_events.push(CalendarEvent {
            summary: "Standup".to_string(),
            start: "2026-04-18T09:00:00-04:00".to_string(),
            end: "2026-04-18T09:30:00-04:00".to_string(),
            all_day: false,
            location: String::new(),
            html_link: String::new(),
            calendar_name: "Work".to_string(),
        });

        // all_view_items layout is [AgendaEvent(0), Task(0)]. Land the
        // cursor on the Task row (the Today section).
        app.all_view_active = true;
        app.today_view_active = false;
        app.selected_all_item = 1;

        app.start_input();

        let form = app.task_form.as_ref().expect("form opens");
        assert_eq!(
            form.due_string, "today",
            "All-view Today-section adds default to today"
        );
        assert_eq!(
            form.project_id, "inbox_1",
            "All-view Today-section adds route to Inbox"
        );

        // Cursor on the Agenda row should NOT pre-fill — that's the Agenda
        // section, not Today.
        app.cancel_input();
        app.selected_all_item = 0;
        app.start_input();
        let form = app.task_form.as_ref().expect("form opens");
        assert_eq!(
            form.due_string, "",
            "All-view Agenda-section adds keep empty due"
        );
    }

    /// Adding from a regular project does NOT pre-fill the due date.
    #[test]
    fn add_from_project_keeps_due_empty() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "proj_1".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });
        app.selected_project = 0;
        // Today view NOT active.

        app.start_input();

        let form = app.task_form.as_ref().expect("form opens");
        assert_eq!(form.due_string, "");
        assert_eq!(form.project_id, "proj_1");
    }

    use ratatoist_core::api::models::Due;

    fn pending_cmd_types(app: &App) -> Vec<String> {
        app.pending_commands
            .iter()
            .map(|c| c.r#type.clone())
            .collect()
    }

    /// Completing a recurring task in the Today view hides it immediately
    /// (optimistic), so a double-tap of `x` doesn't advance the series twice.
    /// The underlying `checked` field stays false; the hide is via the
    /// pending_close_recurring set.
    #[test]
    fn completing_recurring_in_today_hides_optimistically() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "p1".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });
        app.tasks.push(Task {
            id: "rec".to_string(),
            content: "Brush teeth".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: crate::ui::dates::today_str(),
                is_recurring: true,
                string: Some("every day".to_string()),
                ..Due::default()
            }),
            ..Task::default()
        });
        app.tasks.push(Task {
            id: "other".to_string(),
            content: "Take pills".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: crate::ui::dates::today_str(),
                is_recurring: true,
                string: Some("every day".to_string()),
                ..Due::default()
            }),
            ..Task::default()
        });
        app.activate_today_view();
        app.active_pane = Pane::Tasks;
        app.selected_task = 0;
        assert_eq!(app.visible_tasks().len(), 2);

        app.enqueue_complete_selected();

        // Brush teeth should be gone from the visible list even though its
        // `checked` field is still false (server hasn't advanced the date).
        let visible_ids: Vec<&str> =
            app.visible_tasks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(visible_ids, vec!["other"], "Brush teeth hides immediately");
        assert!(!app.tasks.iter().find(|t| t.id == "rec").unwrap().checked);
        assert!(app.pending_close_recurring.contains("rec"));
        assert_eq!(pending_cmd_types(&app), vec!["item_close".to_string()]);
    }

    /// Completing a recurring task must not flip it to checked optimistically —
    /// the server will advance the due date and the task should remain on the
    /// list. Hiding it makes it look like the whole recurring series was
    /// deleted.
    #[test]
    fn complete_recurring_keeps_task_visible_and_sends_item_close() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "p1".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });
        app.tasks.push(Task {
            id: "t_rec".to_string(),
            content: "Stand-up".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: "2026-04-15".to_string(),
                is_recurring: true,
                string: Some("every weekday".to_string()),
                ..Due::default()
            }),
            ..Task::default()
        });
        app.active_pane = Pane::Tasks;
        app.selected_task = 0;

        app.enqueue_complete_selected();

        // Still unchecked — server will advance the due date.
        assert!(
            !app.tasks[0].checked,
            "recurring task must stay unchecked after optimistic complete"
        );
        assert_eq!(pending_cmd_types(&app), vec!["item_close".to_string()]);
    }

    /// Regression: completing a recurring task in the project view must hide
    /// it immediately via `pending_close_recurring`, not leave it visible.
    #[test]
    fn completing_recurring_in_project_view_hides_immediately() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "p1".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });
        app.tasks.push(Task {
            id: "rec".to_string(),
            content: "Clean air purifier prefilters".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: "2026-07-16".to_string(),
                is_recurring: true,
                string: Some("every month".to_string()),
                ..Due::default()
            }),
            ..Task::default()
        });
        app.tasks.push(Task {
            id: "other".to_string(),
            content: "Other task".to_string(),
            project_id: "p1".to_string(),
            ..Task::default()
        });
        app.selected_project = 0; // "p1" is at index 0
        app.active_pane = Pane::Tasks;
        app.selected_task = 0;

        app.enqueue_complete_selected();

        let visible_ids: Vec<&str> =
            app.visible_tasks().iter().map(|t| t.id.as_str()).collect();
        assert!(
            !visible_ids.contains(&"rec"),
            "recurring task must disappear from project view after completing"
        );
        assert!(app.pending_close_recurring.contains("rec"));
        assert!(!app.tasks.iter().find(|t| t.id == "rec").unwrap().checked);
    }

    /// Regression: pressing `x` on a Todoist task in the All view must
    /// complete *that specific task*, addressed by ID. Previously the
    /// handler routed through `selected_task` + `visible_tasks()`, but on
    /// the All view `visible_tasks()` is project-scoped — so the index
    /// referred to the wrong task (or was out of range, doing nothing).
    #[test]
    fn all_view_x_completes_task_by_id() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "p1".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });
        // A task in the currently-selected project, NOT due today — this
        // is what `visible_tasks()` returns on the All view. If the old
        // index-based path is used, this is what gets completed by mistake.
        app.tasks.push(Task {
            id: "wrong".to_string(),
            content: "Not due today".to_string(),
            project_id: "p1".to_string(),
            ..Task::default()
        });
        // A task due today — this is what should appear in the All view
        // and what should actually get completed when `x` is pressed.
        app.tasks.push(Task {
            id: "right".to_string(),
            content: "Due today".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: crate::ui::dates::today_str(),
                is_recurring: false,
                ..Due::default()
            }),
            ..Task::default()
        });
        app.selected_project = 0;
        app.activate_all_view();
        app.active_pane = Pane::Tasks;
        app.selected_all_item = 0;

        // Sanity: the All view shows exactly one task — "right".
        let items = app.all_view_items();
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], AllViewItem::Task(_)));

        // Dispatch `x` through the real key handler.
        let action =
            keys::handle_key(&mut app, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        match action {
            KeyAction::CompleteTaskById(id) => {
                assert_eq!(id, "right", "handler resolved the correct task ID");
                app.enqueue_complete_task_by_id(id);
            }
            _ => panic!("expected CompleteTaskById"),
        }

        // "right" was completed optimistically; "wrong" is untouched.
        assert!(
            app.tasks.iter().find(|t| t.id == "right").unwrap().checked,
            "Due-today task was marked complete"
        );
        assert!(
            !app.tasks.iter().find(|t| t.id == "wrong").unwrap().checked,
            "Project task outside All view must not be touched"
        );
        assert_eq!(pending_cmd_types(&app), vec!["item_close".to_string()]);
    }

    /// Regression: pressing Enter on a Todoist task in the All view must
    /// open the detail pane for *that* task, not the task that happens to
    /// share the index in the project-scoped `visible_tasks()`. Verified by
    /// putting the All-view task in a project that isn't selected, so the
    /// project-scoped list wouldn't surface it.
    ///
    /// Uses `#[tokio::test]` because `open_detail_for` spawns a background
    /// comments fetch.
    #[tokio::test]
    async fn all_view_enter_opens_detail_for_the_right_task() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "p_selected".to_string(),
            name: "Selected".to_string(),
            ..Project::default()
        });
        app.projects.push(Project {
            id: "p_other".to_string(),
            name: "Other".to_string(),
            ..Project::default()
        });
        // Task that populates visible_tasks() when p_selected is the
        // selected project — the old bug would open *this* task.
        app.tasks.push(Task {
            id: "visible_but_wrong".to_string(),
            content: "In selected project, not due today".to_string(),
            project_id: "p_selected".to_string(),
            ..Task::default()
        });
        // Task that only appears on the All view (different project, due today).
        app.tasks.push(Task {
            id: "all_view_task".to_string(),
            content: "Due today elsewhere".to_string(),
            project_id: "p_other".to_string(),
            due: Some(Due {
                date: crate::ui::dates::today_str(),
                is_recurring: false,
                ..Due::default()
            }),
            ..Task::default()
        });
        app.selected_project = 0; // p_selected
        app.activate_all_view();
        app.active_pane = Pane::Tasks;
        app.selected_all_item = 0;

        let action =
            keys::handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        match action {
            KeyAction::OpenDetailById(id) => {
                assert_eq!(id, "all_view_task");
                app.open_detail_for(id);
            }
            _ => panic!("expected OpenDetailById"),
        }

        assert_eq!(app.detail_task_id.as_deref(), Some("all_view_task"));
        assert!(matches!(app.active_pane, Pane::Detail));
        assert_eq!(
            app.selected_task().map(|t| t.id.as_str()),
            Some("all_view_task"),
            "selected_task() must resolve via detail_task_id, not visible_tasks"
        );
    }

    /// Completing a non-recurring task flips to checked immediately for
    /// instant UI feedback, and sends item_close.
    /// Upcoming shows every active parent task with a due date — overdue,
    /// today, and future — assigned to the current user or unassigned,
    /// sorted by date.
    #[test]
    fn upcoming_view_lists_all_scheduled_tasks_sorted_by_date() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "p1".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });

        // Overdue
        app.tasks.push(Task {
            id: "past".to_string(),
            content: "Old".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: "2026-04-10".to_string(),
                ..Due::default()
            }),
            ..Task::default()
        });
        // Today
        app.tasks.push(Task {
            id: "today".to_string(),
            content: "Now".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: crate::ui::dates::today_str(),
                ..Due::default()
            }),
            ..Task::default()
        });
        // Future
        app.tasks.push(Task {
            id: "future".to_string(),
            content: "Later".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: crate::ui::dates::offset_days_str(30),
                ..Due::default()
            }),
            ..Task::default()
        });
        // No due date — excluded
        app.tasks.push(Task {
            id: "undated".to_string(),
            content: "Someday".to_string(),
            project_id: "p1".to_string(),
            due: None,
            ..Task::default()
        });
        // Completed — excluded
        app.tasks.push(Task {
            id: "done".to_string(),
            content: "Shipped".to_string(),
            project_id: "p1".to_string(),
            checked: true,
            due: Some(Due {
                date: crate::ui::dates::today_str(),
                ..Due::default()
            }),
            ..Task::default()
        });

        app.activate_upcoming_view();
        let ids: Vec<&str> = app.visible_tasks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["past", "today", "future"]);
    }

    /// Adding from Upcoming targets Inbox and leaves the due date blank —
    /// the user picks the day, unlike Today which defaults to "today".
    #[test]
    fn add_from_upcoming_view_targets_inbox_no_default_due() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "inbox_1".to_string(),
            name: "Inbox".to_string(),
            inbox_project: Some(true),
            ..Project::default()
        });
        app.projects.push(Project {
            id: "p2".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });
        app.selected_project = 1;
        app.activate_upcoming_view();

        app.start_input();

        let form = app.task_form.as_ref().expect("form opens");
        assert_eq!(form.project_id, "inbox_1");
        assert_eq!(form.due_string, "");
    }

    #[test]
    fn activating_upcoming_deactivates_today() {
        let mut app = test_app();
        app.activate_today_view();
        assert!(app.today_view_active);
        app.activate_upcoming_view();
        assert!(app.upcoming_view_active);
        assert!(!app.today_view_active);
    }

    /// Sidebar hides the Pull Requests entry when `gh` isn't on PATH and the
    /// Jira Cards entry when `acli` isn't on PATH.
    #[test]
    fn sidebar_hides_external_entries_when_cli_missing() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "inbox".to_string(),
            name: "Inbox".to_string(),
            inbox_project: Some(true),
            ..Project::default()
        });
        app.gh_available = false;
        app.acli_available = false;
        app.gws_available = false;

        let entries = app.project_list_entries();
        let has_prs = entries
            .iter()
            .any(|e| matches!(e, ProjectEntry::GithubPrsView(_)));
        let has_jira = entries
            .iter()
            .any(|e| matches!(e, ProjectEntry::JiraCardsView));
        let has_agenda = entries
            .iter()
            .any(|e| matches!(e, ProjectEntry::AgendaView));
        assert!(!has_prs, "PRs hidden when gh missing");
        assert!(!has_jira, "Jira hidden when acli missing");
        assert!(!has_agenda, "Agenda hidden when gws missing");

        app.gh_available = true;
        // Still no PR entry — gh is available but there are no PRs yet.
        let entries = app.project_list_entries();
        assert!(
            !entries
                .iter()
                .any(|e| matches!(e, ProjectEntry::GithubPrsView(_))),
            "no PR entries until we have PR data"
        );

        app.github_prs.push(PullRequest {
            number: 1,
            title: "t".into(),
            url: String::new(),
            repo_full_name: "cxrlos/ratatoist".into(),
            author_login: "me".into(),
            updated_at: String::new(),
            is_draft: false,
            node_id: String::new(),
            check_status: None,
        });
        let entries = app.project_list_entries();
        assert!(
            entries
                .iter()
                .any(|e| matches!(e, ProjectEntry::GithubPrsView(_))),
            "PR entry appears once we have data"
        );
        assert!(
            !entries
                .iter()
                .any(|e| matches!(e, ProjectEntry::JiraCardsView))
        );

        app.acli_available = true;
        let entries = app.project_list_entries();
        assert!(
            entries
                .iter()
                .any(|e| matches!(e, ProjectEntry::JiraCardsView))
        );
    }

    /// When `show_stats` is false, Tab out of Tasks must never land on
    /// the hidden StatsDock pane — it wraps back to Projects instead.
    /// Conversely, with `show_stats = true` the old behavior stands:
    /// Tab from Tasks descends into StatsDock. BackTab from Projects is
    /// symmetric.
    #[test]
    fn show_stats_false_skips_stats_pane_on_tab() {
        let mut app = test_app();
        app.show_stats = false;
        app.active_pane = Pane::Tasks;
        press(&mut app, KeyCode::Tab);
        assert!(
            matches!(app.active_pane, Pane::Projects),
            "Tab from Tasks wraps to Projects when Stats hidden"
        );
        app.active_pane = Pane::Projects;
        press(&mut app, KeyCode::BackTab);
        assert!(
            matches!(app.active_pane, Pane::Tasks),
            "BackTab from Projects wraps to Tasks when Stats hidden"
        );

        app.show_stats = true;
        app.active_pane = Pane::Tasks;
        press(&mut app, KeyCode::Tab);
        assert!(
            matches!(app.active_pane, Pane::StatsDock),
            "Tab from Tasks descends into Stats when shown"
        );
    }

    /// Switching among virtual views keeps them mutually exclusive.
    #[test]
    fn github_prs_view_is_mutually_exclusive_with_today_and_upcoming() {
        let mut app = test_app();
        app.activate_today_view();
        app.activate_github_prs_view("cxrlos".to_string());
        assert!(app.is_pr_view_active());
        assert_eq!(app.active_pr_org.as_deref(), Some("cxrlos"));
        assert!(!app.today_view_active);
        assert!(!app.upcoming_view_active);

        app.activate_upcoming_view();
        assert!(app.upcoming_view_active);
        assert!(!app.is_pr_view_active());
    }

    /// Agenda events land in the All view under their own section and in
    /// the expected order (agenda first, then today's tasks). Verified by
    /// populating both and asserting the AllViewItem sequence — we don't
    /// call `activate_agenda_view` here because that would spawn a tokio
    /// fetch and we're running without a runtime.
    #[test]
    fn all_view_interleaves_agenda_events_before_tasks() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "p1".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });
        app.tasks.push(Task {
            id: "t1".to_string(),
            content: "Today task".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: crate::ui::dates::today_str(),
                is_recurring: false,
                ..Due::default()
            }),
            ..Task::default()
        });
        app.agenda_events.push(CalendarEvent {
            summary: "Standup".to_string(),
            start: "2026-04-17T09:00:00-04:00".to_string(),
            end: "2026-04-17T09:30:00-04:00".to_string(),
            all_day: false,
            location: String::new(),
            html_link: "https://calendar.google.com/event?eid=abc".to_string(),
            calendar_name: "Work".to_string(),
        });
        app.agenda_events.push(CalendarEvent {
            summary: "Dentist".to_string(),
            start: "2026-04-17T14:00:00-04:00".to_string(),
            end: "2026-04-17T15:00:00-04:00".to_string(),
            all_day: false,
            location: "Main St.".to_string(),
            html_link: "https://calendar.google.com/event?eid=def".to_string(),
            calendar_name: "Personal".to_string(),
        });

        let items = app.all_view_items();
        // Expected: two AgendaEvent (indices 0 and 1) then one Task (index 0).
        assert_eq!(items.len(), 3, "one task + two events");
        assert!(matches!(items[0], AllViewItem::AgendaEvent(0)));
        assert!(matches!(items[1], AllViewItem::AgendaEvent(1)));
        assert!(matches!(items[2], AllViewItem::Task(_)));
    }

    /// With `gws_available = true` the Agenda sidebar entry appears under
    /// Inbox; with `gws_available = false` it's hidden. Mirrors the Jira
    /// gating test.
    #[test]
    fn agenda_sidebar_entry_gated_by_gws_available() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "inbox".to_string(),
            name: "Inbox".to_string(),
            inbox_project: Some(true),
            ..Project::default()
        });
        app.gh_available = false;
        app.acli_available = false;

        app.gws_available = false;
        assert!(
            !app.project_list_entries()
                .iter()
                .any(|e| matches!(e, ProjectEntry::AgendaView)),
            "Agenda hidden when gws missing"
        );

        app.gws_available = true;
        assert!(
            app.project_list_entries()
                .iter()
                .any(|e| matches!(e, ProjectEntry::AgendaView)),
            "Agenda appears once gws is detected"
        );
    }

    /// Hidden owners drop out of pr_owners_with_counts, so they no longer
    /// appear as sidebar entries. The hidden set persists across save/load
    /// (round-tripped via ui_settings.json), but here we just verify the
    /// in-memory filter — the IO half is exercised by load_hidden_pr_orgs.
    #[test]
    fn hide_pr_org_removes_owner_from_sidebar() {
        let mut app = test_app();
        app.github_prs = vec![
            PullRequest {
                number: 1,
                title: "a".into(),
                url: String::new(),
                repo_full_name: "appfolio/apm_bundle".into(),
                author_login: "me".into(),
                updated_at: String::new(),
                is_draft: false,
                node_id: String::new(),
                check_status: None,
            },
            PullRequest {
                number: 2,
                title: "c".into(),
                url: String::new(),
                repo_full_name: "cxrlos/ratatoist".into(),
                author_login: "me".into(),
                updated_at: String::new(),
                is_draft: false,
                node_id: String::new(),
                check_status: None,
            },
        ];

        // Before hiding: both owners visible.
        let before: Vec<_> = app
            .pr_owners_with_counts()
            .into_iter()
            .map(|(o, _)| o)
            .collect();
        assert_eq!(before, vec!["appfolio".to_string(), "cxrlos".to_string()]);

        // Activate appfolio so we can verify the active view also resets on
        // hide.
        app.activate_github_prs_view("appfolio".to_string());
        assert_eq!(app.active_pr_org.as_deref(), Some("appfolio"));

        app.hide_pr_org("appfolio".to_string());

        // appfolio is gone from the sidebar entries…
        let after: Vec<_> = app
            .pr_owners_with_counts()
            .into_iter()
            .map(|(o, _)| o)
            .collect();
        assert_eq!(after, vec!["cxrlos".to_string()]);

        // …and the active view was reset since its row vanished.
        assert!(app.active_pr_org.is_none());
        assert!(app.hidden_pr_orgs.contains("appfolio"));
    }

    /// Each GitHub owner in `github_prs` should yield exactly one sidebar
    /// entry, sorted alphabetically. Owners with zero PRs are dropped.
    #[test]
    fn pr_owners_with_counts_groups_by_owner() {
        let mut app = test_app();
        app.github_prs = vec![
            PullRequest {
                number: 1,
                title: "a".into(),
                url: String::new(),
                repo_full_name: "appfolio/apm_bundle".into(),
                author_login: "me".into(),
                updated_at: String::new(),
                is_draft: false,
                node_id: String::new(),
                check_status: None,
            },
            PullRequest {
                number: 2,
                title: "b".into(),
                url: String::new(),
                repo_full_name: "appfolio/otto".into(),
                author_login: "me".into(),
                updated_at: String::new(),
                is_draft: false,
                node_id: String::new(),
                check_status: None,
            },
            PullRequest {
                number: 3,
                title: "c".into(),
                url: String::new(),
                repo_full_name: "cxrlos/ratatoist".into(),
                author_login: "me".into(),
                updated_at: String::new(),
                is_draft: false,
                node_id: String::new(),
                check_status: None,
            },
        ];

        let owners = app.pr_owners_with_counts();
        assert_eq!(
            owners,
            vec![
                ("appfolio".to_string(), 2),
                ("cxrlos".to_string(), 1),
            ]
        );
    }

    #[test]
    fn active_org_prs_filters_to_chosen_org() {
        let mut app = test_app();
        app.github_prs = vec![
            PullRequest {
                number: 1,
                title: "apm".into(),
                url: String::new(),
                repo_full_name: "appfolio/apm_bundle".into(),
                author_login: "me".into(),
                updated_at: String::new(),
                is_draft: false,
                node_id: String::new(),
                check_status: None,
            },
            PullRequest {
                number: 2,
                title: "rata".into(),
                url: String::new(),
                repo_full_name: "cxrlos/ratatoist".into(),
                author_login: "me".into(),
                updated_at: String::new(),
                is_draft: false,
                node_id: String::new(),
                check_status: None,
            },
        ];
        app.activate_github_prs_view("cxrlos".to_string());
        let filtered: Vec<_> = app
            .active_org_prs()
            .iter()
            .map(|p| p.repo_full_name.as_str())
            .collect();
        assert_eq!(filtered, vec!["cxrlos/ratatoist"]);
    }

    #[test]
    fn complete_nonrecurring_flips_checked_and_sends_item_close() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "p1".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });
        app.tasks.push(Task {
            id: "t_once".to_string(),
            content: "Write memo".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: "2026-04-15".to_string(),
                is_recurring: false,
                ..Due::default()
            }),
            ..Task::default()
        });
        app.active_pane = Pane::Tasks;
        app.selected_task = 0;

        app.enqueue_complete_selected();

        assert!(app.tasks[0].checked);
        assert_eq!(pending_cmd_types(&app), vec!["item_close".to_string()]);
    }

    /// Star jar: each `item_close` earns one star; an `item_reopen` (closing
    /// an already-checked task) does not subtract; rolling the date
    /// resets the jar to zero. Covers the one-way-tally contract and the
    /// lazy midnight rollover described in star-jar.spec.md.
    #[test]
    fn star_jar_earns_one_per_completion_and_resets_at_day_rollover() {
        let mut app = test_app();
        app.projects.push(Project {
            id: "p1".to_string(),
            name: "Work".to_string(),
            ..Project::default()
        });
        app.tasks.push(Task {
            id: "t_once".to_string(),
            content: "Write memo".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: "2026-04-15".to_string(),
                is_recurring: false,
                ..Due::default()
            }),
            ..Task::default()
        });
        app.tasks.push(Task {
            id: "t_recur".to_string(),
            content: "Standup".to_string(),
            project_id: "p1".to_string(),
            due: Some(Due {
                date: "2026-04-15".to_string(),
                is_recurring: true,
                ..Due::default()
            }),
            ..Task::default()
        });

        assert_eq!(app.star_count, 0, "jar starts empty");

        // Non-recurring complete → +1.
        app.enqueue_complete_task_by_id("t_once".to_string());
        assert_eq!(app.star_count, 1);

        // Recurring complete also earns a star — it's still work done.
        app.enqueue_complete_task_by_id("t_recur".to_string());
        assert_eq!(app.star_count, 2);

        // Reopening the non-recurring task must NOT decrement (and actually
        // enqueues item_reopen because `checked` is already true).
        app.enqueue_complete_task_by_id("t_once".to_string());
        assert_eq!(app.star_count, 2, "reopen does not remove stars");

        // Simulate the local date rolling over; the next completion ticks
        // from zero on the new day, not from yesterday's final count.
        app.star_date = "1999-01-01".to_string();
        app.tasks[0].checked = false;
        app.enqueue_complete_task_by_id("t_once".to_string());
        assert_eq!(app.star_count, 1, "jar reset at day rollover");
        assert_eq!(app.star_date, crate::ui::dates::today_str());
    }

    /// `p` toggles the pomodoro: start sets `pomodoro_started_at`,
    /// second press clears it without incrementing tomatoes. Cancellation
    /// throws away the elapsed time per spec.
    #[test]
    fn toggle_pomodoro_starts_and_cancels() {
        let mut app = test_app();
        app.tomato_count = 0;
        assert!(app.pomodoro_started_at.is_none(), "starts idle");

        app.toggle_pomodoro();
        assert!(app.pomodoro_started_at.is_some(), "p starts a pomodoro");
        assert_eq!(app.tomato_count, 0, "start doesn't award a tomato");

        app.toggle_pomodoro();
        assert!(
            app.pomodoro_started_at.is_none(),
            "second p cancels the pomodoro"
        );
        assert_eq!(app.tomato_count, 0, "cancel doesn't award a tomato");
    }

    /// When the full POMODORO_DURATION has elapsed, `maybe_award_tomato`
    /// increments the count and clears the running state. Simulated by
    /// backdating `pomodoro_started_at` so the test runs instantly.
    /// Tomato reset at day rollover is also verified here.
    #[test]
    fn maybe_award_tomato_credits_and_resets_at_rollover() {
        let mut app = test_app();
        app.tomato_count = 0;
        app.tomato_date = crate::ui::dates::today_str();

        // No pomodoro → no-op.
        app.maybe_award_tomato();
        assert_eq!(app.tomato_count, 0);

        // Running but not yet elapsed → no-op.
        app.toggle_pomodoro();
        app.maybe_award_tomato();
        assert_eq!(app.tomato_count, 0, "mid-timer awards nothing");
        assert!(app.pomodoro_started_at.is_some());

        // Backdate the start so the timer has fully elapsed.
        app.pomodoro_started_at = Some(
            Instant::now()
                .checked_sub(POMODORO_DURATION)
                .expect("Instant should subtract"),
        );
        app.maybe_award_tomato();
        assert_eq!(app.tomato_count, 1, "completion awards a tomato");
        assert!(
            app.pomodoro_started_at.is_none(),
            "completion clears running state"
        );

        // A second elapsed pomodoro on a different date resets first.
        app.tomato_date = "1999-01-01".to_string();
        app.pomodoro_started_at = Some(
            Instant::now()
                .checked_sub(POMODORO_DURATION)
                .expect("Instant should subtract"),
        );
        app.maybe_award_tomato();
        assert_eq!(app.tomato_count, 1, "rollover zeroed then incremented");
        assert_eq!(app.tomato_date, crate::ui::dates::today_str());
    }
}
