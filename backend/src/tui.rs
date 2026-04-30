use std::{
    collections::HashMap,
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, error::TryRecvError, unbounded_channel};

use crate::{
    error::{AppError, AppResult},
    runtime::{DownloadProgress, download_script_path, resolve_download_target_dir},
    service::{AppService, InstanceMemoryPreview, SystemMetrics},
    types::{
        ConfigKeySource, Instance, InstanceStatus, Model, ModelDownloadRequest, Template,
        config_key_source, display_config_key,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Instances,
    Families,
    Models,
    System,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Self::Instances => "1 Instances",
            Self::Families => "2 Families",
            Self::Models => "3 Models",
            Self::System => "4 System",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FamilyFocus {
    Templates,
    Variants,
}

#[derive(Clone)]
enum PromptAction {
    CreateInstance,
    CreateInstanceFromModel {
        model_ref: String,
        mmproj: Option<String>,
    },
    CreateFamilyFromModel {
        model_ref: String,
        mmproj: Option<String>,
        default_family: String,
    },
    EditInstance {
        name: String,
    },
    InstantiateTemplate {
        template: String,
    },
    BatchApplyTemplate {
        template: String,
    },
    SetTemplateOverride {
        template: String,
    },
    SetTemplateBase {
        template: String,
    },
    DownloadModel,
    RenameModel {
        name: String,
    },
    ConfirmDeleteModel {
        name: String,
    },
}

#[derive(Clone)]
struct PromptState {
    action: PromptAction,
    label: String,
    fields: Vec<String>,
    inputs: Vec<String>,
    active_field: usize,
    cursors: Vec<usize>,
    key_hints: Vec<String>,
    key_field_index: Option<usize>,
}

struct TuiState {
    tab: Tab,
    family_focus: FamilyFocus,
    instances: Vec<(Instance, InstanceStatus)>,
    templates: Vec<Template>,
    models: Vec<Model>,
    selected_instance: usize,
    selected_template: usize,
    selected_variant: usize,
    selected_model: usize,
    metrics: Option<SystemMetrics>,
    memory_previews: HashMap<String, InstanceMemoryPreview>,
    log_viewer: Option<LogViewerState>,
    download_job: Option<DownloadJobState>,
    download_events: Option<UnboundedReceiver<DownloadUiEvent>>,
    message: String,
    prompt: Option<PromptState>,
}

struct LogViewerState {
    instance_name: String,
    lines: Vec<String>,
    scroll: usize,
    search_query: String,
    search_mode: bool,
}

#[derive(Clone)]
struct DownloadJobState {
    repo_id: String,
    target_dir: String,
    script_path: String,
    progress_percent: f64,
    phase: String,
    latest_message: String,
    running: bool,
    foreground: bool,
}

enum DownloadUiEvent {
    Progress(DownloadProgress),
    Completed { path: String },
    Failed(String),
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            tab: Tab::Instances,
            family_focus: FamilyFocus::Templates,
            instances: Vec::new(),
            templates: Vec::new(),
            models: Vec::new(),
            selected_instance: 0,
            selected_template: 0,
            selected_variant: 0,
            selected_model: 0,
            metrics: None,
            memory_previews: HashMap::new(),
            log_viewer: None,
            download_job: None,
            download_events: None,
            message: String::new(),
            prompt: None,
        }
    }
}

pub async fn run(service: Arc<AppService>) -> AppResult<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = TuiState::default();
    refresh_all(&service, &mut state).await?;

    let result = run_loop(&service, &mut terminal, &mut state).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn run_loop(
    service: &Arc<AppService>,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut TuiState,
) -> AppResult<()> {
    let mut last_metrics_refresh = Instant::now();
    loop {
        drain_download_events(service, state).await?;

        if last_metrics_refresh.elapsed() >= Duration::from_secs(3) {
            refresh_metrics(service, state).await;
            last_metrics_refresh = Instant::now();
        }

        terminal.draw(|frame| draw(frame, state))?;

        if !event::poll(Duration::from_millis(180))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if state.log_viewer.is_some() {
            handle_log_viewer_key(service, state, key).await?;
            continue;
        }

        if state.prompt.is_some() {
            if handle_prompt_key(service, state, key).await? {
                break;
            }
            continue;
        }

        match key.code {
            KeyCode::Char('q') => break,
            KeyCode::Char('1') => state.tab = Tab::Instances,
            KeyCode::Char('2') => state.tab = Tab::Families,
            KeyCode::Char('3') => state.tab = Tab::Models,
            KeyCode::Char('4') => state.tab = Tab::System,
            KeyCode::Char('r') => {
                refresh_all(service, state).await?;
                state.message = "refreshed".to_string();
            }
            KeyCode::Down => move_selection(state, true),
            KeyCode::Up => move_selection(state, false),
            KeyCode::Left if state.tab == Tab::Families => {
                state.family_focus = FamilyFocus::Templates
            }
            KeyCode::Right if state.tab == Tab::Families => {
                state.family_focus = FamilyFocus::Variants
            }
            KeyCode::Char('n') => {
                let prompt = match state.tab {
                    Tab::Instances => Some(prompt_form(
                        PromptAction::CreateInstance,
                        "new instance: <name> <model_rel> [port] [mmproj_rel]",
                        &["name", "model_rel", "port (optional)", "mmproj (optional)"],
                    )),
                    Tab::Models => Some(prompt_form(
                        PromptAction::DownloadModel,
                        "download model: <repo_id> [patterns_csv] [local_dir]",
                        &["repo_id", "patterns csv", "local_dir"],
                    )),
                    Tab::Families => None,
                    Tab::System => None,
                };
                state.prompt = prompt;
            }
            KeyCode::Char('g') if state.tab == Tab::Models => {
                if let Some(job) = &mut state.download_job {
                    job.foreground = !job.foreground;
                    state.message = if job.foreground {
                        "download progress switched to foreground view".to_string()
                    } else {
                        "download progress moved to background".to_string()
                    };
                }
            }
            KeyCode::Char('s') if state.tab == Tab::Instances => {
                if let Some(name) = selected_instance_name(state) {
                    match service.start_instance(&name).await {
                        Ok(_) => state.message = format!("started '{name}'"),
                        Err(err) => state.message = err.to_string(),
                    }
                    refresh_all(service, state).await?;
                }
            }
            KeyCode::Char('x') if state.tab == Tab::Instances => {
                if let Some(name) = selected_instance_name(state) {
                    match service.stop_instance(&name).await {
                        Ok(_) => state.message = format!("stopped '{name}'"),
                        Err(err) => state.message = err.to_string(),
                    }
                    refresh_all(service, state).await?;
                }
            }
            KeyCode::Char('t') if state.tab == Tab::Instances => {
                if let Some(name) = selected_instance_name(state) {
                    match service.restart_instance(&name).await {
                        Ok(_) => state.message = format!("restarted '{name}'"),
                        Err(err) => state.message = err.to_string(),
                    }
                    refresh_all(service, state).await?;
                }
            }
            KeyCode::Char('l') if state.tab == Tab::Instances => {
                if let Some(name) = selected_instance_name(state) {
                    match service.instance_logs(&name, 300).await {
                        Ok(logs) => {
                            state.log_viewer = Some(LogViewerState {
                                instance_name: name.clone(),
                                lines: logs.lines().map(ToString::to_string).collect::<Vec<_>>(),
                                scroll: 0,
                                search_query: String::new(),
                                search_mode: false,
                            });
                            state.message = format!("opened logs for '{name}'");
                        }
                        Err(err) => state.message = err.to_string(),
                    }
                }
            }
            KeyCode::Char('e') if state.tab == Tab::Instances => {
                if let Some(name) = selected_instance_name(state) {
                    state.prompt = Some(prompt_form_with_key_hints(
                        PromptAction::EditInstance { name },
                        "edit instance: <key> <json_or_text_value>",
                        &["key", "value"],
                        0,
                        instance_key_hints(state),
                    ));
                }
            }
            KeyCode::Char('a') if state.tab == Tab::Families => match service.scan_templates() {
                Ok(found) => {
                    state.message = format!("scanned templates: {}", found.len());
                    refresh_all(service, state).await?;
                }
                Err(err) => state.message = err.to_string(),
            },
            KeyCode::Char('i') if state.tab == Tab::Families => {
                if let Some(template) = selected_template_name(state) {
                    state.prompt = Some(prompt_form(
                        PromptAction::InstantiateTemplate { template },
                        "instantiate: <instance_name> [overrides_json_object]",
                        &["instance_name", "overrides_json (optional)"],
                    ));
                }
            }
            KeyCode::Char('b') if state.tab == Tab::Families => {
                if let Some(template) = selected_template_name(state) {
                    state.prompt = Some(prompt_form_with_key_hints(
                        PromptAction::BatchApplyTemplate { template },
                        "batch apply: <key> <json_or_text_value>",
                        &["key", "value"],
                        0,
                        template_key_hints(state),
                    ));
                }
            }
            KeyCode::Char('o') if state.tab == Tab::Families => {
                if let Some(template) = selected_template_name(state) {
                    state.prompt = Some(prompt_form_with_key_hints(
                        PromptAction::SetTemplateOverride { template },
                        "set override: <variant_name> <key> <json_or_text_value>",
                        &["variant_name", "key", "value"],
                        1,
                        template_key_hints(state),
                    ));
                }
            }
            KeyCode::Char('e') if state.tab == Tab::Families => {
                if let Some(template) = selected_template_name(state) {
                    state.prompt = Some(prompt_form_with_key_hints(
                        PromptAction::SetTemplateBase { template },
                        "set base: <key> <json_or_text_value>",
                        &["key", "value"],
                        0,
                        template_key_hints(state),
                    ));
                }
            }
            KeyCode::Char('d') if state.tab == Tab::Models => {
                if let Some(name) = selected_model_name(state) {
                    state.prompt = Some(prompt_form(
                        PromptAction::ConfirmDeleteModel { name },
                        "confirm delete model: type YES",
                        &["type YES"],
                    ));
                }
            }
            KeyCode::Char('u') if state.tab == Tab::Models => {
                if let Some(name) = selected_model_name(state) {
                    state.prompt = Some(prompt_form(
                        PromptAction::RenameModel { name },
                        "rename model: <new_name>",
                        &["new_name"],
                    ));
                }
            }
            KeyCode::Char('i') if state.tab == Tab::Models => {
                if let Some((model_ref, mmproj)) = selected_model_model_refs(state) {
                    state.prompt = Some(prompt_form(
                        PromptAction::CreateInstanceFromModel { model_ref, mmproj },
                        "new instance from selected model: <instance_name> [port]",
                        &["instance_name", "port (optional)"],
                    ));
                }
            }
            KeyCode::Char('f') if state.tab == Tab::Models => {
                if let Some((model_ref, mmproj, default_family)) =
                    selected_model_family_source(state)
                {
                    state.prompt = Some(prompt_form(
                        PromptAction::CreateFamilyFromModel {
                            model_ref,
                            mmproj,
                            default_family,
                        },
                        "new family from selected model: <template_name> [family_name]",
                        &["template_name", "family_name (optional)"],
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn prompt_form(action: PromptAction, label: &str, fields: &[&str]) -> PromptState {
    PromptState {
        action,
        label: label.to_string(),
        fields: fields.iter().map(|item| item.to_string()).collect(),
        inputs: vec![String::new(); fields.len()],
        active_field: 0,
        cursors: vec![0; fields.len()],
        key_hints: vec![],
        key_field_index: None,
    }
}

fn prompt_form_with_key_hints(
    action: PromptAction,
    label: &str,
    fields: &[&str],
    key_field_index: usize,
    key_hints: Vec<String>,
) -> PromptState {
    PromptState {
        action,
        label: label.to_string(),
        fields: fields.iter().map(|item| item.to_string()).collect(),
        inputs: vec![String::new(); fields.len()],
        active_field: 0,
        cursors: vec![0; fields.len()],
        key_hints,
        key_field_index: Some(key_field_index),
    }
}

async fn handle_prompt_key(
    service: &Arc<AppService>,
    state: &mut TuiState,
    key: KeyEvent,
) -> AppResult<bool> {
    if let Some(prompt_state) = &mut state.prompt {
        match key.code {
            KeyCode::Esc => {
                state.prompt = None;
                state.message = "cancelled".to_string();
            }
            KeyCode::Enter => {
                let prompt = state.prompt.take().expect("prompt should exist");
                execute_prompt(service, state, prompt).await?;
            }
            KeyCode::Up => {
                if prompt_state.active_field > 0 {
                    prompt_state.active_field -= 1;
                }
            }
            KeyCode::Down | KeyCode::Tab => {
                if !prompt_state.fields.is_empty() {
                    prompt_state.active_field =
                        (prompt_state.active_field + 1).min(prompt_state.fields.len() - 1);
                }
            }
            KeyCode::Home => {
                if let Some(cursor) = prompt_state.cursors.get_mut(prompt_state.active_field) {
                    *cursor = 0;
                }
            }
            KeyCode::End => {
                let len = prompt_state
                    .inputs
                    .get(prompt_state.active_field)
                    .map(|value| value.chars().count())
                    .unwrap_or(0);
                if let Some(cursor) = prompt_state.cursors.get_mut(prompt_state.active_field) {
                    *cursor = len;
                }
            }
            KeyCode::Left => {
                if let Some(cursor) = prompt_state.cursors.get_mut(prompt_state.active_field) {
                    *cursor = cursor.saturating_sub(1);
                }
            }
            KeyCode::Right => {
                if !autocomplete_prompt_key(prompt_state) {
                    let len = prompt_state
                        .inputs
                        .get(prompt_state.active_field)
                        .map(|value| value.chars().count())
                        .unwrap_or(0);
                    if let Some(cursor) = prompt_state.cursors.get_mut(prompt_state.active_field) {
                        *cursor = (*cursor + 1).min(len);
                    }
                }
            }
            _ if is_backspace_key(key) => {
                let active = prompt_state.active_field;
                if let (Some(value), Some(cursor)) = (
                    prompt_state.inputs.get_mut(active),
                    prompt_state.cursors.get_mut(active),
                ) {
                    if *cursor > 0 {
                        remove_char_at(value, *cursor - 1);
                        *cursor -= 1;
                    }
                }
            }
            KeyCode::Char(ch) if is_text_input(key.modifiers) => {
                let active = prompt_state.active_field;
                if let (Some(value), Some(cursor)) = (
                    prompt_state.inputs.get_mut(active),
                    prompt_state.cursors.get_mut(active),
                ) {
                    insert_char_at(value, *cursor, ch);
                    *cursor += 1;
                }
            }
            _ => {}
        }
    }
    Ok(false)
}

async fn handle_log_viewer_key(
    service: &Arc<AppService>,
    state: &mut TuiState,
    key: KeyEvent,
) -> AppResult<()> {
    let Some(viewer) = &mut state.log_viewer else {
        return Ok(());
    };
    if viewer.search_mode {
        match key.code {
            KeyCode::Esc => viewer.search_mode = false,
            KeyCode::Enter => {
                viewer.search_mode = false;
                if let Some(line) = find_log_match(&viewer.lines, &viewer.search_query, 0, true) {
                    viewer.scroll = line;
                }
            }
            _ if is_backspace_key(key) => {
                viewer.search_query.pop();
            }
            KeyCode::Char(ch) if is_text_input(key.modifiers) => {
                viewer.search_query.push(ch);
            }
            _ => {}
        }
        return Ok(());
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('l') => {
            state.log_viewer = None;
        }
        KeyCode::Home => {
            viewer.scroll = 0;
        }
        KeyCode::End => {
            viewer.scroll = viewer.lines.len().saturating_sub(1);
        }
        KeyCode::Up => {
            viewer.scroll = viewer.scroll.saturating_sub(1);
        }
        KeyCode::Down => {
            if viewer.scroll + 1 < viewer.lines.len() {
                viewer.scroll += 1;
            }
        }
        KeyCode::PageUp => {
            viewer.scroll = viewer.scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            viewer.scroll = (viewer.scroll + 10).min(viewer.lines.len().saturating_sub(1));
        }
        KeyCode::Char('r') => {
            let logs = service.instance_logs(&viewer.instance_name, 300).await?;
            viewer.lines = logs.lines().map(ToString::to_string).collect::<Vec<_>>();
            viewer.scroll = 0;
            state.message = format!("reloaded logs for '{}'", viewer.instance_name);
        }
        KeyCode::Char('/') => {
            viewer.search_mode = true;
            viewer.search_query.clear();
        }
        KeyCode::Char('n') => {
            let start = viewer.scroll.saturating_add(1);
            if let Some(line) = find_log_match(&viewer.lines, &viewer.search_query, start, true) {
                viewer.scroll = line;
            }
        }
        KeyCode::Char('N') => {
            if viewer.scroll > 0 {
                if let Some(line) = find_log_match(
                    &viewer.lines,
                    &viewer.search_query,
                    viewer.scroll - 1,
                    false,
                ) {
                    viewer.scroll = line;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn execute_prompt(
    service: &Arc<AppService>,
    state: &mut TuiState,
    prompt_state: PromptState,
) -> AppResult<()> {
    let input = prompt_state
        .inputs
        .iter()
        .map(|item| item.trim())
        .collect::<Vec<_>>()
        .join(" ");
    let input = input.trim();
    if input.is_empty() {
        state.message = "empty input".to_string();
        return Ok(());
    }

    let outcome = match prompt_state.action {
        PromptAction::CreateInstance => {
            let mut parts = input.split_whitespace();
            let name = required_part(parts.next(), "name")?.to_string();
            let model = required_part(parts.next(), "model_rel")?.to_string();
            let port = parts
                .next()
                .map(|part| part.parse::<u16>())
                .transpose()
                .map_err(|_| AppError::InvalidInput("invalid port".to_string()))?;
            let mmproj = parts.next().map(ToString::to_string);
            service.create_instance(crate::service::InstanceCreateInput {
                name: name.clone(),
                model,
                mmproj,
                port,
                ctx_size: 262_144,
                threads: 8,
                gpu_layers: 999,
                thinking: true,
                parallel: None,
            })?;
            format!("created instance '{name}'")
        }
        PromptAction::CreateInstanceFromModel { model_ref, mmproj } => {
            let mut parts = input.split_whitespace();
            let name = required_part(parts.next(), "instance_name")?.to_string();
            let port = parts
                .next()
                .map(|part| part.parse::<u16>())
                .transpose()
                .map_err(|_| AppError::InvalidInput("invalid port".to_string()))?;
            service.create_instance(crate::service::InstanceCreateInput {
                name: name.clone(),
                model: model_ref,
                mmproj,
                port,
                ctx_size: 262_144,
                threads: 8,
                gpu_layers: 999,
                thinking: true,
                parallel: None,
            })?;
            format!("created instance '{name}' from selected model")
        }
        PromptAction::CreateFamilyFromModel {
            model_ref,
            mmproj,
            default_family,
        } => {
            let mut parts = input.split_whitespace();
            let template_name = required_part(parts.next(), "template_name")?.to_string();
            let family = parts
                .next()
                .map(ToString::to_string)
                .unwrap_or(default_family);
            service.create_template_from_model(
                &template_name,
                &family,
                "Created from Models tab",
                &model_ref,
                mmproj,
            )?;
            format!("created family template '{template_name}' from selected model")
        }
        PromptAction::EditInstance { name } => {
            let (key, value) = parse_key_value_input(input)?;
            service.edit_instance(&name, &key, value)?;
            format!("updated {name}.{key}")
        }
        PromptAction::InstantiateTemplate { template } => {
            let (instance_name, overrides) = parse_instantiate_input(input)?;
            service.instantiate_template(&template, &instance_name, overrides)?;
            format!("instantiated '{instance_name}' from '{template}'")
        }
        PromptAction::BatchApplyTemplate { template } => {
            let (key, value) = parse_key_value_input(input)?;
            let count = service.batch_apply(&template, &key, value)?.len();
            format!("batch applied '{key}' to {count} variants")
        }
        PromptAction::SetTemplateOverride { template } => {
            let (variant, key, value) = parse_variant_key_value_input(input)?;
            service.set_template_override(&template, &variant, &key, value)?;
            format!("set override {template}.{variant}.{key}")
        }
        PromptAction::SetTemplateBase { template } => {
            let (key, value) = parse_key_value_input(input)?;
            service.set_template_base_value(&template, &key, value)?;
            format!("set base {template}.{key}")
        }
        PromptAction::DownloadModel => {
            let request = parse_download_input(input)?;
            if state.download_job.as_ref().is_some_and(|job| job.running) {
                return Err(AppError::InvalidInput(
                    "a model download is already running".to_string(),
                ));
            }
            start_download_job(service, state, request);
            "started model download in background".to_string()
        }
        PromptAction::RenameModel { name } => {
            let new_name = input.split_whitespace().next().unwrap_or_default();
            if new_name.is_empty() {
                return Err(AppError::InvalidInput("missing new model name".to_string()));
            }
            service.rename_model(&name, new_name)?;
            format!("renamed model '{name}' -> '{new_name}'")
        }
        PromptAction::ConfirmDeleteModel { name } => {
            if input != "YES" {
                return Err(AppError::InvalidInput(
                    "deletion cancelled (type YES)".to_string(),
                ));
            }
            service.delete_model(&name)?;
            format!("deleted model '{name}'")
        }
    };

    state.message = outcome;
    refresh_all(service, state).await?;
    Ok(())
}

async fn refresh_metrics(service: &Arc<AppService>, state: &mut TuiState) {
    if let Ok(metrics) = service.system_metrics().await {
        state.metrics = Some(metrics);
    }
}

fn required_part<'a>(part: Option<&'a str>, label: &str) -> AppResult<&'a str> {
    part.ok_or_else(|| AppError::InvalidInput(format!("missing {label}")))
}

fn parse_key_value_input(input: &str) -> AppResult<(String, Value)> {
    let (key, value_text) = input
        .split_once(' ')
        .ok_or_else(|| AppError::InvalidInput("expected: <key> <value>".to_string()))?;
    Ok((key.to_string(), parse_json_or_string(value_text.trim())))
}

fn parse_variant_key_value_input(input: &str) -> AppResult<(String, String, Value)> {
    let mut parts = input.splitn(3, ' ');
    let variant = required_part(parts.next(), "variant_name")?.to_string();
    let key = required_part(parts.next(), "key")?.to_string();
    let value_text = required_part(parts.next(), "value")?;
    Ok((variant, key, parse_json_or_string(value_text.trim())))
}

fn parse_instantiate_input(input: &str) -> AppResult<(String, Option<HashMap<String, Value>>)> {
    let (instance_name, rest) = match input.split_once(' ') {
        Some((name, rest)) => (name.to_string(), Some(rest.trim())),
        None => (input.to_string(), None),
    };
    let overrides = if let Some(rest) = rest {
        if rest.is_empty() {
            None
        } else {
            let parsed = parse_json_or_string(rest);
            let map = parsed
                .as_object()
                .ok_or_else(|| {
                    AppError::InvalidInput("overrides must be a JSON object".to_string())
                })?
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<HashMap<_, _>>();
            Some(map)
        }
    } else {
        None
    };
    Ok((instance_name, overrides))
}

fn parse_download_input(input: &str) -> AppResult<ModelDownloadRequest> {
    let mut parts = input.split_whitespace();
    let repo_id = required_part(parts.next(), "repo_id")?.to_string();
    let patterns = parts.next().and_then(|raw| {
        let values = raw
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if values.is_empty() {
            None
        } else {
            Some(values)
        }
    });
    let local_dir = parts.next().map(ToString::to_string);
    Ok(ModelDownloadRequest {
        repo_id,
        patterns,
        local_dir,
    })
}

fn start_download_job(
    service: &Arc<AppService>,
    state: &mut TuiState,
    request: ModelDownloadRequest,
) {
    let target_dir = resolve_download_target_dir(&request, &service.paths.models_dir);
    let script_path = download_script_path(&target_dir);
    let (progress_tx, mut progress_rx) = unbounded_channel::<DownloadProgress>();
    let (event_tx, event_rx) = unbounded_channel::<DownloadUiEvent>();
    let svc = Arc::clone(service);
    let request_for_task = request.clone();
    let progress_forward_tx = event_tx.clone();

    tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            if progress_forward_tx
                .send(DownloadUiEvent::Progress(progress))
                .is_err()
            {
                break;
            }
        }
    });
    tokio::spawn(async move {
        match svc
            .download_model_with_progress(request_for_task, Some(progress_tx))
            .await
        {
            Ok(path) => {
                let _ = event_tx.send(DownloadUiEvent::Completed { path });
            }
            Err(err) => {
                let _ = event_tx.send(DownloadUiEvent::Failed(err.to_string()));
            }
        }
    });

    state.download_events = Some(event_rx);
    state.download_job = Some(DownloadJobState {
        repo_id: request.repo_id,
        target_dir: target_dir.display().to_string(),
        script_path: script_path.display().to_string(),
        progress_percent: 0.0,
        phase: "queued".to_string(),
        latest_message: "waiting for downloader".to_string(),
        running: true,
        foreground: true,
    });
}

async fn drain_download_events(service: &Arc<AppService>, state: &mut TuiState) -> AppResult<()> {
    let mut refresh_needed = false;
    let mut disconnected = false;
    if let Some(receiver) = state.download_events.as_mut() {
        loop {
            match receiver.try_recv() {
                Ok(DownloadUiEvent::Progress(progress)) => {
                    if let Some(job) = state.download_job.as_mut() {
                        job.phase = "downloading".to_string();
                        if let Some(percent) = progress.percent {
                            job.progress_percent =
                                job.progress_percent.max(percent.clamp(0.0, 99.5));
                        } else if job.running {
                            job.progress_percent = (job.progress_percent + 0.4).min(95.0);
                        }
                        job.latest_message = progress.message;
                    }
                }
                Ok(DownloadUiEvent::Completed { path }) => {
                    if let Some(job) = state.download_job.as_mut() {
                        job.running = false;
                        job.phase = "completed".to_string();
                        job.progress_percent = 100.0;
                        job.target_dir = path.clone();
                        job.script_path =
                            format!("{}/download-model.sh", path.trim_end_matches('/'));
                        job.latest_message = format!("downloaded into {}", path);
                    }
                    state.message = format!("download completed: {}", path);
                    refresh_needed = true;
                }
                Ok(DownloadUiEvent::Failed(err)) => {
                    if let Some(job) = state.download_job.as_mut() {
                        job.running = false;
                        job.phase = "failed".to_string();
                        job.latest_message = err.clone();
                    }
                    state.message = format!("download failed: {err}");
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
    }
    if disconnected {
        state.download_events = None;
    }
    if refresh_needed {
        refresh_all(service, state).await?;
    }
    Ok(())
}

fn parse_json_or_string(input: &str) -> Value {
    serde_json::from_str::<Value>(input).unwrap_or_else(|_| Value::String(input.to_string()))
}

fn move_selection(state: &mut TuiState, down: bool) {
    match state.tab {
        Tab::Instances => move_index(&mut state.selected_instance, state.instances.len(), down),
        Tab::Families => match state.family_focus {
            FamilyFocus::Templates => {
                move_index(&mut state.selected_template, state.templates.len(), down);
                state.selected_variant = 0;
            }
            FamilyFocus::Variants => {
                let variants = selected_template(state)
                    .map(|template| template.overrides.len())
                    .unwrap_or(0);
                move_index(&mut state.selected_variant, variants, down);
            }
        },
        Tab::Models => move_index(&mut state.selected_model, state.models.len(), down),
        Tab::System => {}
    }
}

fn move_index(index: &mut usize, len: usize, down: bool) {
    if len == 0 {
        *index = 0;
        return;
    }
    if down {
        if *index + 1 < len {
            *index += 1;
        }
    } else {
        *index = index.saturating_sub(1);
    }
}

fn draw(frame: &mut Frame<'_>, state: &TuiState) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(5),
        ])
        .split(frame.area());

    draw_header(frame, sections[0], state);
    match state.tab {
        Tab::Instances => draw_instances_tab(frame, sections[1], state),
        Tab::Families => draw_families_tab(frame, sections[1], state),
        Tab::Models => draw_models_tab(frame, sections[1], state),
        Tab::System => draw_system_tab(frame, sections[1], state),
    }
    draw_footer(frame, sections[2], state);
    draw_log_viewer(frame, state);
    draw_prompt(frame, state);
    draw_download_overlay(frame, state);
}

fn draw_header(frame: &mut Frame<'_>, area: ratatui::layout::Rect, state: &TuiState) {
    let tab_text = [Tab::Instances, Tab::Families, Tab::Models, Tab::System]
        .into_iter()
        .map(|tab| {
            if tab == state.tab {
                format!("[{}]", tab.label())
            } else {
                tab.label().to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("  ");
    let text = format!(
        "manager-neo TUI | {tab_text} | inst:{} tpl:{} mdl:{}",
        state.instances.len(),
        state.templates.len(),
        state.models.len()
    );
    frame.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Overview")),
        area,
    );
}

fn draw_instances_tab(frame: &mut Frame<'_>, area: ratatui::layout::Rect, state: &TuiState) {
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
        .split(area);

    let rows = state.instances.iter().map(|(instance, status)| {
        let status_text = status.status.to_uppercase();
        let status_style = tui_status_style(&status.status);
        let (family, model_name, quant) = model_hierarchy_parts(&instance.config.model);
        let fit_level = memory_fit_level(
            state.memory_previews.get(&instance.name),
            state.metrics.as_ref(),
        );
        Row::new(vec![
            Cell::from(crate::service::strip_quant_suffix_from_name(
                &instance.name,
                &quant,
            )),
            Cell::from(family),
            Cell::from(format!("{model_name} [{quant}]")),
            Cell::from(instance.config.host_port.to_string()),
            Cell::from(status_text).style(status_style),
            Cell::from(memory_fit_badge(fit_level)).style(memory_fit_style(fit_level)),
        ])
    });
    let header = Row::new(["Name", "Family", "Model", "Port", "Status", "Mem"]).style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    let mut table_state = TableState::default();
    if !state.instances.is_empty() {
        table_state.select(Some(state.selected_instance));
    }
    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Length(10),
            Constraint::Percentage(50),
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .block(Block::default().borders(Borders::ALL).title("Instances"));
    frame.render_stateful_widget(table, split[0], &mut table_state);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(24),
            Constraint::Min(8),
        ])
        .split(split[1]);

    let runtime_text = selected_instance(state)
        .map(|(instance, status)| {
            let (family, model_name, quant) = model_hierarchy_parts(&instance.config.model);
            format!(
                "Instance  {}\nStatus    {}\nFamily    {}\nModel     {}\nQuant     {}\nMMProj    {}\nPort      {} -> {}",
                crate::service::strip_quant_suffix_from_name(&instance.name, &quant),
                status.status.to_uppercase(),
                family,
                model_name,
                quant,
                instance
                    .config
                    .mmproj
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                instance.config.host_port,
                instance.config.port
            )
        })
        .unwrap_or_else(|| "no instance selected".to_string());
    frame.render_widget(
        Paragraph::new(runtime_text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Runtime")),
        right[0],
    );

    let sampling_text = selected_instance(state)
        .map(|(instance, _)| {
            let temp_pct = (instance.config.sampling.temp * 100.0).clamp(0.0, 100.0);
            let top_p_pct = (instance.config.sampling.top_p * 100.0).clamp(0.0, 100.0);
            format!(
                "Temp      {:>5.2}  {}\nTop-P     {:>5.2}  {}\nTop-K     {:>5}\nMin-P     {:>5.3}",
                instance.config.sampling.temp,
                bar(temp_pct, 18),
                instance.config.sampling.top_p,
                bar(top_p_pct, 18),
                instance.config.sampling.top_k,
                instance.config.sampling.min_p
            )
        })
        .unwrap_or_else(|| "sampling n/a".to_string());
    frame.render_widget(
        Paragraph::new(sampling_text)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Sampling")),
        right[1],
    );

    let infra_rows = selected_instance(state)
        .map(|(instance, _)| {
            let preview = state.memory_previews.get(&instance.name);
            let fit_level = memory_fit_level(preview, state.metrics.as_ref());
            let mut rows = vec![
                kv_row("Ctx Size", instance.config.ctx_size.to_string()),
                kv_row("Threads", instance.config.threads.to_string()),
                kv_row("GPU Layers", instance.config.n_gpu_layers.to_string()),
                kv_row(
                    "Parallel",
                    instance
                        .config
                        .parallel
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "1".to_string()),
                ),
                kv_row(
                    "Cache",
                    format!(
                        "{}/{}",
                        instance.config.cache_type_k, instance.config.cache_type_v
                    ),
                ),
            ];
            if let Some(preview) = preview {
                rows.push(kv_row(
                    "Load Est",
                    bytes_human(preview.estimated_total_bytes),
                ));
                rows.push(kv_row("Weights", bytes_human(preview.model_bytes)));
                rows.push(kv_row("KV Cache", bytes_human(preview.kv_cache_bytes)));
                let available = state
                    .metrics
                    .as_ref()
                    .map(ram_available_bytes)
                    .unwrap_or_default();
                if available > 0 {
                    rows.push(kv_row("Avail RAM", bytes_human(available)));
                }
                rows.push(
                    Row::new(vec![
                        Cell::from("Fit"),
                        Cell::from(memory_fit_badge(fit_level)).style(memory_fit_style(fit_level)),
                    ])
                    .style(memory_fit_style(fit_level)),
                );
                if let Some(warning) = &preview.warning {
                    rows.push(kv_row("Note", ellipsize(warning, 46)));
                }
            } else {
                rows.push(kv_row("Load Est", "n/a"));
                rows.push(kv_row("Fit", "unknown"));
            }
            rows
        })
        .unwrap_or_else(|| vec![kv_row("Execution", "n/a")]);
    frame.render_widget(
        Table::new(infra_rows, [Constraint::Length(10), Constraint::Min(1)])
            .column_spacing(1)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Execution Profile + Memory"),
            ),
        right[2],
    );
}

fn draw_families_tab(frame: &mut Frame<'_>, area: ratatui::layout::Rect, state: &TuiState) {
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let template_rows = state.templates.iter().map(|template| {
        let (_, model_name, quant) = model_hierarchy_parts(&template.config.model);
        Row::new(vec![
            Cell::from(template.family.clone()),
            Cell::from(model_name),
            Cell::from(quant),
            Cell::from(template.name.clone()),
            Cell::from(template.overrides.len().to_string()),
        ])
    });
    let header = Row::new(["Family", "Model", "Quant", "Template", "Variants"]).style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    let mut template_state = TableState::default();
    if !state.templates.is_empty() {
        template_state.select(Some(state.selected_template));
    }
    frame.render_stateful_widget(
        Table::new(
            template_rows,
            [
                Constraint::Percentage(24),
                Constraint::Percentage(28),
                Constraint::Length(9),
                Constraint::Percentage(34),
                Constraint::Length(8),
            ],
        )
        .header(header)
        .row_highlight_style(if state.family_focus == FamilyFocus::Templates {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::Gray)
        })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Model Families"),
        ),
        split[0],
        &mut template_state,
    );

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(split[1]);

    let template = selected_template(state);
    let runtime_rows = template
        .map(|tpl| {
            vec![
                kv_row("Template", tpl.name.clone()),
                kv_row("Family", tpl.family.clone()),
                kv_row("Model", model_hierarchy_parts(&tpl.config.model).1),
                kv_row("Quant", model_hierarchy_parts(&tpl.config.model).2),
                kv_row(
                    "MMProj",
                    tpl.config.mmproj.clone().unwrap_or_else(|| "-".to_string()),
                ),
                kv_row("Model Ref", tpl.config.model.clone()),
                kv_row("Ctx Size", tpl.config.ctx_size.to_string()),
                kv_row("Threads", tpl.config.threads.to_string()),
                kv_row(
                    "Parallel",
                    tpl.config
                        .parallel
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ),
            ]
        })
        .unwrap_or_else(|| vec![kv_row("Template", "none selected")]);
    let sampling_rows = template
        .map(|tpl| {
            vec![
                kv_row(
                    "Temp",
                    format!(
                        "{:.2} {}",
                        tpl.config.sampling.temp,
                        bar((tpl.config.sampling.temp * 100.0).clamp(0.0, 100.0), 12)
                    ),
                ),
                kv_row(
                    "Top-P",
                    format!(
                        "{:.2} {}",
                        tpl.config.sampling.top_p,
                        bar((tpl.config.sampling.top_p * 100.0).clamp(0.0, 100.0), 12)
                    ),
                ),
                kv_row("Top-K", tpl.config.sampling.top_k.to_string()),
                kv_row("Min-P", format!("{:.3}", tpl.config.sampling.min_p)),
            ]
        })
        .unwrap_or_else(|| vec![kv_row("Sampling", "n/a")]);
    let base_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
        .split(right[0]);
    frame.render_widget(
        Table::new(runtime_rows, [Constraint::Length(14), Constraint::Min(1)])
            .column_spacing(1)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Runtime Base (e to edit)"),
            ),
        base_split[0],
    );
    frame.render_widget(
        Table::new(sampling_rows, [Constraint::Length(9), Constraint::Min(1)])
            .column_spacing(1)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Sampling Base"),
            ),
        base_split[1],
    );

    let inner = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(right[1]);

    let variant_entries = template
        .map(|tpl| {
            tpl.overrides
                .iter()
                .map(|(name, map)| (name.clone(), map.len()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let variant_rows = variant_entries.iter().map(|(name, changes)| {
        Row::new(vec![
            Cell::from(name.clone()),
            Cell::from(changes.to_string()),
        ])
    });
    let mut variant_state = TableState::default();
    if !variant_entries.is_empty() {
        variant_state.select(Some(state.selected_variant.min(variant_entries.len() - 1)));
    }
    frame.render_stateful_widget(
        Table::new(
            variant_rows,
            [Constraint::Percentage(72), Constraint::Length(8)],
        )
        .header(
            Row::new(["Variant", "Δ"]).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .row_highlight_style(if state.family_focus == FamilyFocus::Variants {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::Gray)
        })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Variant Diffs"),
        ),
        inner[0],
        &mut variant_state,
    );

    let diff_rows = template
        .and_then(|tpl| {
            let entries = tpl.overrides.iter().collect::<Vec<_>>();
            entries
                .get(state.selected_variant.min(entries.len().saturating_sub(1)))
                .map(|(variant, diff)| {
                    let mut rows = vec![Row::new(vec![
                        Cell::from("META"),
                        Cell::from("meta.variant"),
                        Cell::from((*variant).to_string()),
                    ])];
                    rows.extend(diff.iter().map(|(key, value)| {
                        Row::new(vec![
                            Cell::from(config_source_label(key)),
                            Cell::from(display_config_key(key)),
                            Cell::from(value_to_inline(value)),
                        ])
                    }));
                    rows
                })
        })
        .unwrap_or_else(|| {
            vec![Row::new(vec![
                Cell::from("META"),
                Cell::from("meta.variant"),
                Cell::from("none selected"),
            ])]
        });
    frame.render_widget(
        Table::new(
            diff_rows,
            [
                Constraint::Length(8),
                Constraint::Length(36),
                Constraint::Min(1),
            ],
        )
        .column_spacing(1)
        .header(
            Row::new(["Source", "Key", "Value"]).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Diff Detail (o to edit override)"),
        ),
        inner[1],
    );
}

fn draw_models_tab(frame: &mut Frame<'_>, area: ratatui::layout::Rect, state: &TuiState) {
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(54), Constraint::Percentage(46)])
        .split(area);

    let model_rows = state.models.iter().map(|model| {
        let (family, model_name, folder_quant) = model_hierarchy_parts(&model.name);
        // prefer detected quant from primary gguf filename when available
        let mut gguf_files = model
            .files
            .iter()
            .filter(|file| file.path.extension().and_then(|ext| ext.to_str()) == Some("gguf"))
            .filter(|file| !file.name.to_ascii_lowercase().contains("mmproj"))
            .collect::<Vec<_>>();
        gguf_files.sort_by_key(|file| std::cmp::Reverse(file.size_bytes));
        let detected_quant = gguf_files
            .first()
            .and_then(|file| detect_quant_from_filename_tui(&file.name));
        let quant_display = if let Some(det) = detected_quant.clone() {
            if det != folder_quant {
                format!("{} (!)", det)
            } else {
                det
            }
        } else {
            folder_quant.clone()
        };
        Row::new(vec![
            Cell::from(family),
            Cell::from(model_name),
            Cell::from(quant_display),
            Cell::from(model.files.len().to_string()),
            Cell::from(model.total_size_human()),
        ])
    });
    let mut model_state = TableState::default();
    if !state.models.is_empty() {
        model_state.select(Some(state.selected_model));
    }
    frame.render_stateful_widget(
        Table::new(
            model_rows,
            [
                Constraint::Percentage(24),
                Constraint::Percentage(28),
                Constraint::Length(9),
                Constraint::Length(7),
                Constraint::Length(10),
            ],
        )
        .header(
            Row::new(["Family", "Model", "Quant", "Files", "Size"]).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Model Directories"),
        ),
        split[0],
        &mut model_state,
    );

    let details = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(1)])
        .split(split[1]);

    let summary_rows = selected_model(state)
        .map(|model| {
            let (family, model_name, folder_quant) = model_hierarchy_parts(&model.name);
            // detect primary gguf filename quant when present
            let mut gguf_files = model
                .files
                .iter()
                .filter(|file| file.path.extension().and_then(|ext| ext.to_str()) == Some("gguf"))
                .filter(|file| !file.name.to_ascii_lowercase().contains("mmproj"))
                .collect::<Vec<_>>();
            gguf_files.sort_by_key(|file| std::cmp::Reverse(file.size_bytes));
            let detected_quant = gguf_files
                .first()
                .and_then(|file| detect_quant_from_filename_tui(&file.name));
            let mut rows = vec![
                kv_row("Family", family),
                kv_row("Model", model_name),
                kv_row("Quant", folder_quant.clone()),
                kv_row("Files", model.files.len().to_string()),
                kv_row("Total", model.total_size_human()),
                kv_row("Path", model.path.display().to_string()),
            ];
            if let Some(det) = detected_quant {
                if det != folder_quant {
                    rows.push(kv_row(
                        "Quant Note",
                        format!("folder: {} != detected: {}", folder_quant, det),
                    ));
                }
            }
            if let Some(job) = &state.download_job {
                rows.push(kv_row(
                    "Download",
                    format!("{:.1}% {}", job.progress_percent, job.phase),
                ));
                rows.push(kv_row("Repo", job.repo_id.clone()));
                rows.push(kv_row("Script", job.script_path.clone()));
            }
            rows
        })
        .unwrap_or_else(|| vec![kv_row("Model", "none selected")]);
    frame.render_widget(
        Table::new(summary_rows, [Constraint::Length(12), Constraint::Min(1)])
            .column_spacing(1)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Model Summary (u rename / d delete)"),
            ),
        details[0],
    );

    let file_rows = selected_model(state)
        .map(|model| {
            model
                .files
                .iter()
                .take(200)
                .map(|file| {
                    Row::new(vec![
                        Cell::from(file.name.clone()),
                        Cell::from(file.size_human()),
                        Cell::from(file.path.display().to_string()),
                    ])
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    frame.render_widget(
        Table::new(
            file_rows,
            [
                Constraint::Length(34),
                Constraint::Length(10),
                Constraint::Percentage(60),
            ],
        )
        .header(
            Row::new(["File", "Size", "Path"]).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Model Files (Read + Delete/Rename/Download)"),
        ),
        details[1],
    );
}

fn draw_system_tab(frame: &mut Frame<'_>, area: ratatui::layout::Rect, state: &TuiState) {
    let Some(metrics) = &state.metrics else {
        frame.render_widget(
            Paragraph::new("system metrics unavailable (auto-refresh every 3s)").block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("System Metrics"),
            ),
            area,
        );
        return;
    };

    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Min(8),
        ])
        .split(area);
    let cpu_inner_w = split[0].width.saturating_sub(2) as usize;
    let ram_inner_w = split[1].width.saturating_sub(2) as usize;

    let cpu = &metrics.cpu;
    let cpu_text = format!(
        "{usage}\nCores    {cores}\nLoad avg {l1:.2}/{l5:.2}/{l15:.2}",
        usage = usage_line("Usage", cpu.usage_percent, cpu_inner_w, 16),
        cores = cpu.cores,
        l1 = cpu.load_1,
        l5 = cpu.load_5,
        l15 = cpu.load_15
    );
    frame.render_widget(
        Paragraph::new(cpu_text)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("CPU")),
        split[0],
    );

    let ram = &metrics.ram;
    let ram_text = format!(
        "{usage}\nUsed/Total {used} / {total} MB\nFree/Avail {free} / {avail} MB",
        usage = usage_line("Usage", ram.usage_percent, ram_inner_w, 16),
        used = ram.used_mb,
        total = ram.total_mb,
        free = ram.free_mb,
        avail = ram.available_mb
    );
    frame.render_widget(
        Paragraph::new(ram_text)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("RAM")),
        split[1],
    );

    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(74), Constraint::Percentage(26)])
        .split(split[2]);
    let gpu_metric_col_w = bottom[0]
        .width
        .saturating_sub(34)
        .saturating_div(2)
        .clamp(12, 16);
    let gpu_bar_w = gpu_metric_col_w.saturating_sub(8).clamp(4, 8) as usize;

    if metrics.rocm.devices.is_empty() {
        let text = if metrics.rocm.available {
            "No GPU devices reported by rocm-smi".to_string()
        } else {
            format!(
                "ROCm unavailable\n{}",
                metrics
                    .rocm
                    .error
                    .clone()
                    .unwrap_or_else(|| "rocm-smi not installed".to_string())
            )
        };
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).title("GPU Devices")),
            bottom[0],
        );
    } else {
        let gpu_rows = metrics.rocm.devices.iter().map(|gpu| {
            let util = gpu.utilization_percent.unwrap_or(0.0);
            let mem = gpu.memory_use_percent.unwrap_or(0.0);
            Row::new(vec![
                Cell::from(gpu.id.clone()),
                Cell::from(gpu.name.clone().unwrap_or_else(|| "-".to_string())),
                Cell::from(format!("{util:>5.1}% {}", bar(util, gpu_bar_w))),
                Cell::from(format!("{mem:>5.1}% {}", bar(mem, gpu_bar_w))),
                Cell::from(
                    gpu.temperature_c
                        .map(|v| format!("{v:.1}C"))
                        .unwrap_or_else(|| "-".to_string()),
                ),
            ])
        });
        frame.render_widget(
            Table::new(
                gpu_rows,
                [
                    Constraint::Length(8),
                    Constraint::Min(12),
                    Constraint::Length(gpu_metric_col_w),
                    Constraint::Length(gpu_metric_col_w),
                    Constraint::Length(7),
                ],
            )
            .header(
                Row::new(["ID", "Name", "Utilization", "VRAM Use", "Temp"]).style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("GPU Devices (visual)"),
            ),
            bottom[0],
        );
    }

    let status_lines = vec![
        kv_line("Update", format!("epoch {}", metrics.unix_time)),
        kv_line(
            "ROCm",
            if metrics.rocm.available {
                "ONLINE"
            } else {
                "OFFLINE"
            },
        ),
        kv_line("GPUs", metrics.rocm.devices.len().to_string()),
        kv_line("Refresh", "auto ~3s"),
        kv_line("Manual", "press r"),
    ];
    frame.render_widget(
        Paragraph::new(status_lines.join("\n"))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Monitor Status"),
            ),
        bottom[1],
    );
}

fn draw_log_viewer(frame: &mut Frame<'_>, state: &TuiState) {
    let Some(viewer) = &state.log_viewer else {
        return;
    };
    let area = centered_rect(92, frame.area().height.saturating_sub(6), frame.area());
    frame.render_widget(Clear, area);
    let inner_height = area.height.saturating_sub(2) as usize;
    let visible_lines = inner_height.saturating_sub(1).max(1);
    let max_start = viewer.lines.len().saturating_sub(visible_lines);
    let start = viewer.scroll.min(max_start);
    let text = if viewer.lines.is_empty() {
        "<empty>".to_string()
    } else {
        viewer
            .lines
            .iter()
            .skip(start)
            .take(visible_lines)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    };
    let title = format!(
        "Logs | {} | lines {}-{} / {} | home/end jump | / search | n/N next/prev",
        viewer.instance_name,
        start.saturating_add(1),
        (start + visible_lines).min(viewer.lines.len()),
        viewer.lines.len()
    );
    let title = if viewer.search_mode {
        format!("{title} | SEARCH: {}", viewer.search_query)
    } else if viewer.search_query.is_empty() {
        title
    } else {
        format!("{title} | query: {}", viewer.search_query)
    };
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn bar(percent: f64, width: usize) -> String {
    let clamped = percent.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * width as f64).round() as usize;
    let mut out = String::with_capacity(width);
    for idx in 0..width {
        out.push(if idx < filled { '#' } else { '-' });
    }
    out
}

fn kv_row<K: Into<String>, V: Into<String>>(key: K, value: V) -> Row<'static> {
    Row::new(vec![Cell::from(key.into()), Cell::from(value.into())])
}

fn kv_line<K: AsRef<str>, V: AsRef<str>>(key: K, value: V) -> String {
    format!("{:<7} {}", key.as_ref(), value.as_ref())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MemoryFitLevel {
    Good,
    Warn,
    Bad,
    Unknown,
}

fn value_to_inline(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "<invalid>".to_string()),
    }
}

fn config_source_label(key: &str) -> &'static str {
    match config_key_source(key) {
        ConfigKeySource::Llama => "LLAMA",
        ConfigKeySource::Compose => "COMPOSE",
        ConfigKeySource::Meta => "META",
        ConfigKeySource::Other => "OTHER",
    }
}

fn tui_status_style(status: &str) -> Style {
    let lowered = status.to_lowercase();
    if lowered.contains("up") || lowered.contains("run") {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else if lowered.contains("stop") || lowered.contains("exit") {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Red)
    }
}

fn ram_available_bytes(metrics: &SystemMetrics) -> u64 {
    metrics.ram.available_mb.saturating_mul(1024 * 1024)
}

fn memory_fit_level(
    preview: Option<&InstanceMemoryPreview>,
    metrics: Option<&SystemMetrics>,
) -> MemoryFitLevel {
    let Some(preview) = preview else {
        return MemoryFitLevel::Unknown;
    };
    if preview.estimated_total_bytes == 0 {
        return MemoryFitLevel::Unknown;
    }
    let Some(metrics) = metrics else {
        return MemoryFitLevel::Unknown;
    };
    let available = ram_available_bytes(metrics);
    if available == 0 {
        return MemoryFitLevel::Unknown;
    }
    let ratio = preview.estimated_total_bytes as f64 / available as f64;
    if ratio <= 0.75 {
        MemoryFitLevel::Good
    } else if ratio <= 1.0 {
        MemoryFitLevel::Warn
    } else {
        MemoryFitLevel::Bad
    }
}

fn memory_fit_badge(level: MemoryFitLevel) -> &'static str {
    match level {
        MemoryFitLevel::Good => "GOOD",
        MemoryFitLevel::Warn => "WARN",
        MemoryFitLevel::Bad => "LOW",
        MemoryFitLevel::Unknown => "N/A",
    }
}

fn memory_fit_style(level: MemoryFitLevel) -> Style {
    match level {
        MemoryFitLevel::Good => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        MemoryFitLevel::Warn => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        MemoryFitLevel::Bad => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        MemoryFitLevel::Unknown => Style::default().fg(Color::Gray),
    }
}

fn bytes_human(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = bytes as f64;
    let mut unit = 0usize;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

fn draw_footer(frame: &mut Frame<'_>, area: ratatui::layout::Rect, state: &TuiState) {
    if state.log_viewer.is_some() {
        frame.render_widget(
            Paragraph::new(
                "LOG VIEW | ↑↓/Pg scroll | Home/End jump | / search | n/N next/prev | r reload | esc close",
            )
                .block(Block::default().borders(Borders::ALL).title("Controls")),
            area,
        );
        return;
    }
    let help = match state.tab {
        Tab::Instances => {
            "r refresh | ↑↓ select | n new | e edit key | s start | x stop | t restart | l logs | q quit"
        }
        Tab::Families => {
            "r refresh | ↑↓ select | ←/→ focus templates/variants | a scan | i instantiate | b batch apply | o override | e base edit | q quit"
        }
        Tab::Models => {
            "r refresh | ↑↓ select | n download | g download fg/bg | i new instance | f new family | u rename | d delete(confirm) | q quit"
        }
        Tab::System => "auto-refresh ~3s | r full refresh | visual CPU/RAM/GPU monitor | q quit",
    };
    let download_line = state.download_job.as_ref().map(|job| {
        let state_label = if job.running {
            "RUNNING".to_string()
        } else {
            job.phase.to_uppercase()
        };
        format!(
            "download[{state_label}] {:>5.1}% repo={} script={}",
            job.progress_percent, job.repo_id, job.script_path
        )
    });
    let message = if state.message.is_empty() && download_line.is_none() {
        help.to_string()
    } else if state.message.is_empty() {
        format!("{help}\n> {}", download_line.unwrap_or_default())
    } else if let Some(download_line) = download_line {
        format!("{help}\n> {}\n> {}", state.message, download_line)
    } else {
        format!("{help}\n> {}", state.message)
    };
    frame.render_widget(
        Paragraph::new(message)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Controls")),
        area,
    );
}

fn draw_prompt(frame: &mut Frame<'_>, state: &TuiState) {
    let Some(prompt) = &state.prompt else {
        return;
    };
    let active_idx = prompt
        .active_field
        .min(prompt.fields.len().saturating_sub(1));
    let values = prompt.inputs.clone();
    let hint_rows = prompt_key_hint_rows(prompt, &values, active_idx);
    let help_lines = prompt_help_lines(&prompt.action);
    let fields_height = if prompt.fields.is_empty() {
        1
    } else {
        prompt.fields.len() as u16
    };
    let height = fields_height
        .saturating_add(4)
        .saturating_add(help_lines.len() as u16)
        .saturating_add(hint_rows.len() as u16);
    let area = centered_rect(80, height, frame.area());
    frame.render_widget(Clear, area);
    let prompt_block = Block::default().borders(Borders::ALL).title("Input");
    let inner = prompt_block.inner(area);
    frame.render_widget(prompt_block, area);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(fields_height),
            Constraint::Length(hint_rows.len() as u16),
            Constraint::Length(help_lines.len() as u16),
            Constraint::Min(0),
        ])
        .split(inner);
    let label = ellipsize(&prompt.label, inner.width as usize);
    frame.render_widget(Paragraph::new(label), vertical[0]);

    if !prompt.fields.is_empty() {
        let label_width = prompt_label_width(prompt);
        let field_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(1); prompt.fields.len()])
            .split(vertical[1]);
        for (idx, field) in prompt.fields.iter().enumerate() {
            let marker = if idx == active_idx { '>' } else { ' ' };
            let value = values.get(idx).cloned().unwrap_or_default();
            let label = format!("{field:<label_width$}");
            let prefix = format!("{marker} {label} │ ");
            let prefix_width = prefix.chars().count() as u16;
            let width = field_rows[idx].width.saturating_sub(prefix_width + 1);
            let cursor = prompt.cursors.get(idx).copied().unwrap_or(0);
            let (display, cursor_x) = clipped_value_with_cursor(&value, cursor, width as usize);
            let line = format!("{prefix}{display}");
            frame.render_widget(
                Paragraph::new(line).style(if idx == active_idx {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                }),
                field_rows[idx],
            );
            if idx == active_idx {
                let desired_x = field_rows[idx]
                    .x
                    .saturating_add(prefix_width)
                    .saturating_add(cursor_x as u16);
                let max_x = field_rows[idx]
                    .x
                    .saturating_add(field_rows[idx].width.saturating_sub(1));
                frame.set_cursor_position((desired_x.min(max_x), field_rows[idx].y));
            }
        }
    }

    if !hint_rows.is_empty() {
        frame.render_widget(
            Paragraph::new(hint_rows.join("\n")).style(Style::default().fg(Color::Yellow)),
            vertical[2],
        );
    }
    if !help_lines.is_empty() {
        frame.render_widget(
            Paragraph::new(help_lines.join("\n")).style(Style::default().fg(Color::DarkGray)),
            vertical[3],
        );
    }
}

fn draw_download_overlay(frame: &mut Frame<'_>, state: &TuiState) {
    let Some(job) = &state.download_job else {
        return;
    };
    if !job.foreground || !job.running {
        return;
    }
    let area = centered_rect(76, 9, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Background Download (g to hide)");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
    let pct = job.progress_percent.clamp(0.0, 100.0);
    let bar_width = rows[2].width.saturating_sub(14) as usize;
    frame.render_widget(Paragraph::new(format!("Repo     {}", job.repo_id)), rows[0]);
    frame.render_widget(Paragraph::new(format!("Phase    {}", job.phase)), rows[1]);
    frame.render_widget(
        Paragraph::new(format!(
            "Progress {:>5.1}% {}",
            pct,
            bar(pct, bar_width.max(8))
        ))
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "Message  {}",
            ellipsize(&job.latest_message, rows[3].width as usize)
        )),
        rows[3],
    );
}

fn prompt_key_hint_rows(prompt: &PromptState, values: &[String], active_idx: usize) -> Vec<String> {
    let Some(key_field_index) = prompt.key_field_index else {
        return vec![];
    };
    if key_field_index != active_idx || prompt.key_hints.is_empty() {
        return vec![];
    }
    let prefix = values
        .get(key_field_index)
        .map(|v| v.as_str())
        .unwrap_or("");
    let indent = " ".repeat(prompt_label_width(prompt).saturating_add(5));
    let matches = filter_key_hints(&prompt.key_hints, prefix, 6);
    if matches.is_empty() {
        vec![
            format!("{indent}key hints: (no match, keep typing)"),
            format!("{indent}→ right arrow to autocomplete"),
        ]
    } else {
        let mut rows = vec![format!(
            "{indent}key hints (type to filter, → autocomplete):"
        )];
        rows.extend(matches.into_iter().map(|hint| format!("{indent}- {hint}")));
        rows
    }
}

fn prompt_label_width(prompt: &PromptState) -> usize {
    prompt
        .fields
        .iter()
        .map(|field| field.chars().count())
        .max()
        .unwrap_or(8)
        .clamp(8, 16)
}

fn autocomplete_prompt_key(prompt_state: &mut PromptState) -> bool {
    let Some(key_field_index) = prompt_state.key_field_index else {
        return false;
    };
    if key_field_index != prompt_state.active_field {
        return false;
    }
    if key_field_index >= prompt_state.inputs.len() {
        return false;
    }
    let prefix = prompt_state.inputs[key_field_index].as_str();
    let mut matches = filter_key_hints(&prompt_state.key_hints, prefix, 1);
    let Some(selected) = matches.pop() else {
        return false;
    };
    prompt_state.inputs[key_field_index] = selected;
    if let Some(cursor) = prompt_state.cursors.get_mut(key_field_index) {
        *cursor = prompt_state.inputs[key_field_index].chars().count();
    }
    true
}

fn filter_key_hints(hints: &[String], prefix: &str, limit: usize) -> Vec<String> {
    let term = prefix.trim().to_lowercase();
    let mut starts = hints
        .iter()
        .filter(|hint| hint.to_lowercase().starts_with(&term))
        .cloned()
        .collect::<Vec<_>>();
    if starts.len() >= limit || term.is_empty() {
        starts.truncate(limit);
        return starts;
    }
    let mut contains = hints
        .iter()
        .filter(|hint| hint.to_lowercase().contains(&term))
        .filter(|hint| !starts.contains(*hint))
        .cloned()
        .collect::<Vec<_>>();
    starts.append(&mut contains);
    starts.truncate(limit);
    starts
}

fn find_log_match(lines: &[String], query: &str, start: usize, forward: bool) -> Option<usize> {
    let term = query.trim().to_lowercase();
    if term.is_empty() || lines.is_empty() {
        return None;
    }
    if forward {
        for (idx, line) in lines.iter().enumerate().skip(start.min(lines.len() - 1)) {
            if line.to_lowercase().contains(&term) {
                return Some(idx);
            }
        }
    } else {
        let start_idx = start.min(lines.len() - 1);
        for idx in (0..=start_idx).rev() {
            if lines[idx].to_lowercase().contains(&term) {
                return Some(idx);
            }
        }
    }
    None
}

fn insert_char_at(text: &mut String, idx: usize, ch: char) {
    let byte_idx = char_to_byte_index(text, idx);
    text.insert(byte_idx, ch);
}

fn remove_char_at(text: &mut String, idx: usize) {
    if idx >= text.chars().count() {
        return;
    }
    let start = char_to_byte_index(text, idx);
    let end = char_to_byte_index(text, idx + 1);
    text.replace_range(start..end, "");
}

fn char_to_byte_index(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| text.len())
}

fn clipped_value_with_cursor(value: &str, cursor: usize, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let chars = value.chars().collect::<Vec<_>>();
    let cursor = cursor.min(chars.len());
    let start = cursor.saturating_sub(width.saturating_sub(1));
    let end = (start + width).min(chars.len());
    let display = chars[start..end].iter().collect::<String>();
    let cursor_x = cursor.saturating_sub(start).min(width.saturating_sub(1));
    (display, cursor_x)
}

fn is_text_input(modifiers: KeyModifiers) -> bool {
    !(modifiers.contains(KeyModifiers::CONTROL)
        || modifiers.contains(KeyModifiers::ALT)
        || modifiers.contains(KeyModifiers::SUPER)
        || modifiers.contains(KeyModifiers::META)
        || modifiers.contains(KeyModifiers::HYPER))
}

fn is_backspace_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Backspace)
        || matches!(key.code, KeyCode::Char('\u{8}') | KeyCode::Char('\u{7f}'))
        || (matches!(key.code, KeyCode::Char('h') | KeyCode::Char('H'))
            && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn usage_line(label: &str, percent: f64, inner_width: usize, max_bar_width: usize) -> String {
    let base = format!("{label:<8} {:>5.1}%", percent.clamp(0.0, 100.0));
    let bar_space = inner_width.saturating_sub(base.chars().count().saturating_add(1));
    if bar_space >= 4 {
        let width = bar_space.min(max_bar_width);
        format!("{base} {}", bar(percent, width))
    } else {
        base
    }
}

fn ellipsize(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= width {
        return text.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut out = chars[..width - 1].iter().collect::<String>();
    out.push('…');
    out
}

fn prompt_help_lines(action: &PromptAction) -> Vec<String> {
    match action {
        PromptAction::SetTemplateOverride { .. } => vec![
            "Tips: key path like llama.sampling.temp / llama.n_gpu_layers / compose.host_port"
                .to_string(),
            "Value supports: number, string, boolean, compact JSON".to_string(),
            "Example: qwen3-32b llama.sampling.temp 0.7".to_string(),
        ],
        PromptAction::SetTemplateBase { .. } => vec![
            "Tips: base key updates family default config (llama./compose./meta.)".to_string(),
            "Example: llama.sampling.top_p 0.95".to_string(),
        ],
        PromptAction::BatchApplyTemplate { .. } => vec![
            "Tips: same key applied to all variants".to_string(),
            "Example: llama.n_gpu_layers 999".to_string(),
        ],
        PromptAction::InstantiateTemplate { .. } => vec![
            "Tips: overrides must be JSON object when provided".to_string(),
            r#"Example: infer-01 {"sampling.temp":0.7}"#.to_string(),
        ],
        PromptAction::DownloadModel => vec![
            "Download runs in background with progress updates".to_string(),
            "A shell script is saved as <target_dir>/download-model.sh".to_string(),
            "Press g in Models tab to switch foreground/background progress".to_string(),
        ],
        PromptAction::CreateInstanceFromModel { .. } => vec![
            "Creates a runnable instance with selected model prefilled".to_string(),
            "Optional port can be provided; leave empty for auto assignment".to_string(),
        ],
        PromptAction::CreateFamilyFromModel { .. } => vec![
            "Creates a template/family with selected model as base config".to_string(),
            "Provide template name; family name defaults from model path".to_string(),
        ],
        _ => vec![],
    }
}

fn centered_rect(
    percent_x: u16,
    height: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let width = area.width.saturating_mul(percent_x).saturating_div(100);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn selected_instance_name(state: &TuiState) -> Option<String> {
    state
        .instances
        .get(state.selected_instance)
        .map(|(instance, _)| instance.name.clone())
}

fn selected_instance(state: &TuiState) -> Option<&(Instance, InstanceStatus)> {
    state.instances.get(state.selected_instance)
}

fn selected_template_name(state: &TuiState) -> Option<String> {
    selected_template(state).map(|template| template.name.clone())
}

fn selected_template(state: &TuiState) -> Option<&Template> {
    state.templates.get(state.selected_template)
}

fn selected_model_name(state: &TuiState) -> Option<String> {
    selected_model(state).map(|model| model.name.clone())
}

fn selected_model(state: &TuiState) -> Option<&Model> {
    state.models.get(state.selected_model)
}

fn selected_model_model_refs(state: &TuiState) -> Option<(String, Option<String>)> {
    let model = selected_model(state)?;
    let mut gguf_files = model
        .files
        .iter()
        .filter(|file| file.path.extension().and_then(|ext| ext.to_str()) == Some("gguf"))
        .filter(|file| !file.name.to_ascii_lowercase().contains("mmproj"))
        .collect::<Vec<_>>();
    gguf_files.sort_by_key(|file| std::cmp::Reverse(file.size_bytes));
    let primary = gguf_files.first()?;
    let mmproj = model.files.iter().find(|file| {
        let name = file.name.to_ascii_lowercase();
        name.ends_with(".gguf") && name.contains("mmproj")
    });
    let model_ref = format!(
        "/models/{}/{}",
        model.name.trim_start_matches('/'),
        primary.name
    );
    let mmproj_ref = mmproj.map(|file| {
        format!(
            "/models/{}/{}",
            model.name.trim_start_matches('/'),
            file.name
        )
    });
    let primary_ref = model_ref;
    Some((primary_ref, mmproj_ref))
}

fn selected_model_family_source(state: &TuiState) -> Option<(String, Option<String>, String)> {
    let (model_ref, mmproj) = selected_model_model_refs(state)?;
    let family = infer_family_from_model_ref(&model_ref);
    Some((model_ref, mmproj, family))
}

fn infer_family_from_model_ref(model_ref: &str) -> String {
    let tail = model_ref.trim_start_matches("/models/");
    let first = tail.split('/').next().unwrap_or("family");
    match first.to_ascii_lowercase().as_str() {
        "qwen-3-5" | "qwen3-5" | "qwen3.5" | "qwen35" => "qwen-3.5".to_string(),
        "step-3-5" | "step3-5" | "step3.5" => "step-3.5".to_string(),
        _ => first.to_string(),
    }
}

fn model_hierarchy_parts(model_ref_or_key: &str) -> (String, String, String) {
    let trimmed = model_ref_or_key
        .trim()
        .trim_start_matches("/models/")
        .trim_start_matches("models/")
        .trim_matches('/');
    if trimmed.is_empty() {
        return ("-".to_string(), "-".to_string(), "-".to_string());
    }
    let parts = trimmed
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if parts.len() >= 3 {
        return (
            infer_family_from_model_ref(&format!("/models/{}", parts[0])),
            parts[1].to_ascii_lowercase(),
            parts[2].to_ascii_uppercase(),
        );
    }
    if parts.len() == 2 {
        return (
            infer_family_from_model_ref(&format!("/models/{}", parts[0])),
            parts[1].to_ascii_lowercase(),
            "-".to_string(),
        );
    }
    (
        infer_family_from_model_ref(&format!("/models/{}", parts[0])),
        parts[0].to_ascii_lowercase(),
        "-".to_string(),
    )
}

fn detect_quant_from_filename_tui(filename: &str) -> Option<String> {
    let lower = filename.to_ascii_lowercase();
    if lower.contains("mmproj") {
        return None;
    }
    let key = lower
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    const TOKENS: [&str; 23] = [
        "UD-Q4_K_XL",
        "UD-Q4_K_M",
        "UD-IQ4_XS",
        "UD-IQ4_NL",
        "Q6_K_P",
        "Q6_K",
        "Q5_K_L",
        "Q5_K_M",
        "Q5_K_S",
        "Q4_K_L",
        "Q4_K_M",
        "Q4_K_S",
        "Q3_K_M",
        "Q3_K_S",
        "Q2_K",
        "Q8_1",
        "Q8_0",
        "IQ4_XS",
        "IQ4_NL",
        "IQ3_M",
        "IQ3_S",
        "BF16",
        "F16",
    ];
    let mut sorted = TOKENS
        .iter()
        .map(|token| {
            let token_key = token
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_uppercase();
            (*token, token_key)
        })
        .collect::<Vec<_>>();
    sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (token, token_key) in sorted {
        if key.contains(&token_key) {
            return Some(token.to_string());
        }
    }
    None
}

fn instance_key_hints(state: &TuiState) -> Vec<String> {
    selected_instance(state)
        .map(|(instance, _)| collect_serialized_key_paths(&instance.config))
        .unwrap_or_default()
}

fn template_key_hints(state: &TuiState) -> Vec<String> {
    selected_template(state)
        .map(|template| collect_serialized_key_paths(&template.config))
        .unwrap_or_default()
}

fn collect_serialized_key_paths<T: Serialize>(value: &T) -> Vec<String> {
    let json = serde_json::to_value(value).unwrap_or(Value::Null);
    let mut keys = vec![];
    collect_key_paths(&json, "", &mut keys);
    keys = keys
        .into_iter()
        .map(|key| display_config_key(&key))
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
}

fn collect_key_paths(value: &Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                match child {
                    Value::Object(_) => collect_key_paths(child, &path, out),
                    _ => out.push(path),
                }
            }
        }
        _ => {
            if !prefix.is_empty() {
                out.push(prefix.to_string());
            }
        }
    }
}

async fn refresh_all(service: &Arc<AppService>, state: &mut TuiState) -> AppResult<()> {
    let instances = service.list_instances()?;
    let mut rows = Vec::with_capacity(instances.len());
    for instance in instances {
        let status = service
            .instance_status(&instance.name)
            .await
            .unwrap_or(InstanceStatus {
                name: instance.name.clone(),
                status: "unknown".to_string(),
                ports: None,
                error: Some("status unavailable".to_string()),
                raw: None,
            });
        rows.push((instance, status));
    }
    state.instances = rows;
    state.templates = service.list_templates()?;
    state.models = service.list_models()?;
    state.metrics = service.system_metrics().await.ok();
    state.memory_previews = service
        .instance_memory_previews()?
        .into_iter()
        .map(|preview| (preview.name.clone(), preview))
        .collect();

    normalize_index(&mut state.selected_instance, state.instances.len());
    normalize_index(&mut state.selected_template, state.templates.len());
    normalize_index(&mut state.selected_model, state.models.len());
    let variants_len = selected_template(state)
        .map(|template| template.overrides.len())
        .unwrap_or(0);
    normalize_index(&mut state.selected_variant, variants_len);
    Ok(())
}

fn normalize_index(index: &mut usize, len: usize) {
    if len == 0 {
        *index = 0;
    } else if *index >= len {
        *index = len - 1;
    }
}
