use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
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

const SKILLS: &[&str] = &[
    "research-os-planner",
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

const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/help", "show commands"),
    ("/skills", "list agent skills"),
    ("/scope", "set or show search scope"),
    ("/runs", "list recent runs"),
    ("/resume", "resume a run"),
    ("/artifacts", "refresh artifacts"),
    ("/open", "preview artifact"),
    ("/pane", "focus a pane"),
    ("/focus", "focus a pane"),
    ("/sidebar", "toggle sidebar"),
    ("/clear", "clear conversation"),
    ("/exit", "exit TUI"),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusPane {
    Conversation,
    Pipeline,
    Artifacts,
}

impl FocusPane {
    fn label(self) -> &'static str {
        match self {
            FocusPane::Conversation => "conversation",
            FocusPane::Pipeline => "pipeline",
            FocusPane::Artifacts => "artifacts",
        }
    }
}

impl Default for FocusPane {
    fn default() -> Self {
        FocusPane::Conversation
    }
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
        "run" => {
            let instruction = args.collect::<Vec<_>>().join(" ");
            if instruction.trim().is_empty() {
                return Err("usage: research-os run \"<research instruction>\"".into());
            }
            init_workspace(&root)?;
            run_pipeline(&root, &instruction)
        }
        "resume" => {
            let Some(run_id) = args.next() else {
                return Err("usage: research-os resume <run_id>".into());
            };
            resume_pipeline(&root, &run_id)
        }
        "validate" => {
            let Some(run_id) = args.next() else {
                return Err("usage: research-os validate <run_id>".into());
            };
            validate_run(&root, &run_id)
        }
        "install-skills" => install_skills(&root),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command: {other}")),
    }
}

fn print_help() {
    println!(
        "research-os\n\nCommands:\n  tui\n  init\n  run \"<instruction>\"\n  resume <run_id>\n  validate <run_id>\n  install-skills\n\nRunning without a command opens the TUI.\n"
    );
}

#[derive(Default)]
struct TuiApp {
    input: String,
    input_cursor: usize,
    status: String,
    source_scope: String,
    run_id: String,
    active_stage: String,
    stages: Vec<String>,
    completed_stages: Vec<String>,
    log: Vec<String>,
    log_scroll: usize,
    stage_scroll: usize,
    artifacts: Vec<String>,
    artifact_scroll: usize,
    running: bool,
    completion_index: usize,
    sidebar_visible: bool,
    focus_pane: FocusPane,
}

enum UiMsg {
    RunStarted(String),
    Stages(Vec<String>),
    StageStarted(String),
    StageDone(String),
    Line(String),
    Artifacts(Vec<String>),
    Done(String),
    Failed(String),
}

enum StageOutput {
    None,
}

fn run_tui(root: PathBuf) -> Result<(), String> {
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

    let result = run_tui_loop(&mut terminal, root);

    disable_raw_mode().map_err(|e| e.to_string())?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|e| e.to_string())?;
    terminal.show_cursor().map_err(|e| e.to_string())?;
    result
}

fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    root: PathBuf,
) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<UiMsg>();
    let mut app = TuiApp {
        status: "Type a research instruction. Tab changes pane focus. Use /exit or Ctrl-C to quit."
            .into(),
        sidebar_visible: true,
        focus_pane: FocusPane::Conversation,
        ..TuiApp::default()
    };

    loop {
        drain_ui_messages(&mut app, &rx);
        terminal
            .draw(|frame| draw_tui(frame, &app))
            .map_err(|e| e.to_string())?;

        if event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            let event = event::read().map_err(|e| e.to_string())?;
            let key = match event {
                Event::Key(key) => key,
                _ => continue,
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(());
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
                    let instruction = apply_tui_scope(&instruction, &app.source_scope);
                    app.log.clear();
                    app.log_scroll = 0;
                    app.artifacts.clear();
                    app.stage_scroll = 0;
                    app.artifact_scroll = 0;
                    app.stages.clear();
                    app.completed_stages.clear();
                    app.running = true;
                    app.status = "Starting planner...".into();
                    let tx = tx.clone();
                    let root = root.clone();
                    thread::spawn(move || {
                        if let Err(err) = run_pipeline_tui(&root, &instruction, tx.clone()) {
                            let _ = tx.send(UiMsg::Failed(err));
                        }
                    });
                }
                KeyCode::Backspace if !app.running => {
                    delete_char_before_cursor(&mut app.input, &mut app.input_cursor);
                    clamp_completion_index(&mut app);
                }
                KeyCode::Tab if !app.running && app.input.starts_with('/') => {
                    complete_slash_command(&mut app);
                }
                KeyCode::Tab => {
                    focus_next_pane(&mut app);
                }
                KeyCode::BackTab => {
                    focus_previous_pane(&mut app);
                }
                KeyCode::PageUp => {
                    scroll_focused_pane(&mut app, pane_page_amount(terminal));
                }
                KeyCode::PageDown => {
                    unscroll_focused_pane(&mut app, pane_page_amount(terminal));
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    scroll_focused_pane(&mut app, pane_page_amount(terminal));
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    unscroll_focused_pane(&mut app, pane_page_amount(terminal));
                }
                KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                    scroll_focused_pane(&mut app, pane_page_amount(terminal));
                }
                KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                    unscroll_focused_pane(&mut app, pane_page_amount(terminal));
                }
                KeyCode::Up if !app.input.starts_with('/') => {
                    scroll_focused_pane(&mut app, 1);
                }
                KeyCode::Down if !app.input.starts_with('/') => {
                    unscroll_focused_pane(&mut app, 1);
                }
                KeyCode::F(2) if !app.running => {
                    app.sidebar_visible = !app.sidebar_visible;
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
            UiMsg::Artifacts(artifacts) => {
                app.artifacts = artifacts;
                clamp_artifact_scroll(app);
            }
            UiMsg::Done(message) => {
                app.status = message;
                app.running = false;
                app.active_stage.clear();
            }
            UiMsg::Failed(err) => {
                app.status = format!("Failed: {err}");
                app.running = false;
            }
        }
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
                    "/skills                list research-os skills",
                    "/scope <constraint>    set source scope for the next runs",
                    "/scope clear           clear source scope",
                    "/runs                  list recent runs",
                    "/resume <run_id>       resume a previous run in the TUI",
                    "/artifacts             refresh artifact list",
                    "/open <path>           preview a run/wiki artifact",
                    "/pane <name>           focus conversation, pipeline, or artifacts",
                    "/sidebar               toggle sidebar",
                    "Tab / Shift-Tab        move pane focus",
                    "Up/Down                scroll focused pane by one line",
                    "Ctrl-U/Ctrl-D          page scroll focused pane",
                    "Option-Up/Down         page scroll focused pane",
                    "/clear                 clear the conversation pane",
                    "/exit                  exit",
                ],
            );
        }
        "/skills" => {
            push_system_lines(app, &["Available skills:"]);
            for skill in SKILLS {
                app.log.push(format!("system: - {skill}"));
            }
            app.status = "Listed skills".into();
        }
        "/scope" => {
            if rest.trim().is_empty() {
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
        "/runs" => {
            let runs = list_runs(root);
            if runs.is_empty() {
                app.log.push("system: No runs found.".into());
            } else {
                app.log.push("system: Recent runs:".into());
                for run in runs {
                    app.log.push(format!("system: - {run}"));
                }
            }
            app.status = "Listed runs".into();
        }
        "/artifacts" => {
            if app.run_id.is_empty() {
                app.log.push("system: No active run.".into());
            } else {
                app.artifacts = list_run_artifacts(root, &app.run_id);
                app.log.push(format!(
                    "system: Refreshed {} artifacts.",
                    app.artifacts.len()
                ));
            }
        }
        "/open" => {
            if rest.trim().is_empty() {
                app.log.push("system: Usage: /open <path>".into());
            } else {
                preview_artifact(root, app, rest.trim())?;
            }
        }
        "/pane" | "/focus" => {
            if rest.trim().is_empty() {
                app.log.push(format!(
                    "system: Current pane focus: {}",
                    app.focus_pane.label()
                ));
                app.log
                    .push("system: Usage: /pane conversation|pipeline|artifacts".into());
            } else if set_focus_pane(app, rest.trim()) {
                app.status = format!("Focused {}", app.focus_pane.label());
            } else {
                app.log
                    .push("system: Unknown pane. Use conversation, pipeline, or artifacts.".into());
            }
        }
        "/sidebar" => {
            app.sidebar_visible = !app.sidebar_visible;
            if !app.sidebar_visible
                && matches!(app.focus_pane, FocusPane::Pipeline | FocusPane::Artifacts)
            {
                app.focus_pane = FocusPane::Conversation;
            }
            app.status = if app.sidebar_visible {
                "Sidebar opened".into()
            } else {
                "Sidebar hidden".into()
            };
        }
        "/resume" => {
            if rest.trim().is_empty() {
                app.log.push("system: Usage: /resume <run_id>".into());
            } else if app.running {
                app.log
                    .push("system: Cannot resume while a run is active.".into());
            } else {
                let run_id = rest.trim().to_string();
                app.input.clear();
                app.log.clear();
                app.log_scroll = 0;
                app.artifacts.clear();
                app.stage_scroll = 0;
                app.artifact_scroll = 0;
                app.stages.clear();
                app.completed_stages.clear();
                app.running = true;
                app.run_id = run_id.clone();
                app.status = format!("Resuming {run_id}");
                let root = root.to_path_buf();
                let tx = tx.clone();
                thread::spawn(move || {
                    if let Err(err) = resume_pipeline_tui(&root, &run_id, tx.clone()) {
                        let _ = tx.send(UiMsg::Failed(err));
                    }
                });
            }
        }
        "/clear" => {
            app.log.clear();
            app.log_scroll = 0;
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

fn push_system_lines(app: &mut TuiApp, lines: &[&str]) {
    for line in lines {
        app.log.push(format!("system: {line}"));
    }
    app.status = "Slash command handled".into();
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
        block = block.title_bottom(
            Line::from(vec![Span::styled(
                " scroll: ↑/↓  page: ctrl-u/d or opt-↑/↓ ",
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
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(5),
        ])
        .split(frame.area());

    let stage_count = app.stages.len();
    let done_count = app.completed_stages.len().min(stage_count);
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
                "new session"
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
    let chat_lines = build_chat_lines(app, chat_height);
    let scroll_title = pane_title("conversation", app.log_scroll);
    frame.render_widget(
        Paragraph::new(chat_lines)
            .wrap(Wrap { trim: false })
            .block(focused_block(app, FocusPane::Conversation, scroll_title)),
        body[1],
    );
    render_scrollbar(
        frame,
        app,
        FocusPane::Conversation,
        body[1],
        app.log.len(),
        chat_height,
        app.log_scroll,
        true,
    );

    let mut input_lines = Vec::new();
    if app.running {
        input_lines.push(Line::from(vec![
            Span::styled("agents are running", Style::default().fg(Color::Yellow)),
            Span::raw("  "),
            Span::styled("Ctrl-C exits", Style::default().fg(Color::DarkGray)),
        ]));
    } else {
        input_lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(app.input.as_str()),
        ]));
        input_lines.push(build_completion_line(app));
    }
    let hint = if app.running {
        " live run "
    } else {
        " enter run  |  tab pane  |  ctrl-u/d page  |  /help  |  /exit "
    };
    frame.render_widget(
        Paragraph::new(input_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .title(hint),
        ),
        root[2],
    );
    if !app.running {
        let x = root[2]
            .x
            .saturating_add(3)
            .saturating_add(app.input_cursor as u16)
            .min(root[2].right().saturating_sub(2));
        frame.set_cursor_position(Position::new(x, root[2].y.saturating_add(1)));
    }
}

fn build_chat_lines(app: &TuiApp, max_lines: usize) -> Vec<Line<'_>> {
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

    let visible = max_lines.max(1);
    let max_scroll = app.log.len().saturating_sub(visible);
    let scroll = app.log_scroll.min(max_scroll);
    let end = app.log.len().saturating_sub(scroll);
    let start = end.saturating_sub(visible);
    app.log[start..end]
        .iter()
        .map(|line| {
            if line.starts_with("user:") {
                Line::from(vec![
                    Span::styled(
                        "You",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::raw(line.trim_start_matches("user:").trim()),
                ])
            } else if line.starts_with("stage:") {
                Line::from(vec![Span::styled(
                    format!("-- {} --", line.trim_start_matches("stage:").trim()),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )])
            } else if line.starts_with("error:") {
                Line::from(vec![Span::styled(
                    line.as_str(),
                    Style::default().fg(Color::Red),
                )])
            } else if line.starts_with("answer:") {
                Line::from(vec![
                    Span::styled(
                        "Answer",
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        line.trim_start_matches("answer:").trim(),
                        Style::default().fg(Color::Reset),
                    ),
                ])
            } else if line.starts_with("system:") {
                Line::from(vec![
                    Span::styled("System", Style::default().fg(Color::DarkGray)),
                    Span::raw("  "),
                    Span::styled(
                        line.trim_start_matches("system:").trim(),
                        Style::default().fg(Color::Gray),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled("Agent", Style::default().fg(Color::Blue)),
                    Span::raw("  "),
                    Span::styled(line.as_str(), Style::default().fg(Color::Reset)),
                ])
            }
        })
        .collect()
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
                    "new session".to_string()
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

fn pane_page_amount(terminal: &Terminal<CrosstermBackend<io::Stdout>>) -> usize {
    terminal
        .size()
        .map(|area| area.height.saturating_sub(10).max(1) as usize)
        .unwrap_or(10)
}

fn focus_next_pane(app: &mut TuiApp) {
    app.focus_pane = match app.focus_pane {
        FocusPane::Conversation if app.sidebar_visible => FocusPane::Pipeline,
        FocusPane::Conversation => FocusPane::Conversation,
        FocusPane::Pipeline => FocusPane::Artifacts,
        FocusPane::Artifacts => FocusPane::Conversation,
    };
    app.status = format!("Focused {}", app.focus_pane.label());
}

fn focus_previous_pane(app: &mut TuiApp) {
    app.focus_pane = match app.focus_pane {
        FocusPane::Conversation if app.sidebar_visible => FocusPane::Artifacts,
        FocusPane::Conversation => FocusPane::Conversation,
        FocusPane::Pipeline => FocusPane::Conversation,
        FocusPane::Artifacts => FocusPane::Pipeline,
    };
    app.status = format!("Focused {}", app.focus_pane.label());
}

fn set_focus_pane(app: &mut TuiApp, pane: &str) -> bool {
    let next = match pane.trim().to_ascii_lowercase().as_str() {
        "conversation" | "chat" | "main" | "output" => FocusPane::Conversation,
        "pipeline" | "stages" | "stage" => FocusPane::Pipeline,
        "artifacts" | "artifact" | "files" => FocusPane::Artifacts,
        _ => return false,
    };
    if !app.sidebar_visible && matches!(next, FocusPane::Pipeline | FocusPane::Artifacts) {
        app.sidebar_visible = true;
    }
    app.focus_pane = next;
    true
}

fn scroll_focused_pane(app: &mut TuiApp, amount: usize) {
    match app.focus_pane {
        FocusPane::Conversation => {
            app.log_scroll = app.log_scroll.saturating_add(amount);
            clamp_log_scroll(app, Some(1));
        }
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

fn unscroll_focused_pane(app: &mut TuiApp, amount: usize) {
    match app.focus_pane {
        FocusPane::Conversation => app.log_scroll = app.log_scroll.saturating_sub(amount),
        FocusPane::Pipeline => app.stage_scroll = app.stage_scroll.saturating_sub(amount),
        FocusPane::Artifacts => app.artifact_scroll = app.artifact_scroll.saturating_sub(amount),
    }
}

fn clamp_log_scroll(app: &mut TuiApp, visible_lines: Option<usize>) {
    let visible = visible_lines.unwrap_or(1).max(1);
    let max_scroll = app.log.len().saturating_sub(visible);
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

fn build_completion_line(app: &TuiApp) -> Line<'static> {
    if !app.input.starts_with('/') {
        return Line::from(vec![Span::styled(
            "type / for commands",
            Style::default().fg(Color::DarkGray),
        )]);
    }

    let matches = slash_matches(&app.input);
    if matches.is_empty() {
        return Line::from(vec![Span::styled(
            "no matching slash commands",
            Style::default().fg(Color::DarkGray),
        )]);
    }

    let mut spans = vec![
        Span::styled("Tab", Style::default().fg(Color::Cyan)),
        Span::styled(" complete  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Up/Down", Style::default().fg(Color::Cyan)),
        Span::styled(" select  ", Style::default().fg(Color::DarkGray)),
    ];

    for (idx, (cmd, desc)) in matches.iter().take(5).enumerate() {
        let selected = idx == app.completion_index;
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            *cmd,
            Style::default()
                .fg(if selected { Color::Black } else { Color::Gray })
                .bg(if selected { Color::Cyan } else { Color::Reset })
                .add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
        spans.push(Span::styled(
            format!(" {desc}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
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

fn list_runs(root: &Path) -> Vec<String> {
    let mut runs = Vec::new();
    let Ok(entries) = fs::read_dir(root.join("runs")) else {
        return runs;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                runs.push(name.to_string());
            }
        }
    }
    runs.sort();
    runs.reverse();
    runs.truncate(12);
    runs
}

fn preview_artifact(root: &Path, app: &mut TuiApp, requested: &str) -> Result<(), String> {
    let path = resolve_artifact_path(root, &app.run_id, requested);
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

fn resolve_artifact_path(root: &Path, run_id: &str, requested: &str) -> PathBuf {
    let requested_path = PathBuf::from(requested);
    if requested_path.is_absolute() {
        return requested_path;
    }
    let direct = root.join(requested);
    if direct.exists() {
        return direct;
    }
    if !run_id.is_empty() {
        let run_relative = root.join("runs").join(run_id).join(requested);
        if run_relative.exists() {
            return run_relative;
        }
        if requested.starts_with("raw/")
            || requested.starts_with("parsed/")
            || requested.starts_with("wiki/")
            || requested.starts_with("schemas/")
        {
            return run_relative;
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
            "no active run"
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
        "parsed/sources",
        "wiki/sources",
        "wiki/topics",
        "wiki/synthesis",
        "wiki/outputs",
        "runs",
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
        "transcripts",
        "raw/sources",
        "parsed/sources",
        "schemas",
        "wiki/sources",
        "wiki/topics",
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

fn run_pipeline(root: &Path, instruction: &str) -> Result<(), String> {
    let run_id = make_run_id();
    let run_dir = root.join("runs").join(&run_id);
    init_run_workspace(root, &run_id)?;

    println!("research-os run: {run_id}");
    invoke_agent(
        root,
        &run_id,
        "research-os-planner",
        &planner_prompt(&run_id, instruction),
    )?;
    validate_plan(&run_dir.join("plan.json"))?;

    execute_plan(root, &run_id)
}

fn run_pipeline_tui(root: &Path, instruction: &str, tx: Sender<UiMsg>) -> Result<(), String> {
    init_workspace(root)?;
    let run_id = make_run_id();
    let run_dir = root.join("runs").join(&run_id);
    init_run_workspace(root, &run_id)?;

    tx.send(UiMsg::RunStarted(run_id.clone()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line(format!("user: {instruction}")))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::StageStarted("research-os-planner".into()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Line("stage: research-os-planner".into()))
        .map_err(|e| e.to_string())?;
    invoke_agent_tui(
        root,
        &run_id,
        "research-os-planner",
        &planner_prompt(&run_id, instruction),
        &tx,
    )?;
    validate_plan(&run_dir.join("plan.json"))?;
    emit_plan_summary(root, &run_id, &tx)?;
    tx.send(UiMsg::StageDone("research-os-planner".into()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Artifacts(list_run_artifacts(root, &run_id)))
        .map_err(|e| e.to_string())?;

    execute_plan_tui(root, &run_id, tx.clone())?;
    tx.send(UiMsg::Done(format!("Run complete: runs/{run_id}")))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn resume_pipeline(root: &Path, run_id: &str) -> Result<(), String> {
    validate_plan(&root.join("runs").join(run_id).join("plan.json"))?;
    execute_plan(root, run_id)
}

fn resume_pipeline_tui(root: &Path, run_id: &str, tx: Sender<UiMsg>) -> Result<(), String> {
    validate_plan(&root.join("runs").join(run_id).join("plan.json"))?;
    tx.send(UiMsg::RunStarted(run_id.to_string()))
        .map_err(|e| e.to_string())?;
    tx.send(UiMsg::Artifacts(list_run_artifacts(root, run_id)))
        .map_err(|e| e.to_string())?;
    execute_plan_tui(root, run_id, tx.clone())?;
    tx.send(UiMsg::Done(format!("Run complete: runs/{run_id}")))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn execute_plan_tui(root: &Path, run_id: &str, tx: Sender<UiMsg>) -> Result<(), String> {
    let plan_path = root.join("runs").join(run_id).join("plan.json");
    let plan = fs::read_to_string(&plan_path).map_err(|e| format!("read plan.json: {e}"))?;
    let stages = extract_stage_skills(&plan);
    if stages.is_empty() {
        return Err("plan.json did not contain any recognized research-os-* stage skills".into());
    }
    tx.send(UiMsg::Stages(
        stages.iter().map(|s| (*s).to_string()).collect(),
    ))
    .map_err(|e| e.to_string())?;

    for (idx, skill) in stages.iter().enumerate() {
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

        if validate_stage_output(root, run_id, skill).is_ok() {
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
        invoke_agent_tui(root, run_id, skill, &stage_prompt(run_id, skill), &tx)?;
        enforce_wiki_mutation(root, run_id, skill, &before)?;
        validate_stage_output(root, run_id, skill)?;
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

fn execute_plan(root: &Path, run_id: &str) -> Result<(), String> {
    let plan_path = root.join("runs").join(run_id).join("plan.json");
    let plan = fs::read_to_string(&plan_path).map_err(|e| format!("read plan.json: {e}"))?;
    let stages = extract_stage_skills(&plan);
    if stages.is_empty() {
        return Err("plan.json did not contain any recognized research-os-* stage skills".into());
    }

    for (idx, skill) in stages.iter().enumerate() {
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

        if validate_stage_output(root, run_id, skill).is_ok() {
            fs::write(&status_path, "done\n").map_err(|e| format!("write status: {e}"))?;
            println!("stage {stage_no}: {skill} already has valid outputs");
            continue;
        }

        println!("\n=== stage {stage_no}: {skill} ===");
        let before = snapshot_wiki(root, run_id)?;
        invoke_agent(root, run_id, skill, &stage_prompt(run_id, skill))?;
        enforce_wiki_mutation(root, run_id, skill, &before)?;
        validate_stage_output(root, run_id, skill)?;
        fs::write(&status_path, "done\n").map_err(|e| format!("write status: {e}"))?;
    }

    validate_run(root, run_id)?;
    emit_final_expected_artifact_cli(root, run_id)?;
    println!("run complete: runs/{run_id}");
    Ok(())
}

fn planner_prompt(run_id: &str, instruction: &str) -> String {
    format!(
        "Use the local skill at skills/research-os-planner/SKILL.md.\n\
         User instruction: {instruction}\n\
         Run id: {run_id}\n\
         Write exactly one artifact: runs/{run_id}/plan.json.\n\
         This run is self-contained. Use run-local artifact roots for stage outputs: runs/{run_id}/raw/, runs/{run_id}/parsed/, runs/{run_id}/schemas/, and runs/{run_id}/wiki/.\n\
         The plan must include ordered stages with agent skill names, source_scope, required inputs, required outputs, and wiki mutation permissions.\n\
         Do not execute downstream agents."
    )
}

fn stage_prompt(run_id: &str, skill: &str) -> String {
    let skill_specific = if skill == "research-os-search" {
        "\n\
         If an allowed source page is JavaScript-rendered and static fetch/search snippets do not expose source records, use a headless browser rendering fallback. Prefer the repo-local helper: npm run render-source -- '<allowed-url>' --out 'runs/{run_id}/raw/sources/rendered-source.json'. Record every attempt in browser_render_attempts."
    } else if skill == "research-os-ingest" {
        "\n\
         For each selected paper/report source, do targeted PDF/full-text discovery before text fallback. Use selected metadata, official detail pages, run-local official collection artifacts, and exact-title web lookup to find a matching downloadable file. Do not add new research sources; only find files for the selected sources. Record discovery queries, candidate URLs, match rationale, and download result in ingest_manifest.json."
    } else {
        ""
    };
    format!(
        "Use the local skill at skills/{skill}/SKILL.md.\n\
         Run id: {run_id}\n\
         Read runs/{run_id}/plan.json first.\n\
         Follow only this agent's contract. Write the outputs assigned to this stage. Treat runs/{run_id}/raw/, runs/{run_id}/parsed/, runs/{run_id}/schemas/, and runs/{run_id}/wiki/ as this run's artifact roots. Preserve citation traceability and source-scope constraints.{skill_specific}"
    )
}

fn invoke_agent(root: &Path, run_id: &str, skill: &str, prompt: &str) -> Result<(), String> {
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

    let mut cmd = Command::new("codex");
    if agent_needs_search(skill) {
        cmd.arg("--search");
    }
    let sandbox_mode = agent_sandbox_mode(skill);
    cmd.arg("exec")
        .arg("--cd")
        .arg(root)
        .arg("--sandbox")
        .arg(sandbox_mode)
        .arg("--skip-git-repo-check")
        .arg("--json");
    cmd.arg(prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("spawn codex: {e}"))?;
    let stdout = child.stdout.take().ok_or("missing codex stdout")?;
    let stderr = child.stderr.take().ok_or("missing codex stderr")?;

    let out_file = transcript.clone();
    let out = thread::spawn(move || stream_lines(stdout, out_file, false));
    let err_file = transcript.clone();
    let err = thread::spawn(move || stream_lines(stderr, err_file, true));

    let status = child.wait().map_err(|e| format!("wait codex: {e}"))?;
    out.join().map_err(|_| "stdout stream thread failed")??;
    err.join().map_err(|_| "stderr stream thread failed")??;
    if !status.success() {
        return Err(format!("{skill} failed with status {status}"));
    }
    Ok(())
}

fn invoke_agent_tui(
    root: &Path,
    run_id: &str,
    skill: &str,
    prompt: &str,
    tx: &Sender<UiMsg>,
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

    let mut cmd = Command::new("codex");
    if agent_needs_search(skill) {
        cmd.arg("--search");
    }
    let sandbox_mode = agent_sandbox_mode(skill);
    cmd.arg("exec")
        .arg("--cd")
        .arg(root)
        .arg("--sandbox")
        .arg(sandbox_mode)
        .arg("--skip-git-repo-check")
        .arg("--json");
    cmd.arg(prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("spawn codex: {e}"))?;
    let stdout = child.stdout.take().ok_or("missing codex stdout")?;
    let stderr = child.stderr.take().ok_or("missing codex stderr")?;

    let out_file = transcript.clone();
    let out_tx = tx.clone();
    let out = thread::spawn(move || stream_lines_tui(stdout, out_file, out_tx));
    let err_file = transcript.clone();
    let err_tx = tx.clone();
    let err = thread::spawn(move || stream_lines_tui(stderr, err_file, err_tx));

    let status = child.wait().map_err(|e| format!("wait codex: {e}"))?;
    out.join().map_err(|_| "stdout stream thread failed")??;
    err.join().map_err(|_| "stderr stream thread failed")??;
    if !status.success() {
        return Err(format!("{skill} failed with status {status}"));
    }
    Ok(())
}

fn agent_sandbox_mode(skill: &str) -> &'static str {
    if skill == "research-os-search" || skill == "research-os-ingest" {
        "danger-full-access"
    } else {
        "workspace-write"
    }
}

fn agent_needs_search(skill: &str) -> bool {
    skill == "research-os-search" || skill == "research-os-ingest"
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
            tx.send(UiMsg::Line(display)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn compact_agent_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.starts_with('{') {
        return Some(trimmed.to_string());
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

    for key in ["\"message\"", "\"content\"", "\"text\"", "\"delta\""] {
        if let Some(value) = extract_json_string_field(trimmed, key) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    None
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
    let mut hits = Vec::new();
    for skill in SKILLS {
        let mut start = 0;
        while let Some(pos) = plan[start..].find(skill) {
            hits.push((start + pos, *skill));
            start += pos + skill.len();
        }
    }
    hits.sort_by_key(|(pos, _)| *pos);
    hits.into_iter().map(|(_, skill)| skill).collect()
}

fn validate_stage_output(root: &Path, run_id: &str, skill: &str) -> Result<StageOutput, String> {
    let run_dir = root.join("runs").join(run_id);
    match skill {
        "research-os-search" => {
            validate_planned_outputs(root, run_id, skill)
                .or_else(|_| validate_artifact(&run_dir.join("search_results.json")))?;
            Ok(StageOutput::None)
        }
        "research-os-source-triage" => {
            validate_planned_outputs(root, run_id, skill).or_else(|_| {
                validate_artifact(&run_dir.join("selected_sources.json"))
                    .or_else(|_| validate_artifact(&run_dir.join("triage.json")))
            })?;
            Ok(StageOutput::None)
        }
        "research-os-ingest" => {
            validate_planned_outputs(root, run_id, skill)
                .or_else(|_| validate_artifact(&run_dir.join("ingest_manifest.json")))?;
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
            validate_planned_outputs(root, run_id, skill)
                .or_else(|_| validate_artifact(&run_dir.join("wiki_update_report.md")))?;
            Ok(StageOutput::None)
        }
        "research-os-synthesis" => {
            validate_planned_outputs(root, run_id, skill).or_else(|_| {
                validate_any_markdown(
                    &root.join("runs").join(run_id).join("wiki/synthesis"),
                    "synthesis page",
                )
            })?;
            Ok(StageOutput::None)
        }
        "research-os-lint-critic" => {
            validate_planned_outputs(root, run_id, skill).or_else(|_| {
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
            } else if validate_planned_outputs(root, run_id, skill).is_ok() {
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

fn validate_planned_outputs(root: &Path, run_id: &str, skill: &str) -> Result<(), String> {
    let plan_path = root.join("runs").join(run_id).join("plan.json");
    let plan = fs::read_to_string(&plan_path)
        .map_err(|e| format!("read plan for output validation: {e}"))?;
    let outputs = extract_stage_outputs(&plan, skill);
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
        || rel.starts_with("parsed/")
        || rel.starts_with("wiki/")
        || rel.starts_with("schemas/")
    {
        root.join("runs").join(run_id).join(rel)
    } else {
        root.join(rel)
    }
}

fn extract_stage_outputs(plan: &str, skill: &str) -> Vec<String> {
    let Some(skill_pos) = plan.find(skill) else {
        return Vec::new();
    };
    let after_skill = &plan[skill_pos..];
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
    Ok(())
}

fn final_expected_artifact_path(root: &Path, run_id: &str) -> Result<PathBuf, String> {
    let plan_path = root.join("runs").join(run_id).join("plan.json");
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

fn emit_final_expected_artifact_cli(root: &Path, run_id: &str) -> Result<(), String> {
    let path = final_expected_artifact_path(root, run_id)?;
    validate_artifact(&path)?;
    println!("\n=== final output: {} ===", display_path(root, &path));
    if is_text_artifact(&path) {
        let answer = fs::read_to_string(&path)
            .map_err(|e| format!("read final artifact {}: {e}", path.display()))?;
        println!("{answer}");
    } else {
        println!("final artifact is not a text previewable file: {}", path.display());
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

fn emit_plan_summary(root: &Path, run_id: &str, tx: &Sender<UiMsg>) -> Result<(), String> {
    let plan_path = root.join("runs").join(run_id).join("plan.json");
    let plan = fs::read_to_string(&plan_path)
        .map_err(|e| format!("read plan for summary {}: {e}", plan_path.display()))?;
    let task_type = extract_json_string_field(&plan, "\"task_type\"").unwrap_or("unknown".into());
    let source_mode = extract_json_string_field(&plan, "\"mode\"").unwrap_or("unspecified".into());
    let expansion =
        extract_json_string_field(&plan, "\"expansion_policy\"").unwrap_or("unspecified".into());
    let final_artifact = extract_json_string_field(&plan, "\"final_expected_artifact\"")
        .unwrap_or_else(|| format!("runs/{run_id}/final_answer.md"));
    let stages = extract_stage_skills(&plan);

    let mut summary = String::new();
    summary.push_str("# Plan Summary\n\n");
    summary.push_str(&format!("- task: {task_type}\n"));
    summary.push_str(&format!("- source scope: {source_mode}\n"));
    summary.push_str(&format!("- expansion: {expansion}\n"));
    summary.push_str(&format!("- final artifact: {final_artifact}\n"));
    summary.push_str("- stages:\n");
    for stage in &stages {
        summary.push_str(&format!("  - {}\n", trim_skill_name(stage)));
    }

    let summary_path = root.join("runs").join(run_id).join("plan.md");
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
    validate_plan(&run_dir.join("plan.json"))?;
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
    let plan_path = root.join("runs").join(run_id).join("plan.json");
    let Ok(plan) = fs::read_to_string(plan_path) else {
        return Vec::new();
    };
    extract_stage_outputs(&plan, skill)
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

fn make_run_id() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("run-{secs}")
}
