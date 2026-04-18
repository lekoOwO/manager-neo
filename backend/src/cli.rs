use std::{collections::HashMap, path::PathBuf, sync::Arc};

use clap::{Args, Parser, Subcommand};
use serde_json::Value;

use crate::{
    api::{ApiState, serve},
    config::WorkspacePaths,
    error::{AppError, AppResult},
    runtime::{DockerComposeClient, HfCliDownloader},
    service::{AppService, InstanceCreateInput, TemplateCreateInput},
    tui,
    types::{InstanceConfig, ModelDownloadRequest},
};

#[derive(Debug, Parser)]
#[command(name = "manager-neo", version, about = "Local LLM manager (Rust)")]
pub struct Cli {
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Serve(ServeArgs),
    Tui,
    Debug {
        #[command(subcommand)]
        command: DebugCommands,
    },
    Instance {
        #[command(subcommand)]
        command: InstanceCommands,
    },
    Model {
        #[command(subcommand)]
        command: ModelCommands,
    },
    Template {
        #[command(subcommand)]
        command: TemplateCommands,
    },
    Ports,
    System {
        #[command(subcommand)]
        command: SystemCommands,
    },
    Layout {
        #[command(subcommand)]
        command: LayoutCommands,
    },
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    #[arg(long, default_value = "0.0.0.0")]
    pub host: String,
    #[arg(long, default_value_t = 9999)]
    pub port: u16,
}

#[derive(Debug, Subcommand)]
pub enum InstanceCommands {
    List,
    Show(InstanceNameArg),
    Create(CreateInstanceArgs),
    Delete(InstanceNameArg),
    Edit(EditInstanceArgs),
    Start(InstanceNameArg),
    Stop(InstanceNameArg),
    Restart(InstanceNameArg),
    Status { name: String },
    Health(InstanceNameArg),
    Logs(InstanceLogsArgs),
}

#[derive(Debug, Args)]
pub struct InstanceNameArg {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct InstanceLogsArgs {
    pub name: String,
    #[arg(long, default_value_t = 100)]
    pub tail: usize,
}

#[derive(Debug, Args)]
pub struct CreateInstanceArgs {
    pub name: String,
    #[arg(long)]
    pub model: String,
    #[arg(long)]
    pub mmproj: Option<String>,
    #[arg(long)]
    pub port: Option<u16>,
    #[arg(long, default_value_t = 262_144)]
    pub ctx_size: u32,
    #[arg(long, default_value_t = 8)]
    pub threads: u32,
    #[arg(long, default_value_t = 999)]
    pub gpu_layers: u32,
    #[arg(long, default_value_t = true)]
    pub thinking: bool,
    #[arg(long)]
    pub parallel: Option<u32>,
}

#[derive(Debug, Args)]
pub struct EditInstanceArgs {
    pub name: String,
    #[arg(long)]
    pub key: String,
    #[arg(long)]
    pub value: String,
}

#[derive(Debug, Subcommand)]
pub enum ModelCommands {
    List,
    Download(DownloadModelArgs),
    Delete(ModelNameArg),
    Rename(ModelRenameArgs),
}

#[derive(Debug, Args)]
pub struct DownloadModelArgs {
    pub repo_id: String,
    #[arg(long = "pattern")]
    pub patterns: Vec<String>,
    #[arg(long)]
    pub local_dir: Option<String>,
}

#[derive(Debug, Args)]
pub struct ModelNameArg {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct ModelRenameArgs {
    pub name: String,
    pub new_name: String,
}

#[derive(Debug, Subcommand)]
pub enum TemplateCommands {
    List,
    Create(TemplateCreateArgs),
    Delete(TemplateNameArg),
    Instantiate(TemplateInstantiateArgs),
    BatchApply(TemplateBatchApplyArgs),
    SetOverride(TemplateOverrideArgs),
    SetBase(TemplateBaseArgs),
    Scan,
}

#[derive(Debug, Subcommand)]
pub enum SystemCommands {
    Metrics,
}

#[derive(Debug, Subcommand)]
pub enum DebugCommands {
    Memory(DebugMemoryArgs),
    Architectures,
}

#[derive(Debug, Subcommand)]
pub enum LayoutCommands {
    Migrate {
        #[arg(long)]
        dry_run: bool,
    },
    SyncComposeNames,
}

#[derive(Debug, Args)]
pub struct DebugMemoryArgs {
    #[arg(long)]
    pub instance: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long, default_value = "debug-target")]
    pub name: String,
    #[arg(long)]
    pub ctx_size: Option<u32>,
    #[arg(long)]
    pub parallel: Option<u32>,
    #[arg(long)]
    pub cache_type_k: Option<String>,
    #[arg(long)]
    pub cache_type_v: Option<String>,
}

#[derive(Debug, Args)]
pub struct TemplateCreateArgs {
    pub name: String,
    #[arg(long)]
    pub family: Option<String>,
    #[arg(long, default_value = "")]
    pub description: String,
    #[arg(long)]
    pub from_instance: Option<String>,
}

#[derive(Debug, Args)]
pub struct TemplateNameArg {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct TemplateInstantiateArgs {
    pub template_name: String,
    pub instance_name: String,
    #[arg(long)]
    pub overrides: Option<String>,
}

#[derive(Debug, Args)]
pub struct TemplateBatchApplyArgs {
    pub template_name: String,
    #[arg(long)]
    pub key: String,
    #[arg(long)]
    pub value: String,
}

#[derive(Debug, Args)]
pub struct TemplateOverrideArgs {
    pub template_name: String,
    pub variant_name: String,
    #[arg(long)]
    pub key: String,
    #[arg(long)]
    pub value: String,
}

#[derive(Debug, Args)]
pub struct TemplateBaseArgs {
    pub template_name: String,
    #[arg(long)]
    pub key: String,
    #[arg(long)]
    pub value: String,
}

pub async fn run() -> AppResult<()> {
    let cli = Cli::parse();
    let workspace = resolve_workspace_paths(cli.root)?;

    // Detect legacy reasoning usage: specifically the old --chat-template-kwargs
    // form that only sets `enable_thinking`, and the old repetition flag name.
    // We allow other chat-template-kwargs usages to remain (they may contain other
    // keys); only the legacy reasoning pattern will be rejected at startup.
    fn detect_legacy_flags(paths: &WorkspacePaths) -> AppResult<Vec<std::path::PathBuf>> {
        use std::fs;
        use serde_json::Value;
        let mut found = vec![];

        for entry in walkdir::WalkDir::new(&paths.instances_dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file() && e.file_name() == "compose.yml")
        {
            if let Ok(txt) = fs::read_to_string(entry.path()) {
                // old repetition-penalty flag (legacy)
                if txt.contains("--repetition-penalty") {
                    found.push(entry.path().to_path_buf());
                    continue;
                }

                if let Some(pos) = txt.find("--chat-template-kwargs") {
                    // attempt to find an inline JSON object following the flag
                    let after = &txt[pos..];
                    if let Some(beg) = after.find('{') {
                        let sub = &after[beg..];
                        // find matching closing brace
                        let mut depth = 0usize;
                        let mut end_idx: Option<usize> = None;
                        for (i, ch) in sub.char_indices() {
                            if ch == '{' {
                                depth += 1;
                            } else if ch == '}' {
                                depth = depth.saturating_sub(1);
                                if depth == 0 {
                                    end_idx = Some(i);
                                    break;
                                }
                            }
                        }
                        if let Some(ei) = end_idx {
                            let json_str = &sub[..=ei];
                            if let Ok(val) = serde_json::from_str::<Value>(json_str) {
                                if let Some(map) = val.as_object() {
                                    if map.len() == 1
                                        && map.contains_key("enable_thinking")
                                        && map.get("enable_thinking").unwrap().is_boolean()
                                    {
                                        found.push(entry.path().to_path_buf());
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    // fallback: YAML list style where next line is the JSON literal (possibly quoted)
                    let lines: Vec<&str> = txt.lines().collect();
                    for (i, line) in lines.iter().enumerate() {
                        if line.contains("--chat-template-kwargs") {
                            for j in (i + 1)..std::cmp::min(lines.len(), i + 6) {
                                let l = lines[j].trim();
                                if l.starts_with('-') {
                                    let mut val = l.trim_start_matches('-').trim();
                                    if (val.starts_with('"') && val.ends_with('"'))
                                        || (val.starts_with('\'') && val.ends_with('\''))
                                    {
                                        val = &val[1..val.len() - 1];
                                    }
                                    if let Ok(v) = serde_json::from_str::<Value>(val) {
                                        if let Some(map) = v.as_object() {
                                            if map.len() == 1
                                                && map.contains_key("enable_thinking")
                                                && map.get("enable_thinking").unwrap().is_boolean()
                                            {
                                                found.push(entry.path().to_path_buf());
                                            }
                                        }
                                    }
                                    break;
                                } else if !l.is_empty() {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(found)
    }

    let legacy = detect_legacy_flags(&workspace)?;
    if !legacy.is_empty() {
        eprintln!("manager-neo detected legacy llama flags in compose files. These are no longer supported.");
        eprintln!("Files:");
        for p in &legacy {
            eprintln!("  - {}", p.display());
        }
        eprintln!("Please run: 'manager-neo layout migrate' to convert compose files, then restart manager-neo.");
        // Exit by returning an error so caller exits with code 1
        return Err(crate::error::AppError::InvalidInput(
            "legacy flags detected; migration required".to_string(),
        ));
    }

    let enforce_canonical_layout = !matches!(cli.command, Commands::Layout { .. });
    let service = Arc::new(AppService::new_with_layout_enforcement(
        workspace,
        Arc::new(DockerComposeClient),
        Arc::new(HfCliDownloader),
        enforce_canonical_layout,
    )?);

    match cli.command {
        Commands::Serve(args) => {
            serve(ApiState { service }, args.host, args.port).await?;
        }
        Commands::Tui => tui::run(service).await?,
        Commands::Debug { command } => {
            handle_debug_commands(service, command).await?;
        }
        Commands::Ports => {
            println!("{}", serde_json::to_string_pretty(&service.port_map()?)?);
        }
        Commands::System { command } => {
            handle_system_commands(service, command).await?;
        }
        Commands::Layout { command } => {
            handle_layout_commands(service, command).await?;
        }
        Commands::Instance { command } => {
            handle_instance_commands(service, command).await?;
        }
        Commands::Model { command } => {
            handle_model_commands(service, command).await?;
        }
        Commands::Template { command } => {
            handle_template_commands(service, command).await?;
        }
    }
    Ok(())
}

fn resolve_workspace_paths(root_arg: Option<PathBuf>) -> AppResult<WorkspacePaths> {
    if let Some(root) = root_arg {
        return Ok(WorkspacePaths::new(root));
    }
    let exe_dir = std::env::current_exe()?
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| AppError::InvalidInput("cannot resolve executable directory".to_string()))?;
    Ok(WorkspacePaths::new(exe_dir))
}

async fn handle_debug_commands(service: Arc<AppService>, command: DebugCommands) -> AppResult<()> {
    match command {
        DebugCommands::Architectures => {
            println!(
                "{}",
                serde_json::to_string_pretty(&service.memory_estimator_architectures())?
            );
        }
        DebugCommands::Memory(args) => {
            if args.instance.is_some() == args.model.is_some() {
                return Err(AppError::InvalidInput(
                    "provide exactly one of --instance or --model".to_string(),
                ));
            }

            let report = if let Some(instance_name) = args.instance {
                service.instance_memory_debug(&instance_name)?
            } else {
                let mut config = InstanceConfig::default();
                let raw_model = args.model.unwrap_or_default();
                config.model = normalize_model_ref(&raw_model);
                if let Some(ctx_size) = args.ctx_size {
                    config.ctx_size = ctx_size.max(1);
                }
                if let Some(parallel) = args.parallel {
                    config.parallel = Some(parallel.max(1));
                }
                if let Some(cache_type_k) = args.cache_type_k {
                    config.cache_type_k = cache_type_k;
                }
                if let Some(cache_type_v) = args.cache_type_v {
                    config.cache_type_v = cache_type_v;
                }
                service.memory_debug_for_config(&args.name, config)?
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}

async fn handle_system_commands(
    service: Arc<AppService>,
    command: SystemCommands,
) -> AppResult<()> {
    match command {
        SystemCommands::Metrics => {
            println!(
                "{}",
                serde_json::to_string_pretty(&service.system_metrics().await?)?
            );
        }
    }
    Ok(())
}

async fn handle_layout_commands(
    service: Arc<AppService>,
    command: LayoutCommands,
) -> AppResult<()> {
    match command {
        LayoutCommands::Migrate { dry_run } => {
            if dry_run {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&service.migrate_workspace_layout_dry_run().await?)?
                );
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&service.migrate_workspace_layout().await?)?
                );
            }
        }
        LayoutCommands::SyncComposeNames => {
            let updated = service.backfill_compose_project_names()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "updated": updated.len(),
                    "compose_files": updated
                }))?
            );
        }
    }
    Ok(())
}

async fn handle_instance_commands(
    service: Arc<AppService>,
    command: InstanceCommands,
) -> AppResult<()> {
    match command {
        InstanceCommands::List => println!(
            "{}",
            serde_json::to_string_pretty(&service.list_instances()?)?
        ),
        InstanceCommands::Show(arg) => println!(
            "{}",
            serde_json::to_string_pretty(&service.get_instance(&arg.name)?)?
        ),
        InstanceCommands::Create(args) => {
            let created = service.create_instance(InstanceCreateInput {
                name: args.name,
                model: args.model,
                mmproj: args.mmproj,
                port: args.port,
                ctx_size: args.ctx_size,
                threads: args.threads,
                gpu_layers: args.gpu_layers,
                thinking: args.thinking,
                parallel: args.parallel,
            })?;
            println!("{}", serde_json::to_string_pretty(&created)?);
        }
        InstanceCommands::Delete(arg) => {
            println!(
                "{}",
                serde_json::json!({ "deleted": service.delete_instance(&arg.name)? })
            );
        }
        InstanceCommands::Edit(args) => {
            let value = parse_json_or_string(&args.value);
            let updated = service.edit_instance(&args.name, &args.key, value)?;
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        InstanceCommands::Start(arg) => {
            service.start_instance(&arg.name).await?;
            println!(
                "{}",
                serde_json::json!({ "started": true, "name": arg.name })
            );
        }
        InstanceCommands::Stop(arg) => {
            service.stop_instance(&arg.name).await?;
            println!(
                "{}",
                serde_json::json!({ "stopped": true, "name": arg.name })
            );
        }
        InstanceCommands::Restart(arg) => {
            service.restart_instance(&arg.name).await?;
            println!(
                "{}",
                serde_json::json!({ "restarted": true, "name": arg.name })
            );
        }
        InstanceCommands::Status { name } => {
            if name == "all" {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&service.all_instances_status().await?)?
                );
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&service.instance_status(&name).await?)?
                );
            }
        }
        InstanceCommands::Health(arg) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&service.health_check(&arg.name).await?)?
            );
        }
        InstanceCommands::Logs(args) => {
            println!("{}", service.instance_logs(&args.name, args.tail).await?);
        }
    }
    Ok(())
}

async fn handle_model_commands(service: Arc<AppService>, command: ModelCommands) -> AppResult<()> {
    match command {
        ModelCommands::List => {
            println!("{}", serde_json::to_string_pretty(&service.list_models_hierarchy()?)?)
        }
        ModelCommands::Download(args) => {
            let path = service
                .download_model(ModelDownloadRequest {
                    repo_id: args.repo_id,
                    patterns: if args.patterns.is_empty() {
                        None
                    } else {
                        Some(args.patterns)
                    },
                    local_dir: args.local_dir,
                })
                .await?;
            println!(
                "{}",
                serde_json::json!({ "downloaded": true, "path": path })
            );
        }
        ModelCommands::Delete(arg) => {
            println!(
                "{}",
                serde_json::json!({ "deleted": service.delete_model(&arg.name)? })
            );
        }
        ModelCommands::Rename(args) => {
            println!(
                "{}",
                serde_json::json!({ "renamed": service.rename_model(&args.name, &args.new_name)? })
            );
        }
    }
    Ok(())
}

async fn handle_template_commands(
    service: Arc<AppService>,
    command: TemplateCommands,
) -> AppResult<()> {
    match command {
        TemplateCommands::List => println!(
            "{}",
            serde_json::to_string_pretty(&service.list_templates()?)?
        ),
        TemplateCommands::Create(args) => {
            let created = service.create_template(TemplateCreateInput {
                name: args.name,
                family: args.family,
                description: args.description,
                from_instance: args.from_instance,
            })?;
            println!("{}", serde_json::to_string_pretty(&created)?);
        }
        TemplateCommands::Delete(arg) => {
            println!(
                "{}",
                serde_json::json!({ "deleted": service.delete_template(&arg.name)? })
            );
        }
        TemplateCommands::Instantiate(args) => {
            let overrides = match args.overrides {
                Some(text) => Some(parse_hashmap_json(&text)?),
                None => None,
            };
            let created = service.instantiate_template(
                &args.template_name,
                &args.instance_name,
                overrides,
            )?;
            println!("{}", serde_json::to_string_pretty(&created)?);
        }
        TemplateCommands::BatchApply(args) => {
            let value = parse_json_or_string(&args.value);
            let updated = service.batch_apply(&args.template_name, &args.key, value)?;
            println!("{}", serde_json::to_string_pretty(&updated)?);
        }
        TemplateCommands::SetOverride(args) => {
            let value = parse_json_or_string(&args.value);
            let template = service.set_template_override(
                &args.template_name,
                &args.variant_name,
                &args.key,
                value,
            )?;
            println!("{}", serde_json::to_string_pretty(&template)?);
        }
        TemplateCommands::SetBase(args) => {
            let value = parse_json_or_string(&args.value);
            let template =
                service.set_template_base_value(&args.template_name, &args.key, value)?;
            println!("{}", serde_json::to_string_pretty(&template)?);
        }
        TemplateCommands::Scan => {
            println!(
                "{}",
                serde_json::to_string_pretty(&service.scan_templates()?)?
            );
        }
    }
    Ok(())
}

fn parse_json_or_string(input: &str) -> Value {
    serde_json::from_str::<Value>(input).unwrap_or_else(|_| Value::String(input.to_string()))
}

fn parse_hashmap_json(input: &str) -> AppResult<HashMap<String, Value>> {
    let parsed = serde_json::from_str::<Value>(input)?;
    parsed
        .as_object()
        .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .ok_or_else(|| AppError::InvalidInput("overrides must be a JSON object".to_string()))
}

fn normalize_model_ref(value: &str) -> String {
    if value.starts_with("/models/")
        || value.starts_with("models/")
        || value.starts_with('/')
        || value.starts_with("./")
        || value.starts_with("../")
    {
        value.to_string()
    } else {
        format!("/models/{}", value.trim_start_matches('/'))
    }
}
