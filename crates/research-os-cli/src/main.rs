use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::Terminal;
use unicode_width::UnicodeWidthStr;

// Some ledger items are consumed by the phase driver (Phase 3) which isn't
// wired yet; allow dead_code until then so the build stays warning-clean.
#[allow(dead_code)]
mod ledger;
mod checkpoint_ipc;
mod mcp_sidecar;
// Phase-driver decision logic; consumed by the driver integration (Phase 3b).
#[allow(dead_code)]
mod phase;
// Derived wiki relationship graph; consumed by graph_query + Context Assembler.
#[allow(dead_code)]
mod graph;

const SKILLS: &[&str] = &[
    "research-os-planner",
    "research-os-doc-search",
    "research-os-search",
    "research-os-source-triage",
    "research-os-ingest",
    "research-os-reader",
    "research-os-wiki-update",
    "research-os-synthesis",
    "research-os-lint-critic",
    "research-os-qa",
    "research-os-discussion",
    "research-os-ideation",
    "research-os-writing",
    "research-os-visualization",
    "research-os-coding",
    "research-os-experiment-planning",
];

const DEPRECATED_SKILLS: &[&str] = &[
    "research-os-search",
    "research-os-source-triage",
    "research-os-ingest",
    "research-os-reader",
];

const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/help", "show commands"),
    ("/init", "start a new research session"),
    ("/search", "search sources in this session"),
    ("/loop", "run the human-steered phase research loop on a subject"),
    ("/skills", "list agent skills"),
    ("/scope", "set or show search scope"),
    ("/agent", "show or switch agent backend (codex|claude)"),
    ("/resume", "list and resume a session"),
    ("/artifacts", "show session artifacts"),
    ("/open", "preview artifact"),
    ("/clear", "clear conversation"),
    ("/exit", "exit TUI"),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusPane {
    Conversation,
    Pipeline,
    Artifacts,
}

impl Default for FocusPane {
    fn default() -> Self {
        FocusPane::Conversation
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum InputToolView {
    #[default]
    None,
    Resume,
    PlanQuestion,
    Checkpoint,
    Artifacts,
}

/// Which external agent CLI backend research-os shells out to for each stage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Backend {
    #[default]
    Codex,
    Claude,
}

fn backend_label(backend: Backend) -> &'static str {
    match backend {
        Backend::Codex => "codex",
        Backend::Claude => "claude",
    }
}

fn parse_backend(value: &str) -> Option<Backend> {
    match value.trim().to_ascii_lowercase().as_str() {
        "codex" => Some(Backend::Codex),
        "claude" => Some(Backend::Claude),
        _ => None,
    }
}

/// Initial backend for a process, read from `RESEARCH_OS_AGENT`. Defaults to
/// codex so existing behavior is unchanged when the variable is unset.
fn resolve_backend_env() -> Backend {
    env::var("RESEARCH_OS_AGENT")
        .ok()
        .and_then(|value| parse_backend(&value))
        .unwrap_or(Backend::Codex)
}

fn main() {
    if let Err(err) = run() {
        eprintln!("research-os: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        let root = env::current_dir().map_err(|e| e.to_string())?;
        init_workspace(&root)?;
        return run_tui(root);
    };

    let root = env::current_dir().map_err(|e| e.to_string())?;
    match cmd.as_str() {
        "tui" => {
            init_workspace(&root)?;
            run_tui(root)
        }
        "init" => init_workspace(&root),
        "run" => Err(
            "`research-os run` is deprecated. Use `research-os tui` and run work inside a session."
                .into(),
        ),
        "resume" => Err(
            "`research-os resume <run_id>` is deprecated. Use TUI `/resume` to resume a session."
                .into(),
        ),
        "validate" => {
            Err("`research-os validate <run_id>` is deprecated for session-only mode.".into())
        }
        "install-skills" => install_skills(&root),
        // Hidden: started by a claude stage via --mcp-config. Serves the local
        // MCP sidecar (custom domain tools) over stdio for `<session_id>`.
        "__mcp" => mcp_sidecar::run(&root, args.next()),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command: {other}")),
    }
}

fn print_help() {
    println!(
        "research-os\n\nCommands:\n  tui\n  init\n  install-skills\n\nRunning without a command opens the TUI. Research work now runs inside sessions.\n"
    );
}

#[derive(Default)]
struct TuiApp {
    input: String,
    input_cursor: usize,
    status: String,
    source_scope: String,
    session_id: String,
    run_id: String,
    active_turn_id: String,
    active_stage: String,
    stages: Vec<String>,
    completed_stages: Vec<String>,
    log: Vec<String>,
    expand_conversation: bool,
    log_scroll: usize,
    stage_scroll: usize,
    artifacts: Vec<String>,
    artifact_scroll: usize,
    running: bool,
    planning_mode: bool,
    run_started_at: Option<Instant>,
    last_run_elapsed: Option<Duration>,
    completion_index: usize,
    input_tool_view: InputToolView,
    resume_options: Vec<SessionSummary>,
    resume_index: usize,
    plan_question: Option<PlanQuestion>,
    plan_question_turn_id: Option<String>,
    plan_choice_index: usize,
    // Blocking checkpoint question from the MCP sidecar (answerable while a
    // stage is running). The responder sends the chosen index back to the
    // socket server thread, unblocking the sidecar.
    checkpoint: Option<PlanQuestion>,
    checkpoint_responder: Option<Sender<usize>>,
    checkpoint_choice_index: usize,
    sidebar_visible: bool,
    focus_pane: FocusPane,
    run_control: RunControl,
    backend: Backend,
}

#[derive(Clone, Debug)]
struct SessionSummary {
    session_id: String,
    prompt: String,
    turn_type: String,
    stage_count: usize,
    turn_count: usize,
}

#[derive(Clone, Debug)]
struct PlanQuestion {
    question: String,
    options: Vec<String>,
    final_confirmation: bool,
}

#[derive(Clone, Debug)]
struct StagePlanSummary {
    skill: &'static str,
    reason: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
}

#[derive(Clone, Default)]
struct RunControl {
    cancel_requested: Arc<AtomicBool>,
    current_child_pid: Arc<Mutex<Option<u32>>>,
}

enum UiMsg {
    RunStarted(String),
    SessionTurnStarted(String),
    Stages(Vec<String>),
    StageStarted(String),
    StageDone(String),
    Line(String),
    PlanQuestion(PlanQuestion),
    /// A blocking checkpoint question from the MCP sidecar: render it and send
    /// the chosen option index back through the channel so the sidecar (and the
    /// claude stage waiting on it) can continue.
    Checkpoint(PlanQuestion, Sender<usize>),
    Artifacts(Vec<String>),
    Done(String),
    Failed(String),
}

enum StageOutput {
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionTurnKind {
    Discussion,
    Search,
    Plan,
    Loop,
}

impl SessionTurnKind {
    fn as_str(self) -> &'static str {
        match self {
            SessionTurnKind::Discussion => "discussion",
            SessionTurnKind::Search => "search",
            SessionTurnKind::Plan => "plan",
            SessionTurnKind::Loop => "loop",
        }
    }
}

fn run_tui(root: PathBuf) -> Result<(), String> {
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

    let result = run_tui_loop(&mut terminal, root);

    disable_raw_mode().map_err(|e| e.to_string())?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        LeaveAlternateScreen
    )
    .map_err(|e| e.to_string())?;
    terminal.show_cursor().map_err(|e| e.to_string())?;
    result
}

fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    root: PathBuf,
) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<UiMsg>();
    // Listen for checkpoint questions from MCP sidecars spawned by claude
    // stages (best-effort; if the socket can't bind, checkpoint_ask just errors
    // in the sidecar and the stage reports it).
    spawn_checkpoint_server(root.clone(), tx.clone());
    let mut app = TuiApp {
        status: "Type a research instruction. Tab toggles plan mode. Use /exit to quit.".into(),
        session_id: make_session_id(),
        sidebar_visible: false,
        focus_pane: FocusPane::Conversation,
        run_control: RunControl::default(),
        backend: resolve_backend_env(),
        ..TuiApp::default()
    };

    loop {
        drain_ui_messages(&mut app, &rx);
        // A checkpoint can only be answered while its stage runs; if the run was
        // cancelled with one still open, resolve it (default option) so the
        // sidecar/server thread doesn't block forever.
        if !app.running && app.checkpoint_responder.is_some() {
            if let Some(responder) = app.checkpoint_responder.take() {
                let _ = responder.send(0);
            }
            app.checkpoint = None;
            app.checkpoint_choice_index = 0;
            if app.input_tool_view == InputToolView::Checkpoint {
                app.input_tool_view = InputToolView::None;
            }
        }
        terminal
            .draw(|frame| draw_tui(frame, &app))
            .map_err(|e| e.to_string())?;

        if event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            let event = event::read().map_err(|e| e.to_string())?;
            let key = match event {
                Event::Key(key) => key,
                Event::Paste(text) if !app.running => {
                    clear_input_tool_view(&mut app);
                    insert_str_at_cursor(&mut app.input, &mut app.input_cursor, &text);
                    clamp_completion_index(&mut app);
                    continue;
                }
                _ => continue,
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if !app.running {
                        return Ok(());
                    }
                }
                KeyCode::Esc => {
                    if app.running {
                        request_run_cancel(&mut app);
                    } else {
                        return_to_prompt(&mut app);
                    }
                }
                // Checkpoint questions are answered WHILE a stage runs (the
                // sidecar blocks on the answer), so these are not gated on
                // `!app.running`.
                KeyCode::Up if app.input_tool_view == InputToolView::Checkpoint => {
                    move_checkpoint_choice(&mut app, -1);
                }
                KeyCode::Down if app.input_tool_view == InputToolView::Checkpoint => {
                    move_checkpoint_choice(&mut app, 1);
                }
                KeyCode::Enter if app.input_tool_view == InputToolView::Checkpoint => {
                    submit_checkpoint_choice(&mut app);
                }
                KeyCode::Enter
                    if !app.running
                        && app.input.trim().is_empty()
                        && app.input_tool_view == InputToolView::PlanQuestion =>
                {
                    submit_plan_choice(&root, &mut app, &tx)?;
                }
                KeyCode::Enter
                    if !app.running
                        && app.input.trim().is_empty()
                        && resume_picker_active(&app) =>
                {
                    resume_selected_run(&root, &mut app, &tx);
                }
                KeyCode::Enter if slash_completion_open(&app) => {
                    complete_slash_command(&mut app);
                }
                KeyCode::Enter if !app.running && !app.input.trim().is_empty() => {
                    let instruction = app.input.trim().to_string();
                    app.input.clear();
                    app.input_cursor = 0;
                    if instruction.starts_with('/') {
                        if handle_slash_command(&root, &mut app, &instruction, &tx)? {
                            return Ok(());
                        }
                        continue;
                    }
                    let turn_kind = if app.planning_mode {
                        SessionTurnKind::Plan
                    } else {
                        SessionTurnKind::Discussion
                    };
                    start_session_turn(&root, &mut app, &tx, instruction, turn_kind)?;
                }
                KeyCode::Backspace if !app.running => {
                    clear_input_tool_view(&mut app);
                    delete_char_before_cursor(&mut app.input, &mut app.input_cursor);
                    clamp_completion_index(&mut app);
                }
                KeyCode::Tab if !app.running && app.input.starts_with('/') => {
                    complete_slash_command(&mut app);
                }
                KeyCode::Tab if !app.running && app.input_tool_view == InputToolView::None => {
                    toggle_planning_mode(&mut app);
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    scroll_focused_pane_up(&mut app, pane_page_amount(terminal));
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    scroll_focused_pane_down(&mut app, pane_page_amount(terminal));
                }
                KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.expand_conversation = !app.expand_conversation;
                    clamp_log_scroll(&mut app, None);
                    app.status = if app.expand_conversation {
                        "Conversation expanded".into()
                    } else {
                        "Conversation compacted".into()
                    };
                }
                KeyCode::Down
                    if !app.running && app.input_tool_view == InputToolView::PlanQuestion =>
                {
                    move_plan_choice(&mut app, 1);
                }
                KeyCode::Up
                    if !app.running && app.input_tool_view == InputToolView::PlanQuestion =>
                {
                    move_plan_choice(&mut app, -1);
                }
                KeyCode::Down
                    if !app.running
                        && app.input.trim().is_empty()
                        && resume_picker_active(&app) =>
                {
                    move_resume_selection(&mut app, 1);
                }
                KeyCode::Up
                    if !app.running
                        && app.input.trim().is_empty()
                        && resume_picker_active(&app) =>
                {
                    move_resume_selection(&mut app, -1);
                }
                KeyCode::Down
                    if !app.running && app.input_tool_view == InputToolView::Artifacts =>
                {
                    app.artifact_scroll = app.artifact_scroll.saturating_add(1);
                    clamp_artifact_scroll(&mut app);
                }
                KeyCode::Up if !app.running && app.input_tool_view == InputToolView::Artifacts => {
                    app.artifact_scroll = app.artifact_scroll.saturating_sub(1);
                }
                KeyCode::Up if !app.input.starts_with('/') => {
                    scroll_focused_pane_up(&mut app, 1);
                }
                KeyCode::Down if !app.input.starts_with('/') => {
                    scroll_focused_pane_down(&mut app, 1);
                }
                KeyCode::Left if !app.running => {
                    app.input_cursor = app.input_cursor.saturating_sub(1);
                }
                KeyCode::Right if !app.running => {
                    app.input_cursor = (app.input_cursor + 1).min(app.input.chars().count());
                }
                KeyCode::Home if !app.running => {
                    app.input_cursor = 0;
                }
                KeyCode::End if !app.running => {
                    app.input_cursor = app.input.chars().count();
                }
                KeyCode::Delete if !app.running => {
                    clear_input_tool_view(&mut app);
                    delete_char_at_cursor(&mut app.input, app.input_cursor);
                    clamp_completion_index(&mut app);
                }
                KeyCode::Down if !app.running && app.input.starts_with('/') => {
                    let count = slash_matches(&app.input).len();
                    if count > 0 {
                        app.completion_index = (app.completion_index + 1) % count;
                    }
                }
                KeyCode::Up if !app.running && app.input.starts_with('/') => {
                    let count = slash_matches(&app.input).len();
                    if count > 0 {
                        app.completion_index = if app.completion_index == 0 {
                            count - 1
                        } else {
                            app.completion_index - 1
                        };
                    }
                }
                KeyCode::Char(ch) if !app.running => {
                    clear_input_tool_view(&mut app);
                    insert_char_at_cursor(&mut app.input, &mut app.input_cursor, ch);
                    clamp_completion_index(&mut app);
                }
                _ => {}
            }
        }
    }
}

fn drain_ui_messages(app: &mut TuiApp, rx: &Receiver<UiMsg>) {
    while let Ok(msg) = rx.try_recv() {
        match msg {
            UiMsg::RunStarted(run_id) => {
                app.run_id = run_id;
                app.status = "Planner running".into();
            }
            UiMsg::SessionTurnStarted(turn_id) => {
                app.run_id.clear();
                app.active_turn_id = turn_id.clone();
                app.active_stage.clear();
                app.status = format!("Session turn running: {turn_id}");
            }
            UiMsg::Stages(stages) => {
                app.stages = stages;
                clamp_stage_scroll(app);
            }
            UiMsg::StageStarted(stage) => {
                app.active_stage = stage.clone();
                app.status = format!("Running {stage}");
            }
            UiMsg::StageDone(stage) => {
                app.completed_stages.push(stage);
            }
            UiMsg::Line(line) => {
                let was_scrolled = app.log_scroll > 0;
                app.log.push(line);
                if app.log.len() > 1_000 {
                    let drained = 200;
                    app.log.drain(0..200);
                    app.log_scroll = app.log_scroll.saturating_sub(drained);
                } else if was_scrolled {
                    app.log_scroll = app.log_scroll.saturating_add(1);
                }
                clamp_log_scroll(app, None);
            }
            UiMsg::PlanQuestion(question) => {
                app.plan_question = Some(question);
                app.plan_question_turn_id = Some(app.active_turn_id.clone());
                app.plan_choice_index = 0;
                app.input_tool_view = InputToolView::PlanQuestion;
                app.input.clear();
                app.input_cursor = 0;
                app.planning_mode = true;
                app.status = "Planner is asking for a choice".into();
            }
            UiMsg::Checkpoint(question, responder) => {
                app.checkpoint = Some(question);
                app.checkpoint_responder = Some(responder);
                app.checkpoint_choice_index = 0;
                app.input_tool_view = InputToolView::Checkpoint;
                app.status = "Checkpoint: Up/Down to select, Enter to answer".into();
            }
            UiMsg::Artifacts(artifacts) => {
                app.artifacts = artifacts;
                clamp_artifact_scroll(app);
            }
            UiMsg::Done(message) => {
                finish_run_timer(app);
                app.status = message;
                app.running = false;
                app.active_stage.clear();
            }
            UiMsg::Failed(err) => {
                finish_run_timer(app);
                app.status = format!("Failed: {err}");
                app.running = false;
            }
        }
    }
}

fn finish_run_timer(app: &mut TuiApp) {
    if let Some(started_at) = app.run_started_at.take() {
        app.last_run_elapsed = Some(started_at.elapsed());
    }
}

fn handle_slash_command(
    root: &Path,
    app: &mut TuiApp,
    input: &str,
    tx: &Sender<UiMsg>,
) -> Result<bool, String> {
    let mut parts = input.split_whitespace();
    let command = parts.next().unwrap_or("");
    let rest = parts.collect::<Vec<_>>().join(" ");
    match command {
        "/help" => {
            push_system_lines(
                app,
                &[
                    "/help                  show this help",
                    "/init                  start a new multi-turn research session",
                    "/search <prompt>       search sources in the current session",
                    "/skills                list research-os skills",
                    "/scope <constraint>    set source scope for session search turns",
                    "/scope clear           clear source scope",
                    "/agent [codex|claude]  show or switch the agent CLI backend",
                    "/resume                list sessions; Enter resumes selected session",
                    "/artifacts             show session artifacts by stage",
                    "/open <path>           preview a session artifact",
                    "Tab                    switch discussion/plan mode",
                    "Up/Down                scroll conversation or select an input tool item",
                    "Ctrl-U/Ctrl-D          page scroll focused pane",
                    "/clear                 clear the conversation pane",
                    "/exit                  exit",
                ],
            );
        }
        "/init" => {
            if app.running {
                app.log.push(
                    "system: Cannot init while a run is active. Press Esc to cancel first.".into(),
                );
            } else {
                reset_tui_session(root, app)?;
            }
        }
        "/search" => {
            if rest.trim().is_empty() {
                app.log.push("system: Usage: /search <prompt>".into());
            } else {
                app.planning_mode = false;
                start_session_turn(
                    root,
                    app,
                    tx,
                    rest.trim().to_string(),
                    SessionTurnKind::Search,
                )?;
            }
        }
        "/loop" => {
            // Empty subject => continue the loop from the ledger's current phase.
            app.planning_mode = false;
            start_session_turn(
                root,
                app,
                tx,
                rest.trim().to_string(),
                SessionTurnKind::Loop,
            )?;
        }
        "/skills" => {
            push_system_lines(app, &["Available skills:"]);
            for skill in SKILLS {
                if is_deprecated_skill(skill) {
                    app.log
                        .push(format!("system: - {skill} (deprecated legacy)"));
                } else {
                    app.log.push(format!("system: - {skill}"));
                }
            }
            app.status = "Listed skills".into();
        }
        "/scope" => {
            if rest.trim().is_empty() {
                push_scope_help(app);
                if app.source_scope.is_empty() {
                    app.log.push("system: No source scope is set.".into());
                } else {
                    app.log.push(format!(
                        "system: Current source scope: {}",
                        app.source_scope
                    ));
                }
            } else if rest.trim() == "clear" {
                app.source_scope.clear();
                app.status = "Cleared source scope".into();
                app.log.push("system: Source scope cleared.".into());
            } else {
                app.source_scope = rest.trim().to_string();
                app.status = "Source scope set".into();
                app.log
                    .push(format!("system: Source scope set: {}", app.source_scope));
            }
        }
        "/agent" => {
            if rest.trim().is_empty() {
                app.log.push(format!(
                    "system: Current agent backend: {}",
                    backend_label(app.backend)
                ));
                app.log
                    .push("system: Usage: /agent codex | /agent claude".into());
            } else if app.running {
                app.log.push(
                    "system: Cannot switch agent while a run is active. Press Esc to cancel first."
                        .into(),
                );
            } else if let Some(backend) = parse_backend(rest.trim()) {
                app.backend = backend;
                app.status = format!("Agent backend: {}", backend_label(backend));
                app.log.push(format!(
                    "system: Agent backend set to {}.",
                    backend_label(backend)
                ));
            } else {
                app.log.push(format!(
                    "system: Unknown agent `{}`. Use codex or claude.",
                    rest.trim()
                ));
            }
        }
        "/artifacts" => {
            app.artifacts = list_session_artifacts(root, &app.session_id);
            app.input_tool_view = InputToolView::Artifacts;
            app.input.clear();
            app.input_cursor = 0;
            app.status = format!("Showing {} artifacts", app.artifacts.len());
        }
        "/open" => {
            if rest.trim().is_empty() {
                app.log.push("system: Usage: /open <path>".into());
            } else {
                preview_artifact(root, app, rest.trim())?;
            }
        }
        "/resume" => show_resume_picker(root, app),
        "/clear" => {
            app.log.clear();
            app.log_scroll = 0;
            app.planning_mode = false;
            clear_input_tool_view(app);
            app.status = "Cleared conversation".into();
        }
        "/exit" | "/quit" => return Ok(true),
        other => {
            app.log.push(format!(
                "system: Unknown command `{other}`. Type /help for commands."
            ));
        }
    }
    Ok(false)
}

fn start_session_turn(
    root: &Path,
    app: &mut TuiApp,
    tx: &Sender<UiMsg>,
    instruction: String,
    turn_kind: SessionTurnKind,
) -> Result<(), String> {
    let instruction = if matches!(
        turn_kind,
        SessionTurnKind::Discussion | SessionTurnKind::Loop
    ) {
        instruction
    } else {
        apply_tui_scope(&instruction, &app.source_scope)
    };
    ensure_named_session(root, app, &instruction)?;
    init_session_workspace(root, &app.session_id)?;
    app.artifacts.clear();
    app.stage_scroll = 0;
    app.artifact_scroll = 0;
    app.stages.clear();
    app.completed_stages.clear();
    app.run_id.clear();
    clear_input_tool_view(app);
    app.running = true;
    app.run_started_at = Some(Instant::now());
    app.last_run_elapsed = None;
    app.run_control = RunControl::default();
    app.status = format!("Starting {} turn...", turn_kind.as_str());
    let tx = tx.clone();
    let root = root.to_path_buf();
    let control = app.run_control.clone();
    let session_id = app.session_id.clone();
    let backend = app.backend;
    thread::spawn(move || {
        if let Err(err) = run_session_turn_tui(
            backend,
            &root,
            &session_id,
            &instruction,
            turn_kind,
            tx.clone(),
            control,
        ) {
            let _ = tx.send(UiMsg::Failed(err));
        }
    });
    Ok(())
}

fn start_approved_plan_execution(
    root: &Path,
    app: &mut TuiApp,
    tx: &Sender<UiMsg>,
    plan_turn_id: String,
    answer: String,
) -> Result<(), String> {
    init_session_workspace(root, &app.session_id)?;
    app.artifacts.clear();
    app.stage_scroll = 0;
    app.artifact_scroll = 0;
    app.stages.clear();
    app.completed_stages.clear();
    app.run_id.clear();
    app.running = true;
    app.run_started_at = Some(Instant::now());
    app.last_run_elapsed = None;
    app.run_control = RunControl::default();
    app.status = "Proceeding with approved plan...".into();
    let tx = tx.clone();
    let root = root.to_path_buf();
    let control = app.run_control.clone();
    let session_id = app.session_id.clone();
    let backend = app.backend;
    thread::spawn(move || {
        if let Err(err) = run_approved_plan_execution_tui(
            backend,
            &root,
            &session_id,
            &plan_turn_id,
            &answer,
            tx.clone(),
            control,
        ) {
            let _ = tx.send(UiMsg::Failed(err));
        }
    });
    Ok(())
}

fn ensure_named_session(root: &Path, app: &mut TuiApp, instruction: &str) -> Result<(), String> {
    if !is_generated_session_id(&app.session_id) {
        return Ok(());
    }
    let new_session_id = make_session_id_from_instruction(root, instruction);
    if new_session_id == app.session_id {
        return Ok(());
    }
    let old_dir = root.join("sessions").join(&app.session_id);
    let new_dir = root.join("sessions").join(&new_session_id);
    if old_dir.exists() && !new_dir.exists() && session_has_state(&old_dir) {
        fs::rename(&old_dir, &new_dir).map_err(|e| {
            format!(
                "rename session {} to {}: {e}",
                old_dir.display(),
                new_dir.display()
            )
        })?;
    }
    app.session_id = new_session_id;
    app.status = format!("Session named {}", app.session_id);
    Ok(())
}

fn is_generated_session_id(session_id: &str) -> bool {
    session_id
        .strip_prefix("session-")
        .map(|rest| rest.chars().all(|ch| ch.is_ascii_digit()))
        .unwrap_or(false)
}

fn session_has_state(session_dir: &Path) -> bool {
    for rel in [
        "turns.jsonl",
        "transcripts",
        "raw",
        "artifacts",
        "assets",
        "plan",
    ] {
        if has_non_metadata_file(&session_dir.join(rel)) {
            return true;
        }
    }
    false
}

fn push_system_lines(app: &mut TuiApp, lines: &[&str]) {
    for line in lines {
        app.log.push(format!("system: {line}"));
    }
    app.status = "Slash command handled".into();
}

fn push_scope_help(app: &mut TuiApp) {
    push_system_lines(
        app,
        &[
            "Scope controls the search source constraint for session search turns.",
            "Usage:",
            "  /scope <constraint>",
            "  /scope clear",
            "Examples:",
            "  /scope only CVPR 2026 papers",
            "  /scope arXiv only, no blogs",
            "  /scope local folder ./papers",
            "  /scope CVPR 2026, ICLR 2026, ICML 2026",
            "If unset, search agents infer constraints from the prompt.",
        ],
    );
}

fn reset_tui_session(root: &Path, app: &mut TuiApp) -> Result<(), String> {
    let session_id = make_session_id();
    init_session_workspace(root, &session_id)?;
    app.input.clear();
    app.input_cursor = 0;
    app.session_id = session_id;
    app.run_id.clear();
    app.active_stage.clear();
    app.stages.clear();
    app.completed_stages.clear();
    app.log.clear();
    app.log_scroll = 0;
    app.stage_scroll = 0;
    app.artifacts.clear();
    app.artifact_scroll = 0;
    app.running = false;
    app.planning_mode = false;
    app.run_started_at = None;
    app.last_run_elapsed = None;
    app.completion_index = 0;
    clear_input_tool_view(app);
    app.focus_pane = FocusPane::Conversation;
    app.run_control = RunControl::default();
    app.status = "Started a fresh research session. Type a research instruction.".into();
    Ok(())
}

fn show_resume_picker(root: &Path, app: &mut TuiApp) {
    if app.running {
        app.log
            .push("system: Cannot resume while a session turn is active.".into());
        return;
    }
    app.resume_options = list_session_summaries(root);
    app.resume_index = 0;
    app.input_tool_view = InputToolView::Resume;
    app.input.clear();
    app.input_cursor = 0;
    if app.resume_options.is_empty() {
        clear_input_tool_view(app);
        app.log.push("system: No sessions found.".into());
        app.status = "No sessions found".into();
    } else if let Some(selected) = app.resume_options.first() {
        app.status = format!("Selected {}", selected.session_id);
    }
}

fn resume_picker_active(app: &TuiApp) -> bool {
    app.input_tool_view == InputToolView::Resume && !app.resume_options.is_empty()
}

fn clear_input_tool_view(app: &mut TuiApp) {
    app.input_tool_view = InputToolView::None;
    app.resume_options.clear();
    app.resume_index = 0;
    app.plan_question = None;
    app.plan_question_turn_id = None;
    app.plan_choice_index = 0;
    // Never strand a sidecar that is blocked on a checkpoint answer: if one is
    // still pending when the tool view is cleared (Backspace, cancel, return to
    // prompt), resolve it with the default (recommended, first) option.
    if let Some(responder) = app.checkpoint_responder.take() {
        let _ = responder.send(0);
    }
    app.checkpoint = None;
    app.checkpoint_choice_index = 0;
}

fn return_to_prompt(app: &mut TuiApp) {
    clear_input_tool_view(app);
    app.input.clear();
    app.input_cursor = 0;
    app.completion_index = 0;
    app.planning_mode = false;
    app.status = "Returned to prompt".into();
}

fn toggle_planning_mode(app: &mut TuiApp) {
    app.planning_mode = !app.planning_mode;
    app.status = if app.planning_mode {
        "Plan mode enabled".into()
    } else {
        "Discussion mode enabled".into()
    };
}

fn move_resume_selection(app: &mut TuiApp, delta: isize) {
    if app.resume_options.is_empty() {
        return;
    }
    let len = app.resume_options.len() as isize;
    let next = (app.resume_index as isize + delta).rem_euclid(len);
    app.resume_index = next as usize;
    if let Some(selected) = app.resume_options.get(app.resume_index) {
        app.status = format!("Selected {}", selected.session_id);
    }
}

fn move_plan_choice(app: &mut TuiApp, delta: isize) {
    let Some(question) = &app.plan_question else {
        return;
    };
    if question.options.is_empty() {
        return;
    }
    let len = question.options.len() as isize;
    app.plan_choice_index = (app.plan_choice_index as isize + delta).rem_euclid(len) as usize;
}

fn move_checkpoint_choice(app: &mut TuiApp, delta: isize) {
    let Some(question) = &app.checkpoint else {
        return;
    };
    if question.options.is_empty() {
        return;
    }
    let len = question.options.len() as isize;
    app.checkpoint_choice_index =
        (app.checkpoint_choice_index as isize + delta).rem_euclid(len) as usize;
}

/// Send the selected option index back to the sidecar (unblocking the stage that
/// is waiting on the answer), log the choice, and clear the checkpoint UI.
fn submit_checkpoint_choice(app: &mut TuiApp) {
    let Some(question) = app.checkpoint.clone() else {
        return;
    };
    let idx = app
        .checkpoint_choice_index
        .min(question.options.len().saturating_sub(1));
    if let Some(label) = question.options.get(idx) {
        app.log.push(format!("user: {label}"));
        app.status = format!("Checkpoint: chose \"{label}\"");
    }
    if let Some(responder) = app.checkpoint_responder.take() {
        let _ = responder.send(idx);
    }
    app.checkpoint = None;
    app.checkpoint_choice_index = 0;
    if app.input_tool_view == InputToolView::Checkpoint {
        app.input_tool_view = InputToolView::None;
    }
}

/// Bind the per-workspace checkpoint socket and serve checkpoint questions from
/// MCP sidecars: read a request, surface it in the TUI via `tx`, block for the
/// user's chosen index, and reply. Best-effort — a bind failure is ignored
/// (checkpoint_ask then errors in the sidecar).
fn spawn_checkpoint_server(root: PathBuf, tx: Sender<UiMsg>) {
    let path = checkpoint_ipc::socket_path(&root);
    let _ = fs::remove_file(&path); // clear any stale socket
    let listener = match std::os::unix::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(_) => return,
    };
    thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(stream) = conn else { continue };
            // Checkpoints are sequential; handle one connection at a time.
            let _ = handle_checkpoint_conn(stream, &tx);
        }
    });
}

fn handle_checkpoint_conn(
    stream: std::os::unix::net::UnixStream,
    tx: &Sender<UiMsg>,
) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let req: checkpoint_ipc::CheckpointRequest = serde_json::from_str(line.trim())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    // Fold the assessment into the displayed question text.
    let question_text = match &req.assessment {
        Some(a) if !a.trim().is_empty() => format!("{}  —  {}", req.question, a),
        _ => req.question.clone(),
    };
    let pq = PlanQuestion {
        question: question_text,
        options: req.options.clone(),
        final_confirmation: false,
    };
    let (resp_tx, resp_rx) = mpsc::channel::<usize>();
    if tx.send(UiMsg::Checkpoint(pq, resp_tx)).is_err() {
        return Ok(()); // TUI is gone
    }
    let chosen = resp_rx.recv().unwrap_or(0);
    let label = req.options.get(chosen).cloned().unwrap_or_default();
    let resp = checkpoint_ipc::CheckpointResponse { chosen, label };
    let mut out = serde_json::to_string(&resp)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    out.push('\n');
    let mut w = stream;
    w.write_all(out.as_bytes())?;
    w.flush()?;
    Ok(())
}

fn submit_plan_choice(root: &Path, app: &mut TuiApp, tx: &Sender<UiMsg>) -> Result<(), String> {
    let Some(question) = app.plan_question.clone() else {
        return Ok(());
    };
    let Some(answer) = question.options.get(app.plan_choice_index).cloned() else {
        return Ok(());
    };
    let plan_turn_id = app.plan_question_turn_id.clone();
    app.log.push(format!("user: {answer}"));
    if question.final_confirmation && answer_approves_plan(&answer) {
        let Some(plan_turn_id) = plan_turn_id else {
            app.log
                .push("system: Cannot proceed because the planner turn id is missing.".into());
            clear_input_tool_view(app);
            return Ok(());
        };
        clear_input_tool_view(app);
        app.planning_mode = false;
        return start_approved_plan_execution(root, app, tx, plan_turn_id, answer);
    }
    let instruction = if question.final_confirmation {
        format!(
            "Planner final confirmation response\n\nQuestion: {}\n\nSelected answer: {}\n\nIf the selected answer approves proceeding, finalize the plan and do not ask more planning questions. If the selected answer requests more planning, continue planning only on that requested issue.",
            question.question, answer
        )
    } else {
        format!(
            "Planner multiple-choice response\n\nQuestion: {}\n\nSelected answer: {}\n\nUse this answer to continue planning. Ask another question only if a blocking ambiguity remains; otherwise move to final confirmation.",
            question.question, answer
        )
    };
    clear_input_tool_view(app);
    app.planning_mode = true;
    start_session_turn(root, app, tx, instruction, SessionTurnKind::Plan)
}

fn answer_approves_plan(answer: &str) -> bool {
    let lower = answer.to_ascii_lowercase();
    lower.contains("proceed")
        || lower.contains("approve")
        || lower.contains("execute")
        || answer.contains("진행")
        || answer.contains("실행")
        || answer.contains("승인")
}

fn plan_confirmation_approves(instruction: &str) -> bool {
    if !instruction.contains("Planner final confirmation response") {
        return false;
    }
    let Some(answer) = extract_selected_answer(instruction) else {
        return false;
    };
    answer_approves_plan(answer)
}

fn extract_selected_answer(instruction: &str) -> Option<&str> {
    instruction
        .lines()
        .find_map(|line| line.strip_prefix("Selected answer:").map(str::trim))
}

fn resume_selected_run(root: &Path, app: &mut TuiApp, _tx: &Sender<UiMsg>) {
    let Some(session_id) = app
        .resume_options
        .get(app.resume_index)
        .map(|session| session.session_id.clone())
    else {
        return;
    };
    start_resume_session(root, app, &session_id);
}

fn start_resume_session(root: &Path, app: &mut TuiApp, session_id: &str) {
    app.input.clear();
    app.input_cursor = 0;
    app.log.clear();
    app.log_scroll = 0;
    app.artifacts.clear();
    app.stage_scroll = 0;
    app.artifact_scroll = 0;
    app.stages.clear();
    app.completed_stages.clear();
    clear_input_tool_view(app);
    app.running = false;
    app.run_started_at = None;
    app.last_run_elapsed = None;
    app.run_control = RunControl::default();
    app.run_id.clear();
    if let Err(err) = init_session_workspace(root, session_id) {
        app.status = format!("Failed to adopt session: {err}");
        app.log.push(format!(
            "system: Failed to resume session {session_id}: {err}"
        ));
        return;
    }
    app.session_id = session_id.to_string();
    app.artifacts = list_session_artifacts(root, &app.session_id);
    app.log = load_session_conversation(root, &app.session_id);
    app.log_scroll = 0;
    app.log
        .push(format!("system: Resumed session {}", app.session_id));
    app.status = format!("Resumed session {}", app.session_id);
}

#[allow(dead_code)]
fn session_wiki_is_empty(root: &Path, session_id: &str) -> bool {
    let wiki = root.join("sessions").join(session_id).join("wiki");
    for subdir in [
        "sources",
        "topics",
        "concepts",
        "entities",
        "methods",
        "datasets",
        "comparisons",
        "synthesis",
        "outputs",
    ] {
        if has_non_metadata_file(&wiki.join(subdir)) {
            return false;
        }
    }
    for file in ["index.md", "followups.md", "log.md"] {
        let path = wiki.join(file);
        if fs::read_to_string(&path)
            .map(|contents| {
                contents
                    .lines()
                    .filter(|line| {
                        let trimmed = line.trim();
                        !trimmed.is_empty() && !trimmed.starts_with('#')
                    })
                    .count()
                    > 0
            })
            .unwrap_or(false)
        {
            return false;
        }
    }
    true
}

#[allow(dead_code)]
fn has_non_metadata_file(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if has_non_metadata_file(&path) {
                return true;
            }
        } else if !is_ignored_metadata_path(&path) {
            return true;
        }
    }
    false
}

fn request_run_cancel(app: &mut TuiApp) {
    app.run_control
        .cancel_requested
        .store(true, Ordering::SeqCst);
    let pid = app
        .run_control
        .current_child_pid
        .lock()
        .ok()
        .and_then(|guard| *guard);
    if let Some(pid) = pid {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status();
        app.status = format!("Cancelling current agent process {pid}...");
        app.log.push(format!(
            "system: Cancellation requested; sent TERM to process {pid}."
        ));
    } else {
        app.status = "Cancelling run after current step...".into();
        app.log
            .push("system: Cancellation requested; no active child process yet.".into());
    }
}

fn focused_block(app: &TuiApp, pane: FocusPane, title: String) -> Block<'static> {
    let style = if app.focus_pane == pane {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(style)
        .title(title);
    if app.focus_pane == pane {
        let hint = if pane == FocusPane::Conversation {
            " scroll: ↑/↓  page: ctrl-u/d  expand: ctrl-o "
        } else {
            " scroll: ↑/↓  page: ctrl-u/d "
        };
        block = block.title_bottom(
            Line::from(vec![Span::styled(
                hint,
                Style::default().fg(Color::DarkGray),
            )])
            .right_aligned(),
        );
    }
    block
}

fn pane_title(name: &str, scroll: usize) -> String {
    if scroll == 0 {
        format!(" {name} ")
    } else {
        format!(" {name}  -{scroll} ")
    }
}

fn render_scrollbar(
    frame: &mut ratatui::Frame<'_>,
    app: &TuiApp,
    pane: FocusPane,
    area: Rect,
    content_len: usize,
    viewport_len: usize,
    scroll_from_bottom: usize,
    reversed: bool,
) {
    if app.focus_pane != pane || content_len <= viewport_len.max(1) {
        return;
    }

    let max_scroll = content_len.saturating_sub(viewport_len.max(1));
    let clamped_scroll = scroll_from_bottom.min(max_scroll);
    let position_from_top = if reversed {
        max_scroll.saturating_sub(clamped_scroll)
    } else {
        clamped_scroll
    };
    let mut state = ScrollbarState::new(content_len)
        .viewport_content_length(viewport_len.max(1))
        .position(position_from_top);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_style(Style::default().fg(Color::Cyan))
        .track_style(Style::default().fg(Color::DarkGray));
    frame.render_stateful_widget(scrollbar, area, &mut state);
}

fn draw_tui(frame: &mut ratatui::Frame<'_>, app: &TuiApp) {
    let input_height = if matches!(
        app.input_tool_view,
        InputToolView::Resume
            | InputToolView::PlanQuestion
            | InputToolView::Checkpoint
            | InputToolView::Artifacts
    ) {
        16
    } else if slash_completion_open(app) {
        12
    } else {
        5
    };
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(input_height),
        ])
        .split(frame.area());

    let stage_count = app.stages.len();
    let done_count = app.completed_stages.len().min(stage_count);
    let elapsed = run_elapsed_label(app);
    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            "research-os",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            if app.running { "RUNNING" } else { "READY" },
            Style::default()
                .fg(if app.running {
                    Color::Yellow
                } else {
                    Color::Green
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            if app.run_id.is_empty() {
                &app.session_id
            } else {
                &app.run_id
            },
            Style::default().fg(Color::Gray),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{done_count}/{stage_count} stages"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  "),
        Span::styled(
            elapsed.as_deref().unwrap_or("elapsed:--"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  "),
        Span::styled(
            if app.source_scope.is_empty() {
                "scope:any"
            } else {
                "scope:set"
            },
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  "),
        Span::styled(&app.status, Style::default().fg(Color::Yellow)),
    ])])
    .alignment(Alignment::Left);
    frame.render_widget(
        header.block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_type(BorderType::Plain),
        ),
        root[0],
    );

    let body = if app.sidebar_visible {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(38), Constraint::Min(60)])
            .split(root[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(0), Constraint::Min(1)])
            .split(root[1])
    };

    if app.sidebar_visible {
        let side = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),
                Constraint::Percentage(55),
                Constraint::Percentage(45),
            ])
            .split(body[0]);
        let side_text_width = side[0].width.saturating_sub(10) as usize;

        frame.render_widget(
            Paragraph::new(build_run_panel(
                app,
                done_count,
                stage_count,
                side_text_width,
            ))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Plain)
                    .title(" run "),
            ),
            side[0],
        );

        let stage_visible = side[1].height.saturating_sub(2) as usize;
        let stage_items = if app.stages.is_empty() {
            vec![
                ListItem::new(Line::from(vec![Span::styled(
                    truncate_label("No plan yet", side[1].width.saturating_sub(2) as usize),
                    Style::default().fg(Color::DarkGray),
                )])),
                ListItem::new(Line::from(vec![Span::styled(
                    truncate_label(
                        "Planner will populate this.",
                        side[1].width.saturating_sub(2) as usize,
                    ),
                    Style::default().fg(Color::DarkGray),
                )])),
            ]
        } else {
            let max_scroll = app.stages.len().saturating_sub(stage_visible.max(1));
            let scroll = app.stage_scroll.min(max_scroll);
            app.stages
                .iter()
                .enumerate()
                .skip(scroll)
                .take(stage_visible.max(1))
                .map(|(idx, stage)| {
                    let is_done = app.completed_stages.iter().any(|s| s == stage);
                    let is_active = *stage == app.active_stage;
                    let marker = if is_done {
                        "ok"
                    } else if is_active {
                        ">>"
                    } else {
                        "--"
                    };
                    let color = if is_done {
                        Color::Green
                    } else if is_active {
                        Color::Cyan
                    } else {
                        Color::DarkGray
                    };
                    let label_width = side[1].width.saturating_sub(11) as usize;
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{marker}  "), Style::default().fg(color)),
                        Span::styled(
                            format!("{:02} ", idx + 1),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(
                            truncate_label(&format_stage_label(stage), label_width),
                            Style::default().fg(color).add_modifier(if is_active {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                        ),
                    ]))
                })
                .collect()
        };
        frame.render_widget(
            List::new(stage_items).block(focused_block(
                app,
                FocusPane::Pipeline,
                pane_title("pipeline", app.stage_scroll),
            )),
            side[1],
        );
        render_scrollbar(
            frame,
            app,
            FocusPane::Pipeline,
            side[1],
            app.stages.len(),
            stage_visible,
            app.stage_scroll,
            false,
        );

        let artifact_visible = side[2].height.saturating_sub(2) as usize;
        let artifact_items = if app.artifacts.is_empty() {
            vec![
                ListItem::new(Line::from(vec![Span::styled(
                    truncate_label("No artifacts yet", side[2].width.saturating_sub(2) as usize),
                    Style::default().fg(Color::DarkGray),
                )])),
                ListItem::new(Line::from(vec![Span::styled(
                    truncate_label(
                        "Use /open <path> later.",
                        side[2].width.saturating_sub(2) as usize,
                    ),
                    Style::default().fg(Color::DarkGray),
                )])),
            ]
        } else {
            app.artifacts
                .iter()
                .rev()
                .skip(app.artifact_scroll)
                .take(artifact_visible.max(1))
                .map(|a| {
                    let color = artifact_color(a);
                    let label_width = side[2].width.saturating_sub(8) as usize;
                    ListItem::new(Line::from(vec![
                        Span::styled(artifact_icon(a), Style::default().fg(color)),
                        Span::raw(" "),
                        Span::styled(
                            format_artifact_label(a, label_width),
                            Style::default().fg(color),
                        ),
                    ]))
                })
                .collect()
        };
        frame.render_widget(
            List::new(artifact_items).block(focused_block(
                app,
                FocusPane::Artifacts,
                pane_title("artifacts", app.artifact_scroll),
            )),
            side[2],
        );
        render_scrollbar(
            frame,
            app,
            FocusPane::Artifacts,
            side[2],
            app.artifacts.len(),
            artifact_visible,
            app.artifact_scroll,
            false,
        );
    }

    let chat_height = body[1].height.saturating_sub(2) as usize;
    let chat_width = body[1].width.saturating_sub(4) as usize;
    let all_chat_lines = build_chat_lines(app, chat_width);
    let chat_content_len = all_chat_lines.len();
    let chat_lines = visible_chat_lines(all_chat_lines, chat_height, app.log_scroll);
    let scroll_title = pane_title("conversation", app.log_scroll);
    frame.render_widget(
        Paragraph::new(chat_lines).block(focused_block(app, FocusPane::Conversation, scroll_title)),
        body[1],
    );
    render_scrollbar(
        frame,
        app,
        FocusPane::Conversation,
        body[1],
        chat_content_len,
        chat_height,
        app.log_scroll,
        true,
    );

    let mut input_lines = Vec::new();
    if app.running {
        input_lines.extend(build_planning_status_lines(
            app,
            root[2].height,
            root[2].width as usize,
            true,
        ));
    } else if resume_picker_active(app) {
        input_lines.extend(build_resume_picker_lines(
            app,
            root[2].height,
            root[2].width as usize,
        ));
    } else if app.input_tool_view == InputToolView::PlanQuestion {
        input_lines.extend(build_plan_question_lines(
            app,
            root[2].height,
            root[2].width as usize,
        ));
    } else if app.input_tool_view == InputToolView::Checkpoint {
        input_lines.extend(build_checkpoint_lines(
            app,
            root[2].height,
            root[2].width as usize,
        ));
    } else if app.input_tool_view == InputToolView::Artifacts {
        input_lines.extend(build_artifacts_tool_lines(
            app,
            root[2].height,
            root[2].width as usize,
        ));
    } else {
        input_lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(app.input.as_str()),
        ]));
        if slash_completion_open(app) {
            input_lines.extend(build_slash_completion_lines(app, root[2].height));
        } else {
            input_lines.push(build_input_hint_line(app));
        }
    }
    frame.render_widget(
        Paragraph::new(input_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain),
        ),
        root[2],
    );
    if !app.running && app.input_tool_view == InputToolView::None {
        let cursor_width = input_prefix_width(&app.input, app.input_cursor);
        let x = root[2]
            .x
            .saturating_add(3)
            .saturating_add(cursor_width as u16)
            .min(root[2].right().saturating_sub(2));
        frame.set_cursor_position(Position::new(x, root[2].y.saturating_add(1)));
    }
}

fn build_chat_lines(app: &TuiApp, width: usize) -> Vec<Line<'static>> {
    if app.log.is_empty() {
        return vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "research-os is idle",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![Span::styled(
                "Type a research instruction below.",
                Style::default().fg(Color::Gray),
            )]),
            Line::from(vec![Span::styled(
                "Example: Please research recent semiconductor HBM research trends",
                Style::default().fg(Color::DarkGray),
            )]),
        ];
    }

    let width = width.max(20);
    let log_lines = if app.expand_conversation {
        app.log.clone()
    } else {
        compact_log_lines_by_phase(&app.log, width)
    };
    let mut rendered = Vec::new();
    for line in &log_lines {
        let display_line = if app.expand_conversation {
            conversation_display_line(line, width, true)
        } else {
            line.to_string()
        };
        if display_line.starts_with("user:") {
            push_wrapped_chat_entry(
                &mut rendered,
                "You",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
                display_line.trim_start_matches("user:").trim(),
                Style::default().fg(Color::Reset),
                width,
            );
        } else if display_line.starts_with("stage:") {
            push_wrapped_full_line(
                &mut rendered,
                &format!("-- {} --", display_line.trim_start_matches("stage:").trim()),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                width,
            );
        } else if display_line.starts_with("error:") {
            push_wrapped_full_line(
                &mut rendered,
                &display_line,
                Style::default().fg(Color::Red),
                width,
            );
        } else if display_line.starts_with("thinking:") {
            push_wrapped_chat_entry(
                &mut rendered,
                "Thinking",
                Style::default().fg(Color::DarkGray),
                display_line.trim_start_matches("thinking:").trim(),
                Style::default().fg(Color::DarkGray),
                width,
            );
        } else if display_line.starts_with("plan:title:") {
            push_wrapped_full_line(
                &mut rendered,
                display_line.trim_start_matches("plan:title:").trim(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                width,
            );
        } else if display_line.starts_with("plan:field:") {
            push_wrapped_full_line(
                &mut rendered,
                display_line.trim_start_matches("plan:field:").trim(),
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
                width,
            );
        } else if display_line.starts_with("plan:section:") {
            push_wrapped_full_line(
                &mut rendered,
                display_line.trim_start_matches("plan:section:").trim(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                width,
            );
        } else if display_line.starts_with("plan:stage:") {
            push_wrapped_full_line(
                &mut rendered,
                display_line.trim_start_matches("plan:stage:").trim(),
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::BOLD),
                width,
            );
        } else if display_line.starts_with("plan:io:") {
            push_wrapped_full_line(
                &mut rendered,
                display_line.trim_start_matches("plan:io:").trim(),
                Style::default().fg(Color::DarkGray),
                width,
            );
        } else if display_line.starts_with("answer:") {
            push_wrapped_chat_entry(
                &mut rendered,
                "Answer",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
                display_line.trim_start_matches("answer:").trim(),
                Style::default().fg(Color::Reset),
                width,
            );
        } else if display_line.starts_with("system:") {
            push_wrapped_chat_entry(
                &mut rendered,
                "System",
                Style::default().fg(Color::DarkGray),
                display_line.trim_start_matches("system:").trim(),
                Style::default().fg(Color::Gray),
                width,
            );
        } else {
            push_wrapped_chat_entry(
                &mut rendered,
                "Agent",
                Style::default().fg(Color::Blue),
                &display_line,
                Style::default().fg(Color::Reset),
                width,
            );
        }
    }
    rendered
}

fn compact_log_lines_by_phase(log: &[String], width: usize) -> Vec<String> {
    let mut compacted = Vec::new();
    let mut phase_last: Option<String> = None;
    let mut phase_line_count = 0usize;
    let mut in_phase = false;

    for line in log {
        if line.starts_with("stage:") {
            flush_compact_phase(&mut compacted, phase_last.take(), phase_line_count, width);
            phase_line_count = 0;
            compacted.push(line.clone());
            in_phase = true;
        } else if line.starts_with("user:") {
            flush_compact_phase(&mut compacted, phase_last.take(), phase_line_count, width);
            phase_line_count = 0;
            compacted.push(conversation_display_line(line, width, false));
            in_phase = false;
        } else if is_plan_render_line(line) {
            flush_compact_phase(&mut compacted, phase_last.take(), phase_line_count, width);
            phase_line_count = 0;
            compacted.push(line.clone());
        } else if in_phase {
            if !line.trim().is_empty() {
                phase_last = Some(line.clone());
                phase_line_count += 1;
            }
        } else {
            compacted.push(conversation_display_line(line, width, false));
        }
    }

    flush_compact_phase(&mut compacted, phase_last.take(), phase_line_count, width);
    compacted
}

fn is_plan_render_line(line: &str) -> bool {
    line.starts_with("plan:title:")
        || line.starts_with("plan:field:")
        || line.starts_with("plan:section:")
        || line.starts_with("plan:stage:")
        || line.starts_with("plan:io:")
}

fn flush_compact_phase(
    out: &mut Vec<String>,
    phase_last: Option<String>,
    phase_line_count: usize,
    width: usize,
) {
    let Some(line) = phase_last else {
        return;
    };
    if phase_line_count <= 1 {
        out.push(conversation_display_line(&line, width, false));
    } else {
        out.push(force_collapsed_line(&line, width));
    }
}

fn run_elapsed_label(app: &TuiApp) -> Option<String> {
    let elapsed = if app.running {
        app.run_started_at.map(|started_at| started_at.elapsed())
    } else {
        app.last_run_elapsed
    }?;
    Some(format!("elapsed:{}", format_elapsed(elapsed)))
}

fn format_elapsed(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn conversation_display_line(line: &str, width: usize, expanded: bool) -> String {
    if expanded || is_never_collapsed_line(line) {
        return line.to_string();
    }
    let max_chars = collapsed_line_width(line, width);
    let logical_lines: Vec<&str> = line
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if logical_lines.len() > 1 {
        let last = logical_lines.last().copied().unwrap_or(line).trim();
        prefix_ellipsis(last, max_chars)
    } else if line.chars().count() <= max_chars {
        line.to_string()
    } else {
        prefix_ellipsis(line, max_chars)
    }
}

fn is_never_collapsed_line(line: &str) -> bool {
    line.starts_with("stage:")
        || is_plan_render_line(line)
        || line.starts_with("answer:")
        || line.starts_with("error:")
        || line.starts_with("system: final artifact:")
        || line.starts_with("wrote ")
}

fn collapsed_line_width(line: &str, width: usize) -> usize {
    if line.starts_with("user:") {
        return width.saturating_sub(8).max(80).min(180);
    }
    if line.starts_with("system:") {
        return width.saturating_sub(10).max(80).min(150);
    }
    if line.starts_with("thinking:") {
        return width.saturating_sub(12).max(60).min(120);
    }
    if is_plan_render_line(line) {
        return width.saturating_sub(4).max(100).min(220);
    }
    if line.starts_with("Agent ") || line.starts_with("Agent:") {
        return width.saturating_sub(10).max(80).min(140);
    }
    width.saturating_sub(10).max(80).min(140)
}

fn force_collapsed_line(line: &str, width: usize) -> String {
    if let Some((prefix, value)) = split_log_prefix(line) {
        let max_chars = collapsed_line_width(line, width)
            .saturating_sub(prefix.chars().count() + 1)
            .max(8);
        return format!(
            "{prefix} {}",
            prefix_ellipsis(last_meaningful_line(value), max_chars)
        );
    }
    prefix_ellipsis(
        last_meaningful_line(line),
        collapsed_line_width(line, width),
    )
}

fn split_log_prefix(line: &str) -> Option<(&'static str, &str)> {
    for prefix in [
        "user:",
        "system:",
        "thinking:",
        "answer:",
        "error:",
        "plan:title:",
        "plan:field:",
        "plan:section:",
        "plan:stage:",
        "plan:io:",
    ] {
        if let Some(value) = line.strip_prefix(prefix) {
            return Some((prefix, value.trim()));
        }
    }
    None
}

fn last_meaningful_line(value: &str) -> &str {
    value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .last()
        .unwrap_or(value)
        .trim()
}

fn prefix_ellipsis(value: &str, max_width: usize) -> String {
    if max_width <= 4 {
        return "...".chars().take(max_width).collect();
    }
    let value = value.trim();
    if value.chars().count() + 4 <= max_width {
        return format!("... {value}");
    }
    let keep = max_width - 4;
    let tail_len = value.chars().count().saturating_sub(keep);
    let tail: String = value.chars().skip(tail_len).collect();
    format!("... {tail}")
}

fn visible_chat_lines(
    lines: Vec<Line<'static>>,
    height: usize,
    scroll_from_bottom: usize,
) -> Vec<Line<'static>> {
    let visible = height.max(1);
    let max_scroll = lines.len().saturating_sub(visible);
    let scroll = scroll_from_bottom.min(max_scroll);
    let end = lines.len().saturating_sub(scroll);
    let start = end.saturating_sub(visible);
    lines.into_iter().skip(start).take(end - start).collect()
}

fn push_wrapped_chat_entry(
    out: &mut Vec<Line<'static>>,
    label: &'static str,
    label_style: Style,
    text: &str,
    text_style: Style,
    width: usize,
) {
    let prefix_width = label.chars().count() + 2;
    let text_width = width.saturating_sub(prefix_width).max(8);
    let chunks = wrap_plain_text(text, text_width);
    if chunks.is_empty() {
        out.push(Line::from(vec![
            Span::styled(label, label_style),
            Span::raw("  "),
        ]));
        return;
    }

    for (idx, chunk) in chunks.into_iter().enumerate() {
        if idx == 0 {
            out.push(Line::from(vec![
                Span::styled(label, label_style),
                Span::raw("  "),
                Span::styled(chunk, text_style),
            ]));
        } else {
            out.push(Line::from(vec![
                Span::raw(" ".repeat(prefix_width)),
                Span::styled(chunk, text_style),
            ]));
        }
    }
}

fn push_wrapped_full_line(out: &mut Vec<Line<'static>>, text: &str, style: Style, width: usize) {
    for chunk in wrap_plain_text(text, width.max(8)) {
        out.push(Line::from(vec![Span::styled(chunk, style)]));
    }
}

fn wrap_plain_text(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let width = width.max(1);
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            let current_len = current.chars().count();
            let word_len = word.chars().count();
            if current_len == 0 {
                if word_len <= width {
                    current.push_str(word);
                } else {
                    push_hard_wrapped_word(&mut lines, word, width);
                }
            } else if current_len + 1 + word_len <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current);
                current = String::new();
                if word_len <= width {
                    current.push_str(word);
                } else {
                    push_hard_wrapped_word(&mut lines, word, width);
                }
            }
        }
        if !current.is_empty() {
            lines.push(current);
        } else if raw_line.trim().is_empty() {
            lines.push(String::new());
        }
    }
    lines
}

fn push_hard_wrapped_word(lines: &mut Vec<String>, word: &str, width: usize) {
    let mut current = String::new();
    for ch in word.chars() {
        current.push(ch);
        if current.chars().count() >= width {
            lines.push(current);
            current = String::new();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
}

fn build_run_panel(
    app: &TuiApp,
    done_count: usize,
    stage_count: usize,
    value_width: usize,
) -> Vec<Line<'_>> {
    let progress = if stage_count == 0 {
        "idle".to_string()
    } else {
        format!("{done_count}/{stage_count}")
    };
    let active = if app.active_stage.is_empty() {
        "none".to_string()
    } else {
        truncate_label(&format_stage_label(&app.active_stage), value_width)
    };
    let scope = if app.source_scope.is_empty() {
        "any".to_string()
    } else {
        truncate_label(&app.source_scope, value_width)
    };
    vec![
        Line::from(vec![
            Span::styled("id     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                if app.run_id.is_empty() {
                    truncate_label(&app.session_id, value_width)
                } else {
                    truncate_label(&app.run_id, value_width)
                },
                Style::default().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("active ", Style::default().fg(Color::DarkGray)),
            Span::styled(active, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("scope  ", Style::default().fg(Color::DarkGray)),
            Span::styled(scope, Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("done   ", Style::default().fg(Color::DarkGray)),
            Span::styled(progress, Style::default().fg(Color::Green)),
        ]),
    ]
}

fn trim_skill_name(stage: &str) -> &str {
    stage.strip_prefix("research-os-").unwrap_or(stage)
}

fn slash_matches(input: &str) -> Vec<(&'static str, &'static str)> {
    if !input.starts_with('/') {
        return Vec::new();
    }
    let prefix = input.split_whitespace().next().unwrap_or(input);
    SLASH_COMMANDS
        .iter()
        .copied()
        .filter(|(cmd, _)| cmd.starts_with(prefix))
        .collect()
}

fn clamp_completion_index(app: &mut TuiApp) {
    let count = slash_matches(&app.input).len();
    if count == 0 {
        app.completion_index = 0;
    } else if app.completion_index >= count {
        app.completion_index = count - 1;
    }
}

fn slash_completion_open(app: &TuiApp) -> bool {
    !app.running
        && app.input_tool_view == InputToolView::None
        && app.input.starts_with('/')
        && !app.input.chars().any(char::is_whitespace)
}

fn pane_page_amount(terminal: &Terminal<CrosstermBackend<io::Stdout>>) -> usize {
    terminal
        .size()
        .map(|area| area.height.saturating_sub(10).max(1) as usize)
        .unwrap_or(10)
}

fn scroll_focused_pane_up(app: &mut TuiApp, amount: usize) {
    match app.focus_pane {
        FocusPane::Conversation => {
            app.log_scroll = app.log_scroll.saturating_add(amount);
        }
        FocusPane::Pipeline => app.stage_scroll = app.stage_scroll.saturating_sub(amount),
        FocusPane::Artifacts => app.artifact_scroll = app.artifact_scroll.saturating_sub(amount),
    }
}

fn scroll_focused_pane_down(app: &mut TuiApp, amount: usize) {
    match app.focus_pane {
        FocusPane::Conversation => app.log_scroll = app.log_scroll.saturating_sub(amount),
        FocusPane::Pipeline => {
            app.stage_scroll = app.stage_scroll.saturating_add(amount);
            clamp_stage_scroll(app);
        }
        FocusPane::Artifacts => {
            app.artifact_scroll = app.artifact_scroll.saturating_add(amount);
            clamp_artifact_scroll(app);
        }
    }
}

fn clamp_log_scroll(app: &mut TuiApp, visible_lines: Option<usize>) {
    let visible = visible_lines.unwrap_or(1).max(1);
    let max_scroll = app.log.len().saturating_mul(20).saturating_sub(visible);
    app.log_scroll = app.log_scroll.min(max_scroll);
}

fn clamp_stage_scroll(app: &mut TuiApp) {
    app.stage_scroll = app.stage_scroll.min(app.stages.len().saturating_sub(1));
}

fn clamp_artifact_scroll(app: &mut TuiApp) {
    app.artifact_scroll = app
        .artifact_scroll
        .min(app.artifacts.len().saturating_sub(1));
}

fn complete_slash_command(app: &mut TuiApp) {
    let matches = slash_matches(&app.input);
    if matches.is_empty() {
        return;
    }
    let selected = matches
        .get(app.completion_index)
        .copied()
        .unwrap_or(matches[0])
        .0;
    let rest = app
        .input
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest)
        .unwrap_or("");
    app.input = if rest.is_empty() {
        format!("{selected} ")
    } else {
        format!("{selected} {rest}")
    };
    app.input_cursor = app.input.chars().count();
    app.completion_index = 0;
}

fn byte_index_for_char(s: &str, char_index: usize) -> usize {
    s.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}

fn insert_char_at_cursor(input: &mut String, cursor: &mut usize, ch: char) {
    let byte_idx = byte_index_for_char(input, *cursor);
    input.insert(byte_idx, ch);
    *cursor += 1;
}

fn insert_str_at_cursor(input: &mut String, cursor: &mut usize, text: &str) {
    let byte_idx = byte_index_for_char(input, *cursor);
    input.insert_str(byte_idx, text);
    *cursor += text.chars().count();
}

fn input_prefix_width(input: &str, cursor: usize) -> usize {
    let byte_idx = byte_index_for_char(input, cursor);
    UnicodeWidthStr::width(&input[..byte_idx])
}

fn delete_char_before_cursor(input: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let start = byte_index_for_char(input, *cursor - 1);
    let end = byte_index_for_char(input, *cursor);
    input.replace_range(start..end, "");
    *cursor -= 1;
}

fn delete_char_at_cursor(input: &mut String, cursor: usize) {
    if cursor >= input.chars().count() {
        return;
    }
    let start = byte_index_for_char(input, cursor);
    let end = byte_index_for_char(input, cursor + 1);
    input.replace_range(start..end, "");
}

fn build_input_hint_line(app: &TuiApp) -> Line<'static> {
    if resume_picker_active(app) && app.input.trim().is_empty() {
        return Line::from(vec![
            Span::styled("Up/Down", Style::default().fg(Color::Cyan)),
            Span::styled(" select  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::styled(" resume  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "type to cancel selection",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
    }
    if app.planning_mode {
        return Line::from(vec![Span::styled(
            "Plan mode: Enter sends to planner, Tab exits plan mode, Esc returns to prompt",
            Style::default().fg(Color::DarkGray),
        )]);
    }
    Line::from(vec![
        Span::styled(
            "Tab switches to plan mode; type / to call tools",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled("  •  agent: ", Style::default().fg(Color::DarkGray)),
        Span::styled(backend_label(app.backend), Style::default().fg(Color::Cyan)),
    ])
}

fn build_resume_picker_lines(
    app: &TuiApp,
    input_area_height: u16,
    input_area_width: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled("Resume session", Style::default().fg(Color::Cyan)),
        Span::styled(
            "  Up/Down to select, Enter to resume",
            Style::default().fg(Color::DarkGray),
        ),
    ])];
    let max_items = input_area_height.saturating_sub(3).max(2) as usize / 2;
    let max_items = max_items.max(1);
    let start = if app.resume_index >= max_items {
        app.resume_index + 1 - max_items
    } else {
        0
    };
    let text_width = input_area_width.saturating_sub(6).max(20);
    for (idx, run) in app
        .resume_options
        .iter()
        .enumerate()
        .skip(start)
        .take(max_items)
    {
        let selected = idx == app.resume_index;
        let marker_style = Style::default().fg(if selected {
            Color::Cyan
        } else {
            Color::DarkGray
        });
        let run_style = Style::default()
            .fg(if selected { Color::Black } else { Color::Gray })
            .bg(if selected { Color::Cyan } else { Color::Reset })
            .add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
        lines.push(Line::from(vec![
            Span::styled(if selected { "> " } else { "  " }, marker_style),
            Span::styled(run.session_id.clone(), run_style),
        ]));
        let detail = format!(
            "{}  [{} turns / {} stages]  {}",
            run.prompt, run.turn_count, run.stage_count, run.turn_type
        );
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(
                truncate_label(&detail, text_width),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    lines
}

fn build_checkpoint_lines(
    app: &TuiApp,
    input_area_height: u16,
    input_area_width: usize,
) -> Vec<Line<'static>> {
    let Some(question) = &app.checkpoint else {
        return vec![Line::from(vec![Span::styled(
            "No checkpoint is active.",
            Style::default().fg(Color::DarkGray),
        )])];
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Checkpoint",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  Up/Down to select, Enter to answer",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![Span::styled(
            truncate_label(
                &question.question,
                input_area_width.saturating_sub(4).max(20),
            ),
            Style::default().fg(Color::Gray),
        )]),
    ];
    let max_rows = input_area_height.saturating_sub(4).max(1) as usize;
    let label_width = input_area_width.saturating_sub(8).max(20);
    for (idx, option) in question.options.iter().take(max_rows).enumerate() {
        let selected = idx == app.checkpoint_choice_index;
        let color = if selected {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "[x] " } else { "[ ] " },
                Style::default().fg(color),
            ),
            Span::styled(
                truncate_label(option, label_width),
                Style::default().fg(if selected { Color::White } else { Color::Gray }),
            ),
        ]));
    }
    lines
}

fn build_plan_question_lines(
    app: &TuiApp,
    input_area_height: u16,
    input_area_width: usize,
) -> Vec<Line<'static>> {
    let Some(question) = &app.plan_question else {
        return vec![Line::from(vec![Span::styled(
            "No planner question is active.",
            Style::default().fg(Color::DarkGray),
        )])];
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Planner Question",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  Up/Down to select, Enter to answer, Esc to cancel",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![Span::styled(
            truncate_label(
                &question.question,
                input_area_width.saturating_sub(4).max(20),
            ),
            Style::default().fg(Color::Gray),
        )]),
    ];
    let max_rows = input_area_height.saturating_sub(4).max(1) as usize;
    let label_width = input_area_width.saturating_sub(8).max(20);
    for (idx, option) in question.options.iter().take(max_rows).enumerate() {
        let selected = idx == app.plan_choice_index;
        let color = if selected {
            Color::Cyan
        } else {
            Color::DarkGray
        };
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "[x] " } else { "[ ] " },
                Style::default().fg(color),
            ),
            Span::styled(
                truncate_label(option, label_width),
                Style::default().fg(if selected { Color::White } else { Color::Gray }),
            ),
        ]));
    }
    lines
}

fn build_planning_status_lines(
    app: &TuiApp,
    input_area_height: u16,
    input_area_width: usize,
    running_view: bool,
) -> Vec<Line<'static>> {
    let title = if running_view {
        "Updated Plan"
    } else {
        "Pipeline"
    };
    let hint = if running_view {
        "  Esc to interrupt, Ctrl-O expands logs"
    } else {
        "  Up/Down to scroll, Esc to close"
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(hint, Style::default().fg(Color::DarkGray)),
    ])];

    if app.stages.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "No plan loaded yet.",
            Style::default().fg(Color::DarkGray),
        )]));
        lines.extend(build_working_footer_lines(app, running_view));
        return lines;
    }

    let reserved_rows = if running_view { 6 } else { 5 };
    let max_rows = input_area_height.saturating_sub(reserved_rows).max(1) as usize;
    let max_scroll = app.stages.len().saturating_sub(max_rows);
    let scroll = app.stage_scroll.min(max_scroll);
    let label_width = input_area_width.saturating_sub(10).max(20);
    for (idx, stage) in app.stages.iter().enumerate().skip(scroll).take(max_rows) {
        let is_done = app.completed_stages.iter().any(|done| done == stage);
        let is_active = *stage == app.active_stage;
        let marker = if is_done {
            "[x]"
        } else if is_active {
            "[>]"
        } else {
            "[ ]"
        };
        let color = if is_done {
            Color::Green
        } else if is_active {
            Color::Cyan
        } else {
            Color::DarkGray
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), Style::default().fg(color)),
            Span::styled(
                format!("{} ", idx + 1),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                truncate_label(&format_stage_label(stage), label_width),
                Style::default().fg(color).add_modifier(if is_active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ),
        ]));
    }
    lines.extend(build_explored_lines(app, input_area_width));
    lines.extend(build_working_footer_lines(app, running_view));
    lines
}

fn build_explored_lines(app: &TuiApp, input_area_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if app.artifacts.is_empty() {
        return lines;
    }
    lines.push(Line::from(vec![Span::styled(
        "Explored",
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD),
    )]));
    let label_width = input_area_width.saturating_sub(6).max(20);
    for artifact in app.artifacts.iter().rev().take(2) {
        lines.push(Line::from(vec![
            Span::styled("  - ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate_label(&format_artifact_label(artifact, label_width), label_width),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    lines
}

fn build_working_footer_lines(app: &TuiApp, running_view: bool) -> Vec<Line<'static>> {
    if running_view {
        return vec![Line::from(vec![
            Span::styled("Working", Style::default().fg(Color::Yellow)),
            Span::raw("  "),
            Span::styled(
                run_elapsed_label(app).unwrap_or_else(|| "elapsed:00:00".into()),
                Style::default().fg(Color::Cyan),
            ),
        ])];
    }
    vec![Line::from(vec![
        Span::styled("Status", Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(app.status.clone(), Style::default().fg(Color::Yellow)),
    ])]
}

fn build_artifacts_tool_lines(
    app: &TuiApp,
    input_area_height: u16,
    input_area_width: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled("Artifacts", Style::default().fg(Color::Cyan)),
        Span::styled(
            "  Up/Down to scroll, Esc to close",
            Style::default().fg(Color::DarkGray),
        ),
    ])];
    if app.artifacts.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "No artifacts for the active session.",
            Style::default().fg(Color::DarkGray),
        )]));
        return lines;
    }

    let rows = artifact_tool_rows(app);
    let max_rows = input_area_height.saturating_sub(3).max(1) as usize;
    let max_scroll = rows.len().saturating_sub(max_rows);
    let scroll = app.artifact_scroll.min(max_scroll);
    let label_width = input_area_width.saturating_sub(8).max(20);
    for row in rows.into_iter().skip(scroll).take(max_rows) {
        match row {
            ArtifactToolRow::Stage(stage) => lines.push(Line::from(vec![Span::styled(
                stage,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )])),
            ArtifactToolRow::Artifact(path) => {
                let color = artifact_color(&path);
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(artifact_icon(&path), Style::default().fg(color)),
                    Span::raw(" "),
                    Span::styled(
                        format_artifact_label(&path, label_width),
                        Style::default().fg(color),
                    ),
                ]));
            }
        }
    }
    lines
}

enum ArtifactToolRow {
    Stage(String),
    Artifact(String),
}

fn artifact_tool_rows(app: &TuiApp) -> Vec<ArtifactToolRow> {
    let mut rows = Vec::new();
    let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
    for artifact in &app.artifacts {
        grouped
            .entry(artifact_stage_label(artifact))
            .or_default()
            .push(artifact.clone());
    }

    for stage in &app.stages {
        let label = trim_skill_name(stage).to_string();
        if let Some(mut artifacts) = grouped.remove(&label) {
            artifacts.sort();
            rows.push(ArtifactToolRow::Stage(label));
            rows.extend(artifacts.into_iter().map(ArtifactToolRow::Artifact));
        }
    }
    let mut remaining: Vec<_> = grouped.into_iter().collect();
    remaining.sort_by(|a, b| a.0.cmp(&b.0));
    for (label, mut artifacts) in remaining {
        artifacts.sort();
        rows.push(ArtifactToolRow::Stage(label));
        rows.extend(artifacts.into_iter().map(ArtifactToolRow::Artifact));
    }
    rows
}

fn artifact_stage_label(path: &str) -> String {
    if path.contains("/transcripts/") {
        if let Some(file) = path.rsplit('/').next() {
            return file
                .trim_end_matches(".jsonl")
                .trim_start_matches("research-os-")
                .to_string();
        }
    }
    if path.ends_with("plan.json") || path.ends_with("plan.md") {
        "planner".into()
    } else if path.contains("doc_search_manifest")
        || path.contains("/raw/sources/")
        || path.contains("/artifacts/discovery/")
        || path.contains("/artifacts/extracted/")
    {
        "doc-search".into()
    } else if path.contains("/artifacts/implementation/") {
        "coding".into()
    } else if path.contains("wiki_update_report") || path.contains("/wiki/sources/") {
        "wiki-update".into()
    } else if path.contains("/wiki/synthesis/") {
        "synthesis".into()
    } else if path.contains("lint_report") {
        "lint-critic".into()
    } else if path.contains("/wiki/outputs/") || path.ends_with("final_answer.md") {
        "output".into()
    } else if path.contains("/wiki/") {
        "wiki-update".into()
    } else {
        "session".into()
    }
}

fn build_slash_completion_lines(app: &TuiApp, input_area_height: u16) -> Vec<Line<'static>> {
    let matches = slash_matches(&app.input);
    if matches.is_empty() {
        return vec![Line::from(vec![Span::styled(
            "No matching tools",
            Style::default().fg(Color::DarkGray),
        )])];
    }

    let max_rows = input_area_height.saturating_sub(3).max(1) as usize;
    let start = if app.completion_index >= max_rows {
        app.completion_index + 1 - max_rows
    } else {
        0
    };
    matches
        .iter()
        .enumerate()
        .skip(start)
        .take(max_rows)
        .map(|(idx, (cmd, desc))| {
            let selected = idx == app.completion_index;
            let style = Style::default()
                .fg(if selected { Color::Black } else { Color::Gray })
                .bg(if selected { Color::Cyan } else { Color::Reset })
                .add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                });
            Line::from(vec![
                Span::styled(if selected { "> " } else { "  " }, style),
                Span::styled(format!("{cmd:<10}"), style),
                Span::styled(
                    format!(" {desc}"),
                    Style::default().fg(if selected {
                        Color::Black
                    } else {
                        Color::DarkGray
                    }),
                ),
            ])
        })
        .collect()
}

fn format_stage_label(stage: &str) -> String {
    let label = trim_skill_name(stage).replace('-', " ");
    if label.chars().count() > 25 {
        format!("{}...", label.chars().take(22).collect::<String>())
    } else {
        label
    }
}

fn format_artifact_label(path: &str, max_width: usize) -> String {
    let compact = path
        .strip_prefix("runs/")
        .or_else(|| path.strip_prefix("sessions/"))
        .or_else(|| path.strip_prefix("wiki/"))
        .unwrap_or(path);
    truncate_label(compact, max_width)
}

fn truncate_label(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let len = value.chars().count();
    if len <= max_width {
        return value.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }
    let keep = max_width - 3;
    let mut truncated: String = value.chars().take(keep).collect();
    truncated.push_str("...");
    truncated
}

fn artifact_icon(path: &str) -> &'static str {
    if path.ends_with("plan.json") || path.ends_with("plan.md") {
        "pl"
    } else if path.ends_with("final_answer.md") {
        "ans"
    } else if path.contains("/artifacts/implementation/") {
        "code"
    } else if path.contains("/transcripts/") {
        "log"
    } else if path.starts_with("wiki/") {
        "wiki"
    } else if path.ends_with(".json") {
        "js"
    } else if path.ends_with(".md") {
        "md"
    } else {
        "--"
    }
}

fn artifact_color(path: &str) -> Color {
    if path.ends_with("final_answer.md") {
        Color::Magenta
    } else if path.ends_with("plan.json") || path.ends_with("plan.md") {
        Color::Cyan
    } else if path.contains("/artifacts/implementation/") {
        Color::Yellow
    } else if path.starts_with("wiki/") {
        Color::Green
    } else {
        Color::Gray
    }
}

fn apply_tui_scope(instruction: &str, source_scope: &str) -> String {
    if source_scope.trim().is_empty() {
        instruction.to_string()
    } else {
        format!(
            "{instruction}\n\nUser-configured source scope constraint from TUI /scope: {}",
            source_scope.trim()
        )
    }
}

fn list_session_summaries(root: &Path) -> Vec<SessionSummary> {
    let mut sessions = Vec::new();
    let Ok(entries) = fs::read_dir(root.join("sessions")) else {
        return sessions;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                sessions.push(read_session_summary(root, name));
            }
        }
    }
    sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));
    sessions.reverse();
    sessions.truncate(12);
    sessions
}

fn read_session_summary(root: &Path, session_id: &str) -> SessionSummary {
    let turns_path = root.join("sessions").join(session_id).join("turns.jsonl");
    let turns = fs::read_to_string(&turns_path).unwrap_or_default();
    let turn_count = turns.lines().filter(|line| !line.trim().is_empty()).count();
    let last_turn = turns
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let prompt = extract_json_string_field(last_turn, "\"instruction\"")
        .unwrap_or_else(|| "(new session)".into())
        .replace('\n', " ");
    let turn_type =
        extract_json_string_field(last_turn, "\"turn_type\"").unwrap_or_else(|| "none".into());
    let stage_count = extract_json_string_array_field(last_turn, "\"stages\"").len();
    SessionSummary {
        session_id: session_id.to_string(),
        prompt,
        turn_type,
        stage_count,
        turn_count,
    }
}

fn load_session_conversation(root: &Path, session_id: &str) -> Vec<String> {
    let turns_path = root.join("sessions").join(session_id).join("turns.jsonl");
    let Ok(turns) = fs::read_to_string(&turns_path) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for turn in turns.lines().filter(|line| !line.trim().is_empty()) {
        let turn_id = extract_json_string_field(turn, "\"turn_id\"").unwrap_or_default();
        let turn_type =
            extract_json_string_field(turn, "\"turn_type\"").unwrap_or_else(|| "turn".into());
        let instruction = extract_json_string_field(turn, "\"instruction\"").unwrap_or_default();
        if !turn_id.is_empty() {
            lines.push(format!("system: turn {turn_type} {turn_id}"));
        } else {
            lines.push(format!("system: turn {turn_type}"));
        }
        if !instruction.is_empty() {
            lines.push(format!("user: {instruction}"));
        }
        let stages = extract_json_string_array_field(turn, "\"stages\"");
        if !stages.is_empty() {
            lines.push("stage: plan".into());
            for (idx, stage) in stages.iter().enumerate() {
                lines.push(format!(
                    "system: [x] {} {}",
                    idx + 1,
                    format_stage_label(stage)
                ));
            }
        }
        for stage in stages {
            lines.push(format!("stage: {stage}"));
            let transcript = root
                .join("sessions")
                .join(session_id)
                .join("transcripts")
                .join(format!("{turn_id}-{stage}.jsonl"));
            append_compact_transcript_lines(&mut lines, &transcript, 8);
        }
    }
    let max_lines = 900usize;
    if lines.len() > max_lines {
        lines.drain(0..lines.len() - max_lines);
        lines.insert(
            0,
            "system: ... earlier session conversation omitted from view".into(),
        );
    }
    lines
}

fn append_compact_transcript_lines(out: &mut Vec<String>, path: &Path, max_lines: usize) {
    let Ok(contents) = fs::read_to_string(path) else {
        return;
    };
    let mut compact = Vec::new();
    for line in contents.lines() {
        if let Some(display) = compact_agent_line(line) {
            compact.push(display);
        }
    }
    let start = compact.len().saturating_sub(max_lines);
    if start > 0 {
        out.push("system: ... earlier agent output omitted".into());
    }
    out.extend(compact.into_iter().skip(start));
}

fn preview_artifact(root: &Path, app: &mut TuiApp, requested: &str) -> Result<(), String> {
    let path = resolve_artifact_path(root, &app.session_id, requested);
    if !path.exists() {
        app.log
            .push(format!("system: Artifact not found: {}", path.display()));
        return Ok(());
    }
    let contents =
        fs::read_to_string(&path).map_err(|e| format!("read artifact {}: {e}", path.display()))?;
    let display = path
        .strip_prefix(root)
        .unwrap_or(&path)
        .to_string_lossy()
        .to_string();
    app.log.push(format!("stage: open {display}"));
    for line in contents.lines().take(120) {
        app.log.push(format!("system: {line}"));
    }
    if contents.lines().count() > 120 {
        app.log
            .push("system: ... truncated preview after 120 lines".into());
    }
    app.status = format!("Opened {display}");
    Ok(())
}

fn resolve_artifact_path(root: &Path, session_id: &str, requested: &str) -> PathBuf {
    let requested_path = PathBuf::from(requested);
    if requested_path.is_absolute() {
        return requested_path;
    }
    let direct = root.join(requested);
    if direct.exists() {
        return direct;
    }
    if !session_id.is_empty() {
        let session_relative = root.join("sessions").join(session_id).join(requested);
        if session_relative.exists() {
            return session_relative;
        }
        if requested.starts_with("raw/")
            || requested.starts_with("artifacts/")
            || requested.starts_with("plan/")
            || requested.starts_with("wiki/")
            || requested.starts_with("schemas/")
        {
            return session_relative;
        }
    }
    direct
}

fn shorten_middle(value: &str, max_chars: usize) -> String {
    let len = value.chars().count();
    if len <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(3) / 2;
    let start: String = value.chars().take(keep).collect();
    let end: String = value
        .chars()
        .rev()
        .take(keep)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{start}...{end}")
}

fn _old_draw_tui_removed(frame: &mut ratatui::Frame<'_>, app: &TuiApp) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            "research-os",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(if app.run_id.is_empty() {
            "no active session"
        } else {
            &app.run_id
        }),
        Span::raw("  "),
        Span::styled(&app.status, Style::default().fg(Color::Yellow)),
    ])])
    .block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(header, root[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(24),
            Constraint::Percentage(56),
            Constraint::Percentage(20),
        ])
        .split(root[1]);

    let stage_items = if app.stages.is_empty() {
        vec![ListItem::new("waiting for plan")]
    } else {
        app.stages
            .iter()
            .map(|stage| {
                if *stage == app.active_stage {
                    ListItem::new(Line::from(vec![
                        Span::styled("> ", Style::default().fg(Color::Green)),
                        Span::styled(
                            stage,
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]))
                } else {
                    ListItem::new(stage.as_str())
                }
            })
            .collect()
    };
    frame.render_widget(
        List::new(stage_items).block(Block::default().borders(Borders::ALL).title("Pipeline")),
        body[0],
    );

    let log_start = app
        .log
        .len()
        .saturating_sub(body[1].height.saturating_sub(2) as usize);
    let log = app.log[log_start..].join("\n");
    frame.render_widget(
        Paragraph::new(log).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Agent Conversation"),
        ),
        body[1],
    );

    let artifact_items = if app.artifacts.is_empty() {
        vec![ListItem::new("no artifacts yet")]
    } else {
        app.artifacts
            .iter()
            .map(|a| ListItem::new(a.as_str()))
            .collect()
    };
    frame.render_widget(
        List::new(artifact_items).block(Block::default().borders(Borders::ALL).title("Artifacts")),
        body[2],
    );

    let input_title = if app.running {
        "Input locked while agents run"
    } else {
        "Input"
    };
    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title(input_title));
    frame.render_widget(input, root[2]);
}

fn init_workspace(root: &Path) -> Result<(), String> {
    for dir in [
        "raw/sources",
        "artifacts/extracted",
        "artifacts/discovery",
        "wiki/sources",
        "wiki/topics",
        "wiki/concepts",
        "wiki/entities",
        "wiki/methods",
        "wiki/datasets",
        "wiki/comparisons",
        "wiki/synthesis",
        "wiki/outputs",
        "sessions",
        "schemas",
        "skills",
    ] {
        fs::create_dir_all(root.join(dir)).map_err(|e| format!("create {dir}: {e}"))?;
    }

    create_if_missing(
        &root.join("wiki/index.md"),
        "# Research Wiki Index\n\nCatalog wiki pages here. Each entry should have a one-line summary and source count when useful.\n",
    )?;
    create_if_missing(
        &root.join("wiki/log.md"),
        "# Research Wiki Log\n\nAppend-only history. Use entries like `## [YYYY-MM-DD] ingest | Title`.\n",
    )?;
    create_if_missing(
        &root.join("wiki/followups.md"),
        "# Follow-ups\n\nOpen questions, weak evidence, missing sources, and future experiments.\n",
    )?;
    Ok(())
}

fn create_if_missing(path: &Path, contents: &str) -> Result<(), String> {
    if !path.exists() {
        fs::write(path, contents).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

fn init_run_workspace(root: &Path, run_id: &str) -> Result<(), String> {
    let run_dir = root.join("runs").join(run_id);
    for dir in [
        "plan",
        "transcripts",
        "raw/sources",
        "artifacts/extracted",
        "artifacts/discovery",
        "assets/figures",
        "assets/tables",
        "assets/pages",
        "schemas",
        "wiki/sources",
        "wiki/topics",
        "wiki/concepts",
        "wiki/entities",
        "wiki/methods",
        "wiki/datasets",
        "wiki/comparisons",
        "wiki/synthesis",
        "wiki/outputs",
    ] {
        fs::create_dir_all(run_dir.join(dir))
            .map_err(|e| format!("create run artifact dir runs/{run_id}/{dir}: {e}"))?;
    }

    seed_run_file(
        root,
        run_id,
        "wiki/index.md",
        "# Research Wiki Index\n\nCatalog wiki pages here. Each entry should have a one-line summary and source count when useful.\n",
    )?;
    seed_run_file(
        root,
        run_id,
        "wiki/log.md",
        "# Research Wiki Log\n\nAppend-only history. Use entries like `## [YYYY-MM-DD] ingest | Title`.\n",
    )?;
    seed_run_file(
        root,
        run_id,
        "wiki/followups.md",
        "# Follow-ups\n\nOpen questions, weak evidence, missing sources, and future experiments.\n",
    )?;
    copy_schema_snapshot(root, run_id)?;
    Ok(())
}

fn init_session_workspace(root: &Path, session_id: &str) -> Result<(), String> {
    let session_dir = root.join("sessions").join(session_id);
    for dir in [
        "transcripts",
        "plan",
        "raw/sources",
        "artifacts/extracted",
        "artifacts/discovery",
        "artifacts/implementation",
        "assets/figures",
        "assets/tables",
        "assets/pages",
        "schemas",
        "wiki/sources",
        "wiki/topics",
        "wiki/concepts",
        "wiki/entities",
        "wiki/methods",
        "wiki/datasets",
        "wiki/comparisons",
        "wiki/synthesis",
        "wiki/outputs",
    ] {
        fs::create_dir_all(session_dir.join(dir))
            .map_err(|e| format!("create session dir sessions/{session_id}/{dir}: {e}"))?;
    }

    seed_session_file(
        root,
        session_id,
        "wiki/index.md",
        "# Research Wiki Index\n\nCatalog wiki pages here. Each entry should have a one-line summary and source count when useful.\n",
    )?;
    seed_session_file(
        root,
        session_id,
        "wiki/log.md",
        "# Research Wiki Log\n\nAppend-only history. Use entries like `## [YYYY-MM-DD] ingest | Title`.\n",
    )?;
    seed_session_file(
        root,
        session_id,
        "wiki/followups.md",
        "# Follow-ups\n\nOpen questions, weak evidence, missing sources, and future experiments.\n",
    )?;
    copy_session_schema_snapshot(root, session_id)?;
    Ok(())
}

fn seed_session_file(
    root: &Path,
    session_id: &str,
    rel: &str,
    default_contents: &str,
) -> Result<(), String> {
    let session_path = root.join("sessions").join(session_id).join(rel);
    if session_path.exists() {
        return Ok(());
    }
    if let Some(parent) = session_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let root_path = root.join(rel);
    if root_path.exists() {
        fs::copy(&root_path, &session_path).map_err(|e| {
            format!(
                "copy seed {} to {}: {e}",
                root_path.display(),
                session_path.display()
            )
        })?;
    } else {
        fs::write(&session_path, default_contents)
            .map_err(|e| format!("write {}: {e}", session_path.display()))?;
    }
    Ok(())
}

#[allow(dead_code)]
fn seed_run_wiki_from_session(root: &Path, session_id: &str, run_id: &str) -> Result<(), String> {
    let session_wiki = root.join("sessions").join(session_id).join("wiki");
    if !session_wiki.exists() {
        return Ok(());
    }
    let run_wiki = root.join("runs").join(run_id).join("wiki");
    copy_dir(&session_wiki, &run_wiki)
}

fn sync_session_wiki_from_run(root: &Path, session_id: &str, run_id: &str) -> Result<(), String> {
    let run_wiki = root.join("runs").join(run_id).join("wiki");
    if !run_wiki.exists() {
        return Ok(());
    }
    let session_wiki = root.join("sessions").join(session_id).join("wiki");
    copy_dir(&run_wiki, &session_wiki)
}

fn record_session_turn(
    root: &Path,
    session_id: &str,
    turn_id: &str,
    turn_kind: SessionTurnKind,
    instruction: &str,
    stages: &[&str],
    started_at_ms: u128,
) -> Result<(), String> {
    let session_dir = root.join("sessions").join(session_id);
    fs::create_dir_all(&session_dir)
        .map_err(|e| format!("create session {}: {e}", session_dir.display()))?;
    let turn_path = session_dir.join("turns.jsonl");
    let stage_json = stages
        .iter()
        .map(|stage| format!("\"{}\"", json_escape(stage)))
        .collect::<Vec<_>>()
        .join(", ");
    let transcript_json = stages
        .iter()
        .map(|stage| {
            format!(
                "\"sessions/{session_id}/transcripts/{turn_id}-{stage}.jsonl\"",
                session_id = json_escape(session_id),
                turn_id = json_escape(turn_id),
                stage = json_escape(stage)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let json = format!(
        "{{\"session_id\":\"{}\",\"turn_id\":\"{}\",\"turn_type\":\"{}\",\"instruction\":\"{}\",\"stages\":[{}],\"status\":\"completed\",\"started_at_ms\":{},\"finished_at_ms\":{},\"transcript_paths\":[{}]}}\n",
        json_escape(session_id),
        json_escape(turn_id),
        turn_kind.as_str(),
        json_escape(instruction),
        stage_json,
        started_at_ms,
        current_millis(),
        transcript_json
    );
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&turn_path)
        .and_then(|mut file| file.write_all(json.as_bytes()))
        .map_err(|e| format!("append {}: {e}", turn_path.display()))
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn seed_run_file(
    root: &Path,
    run_id: &str,
    rel: &str,
    default_contents: &str,
) -> Result<(), String> {
    let run_path = root.join("runs").join(run_id).join(rel);
    if run_path.exists() {
        return Ok(());
    }
    if let Some(parent) = run_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let root_path = root.join(rel);
    if root_path.exists() {
        fs::copy(&root_path, &run_path).map_err(|e| {
            format!(
                "copy seed {} to {}: {e}",
                root_path.display(),
                run_path.display()
            )
        })?;
    } else {
        fs::write(&run_path, default_contents)
            .map_err(|e| format!("write {}: {e}", run_path.display()))?;
    }
    Ok(())
}

fn copy_schema_snapshot(root: &Path, run_id: &str) -> Result<(), String> {
    let src = root.join("schemas");
    let dst = root.join("runs").join(run_id).join("schemas");
    if !src.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&src).map_err(|e| format!("read schemas: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        if from.is_file() {
            let to = dst.join(entry.file_name());
            fs::copy(&from, &to)
                .map_err(|e| format!("copy schema {} to {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

fn copy_session_schema_snapshot(root: &Path, session_id: &str) -> Result<(), String> {
    let src = root.join("schemas");
    let dst = root.join("sessions").join(session_id).join("schemas");
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(&dst).map_err(|e| format!("create {}: {e}", dst.display()))?;
    for entry in fs::read_dir(&src).map_err(|e| format!("read schemas: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        if from.is_file() {
            let to = dst.join(entry.file_name());
            fs::copy(&from, &to)
                .map_err(|e| format!("copy schema {} to {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn run_pipeline(root: &Path, instruction: &str) -> Result<(), String> {
    let run_id = make_run_id(root, instruction);
    init_run_workspace(root, &run_id)?;

    println!("research-os run: {run_id}");
    let backend = resolve_backend_env();
    invoke_agent(
        backend,
        root,
        &run_id,
        "research-os-planner",
        &planner_prompt(&run_id, None, instruction),
    )?;
    validate_plan(&run_plan_json_canonical_path(root, &run_id))?;

    execute_plan(backend, root, &run_id)
}

#[allow(dead_code)]
fn run_pipeline_tui(
    backend: Backend,
    root: &Path,
    session_id: &str,
    instruction: &str,
    tx: Sender<UiMsg>,
    control: RunControl,
) -> Result<(), String> {
    init_workspace(root)?;
    init_session_workspace(root, session_id)?;
    check_cancelled(&control)?;
    let run_id = make_run_id(root, instruction);
    init_run_workspace(root, &run_id)?;
    seed_run_wiki_from_session(root, session_id, &run_id)?;
    check_cancelled(&control)?;

    tx.send(UiMsg::RunStarted(run_id.clone()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!("system: session {session_id}")))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!("user: {instruction}")))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::StageStarted("research-os-planner".into()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line("stage: research-os-planner".into()))
        .map_err(|e| e.to_string())?;
    invoke_agent_tui(
        backend,
        root,
        &run_id,
        "research-os-planner",
        &planner_prompt(&run_id, Some(session_id), instruction),
        &tx,
        &control,
    )?;
    check_cancelled(&control)?;
    validate_plan(&run_plan_json_canonical_path(root, &run_id))?;
    emit_plan_summary(root, &run_id, &tx)?;
    tx.send(UiMsg::StageDone("research-os-planner".into()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Artifacts(list_run_artifacts(root, &run_id)))
        .map_err(|e| e.to_string())?;

    execute_plan_tui(backend, root, &run_id, tx.clone(), &control)?;
    sync_session_wiki_from_run(root, session_id, &run_id)?;
    record_session_turn(
        root,
        session_id,
        &run_id,
        SessionTurnKind::Plan,
        instruction,
        &["research-os-planner"],
        current_millis(),
    )?;
    tx.send(UiMsg::Done(format!("Run complete: runs/{run_id}")))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn run_session_turn_tui(
    backend: Backend,
    root: &Path,
    session_id: &str,
    instruction: &str,
    turn_kind: SessionTurnKind,
    tx: Sender<UiMsg>,
    control: RunControl,
) -> Result<(), String> {
    init_workspace(root)?;
    init_session_workspace(root, session_id)?;
    check_cancelled(&control)?;
    // The phase driver is its own long-running multi-phase loop; it doesn't use
    // the fixed single-pass stage list below.
    if matches!(turn_kind, SessionTurnKind::Loop) {
        return run_phase_loop_tui(backend, root, session_id, instruction, tx, control);
    }
    let turn_id = make_turn_id();
    let started_at_ms = current_millis();
    let stages = session_turn_stages(turn_kind);
    let should_execute_approved_plan =
        matches!(turn_kind, SessionTurnKind::Plan) && plan_confirmation_approves(instruction);
    let mut completed_turn_stages = stages.clone();

    tx.send(UiMsg::SessionTurnStarted(turn_id.clone()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!("system: session {session_id}")))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!("system: turn {}", turn_kind.as_str())))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!("user: {instruction}")))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Stages(
        stages.iter().map(|s| (*s).to_string()).collect(),
    ))
    .map_err(|e| e.to_string())?;
    emit_session_plan_summary(&tx, turn_kind, &stages)?;

    for skill in &stages {
        check_cancelled(&control)?;
        tx.send(UiMsg::StageStarted((*skill).to_string()))
            .map_err(|e| e.to_string())?;
        tx.send(UiMsg::Line(format!("stage: {skill}")))
            .map_err(|e| e.to_string())?;
        invoke_session_agent_tui(
            backend,
            root,
            session_id,
            &turn_id,
            skill,
            &session_stage_prompt(session_id, &turn_id, turn_kind, skill, instruction),
            &tx,
            &control,
        )?;
        check_cancelled(&control)?;
        if matches!(turn_kind, SessionTurnKind::Plan) && *skill == "research-os-planner" {
            emit_session_plan_file_summary(root, session_id, &turn_id, &tx)?;
        }
        tx.send(UiMsg::StageDone((*skill).to_string()))
            .map_err(|e| e.to_string())?;
        tx.send(UiMsg::Artifacts(list_session_artifacts(root, session_id)))
            .map_err(|e| e.to_string())?;
    }

    if should_execute_approved_plan {
        let executed = execute_session_plan_tui(
            backend,
            root,
            session_id,
            &turn_id,
            &turn_id,
            instruction,
            &tx,
            &control,
        )?;
        completed_turn_stages.extend(executed);
    }

    record_session_turn(
        root,
        session_id,
        &turn_id,
        turn_kind,
        instruction,
        &completed_turn_stages,
        started_at_ms,
    )?;
    tx.send(UiMsg::Done(format!(
        "Turn complete: sessions/{session_id}/turns.jsonl#{turn_id}"
    )))
    .map_err(|e| e.to_string())?;
    Ok(())
}

const LOOP_MAX_ITERATIONS: u32 = 24;
const LOOP_FORCE_CHECKPOINT_AFTER: u32 = 3;

fn loop_phase_work_stages(phase: ledger::Phase) -> Vec<&'static str> {
    use ledger::Phase;
    match phase {
        Phase::Init => vec!["research-os-doc-search", "research-os-wiki-update"],
        Phase::Discuss => vec!["research-os-discussion"],
        Phase::Experiment => vec![
            "research-os-experiment-planning",
            "research-os-coding",
            "research-os-visualization",
        ],
        Phase::Post => vec!["research-os-writing"],
    }
}

fn signal_details_brief(signal: &phase::ExitSignal) -> String {
    let d = &signal.details;
    format!(
        "src={} prop={} has_results={} done={} open_hyp={}",
        d.source_count, d.pending_proposals, d.experiments_has_results, d.experiments_done,
        d.open_hypotheses
    )
}

fn action_label(a: ledger::DecisionAction) -> &'static str {
    use ledger::DecisionAction::*;
    match a {
        Stay => "STAY",
        Advance => "ADVANCE",
        Branch => "BRANCH",
        Stop => "STOP",
    }
}

/// Per-phase work-stage prompt: the existing session stage prompt plus loop
/// context (subject + phase goal + the active proposal/experiment).
fn loop_stage_prompt(
    session_id: &str,
    turn_id: &str,
    phase: ledger::Phase,
    skill: &str,
    led: &ledger::Ledger,
) -> String {
    use ledger::Phase;
    let subject = led.subject.as_deref().unwrap_or("(no subject set)");
    let mut ctx = format!(
        "You are running ONE work stage inside an autonomous research-loop iteration (phase {}). \
         Research subject: {subject}. Do the focused work for this stage, then stop; the driver \
         decides the next step.",
        phase::phase_key(phase)
    );
    match phase {
        Phase::Init => ctx.push_str(&format!(
            " INIT goal: gather sources on the subject and build the initial wiki (aim for at least \
             {} notes in wiki/sources/).",
            phase::INIT_MIN_SOURCES
        )),
        Phase::Discuss => ctx.push_str(
            " DISCUSS goal: read the wiki and reason about the subject. When a testable hypothesis \
             crystallizes, call propose_experiment (with a rough_design) so the loop can branch to \
             an experiment.",
        ),
        Phase::Experiment => {
            if let Some(p) = led.proposals.iter().rev().find(|p| {
                matches!(p.status, ledger::ProposalStatus::Proposed) && p.rough_design.is_some()
            }) {
                ctx.push_str(&format!(
                    " EXPERIMENT goal: design and run a toy experiment for proposal {} (hypothesis {}). \
                     Rough design: {}. Place code/results under sessions/{session_id}/experiments/<exp_id>/.",
                    p.id,
                    p.hypothesis_id,
                    p.rough_design.as_deref().unwrap_or("")
                ));
            } else {
                ctx.push_str(" EXPERIMENT goal: design and run a toy experiment for the latest hypothesis.");
            }
        }
        Phase::Post => ctx.push_str(
            " POST goal: write the experiment up as (Question, Setup, Result, Analysis) and call \
             capture_results to promote the finding into wiki/sources as a citable note.",
        ),
    }
    session_stage_prompt(session_id, turn_id, SessionTurnKind::Loop, skill, &ctx)
}

/// Standalone prompt for the synthetic checkpoint stage (no SKILL.md). It must
/// call exactly one of phase_route (low-stakes only) or checkpoint_ask (human).
fn loop_checkpoint_prompt(
    session_id: &str,
    turn_id: &str,
    phase: ledger::Phase,
    signal: &phase::ExitSignal,
    low: bool,
    forced: bool,
) -> String {
    let d = &signal.details;
    let mut s = format!(
        "You are the phase-transition checkpoint for an autonomous research loop.\n\
         Session id: {session_id}\nTurn id: {turn_id}\nCurrent phase: {}\n\
         Work in session-only mode under sessions/{session_id}/. Do NOT write any files.\n\n\
         First call coverage_report and ledger_read to inspect the loop state. Mechanical signal \
         for this phase: boundary_plausible={}, source_count={}, pending_proposals={}, \
         experiments_has_results={}, experiments_done={}, open_hypotheses={}.\n\n",
        phase::phase_key(phase),
        signal.boundary_plausible,
        d.source_count,
        d.pending_proposals,
        d.experiments_has_results,
        d.experiments_done,
        d.open_hypotheses,
    );
    s.push_str(match phase {
        ledger::Phase::Init => {
            "INIT: if there are enough sources, ADVANCE to DISCUSS; else STAY to gather more.\n"
        }
        ledger::Phase::Discuss => {
            "DISCUSS (hub): if a proposal with a rough_design exists, BRANCH to EXPERIMENT \
             (target_phase EXPERIMENT); else STAY to keep discussing, or STOP.\n"
        }
        ledger::Phase::Experiment => {
            "EXPERIMENT: if results exist, ADVANCE to POST; else STAY to keep working, or BRANCH to \
             DISCUSS to drop the experiment.\n"
        }
        ledger::Phase::Post => {
            "POST: once the finding is captured, ADVANCE to DISCUSS to expand the discussion, or STOP.\n"
        }
    });
    if low {
        s.push_str(
            "\nThis is a LOW-STAKES boundary. If the next step is obvious and low-risk (e.g. \
             INIT->DISCUSS once sources suffice, or STAY), call phase_route(action, target_phase?, \
             reason) to proceed WITHOUT bothering the human. If genuinely ambiguous, use \
             checkpoint_ask instead.\n",
        );
    } else {
        s.push_str(
            "\nThis is a HIGH-STAKES boundary (starting an experiment, committing to the wiki, or \
             stopping). You MUST call checkpoint_ask so the human decides — do not self-route.\n",
        );
    }
    if forced {
        s.push_str(
            "\nNOTE: the loop has run several iterations without a checkpoint; re-confirm with the \
             human via checkpoint_ask even if little changed.\n",
        );
    }
    s.push_str(
        "\nCall exactly one of phase_route or checkpoint_ask. For checkpoint_ask, give a concise \
         question, an assessment, and options with the recommended option FIRST; each option must \
         carry action (STAY/ADVANCE/BRANCH/STOP) and target_phase for BRANCH.",
    );
    s
}

fn record_loop_decision(
    led: &mut ledger::Ledger,
    phase: ledger::Phase,
    signal: &phase::ExitSignal,
    route: &Option<ledger::RouteDecision>,
    action: ledger::DecisionAction,
    target: Option<ledger::Phase>,
) {
    let (by, reason, question, chosen_label) = match route {
        Some(r) => (r.by, r.reason.clone(), r.question.clone(), r.chosen_label.clone()),
        None => (
            ledger::DecisionBy::Llm,
            Some("no route from checkpoint".to_string()),
            None,
            None,
        ),
    };
    led.decisions.push(ledger::Decision {
        id: format!("dec-{}", ledger::now_ms()),
        phase,
        signal_hash: Some(signal.signal_hash.clone()),
        question,
        chosen: ledger::Chosen {
            label: chosen_label,
            action,
            target_phase: target,
        },
        by,
        reason,
        note: None,
        at: ledger::now_ms().to_string(),
    });
}

/// The `/loop` phase driver: runs each phase's work stages, computes the exit
/// signal, runs a stakes-gated checkpoint stage (which asks the human via
/// checkpoint_ask or self-routes low-stakes via phase_route), reads the
/// resulting ledger.pending_route, records the decision, and routes — bounded
/// (LOOP_MAX_ITERATIONS) and fully cancellable so it can never silently spin.
fn run_phase_loop_tui(
    backend: Backend,
    root: &Path,
    session_id: &str,
    subject: &str,
    tx: Sender<UiMsg>,
    control: RunControl,
) -> Result<(), String> {
    use ledger::{DecisionAction, Ledger};
    use phase::{compute_exit_signal, is_low_stakes, next_phase, phase_key};

    let loop_turn_id = make_turn_id();
    let started_at_ms = current_millis();
    let ledger_path = ledger::ledger_path(root, session_id);
    let session_root = root.join("sessions").join(session_id);

    tx.send(UiMsg::SessionTurnStarted(loop_turn_id.clone()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!("system: session {session_id}")))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line("system: turn loop".to_string()))
        .map_err(|e| e.to_string())?;

    // Setup: clear any stale route from a cancelled prior run; seed the subject
    // if given, else continue from the ledger's current phase.
    {
        let mut led = Ledger::load_or_new(&ledger_path, session_id).map_err(|e| e.to_string())?;
        led.pending_route = None;
        if !subject.trim().is_empty() {
            led.current_phase = ledger::Phase::Init;
            led.subject = Some(subject.trim().to_string());
            tx.send(UiMsg::Line(format!("user: /loop {}", subject.trim())))
                .map_err(|e| e.to_string())?;
        } else {
            tx.send(UiMsg::Line(format!(
                "system: continuing from {}",
                phase_key(led.current_phase)
            )))
            .map_err(|e| e.to_string())?;
            if led.subject.is_none() {
                tx.send(UiMsg::Line(
                    "system: no subject set; use /loop <subject> to seed".to_string(),
                ))
                .map_err(|e| e.to_string())?;
            }
        }
        led.save(&ledger_path).map_err(|e| e.to_string())?;
    }

    let mut iterations: u32 = 0;
    let mut suppressed: u32 = 0;
    let mut last_phase: Option<ledger::Phase> = None;

    loop {
        check_cancelled(&control)?;
        iterations += 1;
        if iterations > LOOP_MAX_ITERATIONS {
            tx.send(UiMsg::Line(format!(
                "system: hit LOOP_MAX_ITERATIONS ({LOOP_MAX_ITERATIONS}); stopping"
            )))
            .map_err(|e| e.to_string())?;
            break;
        }

        let phase = {
            let mut led =
                Ledger::load_or_new(&ledger_path, session_id).map_err(|e| e.to_string())?;
            let phase = led.current_phase;
            if last_phase != Some(phase) {
                suppressed = 0;
                last_phase = Some(phase);
            }
            led.phases.entry(phase_key(phase).to_string()).or_default().visits += 1;
            led.save(&ledger_path).map_err(|e| e.to_string())?;
            phase
        };

        tx.send(UiMsg::Line(format!(
            "system: --- iteration {iterations}/{LOOP_MAX_ITERATIONS}  phase {} ---",
            phase_key(phase)
        )))
        .map_err(|e| e.to_string())?;

        // 1. Run the phase's work stages.
        let work = loop_phase_work_stages(phase);
        let mut pane: Vec<String> = work.iter().map(|s| s.to_string()).collect();
        pane.push("checkpoint".to_string());
        tx.send(UiMsg::Stages(pane)).map_err(|e| e.to_string())?;
        for skill in &work {
            check_cancelled(&control)?;
            tx.send(UiMsg::StageStarted((*skill).to_string()))
                .map_err(|e| e.to_string())?;
            tx.send(UiMsg::Line(format!("stage: {skill}")))
                .map_err(|e| e.to_string())?;
            let led_now =
                Ledger::load_or_new(&ledger_path, session_id).map_err(|e| e.to_string())?;
            invoke_session_agent_tui(
                backend,
                root,
                session_id,
                &loop_turn_id,
                skill,
                &loop_stage_prompt(session_id, &loop_turn_id, phase, skill, &led_now),
                &tx,
                &control,
            )?;
            check_cancelled(&control)?;
            tx.send(UiMsg::StageDone((*skill).to_string()))
                .map_err(|e| e.to_string())?;
            tx.send(UiMsg::Artifacts(list_session_artifacts(root, session_id)))
                .map_err(|e| e.to_string())?;
        }

        // 2. Compute the exit signal and decide whether to checkpoint.
        let led = Ledger::load_or_new(&ledger_path, session_id).map_err(|e| e.to_string())?;
        let signal = compute_exit_signal(phase, &led, &session_root);
        let forced = suppressed + 1 >= LOOP_FORCE_CHECKPOINT_AFTER;
        // v1: hysteresis-suppression is OFF — always checkpoint on a met gate.
        let run_checkpoint = signal.boundary_plausible || forced;
        tx.send(UiMsg::Line(format!(
            "system: signal gate={} forced={} ({})",
            signal.boundary_plausible,
            forced,
            signal_details_brief(&signal)
        )))
        .map_err(|e| e.to_string())?;
        if !run_checkpoint {
            suppressed += 1;
            tx.send(UiMsg::Line(format!(
                "system: boundary not reached ({suppressed}/{LOOP_FORCE_CHECKPOINT_AFTER}); continuing the phase"
            )))
            .map_err(|e| e.to_string())?;
            continue;
        }
        suppressed = 0;

        // 3. Run the stakes-gated checkpoint stage (clears any stale route first).
        let low = is_low_stakes(phase, DecisionAction::Advance);
        let ck_skill = if low {
            "research-os-checkpoint-low"
        } else {
            "research-os-checkpoint-high"
        };
        {
            let mut led =
                Ledger::load_or_new(&ledger_path, session_id).map_err(|e| e.to_string())?;
            led.pending_route = None;
            led.save(&ledger_path).map_err(|e| e.to_string())?;
        }
        check_cancelled(&control)?;
        tx.send(UiMsg::StageStarted(ck_skill.to_string()))
            .map_err(|e| e.to_string())?;
        tx.send(UiMsg::Line(format!("stage: {ck_skill}")))
            .map_err(|e| e.to_string())?;
        invoke_session_agent_tui(
            backend,
            root,
            session_id,
            &loop_turn_id,
            ck_skill,
            &loop_checkpoint_prompt(session_id, &loop_turn_id, phase, &signal, low, forced),
            &tx,
            &control,
        )?;
        tx.send(UiMsg::StageDone(ck_skill.to_string()))
            .map_err(|e| e.to_string())?;

        // 4. Consume the route, record the decision, stamp hysteresis, route.
        let mut led = Ledger::load_or_new(&ledger_path, session_id).map_err(|e| e.to_string())?;
        let route = led.pending_route.take();
        let (action, target) = match &route {
            Some(r) => (r.action, r.target_phase),
            None => {
                tx.send(UiMsg::Line(
                    "system: checkpoint produced no route; defaulting STAY".to_string(),
                ))
                .map_err(|e| e.to_string())?;
                (DecisionAction::Stay, None)
            }
        };
        record_loop_decision(&mut led, phase, &signal, &route, action, target);
        {
            let st = led.phases.entry(phase_key(phase).to_string()).or_default();
            st.last_signal_hash = Some(signal.signal_hash.clone());
            st.last_decision = Some(action);
            st.last_checkpoint_at = Some(ledger::now_ms().to_string());
        }
        led.pending_route = None;
        led.save(&ledger_path).map_err(|e| e.to_string())?;

        match next_phase(phase, action, target) {
            None => {
                tx.send(UiMsg::Line("system: decision STOP; ending loop".to_string()))
                    .map_err(|e| e.to_string())?;
                break;
            }
            Some(next) => {
                if next != phase {
                    let mut led2 =
                        Ledger::load_or_new(&ledger_path, session_id).map_err(|e| e.to_string())?;
                    led2.current_phase = next;
                    led2.save(&ledger_path).map_err(|e| e.to_string())?;
                }
                tx.send(UiMsg::Line(format!(
                    "system: {} -> {} ({})",
                    phase_key(phase),
                    phase_key(next),
                    action_label(action)
                )))
                .map_err(|e| e.to_string())?;
            }
        }
    }

    record_session_turn(
        root,
        session_id,
        &loop_turn_id,
        SessionTurnKind::Loop,
        subject,
        &[],
        started_at_ms,
    )?;
    tx.send(UiMsg::Done(format!(
        "Loop ended: sessions/{session_id}/ledger.json ({iterations} iterations)"
    )))
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn run_approved_plan_execution_tui(
    backend: Backend,
    root: &Path,
    session_id: &str,
    plan_turn_id: &str,
    answer: &str,
    tx: Sender<UiMsg>,
    control: RunControl,
) -> Result<(), String> {
    init_workspace(root)?;
    init_session_workspace(root, session_id)?;
    check_cancelled(&control)?;
    let exec_turn_id = make_turn_id();
    let started_at_ms = current_millis();
    let instruction = format!("Approved plan execution\n\nSelected answer: {answer}");

    tx.send(UiMsg::SessionTurnStarted(exec_turn_id.clone()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!("system: session {session_id}")))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!(
        "system: executing approved plan from {plan_turn_id}"
    )))
    .map_err(|e| e.to_string())?;

    let executed = execute_session_plan_tui(
        backend,
        root,
        session_id,
        plan_turn_id,
        &exec_turn_id,
        &instruction,
        &tx,
        &control,
    )?;
    record_session_turn(
        root,
        session_id,
        &exec_turn_id,
        SessionTurnKind::Plan,
        &instruction,
        &executed,
        started_at_ms,
    )?;
    tx.send(UiMsg::Done(format!(
        "Turn complete: sessions/{session_id}/turns.jsonl#{exec_turn_id}"
    )))
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn execute_session_plan_tui(
    backend: Backend,
    root: &Path,
    session_id: &str,
    plan_turn_id: &str,
    exec_turn_id: &str,
    fallback_instruction: &str,
    tx: &Sender<UiMsg>,
    control: &RunControl,
) -> Result<Vec<&'static str>, String> {
    let plan_path = session_plan_json_path(root, session_id, plan_turn_id);
    if !plan_path.exists() {
        return Err(format!(
            "approved plan was not written at {}",
            plan_path.display()
        ));
    }
    let plan = fs::read_to_string(&plan_path)
        .map_err(|e| format!("read approved plan {}: {e}", plan_path.display()))?;
    let user_instruction = extract_json_string_field(&plan, "\"user_instruction\"")
        .unwrap_or_else(|| fallback_instruction.to_string());
    let planned_stages = extract_stage_skills(&plan);
    let executable_stages = planned_stages
        .into_iter()
        .filter(|skill| *skill != "research-os-planner")
        .collect::<Vec<_>>();

    if executable_stages.is_empty() {
        return Err("approved plan did not contain executable downstream stages".into());
    }
    for skill in &executable_stages {
        if is_deprecated_skill(skill) {
            return Err(format!(
                "approved plan selected deprecated run-era skill `{skill}`"
            ));
        }
    }

    tx.send(UiMsg::Line("system: Proceeding with approved plan".into()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Stages(
        executable_stages
            .iter()
            .map(|stage| (*stage).to_string())
            .collect(),
    ))
    .map_err(|e| e.to_string())?;
    emit_session_plan_summary(tx, SessionTurnKind::Plan, &executable_stages)?;

    for skill in &executable_stages {
        check_cancelled(control)?;
        tx.send(UiMsg::StageStarted((*skill).to_string()))
            .map_err(|e| e.to_string())?;
        tx.send(UiMsg::Line(format!("stage: {skill}")))
            .map_err(|e| e.to_string())?;
        invoke_session_agent_tui(
            backend,
            root,
            session_id,
            exec_turn_id,
            skill,
            &session_stage_prompt(
                session_id,
                exec_turn_id,
                SessionTurnKind::Plan,
                skill,
                &user_instruction,
            ),
            tx,
            control,
        )?;
        check_cancelled(control)?;
        tx.send(UiMsg::StageDone((*skill).to_string()))
            .map_err(|e| e.to_string())?;
        tx.send(UiMsg::Artifacts(list_session_artifacts(root, session_id)))
            .map_err(|e| e.to_string())?;
    }

    Ok(executable_stages)
}

fn session_plan_json_path(root: &Path, session_id: &str, turn_id: &str) -> PathBuf {
    root.join("sessions")
        .join(session_id)
        .join("plan")
        .join(format!("{turn_id}.json"))
}

fn emit_session_plan_file_summary(
    root: &Path,
    session_id: &str,
    turn_id: &str,
    tx: &Sender<UiMsg>,
) -> Result<(), String> {
    let plan_path = session_plan_json_path(root, session_id, turn_id);
    if !plan_path.exists() {
        return Ok(());
    }
    let plan = fs::read_to_string(&plan_path)
        .map_err(|e| format!("read session plan {}: {e}", plan_path.display()))?;

    let goal = extract_json_string_field(&plan, "\"user_instruction\"")
        .unwrap_or_else(|| "unspecified".into());
    let task_type =
        extract_json_string_field(&plan, "\"task_type\"").unwrap_or_else(|| "unknown".into());
    let scope =
        extract_json_string_field(&plan, "\"mode\"").unwrap_or_else(|| "unspecified".into());
    let wiki_reason =
        extract_json_string_field(&plan, "\"reason\"").unwrap_or_else(|| "not stated".into());
    let rationale = extract_json_string_field(&plan, "\"selection_rationale\"")
        .unwrap_or_else(|| "not stated".into());
    let stage_summaries = extract_stage_plan_summaries(&plan);
    let stages = stage_summaries
        .iter()
        .map(|stage| stage.skill)
        .collect::<Vec<_>>();
    let outputs = stage_summaries
        .iter()
        .flat_map(|stage| stage.outputs.clone())
        .collect::<Vec<_>>();

    tx.send(UiMsg::Line("plan:title:Plan summary".into()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!(
        "plan:field:Goal              {}",
        truncate_label(&goal, 140)
    )))
    .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!(
        "plan:field:Task              {task_type}"
    )))
    .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!("plan:field:Scope             {scope}")))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!(
        "plan:field:Wiki sufficiency  {}",
        truncate_label(&wiki_reason, 140)
    )))
    .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!(
        "plan:field:Rationale         {}",
        truncate_label(&rationale, 140)
    )))
    .map_err(|e| e.to_string())?;
    if outputs.is_empty() {
        tx.send(UiMsg::Line(
            "plan:field:Expected outputs  not specified".into(),
        ))
        .map_err(|e| e.to_string())?;
    } else {
        tx.send(UiMsg::Line(format!(
            "plan:field:Expected outputs  {}",
            outputs
                .iter()
                .take(4)
                .map(|output| truncate_label(output, 80))
                .collect::<Vec<_>>()
                .join(", ")
        )))
        .map_err(|e| e.to_string())?;
    }

    tx.send(UiMsg::Line("plan:section:Proposed pipeline".into()))
        .map_err(|e| e.to_string())?;
    if stages.is_empty() {
        tx.send(UiMsg::Line(
            "plan:stage:1. no executable stages found".into(),
        ))
        .map_err(|e| e.to_string())?;
    } else {
        for (idx, stage) in stage_summaries.iter().enumerate() {
            tx.send(UiMsg::Line(format!(
                "plan:stage:{}. {}  -  {}",
                idx + 1,
                trim_skill_name(stage.skill),
                truncate_label(&stage.reason, 120)
            )))
            .map_err(|e| e.to_string())?;
            let io_line = format_stage_io_line(stage);
            if !io_line.is_empty() {
                tx.send(UiMsg::Line(format!("plan:io:   {io_line}")))
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn resume_pipeline(root: &Path, run_id: &str) -> Result<(), String> {
    validate_plan(&run_plan_json_path(root, run_id))?;
    execute_plan(resolve_backend_env(), root, run_id)
}

#[allow(dead_code)]
fn resume_pipeline_tui(
    backend: Backend,
    root: &Path,
    run_id: &str,
    tx: Sender<UiMsg>,
    control: RunControl,
) -> Result<(), String> {
    validate_plan(&run_plan_json_path(root, run_id))?;
    check_cancelled(&control)?;
    tx.send(UiMsg::RunStarted(run_id.to_string()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Artifacts(list_run_artifacts(root, run_id)))
        .map_err(|e| e.to_string())?;
    execute_plan_tui(backend, root, run_id, tx.clone(), &control)?;
    tx.send(UiMsg::Done(format!("Run complete: runs/{run_id}")))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn execute_plan_tui(
    backend: Backend,
    root: &Path,
    run_id: &str,
    tx: Sender<UiMsg>,
    control: &RunControl,
) -> Result<(), String> {
    let plan_path = run_plan_json_path(root, run_id);
    let plan = fs::read_to_string(&plan_path).map_err(|e| format!("read plan.json: {e}"))?;
    let stages = extract_stage_skills(&plan);
    if stages.is_empty() {
        return Err("plan.json did not contain any recognized research-os-* stage skills".into());
    }
    tx.send(UiMsg::Stages(
        stages.iter().map(|s| (*s).to_string()).collect(),
    ))
    .map_err(|e| e.to_string())?;

    let mut occurrences: HashMap<&str, usize> = HashMap::new();
    for (idx, skill) in stages.iter().enumerate() {
        check_cancelled(control)?;
        let occurrence = *occurrences
            .entry(*skill)
            .and_modify(|n| *n += 1)
            .or_insert(0);
        if *skill == "research-os-planner" {
            continue;
        }
        let stage_no = idx + 1;
        let status_path = root
            .join("runs")
            .join(run_id)
            .join(format!("stage-{stage_no:02}-{skill}.done"));
        if status_path.exists() {
            tx.send(UiMsg::Line(format!(
                "stage {stage_no}: {skill} already complete"
            )))
            .map_err(|e| e.to_string())?;
            continue;
        }

        if validate_stage_output(root, run_id, skill, occurrence).is_ok() {
            fs::write(&status_path, "done\n").map_err(|e| format!("write status: {e}"))?;
            tx.send(UiMsg::Line(format!(
                "stage: {skill} already has valid outputs"
            )))
            .map_err(|e| e.to_string())?;
            tx.send(UiMsg::StageDone((*skill).to_string()))
                .map_err(|e| e.to_string())?;
            tx.send(UiMsg::Artifacts(list_run_artifacts(root, run_id)))
                .map_err(|e| e.to_string())?;
            continue;
        }

        tx.send(UiMsg::StageStarted((*skill).to_string()))
            .map_err(|e| e.to_string())?;
        tx.send(UiMsg::Line(format!("stage: {skill}")))
            .map_err(|e| e.to_string())?;
        let before = snapshot_wiki(root, run_id)?;
        invoke_agent_tui(
            backend,
            root,
            run_id,
            skill,
            &stage_prompt(run_id, skill),
            &tx,
            control,
        )?;
        check_cancelled(control)?;
        enforce_wiki_mutation(root, run_id, skill, &before)?;
        validate_stage_output(root, run_id, skill, occurrence)?;
        fs::write(&status_path, "done\n").map_err(|e| format!("write status: {e}"))?;
        tx.send(UiMsg::StageDone((*skill).to_string()))
            .map_err(|e| e.to_string())?;
        tx.send(UiMsg::Artifacts(list_run_artifacts(root, run_id)))
            .map_err(|e| e.to_string())?;
    }

    validate_run(root, run_id)?;
    emit_final_expected_artifact_tui(root, run_id, &tx)?;
    Ok(())
}

#[allow(dead_code)]
fn execute_plan(backend: Backend, root: &Path, run_id: &str) -> Result<(), String> {
    let plan_path = run_plan_json_path(root, run_id);
    let plan = fs::read_to_string(&plan_path).map_err(|e| format!("read plan.json: {e}"))?;
    let stages = extract_stage_skills(&plan);
    if stages.is_empty() {
        return Err("plan.json did not contain any recognized research-os-* stage skills".into());
    }

    let mut occurrences: HashMap<&str, usize> = HashMap::new();
    for (idx, skill) in stages.iter().enumerate() {
        let occurrence = *occurrences
            .entry(*skill)
            .and_modify(|n| *n += 1)
            .or_insert(0);
        if *skill == "research-os-planner" {
            continue;
        }
        let stage_no = idx + 1;
        let status_path = root
            .join("runs")
            .join(run_id)
            .join(format!("stage-{stage_no:02}-{skill}.done"));
        if status_path.exists() {
            println!("stage {stage_no}: {skill} already complete");
            continue;
        }

        if validate_stage_output(root, run_id, skill, occurrence).is_ok() {
            fs::write(&status_path, "done\n").map_err(|e| format!("write status: {e}"))?;
            println!("stage {stage_no}: {skill} already has valid outputs");
            continue;
        }

        println!("\n=== stage {stage_no}: {skill} ===");
        let before = snapshot_wiki(root, run_id)?;
        invoke_agent(backend, root, run_id, skill, &stage_prompt(run_id, skill))?;
        enforce_wiki_mutation(root, run_id, skill, &before)?;
        validate_stage_output(root, run_id, skill, occurrence)?;
        fs::write(&status_path, "done\n").map_err(|e| format!("write status: {e}"))?;
    }

    validate_run(root, run_id)?;
    emit_final_expected_artifact_cli(root, run_id)?;
    println!("run complete: runs/{run_id}");
    Ok(())
}

fn planner_prompt(run_id: &str, session_id: Option<&str>, instruction: &str) -> String {
    let session_context = session_id
        .map(|id| {
            format!(
                "Session id: {id}\n\
                 This run is one turn in a multi-turn research session. runs/{run_id}/wiki/ has already been seeded from sessions/{id}/wiki/ when prior session memory exists. Read the run-local wiki before choosing agents, then decide whether the current turn can be answered from the wiki, needs new source acquisition, or needs both.\n"
            )
        })
        .unwrap_or_else(|| {
            "Session id: none\n\
             This CLI run is self-contained, but still read runs/{run_id}/wiki/index.md before choosing agents.\n"
                .replace("{run_id}", run_id)
        });
    format!(
        "Use the local skill at skills/research-os-planner/SKILL.md.\n\
         User instruction: {instruction}\n\
         Run id: {run_id}\n\
         {session_context}\
         Write exactly one artifact: runs/{run_id}/plan/plan.json.\n\
         This run is self-contained. Use run-local artifact roots for stage outputs: runs/{run_id}/plan/, runs/{run_id}/raw/, runs/{run_id}/artifacts/, runs/{run_id}/schemas/, and runs/{run_id}/wiki/.\n\
         The plan must include selected_agents, selection_rationale, ordered stages with agent skill names, source_scope, required inputs, required outputs, and wiki mutation permissions.\n\
         For new plans, wiki/topics/ is allowed only for broad research themes. Wiki updates must use wiki/sources/, wiki/topics/, wiki/concepts/, wiki/entities/, wiki/methods/, wiki/datasets/, wiki/comparisons/, wiki/index.md, wiki/log.md, and wiki/followups.md; do not use wiki/topics/ as a catch-all.\n\
         If source_scope is venue-scoped across multiple venues, require doc-search to cover each requested venue independently and record venue_coverage for every venue/year.\n\
         Do not execute downstream agents."
    )
}

fn stage_prompt(run_id: &str, skill: &str) -> String {
    let skill_specific = if skill == "research-os-search" {
        "\n\
         If an allowed source page is JavaScript-rendered and static fetch/search snippets do not expose source records, use a headless browser rendering fallback. Prefer the repo-local helper: npm run render-source -- '<allowed-url>' --out 'runs/{run_id}/artifacts/discovery/rendered-source.json'. Rendered DOM, snippets, abstracts, and other discovery/extraction artifacts are derived records and must not be written under runs/{run_id}/raw/sources/. Record every attempt in browser_render_attempts."
    } else if skill == "research-os-doc-search" {
        "\n\
         This is the unified document acquisition stage. Search only within source_scope, triage candidates, discover/download matching PDF or source-native files, and write runs/{run_id}/doc_search_manifest.json. For multi-venue source scopes, cover every requested venue/year independently and record venue_coverage with official locations, render status, candidate count, selected count, and gap reason. For CVPR 2026, prefer https://cvpr.thecvf.com/virtual/2026/papers.html and https://cvpr.thecvf.com/static/virtual/data/cvpr-2026-orals-posters.json; do not rely only on /Conferences/2026/AcceptedPapers. For ICML 2026, inspect https://icml.cc/virtual/2026/papers.html and official poster/detail pages, not only the top-level list snippets. Once a source is selected from the allowed scope, matching PDFs/full text from arXiv, publisher, author/project, or institutional URLs are allowed when title plus another identifier match; this is file acquisition, not source-scope expansion. For each selected paper, also check code/model/demo/dataset availability via official metadata, project pages, GitHub/GitLab, Hugging Face, Papers With Code, OpenReview artifacts, and exact-title lookups; record code_status with availability official|unofficial|not_found|uncertain, URLs, lookup queries, candidate URLs, and match rationale. Do not clone repos or download model weights unless explicitly planned. Store only source-native downloads under runs/{run_id}/raw/sources/. Store extracted text, browser-rendered DOM, lookup notes, candidate lists, and other run-only derived artifacts under runs/{run_id}/artifacts/extracted/ or runs/{run_id}/artifacts/discovery/. Use readable filenames like {venue_slug}__{title_slug}.pdf instead of source_id-only filenames; keep source_id in doc_search_manifest.json. Do not mutate wiki."
    } else if skill == "research-os-wiki-update" {
        "\n\
         Mutate only the run-local wiki assigned by the plan. Source-specific summaries go under wiki/sources/. Broad research themes go under wiki/topics/. Specific algorithms/procedures go under wiki/methods/, terms under wiki/concepts/, datasets under wiki/datasets/, named models/projects/institutions under wiki/entities/, and side-by-side analyses under wiki/comparisons/. Update wiki/index.md, wiki/log.md, and wiki/followups.md."
    } else if skill == "research-os-ingest" {
        "\n\
         For each selected paper/report source, do mandatory targeted PDF/full-text discovery before text fallback. If selected metadata and official pages do not expose a PDF, search the web by exact title with pdf/arxiv/first-author/venue terms, inspect arXiv and author/project pages, and download a matching file when title plus at least one additional identifier matches. Do not add new research sources; only find files for the selected sources. Store only source-native downloaded files, such as PDFs, official HTML, official JSON, or original text files, under runs/{run_id}/raw/sources/. Use readable filenames like {venue_slug}__{title_slug}.pdf instead of source_id-only filenames; keep source_id in ingest_manifest.json. Store extracted text, browser-visible text fallbacks, abstracts copied from pages, rendered DOM records, candidate lists, and other derived run-only artifacts under runs/{run_id}/artifacts/extracted/ or runs/{run_id}/artifacts/discovery/ using the same readable file stem when possible. Record discovery queries, candidate URLs, match rationale, filename/file_stem, and download result in ingest_manifest.json. A text fallback without recorded exact-title lookup queries is invalid."
    } else {
        ""
    };
    format!(
        "Use the local skill at skills/{skill}/SKILL.md.\n\
         Run id: {run_id}\n\
         Read runs/{run_id}/plan/plan.json first. If that file is absent in a legacy run, read runs/{run_id}/plan.json.\n\
         Follow only this agent's contract. Write the outputs assigned to this stage. Treat runs/{run_id}/raw/, runs/{run_id}/artifacts/, runs/{run_id}/schemas/, and runs/{run_id}/wiki/ as this run's artifact roots. Preserve citation traceability and source-scope constraints.{skill_specific}"
    )
}

fn session_turn_stages(turn_kind: SessionTurnKind) -> Vec<&'static str> {
    match turn_kind {
        SessionTurnKind::Discussion => vec!["research-os-discussion"],
        SessionTurnKind::Search => vec![
            "research-os-doc-search",
            "research-os-wiki-update",
            "research-os-discussion",
        ],
        SessionTurnKind::Plan => vec!["research-os-planner"],
        // The phase driver manages its own per-phase stages; it does not go
        // through the fixed session_turn_stages path.
        SessionTurnKind::Loop => vec![],
    }
}

fn emit_session_plan_summary(
    tx: &Sender<UiMsg>,
    turn_kind: SessionTurnKind,
    stages: &[&str],
) -> Result<(), String> {
    tx.send(UiMsg::Line("stage: plan".into()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!(
        "system: Updated Plan  turn:{}  stages:{}",
        turn_kind.as_str(),
        stages.len()
    )))
    .map_err(|e| e.to_string())?;
    for (idx, stage) in stages.iter().enumerate() {
        tx.send(UiMsg::Line(format!(
            "system: [ ] {} {}",
            idx + 1,
            format_stage_label(stage)
        )))
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[allow(dead_code)]
fn instruction_requests_search(instruction: &str) -> bool {
    let lower = instruction.to_ascii_lowercase();
    let english = [
        "search",
        "research",
        "find",
        "look up",
        "source",
        "paper",
        "papers",
        "arxiv",
        "cvpr",
        "iclr",
        "icml",
        "new source",
    ];
    let korean = ["검색", "조사", "논문", "찾아", "자료", "출처", "새로"];
    english.iter().any(|needle| lower.contains(needle))
        || korean.iter().any(|needle| instruction.contains(needle))
}

fn session_stage_prompt(
    session_id: &str,
    turn_id: &str,
    turn_kind: SessionTurnKind,
    skill: &str,
    instruction: &str,
) -> String {
    let base = format!(
        "Use the local skill at skills/{skill}/SKILL.md.\n\
         Session id: {session_id}\n\
         Turn id: {turn_id}\n\
         Turn type: {turn_type}\n\
         User instruction: {instruction}\n\
         Work in session-only mode. Read sessions/{session_id}/wiki/index.md first, then relevant session wiki pages. Treat sessions/{session_id}/raw/, sessions/{session_id}/artifacts/, sessions/{session_id}/plan/, sessions/{session_id}/schemas/, and sessions/{session_id}/wiki/ as the artifact roots. Do not create a new runs/{{run_id}} directory or require runs/{{run_id}}/plan/plan.json.\n",
        turn_type = turn_kind.as_str()
    );
    let specific = match skill {
        "research-os-planner" => format!(
            "Build a session-local pipeline proposal, but do not execute downstream agents. Read the existing session wiki and any prior plan drafts under sessions/{session_id}/plan/. Write a draft plan to sessions/{session_id}/plan/{turn_id}.json and, if useful, a human-readable summary to sessions/{session_id}/plan/{turn_id}.md. The plan must use only session-local roots and must not create runs/. In the conversation stream, present the proposed pipeline. Ask PLAN_QUESTION only for blocking ambiguities that materially change the pipeline. If the plan is sufficient to implement, stop asking detail questions and ask final confirmation. Immediately before every PLAN_CONFIRM line, show a concise Plan summary with goal, scope, wiki sufficiency, expected outputs, and a Proposed pipeline list with each selected agent and why it will run. Then ask final confirmation using this exact line format: PLAN_CONFIRM: Proceed with this plan? || Proceed with this plan || Continue planning || Revise scope. Never emit PLAN_CONFIRM by itself. Choose plausible options yourself and put the recommended option first. Treat this as a Claude Code style planning conversation: clarify only what is necessary, then move to proceed/continue planning confirmation."
        ),
        "research-os-doc-search" => format!(
            "Search only within the user's requested source scope. For selected sources, preserve source-native files under sessions/{session_id}/raw/sources/, derived extraction/discovery files under sessions/{session_id}/artifacts/extracted/ or sessions/{session_id}/artifacts/discovery/, visual assets under sessions/{session_id}/assets/, and write sessions/{session_id}/doc_search_manifest.json. Also record code_status for selected papers when code/model/demo/dataset availability can be checked. For PDF papers, identify 1-5 important figures/tables and use scripts/extract_pdf_assets.py when a reliable crop box can be inferred; record visual_assets with success/uncertain/failed status. Do not mutate sessions/{session_id}/wiki/."
        ),
        "research-os-wiki-update" => format!(
            "Read sessions/{session_id}/doc_search_manifest.json, session raw/extracted artifacts, and session visual assets. Mutate only sessions/{session_id}/wiki/. Create/update source notes, topics, index.md, log.md, followups.md, and write sessions/{session_id}/wiki_update_report.md. Source notes for papers must include a Key Figures and Tables section; embed successful visual assets with Markdown image links and mark uncertain/failed important visuals explicitly. Preserve citation traceability to session source notes or raw provenance."
        ),
        "research-os-synthesis" => format!(
            "Read the session wiki, source notes, topics, followups, and current turn artifacts. Write or update a synthesis page under sessions/{session_id}/wiki/synthesis/{turn_id}.md. Do not create run artifacts."
        ),
        "research-os-discussion" | "research-os-qa" => format!(
            "Use the existing session wiki as durable memory for a concise research conversation response. Answer in the conversation stream. If the answer is worth preserving, also write a short markdown note under sessions/{session_id}/wiki/outputs/{turn_id}.md. If the wiki lacks evidence for a factual claim, say what is missing instead of inventing it."
        ),
        "research-os-coding" => format!(
            "Implement code in session-only mode after reading the session wiki. If the task is a repo/product feature or bug fix, edit the assigned repository files directly and write a concise implementation note to sessions/{session_id}/artifacts/implementation/{turn_id}/notes.md listing changed files, validation commands, and any wiki evidence used. If the task is a research prototype, experiment scaffold, analysis script, parser, demo, or generated code artifact, place the code under sessions/{session_id}/artifacts/implementation/{turn_id}/, include a README.md or notes.md with run instructions, and keep any outputs in the same implementation artifact subtree. Do not create runs/ and do not copy the whole session into a new workspace."
        ),
        _ => "Follow only this agent's contract, adapted to the session artifact roots above.".into(),
    };
    format!("{base}{specific}")
}

#[allow(dead_code)]
/// Build the subprocess command for one agent stage on the selected backend.
/// Both backends pipe stdout/stderr; the caller streams and waits.
fn build_agent_command(
    backend: Backend,
    skill: &str,
    root: &Path,
    prompt: &str,
    session: Option<&str>,
) -> Command {
    let mut cmd = Command::new(backend_label(backend));
    match backend {
        Backend::Codex => {
            if agent_needs_search(skill) {
                cmd.arg("--search");
            }
            cmd.arg("exec")
                .arg("--cd")
                .arg(root)
                .arg("--sandbox")
                .arg(agent_sandbox_mode(skill))
                .arg("--skip-git-repo-check")
                .arg("--json")
                .arg(prompt);
        }
        Backend::Claude => {
            // Claude has no `--cd`, so run with cwd at the workspace root; the
            // prompt's relative skill path and run/session artifact roots then
            // resolve. WebSearch is a built-in tool, so acquisition stages need
            // no extra flag.
            cmd.current_dir(root)
                .arg("-p")
                .arg(prompt)
                .arg("--output-format")
                .arg("stream-json")
                .arg("--verbose");
            if tool_scoping_enabled() {
                // New agent harness (opt-in via RESEARCH_OS_TOOL_SCOPING):
                // restrict tools per stage and auto-deny anything unlisted
                // (`dontAsk`) instead of bypassing all permissions. `-p` print
                // mode cannot answer interactive prompts, so `dontAsk` avoids
                // hangs on unlisted tools.
                cmd.arg("--permission-mode").arg("dontAsk");
                let mut allowed = claude_allowed_tools(skill).to_string();
                // In session mode, attach the local MCP sidecar (custom loop
                // tools) and allow this stage's MCP tools alongside built-ins.
                // `--strict-mcp-config` loads only our server (avoids the known
                // multi-server stdio hang under `-p`).
                if let Some(session_id) = session {
                    if let Some(cfg) = session_mcp_config_path(root, session_id) {
                        cmd.arg("--strict-mcp-config")
                            .arg("--mcp-config")
                            .arg(&cfg);
                        let mcp = claude_mcp_tools(skill);
                        if !mcp.is_empty() {
                            allowed.push(',');
                            allowed.push_str(mcp);
                        }
                    }
                }
                cmd.arg("--allowedTools").arg(allowed);
            } else {
                // Default (until the new harness is validated): bypass
                // permissions, since `-p` cannot answer interactive prompts.
                cmd.arg("--dangerously-skip-permissions");
            }
            if let Ok(model) = env::var("RESEARCH_OS_CLAUDE_MODEL") {
                if !model.trim().is_empty() {
                    cmd.arg("--model").arg(model);
                }
            }
        }
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd
}

fn invoke_agent(
    backend: Backend,
    root: &Path,
    run_id: &str,
    skill: &str,
    prompt: &str,
) -> Result<(), String> {
    let transcript_path = root
        .join("runs")
        .join(run_id)
        .join("transcripts")
        .join(format!("{skill}.jsonl"));
    let transcript = Arc::new(Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&transcript_path)
            .map_err(|e| format!("open transcript {}: {e}", transcript_path.display()))?,
    ));

    let mut cmd = build_agent_command(backend, skill, root, prompt, None);
    let backend_name = backend_label(backend);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {backend_name}: {e}"))?;
    let stdout = child.stdout.take().ok_or("missing agent stdout")?;
    let stderr = child.stderr.take().ok_or("missing agent stderr")?;

    let out_file = transcript.clone();
    let out = thread::spawn(move || stream_lines(stdout, out_file, false));
    let err_file = transcript.clone();
    let err = thread::spawn(move || stream_lines(stderr, err_file, true));

    let status = child.wait().map_err(|e| format!("wait {backend_name}: {e}"))?;
    out.join().map_err(|_| "stdout stream thread failed")??;
    err.join().map_err(|_| "stderr stream thread failed")??;
    if !status.success() {
        return Err(format!("{skill} failed with status {status}"));
    }
    Ok(())
}

fn invoke_agent_tui(
    backend: Backend,
    root: &Path,
    run_id: &str,
    skill: &str,
    prompt: &str,
    tx: &Sender<UiMsg>,
    control: &RunControl,
) -> Result<(), String> {
    check_cancelled(control)?;
    let transcript_path = root
        .join("runs")
        .join(run_id)
        .join("transcripts")
        .join(format!("{skill}.jsonl"));
    let transcript = Arc::new(Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&transcript_path)
            .map_err(|e| format!("open transcript {}: {e}", transcript_path.display()))?,
    ));

    let mut cmd = build_agent_command(backend, skill, root, prompt, None);
    let backend_name = backend_label(backend);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {backend_name}: {e}"))?;
    {
        let mut pid = control
            .current_child_pid
            .lock()
            .map_err(|_| "child pid lock poisoned")?;
        *pid = Some(child.id());
    }
    let stdout = child.stdout.take().ok_or("missing agent stdout")?;
    let stderr = child.stderr.take().ok_or("missing agent stderr")?;

    let out_file = transcript.clone();
    let out_tx = tx.clone();
    let out = thread::spawn(move || stream_lines_tui(stdout, out_file, out_tx));
    let err_file = transcript.clone();
    let err_tx = tx.clone();
    let err = thread::spawn(move || stream_lines_tui(stderr, err_file, err_tx));

    let status = child.wait().map_err(|e| format!("wait {backend_name}: {e}"))?;
    {
        let mut pid = control
            .current_child_pid
            .lock()
            .map_err(|_| "child pid lock poisoned")?;
        *pid = None;
    }
    out.join().map_err(|_| "stdout stream thread failed")??;
    err.join().map_err(|_| "stderr stream thread failed")??;
    check_cancelled(control)?;
    if !status.success() {
        return Err(format!("{skill} failed with status {status}"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn invoke_session_agent_tui(
    backend: Backend,
    root: &Path,
    session_id: &str,
    turn_id: &str,
    skill: &str,
    prompt: &str,
    tx: &Sender<UiMsg>,
    control: &RunControl,
) -> Result<(), String> {
    check_cancelled(control)?;
    let transcript_path = root
        .join("sessions")
        .join(session_id)
        .join("transcripts")
        .join(format!("{turn_id}-{skill}.jsonl"));
    if let Some(parent) = transcript_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let transcript = Arc::new(Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&transcript_path)
            .map_err(|e| format!("open transcript {}: {e}", transcript_path.display()))?,
    ));

    let mut cmd = build_agent_command(backend, skill, root, prompt, Some(session_id));
    let backend_name = backend_label(backend);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {backend_name}: {e}"))?;
    {
        let mut pid = control
            .current_child_pid
            .lock()
            .map_err(|_| "child pid lock poisoned")?;
        *pid = Some(child.id());
    }
    let stdout = child.stdout.take().ok_or("missing agent stdout")?;
    let stderr = child.stderr.take().ok_or("missing agent stderr")?;

    let out_file = transcript.clone();
    let out_tx = tx.clone();
    let out = thread::spawn(move || stream_lines_tui(stdout, out_file, out_tx));
    let err_file = transcript.clone();
    let err_tx = tx.clone();
    let err = thread::spawn(move || stream_lines_tui(stderr, err_file, err_tx));

    let status = child.wait().map_err(|e| format!("wait {backend_name}: {e}"))?;
    {
        let mut pid = control
            .current_child_pid
            .lock()
            .map_err(|_| "child pid lock poisoned")?;
        *pid = None;
    }
    out.join().map_err(|_| "stdout stream thread failed")??;
    err.join().map_err(|_| "stderr stream thread failed")??;
    check_cancelled(control)?;
    if !status.success() {
        return Err(format!("{skill} failed with status {status}"));
    }
    Ok(())
}

fn check_cancelled(control: &RunControl) -> Result<(), String> {
    if control.cancel_requested.load(Ordering::SeqCst) {
        Err("run cancelled".into())
    } else {
        Ok(())
    }
}

fn agent_sandbox_mode(skill: &str) -> &'static str {
    if skill == "research-os-doc-search"
        || skill == "research-os-search"
        || skill == "research-os-ingest"
    {
        "danger-full-access"
    } else {
        "workspace-write"
    }
}

fn agent_needs_search(skill: &str) -> bool {
    skill == "research-os-doc-search"
        || skill == "research-os-search"
        || skill == "research-os-ingest"
}

/// Phase 1 tool-scoping is opt-in until per-stage tool lists are validated
/// against every skill. Enable with `RESEARCH_OS_TOOL_SCOPING=1`.
fn tool_scoping_enabled() -> bool {
    env::var("RESEARCH_OS_TOOL_SCOPING")
        .map(|v| {
            let v = v.trim();
            !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")
        })
        .unwrap_or(false)
}

/// Comma-separated built-in tools a stage's `claude -p` run may use, passed to
/// `--allowedTools` under `--permission-mode dontAsk`. Phase 1 covers built-in
/// tools only; MCP loop tools (checkpoint/ledger/...) are layered in when the
/// sidecar lands. Lists follow the per-stage action-space matrix and must stay
/// at least as permissive as each skill's contract, or `dontAsk` will silently
/// deny a tool the stage legitimately needs.
fn claude_allowed_tools(skill: &str) -> &'static str {
    match skill {
        // Acquisition: web fetch/search + write raw/manifests + run the
        // render/extract helper scripts via Bash.
        "research-os-doc-search" | "research-os-search" | "research-os-ingest" => {
            "Read,Glob,Grep,Write,Edit,Bash,WebSearch,WebFetch"
        }
        // Code: full local file editing + shell.
        "research-os-coding" => "Read,Glob,Grep,Write,Edit,Bash",
        // Visualization: read evidence + write figures + run plotting.
        "research-os-visualization" => "Read,Glob,Grep,Write,Bash",
        // Read-only review + the driver's checkpoint stages (they only read
        // state and call MCP tools to route; they write nothing to the FS).
        "research-os-lint-critic"
        | "research-os-checkpoint-low"
        | "research-os-checkpoint-high" => "Read,Glob,Grep",
        // Everything else (wiki-update, synthesis, reader, qa, discussion,
        // writing, ideation, experiment-planning, planner, source-triage):
        // read + write/edit markdown artifacts; no shell or web.
        _ => "Read,Glob,Grep,Write,Edit",
    }
}

/// Fully-qualified MCP sidecar tool names (`mcp__researchos__<tool>`) a stage
/// may call, for `--allowedTools` under scoping. Only the file-based tools that
/// exist today (Phase 2b-1) are listed; more land as the sidecar grows. The
/// server key must match the one written by [`session_mcp_config_path`].
fn claude_mcp_tools(skill: &str) -> &'static str {
    match skill {
        // DISCUSS hub: read state, raise hypotheses, read coverage gaps, and
        // ask the human at phase boundaries.
        "research-os-discussion" => concat!(
            "mcp__researchos__ledger_read,",
            "mcp__researchos__propose_experiment,",
            "mcp__researchos__coverage_report,",
            "mcp__researchos__graph_query,",
            "mcp__researchos__checkpoint_ask"
        ),
        // POST / wiki writers: promote experiment results into the wiki.
        "research-os-writing" | "research-os-wiki-update" => {
            "mcp__researchos__ledger_read,mcp__researchos__capture_results"
        }
        // EXPERIMENT planning + read-mostly stages: read state + coverage.
        "research-os-experiment-planning" | "research-os-qa" | "research-os-ideation" => {
            "mcp__researchos__ledger_read,mcp__researchos__coverage_report"
        }
        // Other knowledge stages: read ledger state.
        "research-os-visualization" | "research-os-synthesis" | "research-os-coding" => {
            "mcp__researchos__ledger_read"
        }
        // Driver checkpoint stages: read coverage + ask the human. Only the
        // low-stakes variant gets phase_route (self-routing); withholding it
        // from the high-stakes variant is how "high-stakes always human" is
        // enforced by construction.
        "research-os-checkpoint-low" => concat!(
            "mcp__researchos__ledger_read,",
            "mcp__researchos__coverage_report,",
            "mcp__researchos__checkpoint_ask,",
            "mcp__researchos__phase_route"
        ),
        "research-os-checkpoint-high" => concat!(
            "mcp__researchos__ledger_read,",
            "mcp__researchos__coverage_report,",
            "mcp__researchos__checkpoint_ask"
        ),
        _ => "",
    }
}

/// Write (idempotently) the per-session MCP config that tells a `claude -p`
/// stage to spawn the local sidecar over stdio, and return its path. The server
/// command is this same binary's `__mcp <session_id>` subcommand. Returns None
/// if the executable path can't be resolved or the file can't be written, in
/// which case the caller simply skips `--mcp-config`.
fn session_mcp_config_path(root: &Path, session_id: &str) -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let cfg = serde_json::json!({
        "mcpServers": {
            "researchos": {
                "type": "stdio",
                "command": exe.to_string_lossy(),
                "args": ["__mcp", session_id],
            }
        }
    });
    let path = root.join("sessions").join(session_id).join(".mcp.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    fs::write(&path, serde_json::to_string_pretty(&cfg).ok()?).ok()?;
    Some(path)
}

fn is_deprecated_skill(skill: &str) -> bool {
    DEPRECATED_SKILLS
        .iter()
        .any(|deprecated| deprecated == &skill)
}

fn stream_lines_tui<R: io::Read>(
    reader: R,
    file: Arc<Mutex<File>>,
    tx: Sender<UiMsg>,
) -> Result<(), String> {
    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|e| e.to_string())?;
        {
            let mut file = file.lock().map_err(|_| "transcript lock poisoned")?;
            writeln!(file, "{line}").map_err(|e| e.to_string())?;
        }
        if let Some(display) = compact_agent_line(&line) {
            if let Some(question) = parse_plan_question(&display) {
                tx.send(UiMsg::PlanQuestion(question))
                    .map_err(|e| e.to_string())?;
            } else {
                tx.send(UiMsg::Line(display)).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

fn parse_plan_question(line: &str) -> Option<PlanQuestion> {
    let final_marker = "PLAN_CONFIRM:";
    let question_marker = "PLAN_QUESTION:";
    let (marker, final_confirmation) = if line.contains(final_marker) {
        (final_marker, true)
    } else {
        (question_marker, false)
    };
    let marker_pos = line.find(marker)?;
    let payload = line[marker_pos + marker.len()..].trim();
    let parts: Vec<_> = payload
        .split("||")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    Some(PlanQuestion {
        question: parts[0].to_string(),
        options: parts[1..].iter().map(|part| (*part).to_string()).collect(),
        final_confirmation,
    })
}

fn compact_agent_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.starts_with('{') {
        return Some(trimmed.to_string());
    }

    // Claude stream-json events use a `type` namespace disjoint from codex
    // (assistant/system/user/result/rate_limit_event vs thread/turn/item.*),
    // so detect by content rather than threading the backend here. This also
    // renders historical transcripts from either backend correctly.
    if trimmed.contains("\"type\":\"assistant\"")
        || trimmed.contains("\"type\":\"result\"")
        || trimmed.contains("\"rate_limit_event\"")
        || (trimmed.contains("\"type\":\"system\"") && trimmed.contains("\"session_id\""))
        || (trimmed.contains("\"type\":\"user\"") && trimmed.contains("\"session_id\""))
    {
        return compact_claude_line(trimmed);
    }

    if trimmed.contains("\"type\":\"thread.")
        || trimmed.contains("\"type\":\"turn.")
        || trimmed.contains("\"type\":\"item.started\"")
    {
        return None;
    }

    if trimmed.contains("\"type\":\"item.completed\"") && !trimmed.contains("\"agent_message\"") {
        if trimmed.contains("\"file_change\"") {
            return extract_json_string_field(trimmed, "\"path\"")
                .map(|path| format!("wrote {}", shorten_middle(&path, 80)));
        }
        return None;
    }

    if trimmed.contains("\"type\":\"item.completed\"") && trimmed.contains("\"agent_message\"") {
        if let Some(value) = extract_json_string_field(trimmed, "\"text\"") {
            let value = value.trim();
            if value.is_empty() {
                return None;
            }
            if is_progress_agent_message(value) {
                return Some(format!("thinking: {value}"));
            }
            return Some(format!("answer: {value}"));
        }
    }

    for key in ["\"message\"", "\"content\"", "\"text\"", "\"delta\""] {
        if let Some(value) = extract_json_string_field(trimmed, key) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// Compact one Claude Code `--output-format stream-json` event into a display
/// line, mirroring the codex path's thinking/answer/wrote vocabulary.
fn compact_claude_line(trimmed: &str) -> Option<String> {
    // Session lifecycle, rate-limit, and replayed user events carry no
    // user-facing content. The terminal `result` event duplicates the last
    // assistant text, so drop it to avoid printing the answer twice.
    if trimmed.contains("\"type\":\"system\"")
        || trimmed.contains("\"type\":\"result\"")
        || trimmed.contains("\"type\":\"user\"")
        || trimmed.contains("\"rate_limit_event\"")
    {
        return None;
    }
    if !trimmed.contains("\"type\":\"assistant\"") {
        return None;
    }
    // Reasoning block.
    if trimmed.contains("\"type\":\"thinking\"") {
        return extract_json_string_field(trimmed, "\"thinking\"").and_then(|value| {
            let value = value.trim();
            if value.is_empty() {
                None
            } else {
                Some(format!("thinking: {value}"))
            }
        });
    }
    // Tool call (file writes, bash, search, ...).
    if trimmed.contains("\"type\":\"tool_use\"") {
        let name = extract_json_string_field(trimmed, "\"name\"").unwrap_or_default();
        if matches!(name.as_str(), "Write" | "Edit" | "NotebookEdit") {
            if let Some(path) = extract_json_string_field(trimmed, "\"file_path\"")
                .or_else(|| extract_json_string_field(trimmed, "\"path\""))
            {
                return Some(format!("wrote {}", shorten_middle(&path, 80)));
            }
        }
        if name.is_empty() {
            return None;
        }
        return Some(format!("running {name}"));
    }
    // Text block: distinguish progress narration from the actual answer.
    if let Some(value) = extract_json_string_field(trimmed, "\"text\"") {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        if is_progress_agent_message(value) {
            return Some(format!("thinking: {value}"));
        }
        return Some(format!("answer: {value}"));
    }
    None
}

fn is_progress_agent_message(value: &str) -> bool {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    if trimmed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
        > 1
    {
        return false;
    }
    if trimmed.chars().count() > 240 {
        return false;
    }
    if trimmed.contains("\n- ") || trimmed.contains("\n1.") {
        return false;
    }
    let progress_markers = [
        "i'll ",
        "i will ",
        "i’m ",
        "i'm ",
        "i have ",
        "i searched ",
        "i wrote ",
        "i read ",
        "i’ll ",
        "using ",
        "reading ",
        "checking ",
        "wrote ",
        "validated ",
        "completed ",
    ];
    progress_markers
        .iter()
        .any(|marker| lower.starts_with(marker) || lower.contains(marker))
        || trimmed.contains("작성")
        || trimmed.contains("확인")
        || trimmed.contains("읽")
        || trimmed.contains("검증")
        || trimmed.contains("완료")
        || trimmed.contains("쓰")
}

fn extract_json_string_field(input: &str, key: &str) -> Option<String> {
    let key_pos = input.find(key)?;
    let after_key = &input[key_pos + key.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }

    let mut result = String::new();
    let mut escaped = false;
    for ch in after_colon[1..].chars() {
        if escaped {
            match ch {
                'n' => result.push('\n'),
                't' => result.push('\t'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                other => result.push(other),
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(result);
        } else {
            result.push(ch);
        }
    }
    None
}

#[allow(dead_code)]
fn extract_json_string_array_field(input: &str, key: &str) -> Vec<String> {
    let Some(key_pos) = input.find(key) else {
        return Vec::new();
    };
    let after_key = &input[key_pos + key.len()..];
    let Some(open_rel) = after_key.find('[') else {
        return Vec::new();
    };
    let array = &after_key[open_rel..];
    let Some(close_idx) = find_matching_bracket(array) else {
        return Vec::new();
    };
    extract_json_strings(&array[..=close_idx])
}

#[allow(dead_code)]
fn stream_lines<R: io::Read>(
    reader: R,
    file: Arc<Mutex<File>>,
    stderr: bool,
) -> Result<(), String> {
    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if stderr {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
        let mut file = file.lock().map_err(|_| "transcript lock poisoned")?;
        writeln!(file, "{line}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn extract_stage_skills(plan: &str) -> Vec<&'static str> {
    let mut stages = Vec::new();
    let mut start = 0usize;
    while let Some(rel_pos) = plan[start..].find("\"agent_skill\"") {
        let pos = start + rel_pos;
        if let Some(skill) = extract_json_string_field(&plan[pos..], "\"agent_skill\"") {
            for known in SKILLS {
                if *known == skill {
                    stages.push(*known);
                    break;
                }
            }
        }
        start = pos + "\"agent_skill\"".len();
    }
    stages
}

fn extract_stage_plan_summaries(plan: &str) -> Vec<StagePlanSummary> {
    let mut stages = Vec::new();
    let mut start = 0usize;
    while let Some(rel_pos) = plan[start..].find("\"agent_skill\"") {
        let pos = start + rel_pos;
        let next_pos = plan[pos + "\"agent_skill\"".len()..]
            .find("\"agent_skill\"")
            .map(|next_rel| pos + "\"agent_skill\"".len() + next_rel)
            .unwrap_or(plan.len());
        let stage_slice = &plan[pos..next_pos];
        if let Some(skill_name) = extract_json_string_field(stage_slice, "\"agent_skill\"") {
            if let Some(skill) = SKILLS.iter().copied().find(|known| *known == skill_name) {
                stages.push(StagePlanSummary {
                    skill,
                    reason: extract_json_string_field(stage_slice, "\"reason\"")
                        .unwrap_or_else(|| "no reason provided".into()),
                    inputs: extract_json_string_array_field(stage_slice, "\"inputs\""),
                    outputs: extract_json_string_array_field(stage_slice, "\"outputs\""),
                });
            }
        }
        start = pos + "\"agent_skill\"".len();
    }
    stages
}

fn format_stage_io_line(stage: &StagePlanSummary) -> String {
    let inputs = compact_path_list(&stage.inputs, 2);
    let outputs = compact_path_list(&stage.outputs, 2);
    match (inputs.is_empty(), outputs.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("inputs: {inputs}"),
        (true, false) => format!("outputs: {outputs}"),
        (false, false) => format!("inputs: {inputs} -> outputs: {outputs}"),
    }
}

fn compact_path_list(paths: &[String], limit: usize) -> String {
    let mut values = paths
        .iter()
        .take(limit)
        .map(|path| truncate_label(path, 52))
        .collect::<Vec<_>>();
    if paths.len() > limit {
        values.push(format!("+{} more", paths.len() - limit));
    }
    values.join(", ")
}

fn validate_stage_output(
    root: &Path,
    run_id: &str,
    skill: &str,
    occurrence: usize,
) -> Result<StageOutput, String> {
    let run_dir = root.join("runs").join(run_id);
    match skill {
        "research-os-doc-search" => {
            validate_planned_outputs(root, run_id, skill, occurrence)
                .or_else(|_| validate_artifact(&run_dir.join("doc_search_manifest.json")))?;
            validate_doc_search_manifest(root, run_id)?;
            Ok(StageOutput::None)
        }
        "research-os-search" => {
            validate_planned_outputs(root, run_id, skill, occurrence)
                .or_else(|_| validate_artifact(&run_dir.join("search_results.json")))?;
            Ok(StageOutput::None)
        }
        "research-os-source-triage" => {
            validate_planned_outputs(root, run_id, skill, occurrence).or_else(|_| {
                validate_artifact(&run_dir.join("selected_sources.json"))
                    .or_else(|_| validate_artifact(&run_dir.join("triage.json")))
            })?;
            Ok(StageOutput::None)
        }
        "research-os-ingest" => {
            validate_planned_outputs(root, run_id, skill, occurrence)
                .or_else(|_| validate_artifact(&run_dir.join("ingest_manifest.json")))?;
            validate_ingest_pdf_discovery(root, run_id)?;
            Ok(StageOutput::None)
        }
        "research-os-reader" => {
            validate_any_markdown(
                &root.join("runs").join(run_id).join("wiki/sources"),
                "source note",
            )?;
            Ok(StageOutput::None)
        }
        "research-os-wiki-update" => {
            validate_planned_outputs(root, run_id, skill, occurrence)
                .or_else(|_| validate_artifact(&run_dir.join("wiki_update_report.md")))?;
            Ok(StageOutput::None)
        }
        "research-os-synthesis" => {
            validate_planned_outputs(root, run_id, skill, occurrence).or_else(|_| {
                validate_any_markdown(
                    &root.join("runs").join(run_id).join("wiki/synthesis"),
                    "synthesis page",
                )
            })?;
            Ok(StageOutput::None)
        }
        "research-os-lint-critic" => {
            validate_planned_outputs(root, run_id, skill, occurrence).or_else(|_| {
                validate_artifact(&run_dir.join("lint_report.md"))
                    .or_else(|_| validate_artifact(&run_dir.join("lint_report.json")))
            })?;
            Ok(StageOutput::None)
        }
        "research-os-qa"
        | "research-os-discussion"
        | "research-os-ideation"
        | "research-os-writing"
        | "research-os-visualization"
        | "research-os-coding"
        | "research-os-experiment-planning" => {
            let final_answer = run_dir.join("final_answer.md");
            if final_answer.exists() {
                validate_artifact(&final_answer)?;
                Ok(StageOutput::None)
            } else if validate_planned_outputs(root, run_id, skill, occurrence).is_ok() {
                Ok(StageOutput::None)
            } else {
                validate_any_markdown(
                    &root.join("runs").join(run_id).join("wiki/outputs"),
                    "output page",
                )?;
                Ok(StageOutput::None)
            }
        }
        _ => Ok(StageOutput::None),
    }
}

fn validate_artifact(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("missing required artifact: {}", path.display()));
    }
    let metadata = fs::metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    if metadata.len() == 0 {
        return Err(format!("empty required artifact: {}", path.display()));
    }
    Ok(())
}

fn validate_ingest_pdf_discovery(root: &Path, run_id: &str) -> Result<(), String> {
    let manifest_path = root.join("runs").join(run_id).join("ingest_manifest.json");
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read ingest manifest {}: {e}", manifest_path.display()))?;

    let downloaded_section = json_array_section(&manifest, "\"downloaded_sources\"").unwrap_or("");
    let mut start = 0usize;
    while let Some(source_rel) = downloaded_section[start..].find("\"source_id\"") {
        let source_pos = start + source_rel;
        let item_end = downloaded_section[source_pos..]
            .find("\"raw_path\"")
            .map(|rel| source_pos + rel)
            .unwrap_or(downloaded_section.len());
        let item = &downloaded_section[source_pos..item_end];
        let source_id =
            extract_json_string_field(item, "\"source_id\"").unwrap_or_else(|| "unknown".into());
        let download_status =
            extract_json_string_field(item, "\"pdf_download_status\"").unwrap_or_default();
        let raw_format = extract_json_string_field(item, "\"raw_format\"").unwrap_or_default();
        let is_pdf_failure = raw_format != "pdf"
            && matches!(download_status.as_str(), "not_available" | "failed" | "");

        if is_pdf_failure && !item.contains("\"pdf_discovery_queries\"") {
            return Err(format!(
                "ingest fallback for {source_id} missing pdf_discovery_queries"
            ));
        }
        if is_pdf_failure
            && (item.contains("\"pdf_discovery_queries\": []")
                || item.contains("\"pdf_discovery_queries\":[]"))
        {
            return Err(format!(
                "ingest fallback for {source_id} has empty pdf_discovery_queries"
            ));
        }
        if is_pdf_failure && !item.contains("\"pdf_candidate_urls\"") {
            return Err(format!(
                "ingest fallback for {source_id} missing pdf_candidate_urls"
            ));
        }

        start = item_end.saturating_add(1);
    }
    Ok(())
}

fn validate_doc_search_manifest(root: &Path, run_id: &str) -> Result<(), String> {
    let manifest_path = root
        .join("runs")
        .join(run_id)
        .join("doc_search_manifest.json");
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read doc search manifest {}: {e}", manifest_path.display()))?;

    for required in [
        "\"source_scope_obeyed\"",
        "\"candidates\"",
        "\"selected_sources\"",
        "\"downloaded_sources\"",
        "\"coverage_gaps\"",
    ] {
        if !manifest.contains(required) {
            return Err(format!(
                "doc_search_manifest.json missing required field {required}"
            ));
        }
    }
    if manifest.contains("\"source_scope_obeyed\": false") {
        return Err("doc_search_manifest.json reports source_scope_obeyed=false".into());
    }
    let has_download = manifest.contains("\"downloaded_sources\": [")
        && !manifest.contains("\"downloaded_sources\": []")
        && !manifest.contains("\"downloaded_sources\":[]");
    let has_selected = manifest.contains("\"selected_sources\": [")
        && !manifest.contains("\"selected_sources\": []")
        && !manifest.contains("\"selected_sources\":[]");
    let has_gap = manifest.contains("\"coverage_gaps\": [")
        && !manifest.contains("\"coverage_gaps\": []")
        && !manifest.contains("\"coverage_gaps\":[]");
    if !has_download && !has_selected && !has_gap {
        return Err(
            "doc_search_manifest.json must include selected/downloaded sources or coverage_gaps"
                .into(),
        );
    }

    let downloaded_section = json_array_section(&manifest, "\"downloaded_sources\"").unwrap_or("");
    let mut start = 0usize;
    while let Some(source_rel) = downloaded_section[start..].find("\"source_id\"") {
        let source_pos = start + source_rel;
        let item_end = downloaded_section[source_pos..]
            .find("\"raw_path\"")
            .map(|rel| source_pos + rel)
            .unwrap_or(downloaded_section.len());
        let item = &downloaded_section[source_pos..item_end];
        let source_id =
            extract_json_string_field(item, "\"source_id\"").unwrap_or_else(|| "unknown".into());
        let download_status =
            extract_json_string_field(item, "\"download_status\"").unwrap_or_default();
        let raw_format = extract_json_string_field(item, "\"raw_format\"").unwrap_or_default();
        let is_file_failure = raw_format != "pdf"
            && matches!(download_status.as_str(), "not_available" | "failed" | "");

        if is_file_failure && !item.contains("\"pdf_discovery_queries\"") {
            return Err(format!(
                "doc-search fallback for {source_id} missing pdf_discovery_queries"
            ));
        }
        if is_file_failure
            && (item.contains("\"pdf_discovery_queries\": []")
                || item.contains("\"pdf_discovery_queries\":[]"))
        {
            return Err(format!(
                "doc-search fallback for {source_id} has empty pdf_discovery_queries"
            ));
        }
        if is_file_failure && !item.contains("\"pdf_candidate_urls\"") {
            return Err(format!(
                "doc-search fallback for {source_id} missing pdf_candidate_urls"
            ));
        }

        start = item_end.saturating_add(1);
    }
    Ok(())
}

fn json_array_section<'a>(input: &'a str, key: &str) -> Option<&'a str> {
    let key_pos = input.find(key)?;
    let after_key = &input[key_pos + key.len()..];
    let open_rel = after_key.find('[')?;
    let array = &after_key[open_rel..];
    let close_idx = find_matching_bracket(array)?;
    Some(&array[..=close_idx])
}

fn validate_planned_outputs(
    root: &Path,
    run_id: &str,
    skill: &str,
    occurrence: usize,
) -> Result<(), String> {
    let plan_path = run_plan_json_path(root, run_id);
    let plan = fs::read_to_string(&plan_path)
        .map_err(|e| format!("read plan for output validation: {e}"))?;
    let outputs = extract_stage_outputs(&plan, skill, occurrence);
    let concrete: Vec<_> = outputs
        .into_iter()
        .filter(|path| !path.contains('{') && !path.contains('}'))
        .collect();
    if concrete.is_empty() {
        return Err(format!("no concrete planned outputs for {skill}"));
    }

    for output in &concrete {
        let path = run_artifact_path(root, run_id, output);
        if output.ends_with('/') {
            if !path.is_dir() {
                return Err(format!(
                    "missing planned output directory: {}",
                    path.display()
                ));
            }
        } else {
            validate_artifact(&path)?;
        }
    }
    Ok(())
}

fn run_artifact_path(root: &Path, run_id: &str, rel: &str) -> PathBuf {
    let path = PathBuf::from(rel);
    if path.is_absolute() {
        return path;
    }
    if rel.starts_with("raw/")
        || rel.starts_with("artifacts/")
        || rel.starts_with("plan/")
        || rel.starts_with("wiki/")
        || rel.starts_with("schemas/")
    {
        root.join("runs").join(run_id).join(rel)
    } else {
        root.join(rel)
    }
}

fn run_plan_json_path(root: &Path, run_id: &str) -> PathBuf {
    let run_dir = root.join("runs").join(run_id);
    let canonical = run_plan_json_canonical_path(root, run_id);
    if canonical.exists() {
        canonical
    } else {
        run_dir.join("plan.json")
    }
}

fn run_plan_json_canonical_path(root: &Path, run_id: &str) -> PathBuf {
    root.join("runs")
        .join(run_id)
        .join("plan")
        .join("plan.json")
}

#[allow(dead_code)]
fn run_plan_summary_path(root: &Path, run_id: &str) -> PathBuf {
    root.join("runs").join(run_id).join("plan").join("plan.md")
}

fn extract_stage_outputs(plan: &str, skill: &str, occurrence: usize) -> Vec<String> {
    let Some(stage_pos) = find_stage_agent_skill_pos(plan, skill, occurrence) else {
        return Vec::new();
    };
    let after_skill = &plan[stage_pos..];
    let Some(outputs_pos) = after_skill.find("\"outputs\"") else {
        return Vec::new();
    };
    let after_outputs = &after_skill[outputs_pos..];
    let Some(open_rel) = after_outputs.find('[') else {
        return Vec::new();
    };
    let array = &after_outputs[open_rel..];
    let Some(close_idx) = find_matching_bracket(array) else {
        return Vec::new();
    };
    extract_json_strings(&array[..=close_idx])
}

fn find_stage_agent_skill_pos(plan: &str, skill: &str, occurrence: usize) -> Option<usize> {
    let mut start = 0usize;
    let mut seen = 0usize;
    while let Some(rel_pos) = plan[start..].find("\"agent_skill\"") {
        let pos = start + rel_pos;
        if extract_json_string_field(&plan[pos..], "\"agent_skill\"").as_deref() == Some(skill) {
            if seen == occurrence {
                return Some(pos);
            }
            seen += 1;
        }
        start = pos + "\"agent_skill\"".len();
    }
    None
}

fn find_matching_bracket(input: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_json_strings(input: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '"' {
            continue;
        }
        let mut value = String::new();
        let mut escaped = false;
        for next in chars.by_ref() {
            if escaped {
                match next {
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    '"' => value.push('"'),
                    '\\' => value.push('\\'),
                    other => value.push(other),
                }
                escaped = false;
            } else if next == '\\' {
                escaped = true;
            } else if next == '"' {
                strings.push(value);
                break;
            } else {
                value.push(next);
            }
        }
    }
    strings
}

fn validate_plan(path: &Path) -> Result<(), String> {
    validate_artifact(path)?;
    let plan = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    for required in [
        "\"run_id\"",
        "\"user_instruction\"",
        "\"task_type\"",
        "\"source_scope\"",
        "\"stages\"",
        "\"final_expected_artifact\"",
    ] {
        if !plan.contains(required) {
            return Err(format!("plan.json missing required field {required}"));
        }
    }
    if !plan.contains("\"agent_skill\"") {
        return Err("plan.json must assign agent_skill for each stage".into());
    }
    if !plan.contains("\"expansion_policy\"") {
        return Err("plan.json must include source_scope.expansion_policy".into());
    }
    if plan.contains("\"session_id\"") || plan.contains("\"turn_type\"") {
        for required in [
            "\"turn_type\"",
            "\"wiki_sufficiency\"",
            "\"selected_agents\"",
            "\"selection_rationale\"",
        ] {
            if !plan.contains(required) {
                return Err(format!(
                    "multi-turn plan.json missing required field {required}"
                ));
            }
        }
    }
    Ok(())
}

fn final_expected_artifact_path(root: &Path, run_id: &str) -> Result<PathBuf, String> {
    let plan_path = run_plan_json_path(root, run_id);
    let plan = fs::read_to_string(&plan_path)
        .map_err(|e| format!("read plan for final artifact {}: {e}", plan_path.display()))?;
    let artifact = extract_json_string_field(&plan, "\"final_expected_artifact\"")
        .ok_or("plan.json missing final_expected_artifact")?
        .replace("{run_id}", run_id);
    Ok(run_artifact_path(root, run_id, &artifact))
}

fn emit_final_expected_artifact_tui(
    root: &Path,
    run_id: &str,
    tx: &Sender<UiMsg>,
) -> Result<(), String> {
    let path = final_expected_artifact_path(root, run_id)?;
    validate_artifact(&path)?;
    tx.send(UiMsg::Line("stage: final output".into()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!(
        "system: final artifact: {}",
        display_path(root, &path)
    )))
    .map_err(|e| e.to_string())?;

    if is_text_artifact(&path) {
        let answer = fs::read_to_string(&path)
            .map_err(|e| format!("read final artifact {}: {e}", path.display()))?;
        for line in answer.lines() {
            tx.send(UiMsg::Line(format!("answer: {line}")))
                .map_err(|e| e.to_string())?;
        }
    } else {
        tx.send(UiMsg::Line(format!(
            "system: final artifact is not a text previewable file: {}",
            path.display()
        )))
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[allow(dead_code)]
fn emit_final_expected_artifact_cli(root: &Path, run_id: &str) -> Result<(), String> {
    let path = final_expected_artifact_path(root, run_id)?;
    validate_artifact(&path)?;
    println!("\n=== final output: {} ===", display_path(root, &path));
    if is_text_artifact(&path) {
        let answer = fs::read_to_string(&path)
            .map_err(|e| format!("read final artifact {}: {e}", path.display()))?;
        println!("{answer}");
    } else {
        println!(
            "final artifact is not a text previewable file: {}",
            path.display()
        );
    }
    Ok(())
}

fn is_text_artifact(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str),
        Some("md" | "txt" | "json" | "csv" | "toml" | "yaml" | "yml")
    )
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[allow(dead_code)]
fn emit_plan_summary(root: &Path, run_id: &str, tx: &Sender<UiMsg>) -> Result<(), String> {
    let plan_path = run_plan_json_path(root, run_id);
    let plan = fs::read_to_string(&plan_path)
        .map_err(|e| format!("read plan for summary {}: {e}", plan_path.display()))?;
    let task_type = extract_json_string_field(&plan, "\"task_type\"").unwrap_or("unknown".into());
    let turn_type = extract_json_string_field(&plan, "\"turn_type\"").unwrap_or("unknown".into());
    let source_mode = extract_json_string_field(&plan, "\"mode\"").unwrap_or("unspecified".into());
    let expansion =
        extract_json_string_field(&plan, "\"expansion_policy\"").unwrap_or("unspecified".into());
    let rationale = extract_json_string_field(&plan, "\"selection_rationale\"")
        .unwrap_or_else(|| "not provided".into());
    let selected_agents = extract_json_string_array_field(&plan, "\"selected_agents\"");
    let final_artifact = extract_json_string_field(&plan, "\"final_expected_artifact\"")
        .unwrap_or_else(|| format!("runs/{run_id}/final_answer.md"));
    let stages = extract_stage_skills(&plan);

    let mut summary = String::new();
    summary.push_str("# Plan Summary\n\n");
    summary.push_str(&format!("- task: {task_type}\n"));
    summary.push_str(&format!("- turn: {turn_type}\n"));
    summary.push_str(&format!("- source scope: {source_mode}\n"));
    summary.push_str(&format!("- expansion: {expansion}\n"));
    if !selected_agents.is_empty() {
        summary.push_str(&format!(
            "- selected agents: {}\n",
            selected_agents
                .iter()
                .map(|agent| trim_skill_name(agent))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    summary.push_str(&format!("- rationale: {rationale}\n"));
    summary.push_str(&format!("- final artifact: {final_artifact}\n"));
    summary.push_str("- stages:\n");
    for stage in &stages {
        summary.push_str(&format!("  - {}\n", trim_skill_name(stage)));
    }

    let summary_path = run_plan_summary_path(root, run_id);
    fs::write(&summary_path, &summary)
        .map_err(|e| format!("write plan summary {}: {e}", summary_path.display()))?;

    tx.send(UiMsg::Line("stage: plan summary".into()))
        .map_err(|e| e.to_string())?;
    for line in summary.lines().filter(|line| !line.trim().is_empty()) {
        tx.send(UiMsg::Line(format!("system: {line}")))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn validate_any_markdown(dir: &Path, label: &str) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().extension() == Some(OsStr::new("md")) {
            return Ok(());
        }
    }
    Err(format!("missing {label} in {}", dir.display()))
}

fn validate_run(root: &Path, run_id: &str) -> Result<(), String> {
    let run_dir = root.join("runs").join(run_id);
    validate_plan(&run_plan_json_path(root, run_id))?;
    validate_artifact(&run_dir.join("wiki/index.md"))?;
    validate_artifact(&run_dir.join("wiki/log.md"))?;
    validate_artifact(&run_dir.join("wiki/followups.md"))?;
    println!("validation passed for {run_id}");
    Ok(())
}

fn list_run_artifacts(root: &Path, run_id: &str) -> Vec<String> {
    let run_dir = root.join("runs").join(run_id);
    let mut artifacts = Vec::new();
    collect_artifacts(root, &run_dir, &mut artifacts);
    artifacts.sort();
    artifacts
}

fn list_session_artifacts(root: &Path, session_id: &str) -> Vec<String> {
    let session_dir = root.join("sessions").join(session_id);
    let mut artifacts = Vec::new();
    collect_artifacts(root, &session_dir, &mut artifacts);
    artifacts.sort();
    artifacts
}

fn collect_artifacts(root: &Path, dir: &Path, artifacts: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_artifacts(root, &path, artifacts);
        } else if path.file_name() != Some(OsStr::new(".gitkeep")) {
            let display = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            artifacts.push(display);
        }
    }
}

fn snapshot_wiki(root: &Path, run_id: &str) -> Result<HashMap<PathBuf, u64>, String> {
    let mut map = HashMap::new();
    collect_snapshot(&root.join("runs").join(run_id).join("wiki"), &mut map)?;
    Ok(map)
}

fn collect_snapshot(dir: &Path, map: &mut HashMap<PathBuf, u64>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if is_ignored_metadata_path(&path) {
            continue;
        }
        if path.is_dir() {
            collect_snapshot(&path, map)?;
        } else {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            map.insert(path, modified);
        }
    }
    Ok(())
}

fn is_ignored_metadata_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == ".DS_Store" || name.starts_with("._"))
        .unwrap_or(false)
}

fn enforce_wiki_mutation(
    root: &Path,
    run_id: &str,
    skill: &str,
    before: &HashMap<PathBuf, u64>,
) -> Result<(), String> {
    if skill == "research-os-wiki-update"
        || skill == "research-os-reader"
        || skill == "research-os-synthesis"
    {
        return Ok(());
    }
    let allowed = planned_output_paths(root, run_id, skill);
    let after = snapshot_wiki(root, run_id)?;
    for (path, modified) in after {
        if allowed.iter().any(|allowed_path| allowed_path == &path) {
            continue;
        }
        if before.get(&path).copied() != Some(modified) {
            return Err(format!(
                "{skill} modified wiki without permission: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn planned_output_paths(root: &Path, run_id: &str, skill: &str) -> Vec<PathBuf> {
    let plan_path = run_plan_json_path(root, run_id);
    let Ok(plan) = fs::read_to_string(plan_path) else {
        return Vec::new();
    };
    let mut outputs = Vec::new();
    let mut occurrence = 0usize;
    loop {
        let stage_outputs = extract_stage_outputs(&plan, skill, occurrence);
        if stage_outputs.is_empty() {
            break;
        }
        outputs.extend(stage_outputs);
        occurrence += 1;
    }
    outputs
        .into_iter()
        .filter(|path| !path.contains('{') && !path.contains('}') && !path.ends_with('/'))
        .map(|path| run_artifact_path(root, run_id, &path))
        .collect()
}

fn install_skills(root: &Path) -> Result<(), String> {
    let codex_home = env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".codex"));
    let target = codex_home.join("skills");
    fs::create_dir_all(&target).map_err(|e| format!("create {}: {e}", target.display()))?;
    for skill in SKILLS {
        let src = root.join("skills").join(skill);
        if src.exists() {
            copy_dir(&src, &target.join(skill))?;
            println!("installed {skill}");
        }
    }
    Ok(())
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("create {}: {e}", dst.display()))?;
    for entry in fs::read_dir(src).map_err(|e| format!("read {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .map_err(|e| format!("copy {} to {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

fn home_dir() -> PathBuf {
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn make_run_id(root: &Path, instruction: &str) -> String {
    let base = instruction_slug(instruction);
    let millis = current_millis();
    let mut candidate = format!("{base}-{millis}");
    let mut counter = 2usize;
    while root.join("runs").join(&candidate).exists() {
        candidate = format!("{base}-{millis}-{counter}");
        counter += 1;
    }
    candidate
}

fn instruction_slug(instruction: &str) -> String {
    let title = instruction
        .split("\n\nUser-configured source scope constraint from TUI /scope:")
        .next()
        .unwrap_or(instruction);
    let mut slug = String::new();
    let mut last_was_separator = false;
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                slug.push(lower);
            }
            last_was_separator = false;
        } else if !last_was_separator && !slug.is_empty() {
            slug.push('-');
            last_was_separator = true;
        }
        if slug.chars().count() >= 56 {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "research".into()
    } else {
        slug
    }
}

fn current_millis() -> u128 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    millis
}

fn make_turn_id() -> String {
    let millis = current_millis();
    format!("turn-{millis}")
}

fn make_session_id() -> String {
    let millis = current_millis();
    format!("session-{millis}")
}

fn make_session_id_from_instruction(root: &Path, instruction: &str) -> String {
    let base = instruction_slug(instruction);
    let mut candidate = base.clone();
    let mut counter = 2usize;
    while root.join("sessions").join(&candidate).exists() {
        candidate = format!("{base}-{counter}");
        counter += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    // Claude `--output-format stream-json` events (shapes captured from the
    // real CLI). compact_agent_line must content-detect these and map them to
    // the shared thinking/answer/wrote vocabulary.
    #[test]
    fn claude_text_event_is_answer() {
        let line = r#"{"type":"assistant","message":{"model":"x","content":[{"type":"text","text":"hello world"}]},"session_id":"s"}"#;
        assert_eq!(compact_agent_line(line), Some("answer: hello world".into()));
    }

    #[test]
    fn claude_thinking_event_is_thinking() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Let me think"}]},"session_id":"s"}"#;
        assert_eq!(compact_agent_line(line), Some("thinking: Let me think".into()));
    }

    #[test]
    fn claude_tool_use_write_is_wrote() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"sessions/s/wiki/outputs/t.md"}}]},"session_id":"s"}"#;
        assert_eq!(
            compact_agent_line(line),
            Some("wrote sessions/s/wiki/outputs/t.md".into())
        );
    }

    #[test]
    fn claude_lifecycle_events_are_filtered() {
        let init = r#"{"type":"system","subtype":"init","session_id":"s","tools":[]}"#;
        let result = r#"{"type":"result","subtype":"success","result":"hello world","session_id":"s"}"#;
        let rate = r#"{"type":"rate_limit_event","rate_limit_info":{},"session_id":"s"}"#;
        assert_eq!(compact_agent_line(init), None);
        assert_eq!(compact_agent_line(result), None);
        assert_eq!(compact_agent_line(rate), None);
    }

    // Codex `--json` events must still parse unchanged (no regression).
    #[test]
    fn codex_agent_message_still_answer() {
        let line = r#"{"type":"item.completed","item":{"type":"agent_message"},"text":"final answer"}"#;
        assert_eq!(compact_agent_line(line), Some("answer: final answer".into()));
    }

    #[test]
    fn codex_thread_event_still_filtered() {
        let line = r#"{"type":"thread.started","thread_id":"abc"}"#;
        assert_eq!(compact_agent_line(line), None);
    }

    #[test]
    fn claude_command_is_headless_streaming() {
        let cmd = build_agent_command(
            Backend::Claude,
            "research-os-discussion",
            Path::new("/tmp/root"),
            "do the thing",
            None,
        );
        assert_eq!(cmd.get_program().to_str().unwrap(), "claude");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.contains(&"do the thing".to_string()));
        // Claude must not receive codex-only flags.
        assert!(!args.iter().any(|a| a == "--sandbox" || a == "exec"));
    }

    #[test]
    fn codex_command_keeps_sandbox_and_search() {
        let cmd = build_agent_command(
            Backend::Codex,
            "research-os-doc-search",
            Path::new("/tmp/root"),
            "find papers",
            None,
        );
        assert_eq!(cmd.get_program().to_str().unwrap(), "codex");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"exec".to_string()));
        assert!(args.contains(&"--search".to_string()));
        assert!(args.contains(&"--sandbox".to_string()));
        assert!(args.contains(&"danger-full-access".to_string()));
    }

    #[test]
    fn claude_allowed_tools_scopes_per_stage() {
        // Acquisition gets web + bash; review is read-only; the default writes
        // markdown but gets no shell or web.
        let acq = claude_allowed_tools("research-os-doc-search");
        assert!(acq.contains("WebSearch") && acq.contains("Bash"));
        assert_eq!(claude_allowed_tools("research-os-lint-critic"), "Read,Glob,Grep");
        assert!(claude_allowed_tools("research-os-coding").contains("Bash"));
        let disc = claude_allowed_tools("research-os-discussion");
        assert!(disc.contains("Read") && disc.contains("Write") && !disc.contains("Bash"));
    }

    #[test]
    fn discussion_gets_propose_experiment_mcp_tool() {
        assert!(claude_mcp_tools("research-os-discussion").contains("propose_experiment"));
        assert!(claude_mcp_tools("research-os-wiki-update").contains("ledger_read"));
        assert_eq!(claude_mcp_tools("research-os-doc-search"), "");
    }

    #[test]
    fn checkpoint_stage_stakes_gating() {
        // The enforcement boundary: only the low-stakes checkpoint stage may
        // self-route (phase_route); the high-stakes one can only ask the human.
        let low = claude_mcp_tools("research-os-checkpoint-low");
        let high = claude_mcp_tools("research-os-checkpoint-high");
        assert!(low.contains("phase_route") && low.contains("checkpoint_ask"));
        assert!(!high.contains("phase_route") && high.contains("checkpoint_ask"));
        // Checkpoint stages are read-only on the filesystem.
        assert_eq!(claude_allowed_tools("research-os-checkpoint-high"), "Read,Glob,Grep");
    }

    #[test]
    fn loop_phase_work_stages_per_phase() {
        use crate::ledger::Phase;
        assert_eq!(
            loop_phase_work_stages(Phase::Init),
            vec!["research-os-doc-search", "research-os-wiki-update"]
        );
        assert_eq!(
            loop_phase_work_stages(Phase::Discuss),
            vec!["research-os-discussion"]
        );
        assert!(loop_phase_work_stages(Phase::Experiment).contains(&"research-os-coding"));
        assert_eq!(loop_phase_work_stages(Phase::Post), vec!["research-os-writing"]);
    }

    #[test]
    fn session_mcp_config_points_at_sidecar() {
        let dir = std::env::temp_dir().join(format!("ros-mcp-{}", crate::ledger::now_ms()));
        let path = session_mcp_config_path(&dir, "sess-1").expect("write config");
        let text = std::fs::read_to_string(&path).expect("read config");
        // Spawns this binary's __mcp subcommand for the session, under the
        // server key that claude_mcp_tools' mcp__researchos__ names reference.
        assert!(text.contains("researchos"));
        assert!(text.contains("__mcp"));
        assert!(text.contains("sess-1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_server_round_trips_with_sidecar_client() {
        use std::time::Duration;
        let dir = std::env::temp_dir().join(format!("ros-cp-{}", crate::ledger::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = mpsc::channel::<UiMsg>();
        spawn_checkpoint_server(dir.clone(), tx);

        // Sidecar-side client: retry until the listener is bound, then ask.
        let dir2 = dir.clone();
        let client = thread::spawn(move || {
            let req = crate::checkpoint_ipc::CheckpointRequest {
                question: "Advance?".to_string(),
                assessment: Some("a hypothesis crystallized".to_string()),
                options: vec!["stay".to_string(), "advance".to_string()],
            };
            for _ in 0..100 {
                if let Ok(r) = crate::checkpoint_ipc::ask(&dir2, &req) {
                    return Some(r);
                }
                thread::sleep(Duration::from_millis(20));
            }
            None
        });

        // TUI-side: receive the surfaced question and answer with option index 1.
        let msg = rx.recv_timeout(Duration::from_secs(5)).expect("checkpoint msg");
        match msg {
            UiMsg::Checkpoint(pq, responder) => {
                assert!(pq.question.contains("Advance?"));
                assert_eq!(pq.options.len(), 2);
                responder.send(1).unwrap();
            }
            _ => panic!("expected a Checkpoint message"),
        }
        let r = client.join().unwrap().expect("client got a response");
        assert_eq!(r.chosen, 1);
        assert_eq!(r.label, "advance");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backend_parsing_roundtrip() {
        assert_eq!(parse_backend("claude"), Some(Backend::Claude));
        assert_eq!(parse_backend(" CODEX "), Some(Backend::Codex));
        assert_eq!(parse_backend("gpt"), None);
        assert_eq!(backend_label(Backend::Claude), "claude");
        assert_eq!(backend_label(Backend::Codex), "codex");
    }
}
