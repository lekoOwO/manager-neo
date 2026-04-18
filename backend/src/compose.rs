use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    config::WorkspacePaths,
    error::{AppError, AppResult},
    types::{
        BatchConfig, DEFAULT_SERVICE_NAME, EnvironmentConfig, HealthCheckConfig, InstanceConfig,
        LoggingConfig, RoPEConfig, SamplingConfig,
    },
};

const DEFAULT_SECURITY_OPT: [&str; 1] = ["seccomp=unconfined"];
const DEFAULT_GROUP_ADD: [&str; 3] = ["27", "video", "render"];
const DEFAULT_DEVICE_PATHS: [&str; 2] = ["/dev/kfd", "/dev/dri"];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ComposeFile {
    #[serde(default)]
    name: Option<String>,
    services: BTreeMap<String, ComposeService>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ComposeService {
    image: String,
    #[serde(default)]
    security_opt: Vec<String>,
    #[serde(default)]
    ports: Vec<String>,
    #[serde(default)]
    volumes: Vec<String>,
    #[serde(default)]
    devices: Vec<String>,
    #[serde(default)]
    group_add: Vec<String>,
    #[serde(default)]
    environment: Vec<String>,
    #[serde(default)]
    command: Vec<String>,
    #[serde(default)]
    restart: Option<String>,
    #[serde(default)]
    ipc: Option<String>,
    #[serde(default)]
    deploy: Option<Value>,
    #[serde(default)]
    healthcheck: Option<HealthCheckConfig>,
    #[serde(default)]
    logging: Option<ComposeLogging>,
    #[serde(default)]
    container_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ComposeLogging {
    driver: String,
    options: BTreeMap<String, String>,
}

pub fn parse_command_to_map(command: &[String]) -> HashMap<String, Value> {
    let mut result = HashMap::new();
    let mut idx = 0usize;
    while idx < command.len() {
        let arg = &command[idx];
        if let Some(flag) = arg.strip_prefix("--") {
            // normalize key to underscores by default
            let mut key = flag.replace('-', "_");
            // normalize legacy repetition-penalty to repeat_penalty
            if key == "repetition_penalty" {
                key = "repeat_penalty".to_string();
            }
            if let Some(next) = command.get(idx + 1) {
                if !next.starts_with("--") {
                    // special-case --reasoning on|off -> chat_template_kwargs.enable_thinking
                    if flag == "reasoning" {
                        let val = match next.to_ascii_lowercase().as_str() {
                            "on" => Value::Bool(true),
                            "off" => Value::Bool(false),
                            other => serde_json::from_str::<Value>(other)
                                .unwrap_or_else(|_| Value::String(other.to_string())),
                        };
                        // insert as chat_template_kwargs object
                        result.insert(
                            "chat_template_kwargs".to_string(),
                            serde_json::json!({"enable_thinking": val.as_bool().unwrap_or(false)}),
                        );
                        idx += 2;
                        continue;
                    }
                    let parsed = serde_json::from_str::<Value>(next)
                        .unwrap_or_else(|_| Value::String(next.to_string()));
                    result.insert(key, parsed);
                    idx += 2;
                    continue;
                }
            }
            // handle flags without explicit values
            if flag == "reasoning" {
                result.insert(
                    "chat_template_kwargs".to_string(),
                    serde_json::json!({"enable_thinking": true}),
                );
            } else {
                result.insert(key, Value::Bool(true));
            }
        }
        idx += 1;
    }
    result
}

pub fn compose_to_instance_config(path: &Path) -> AppResult<(String, InstanceConfig)> {
    let raw = fs::read_to_string(path)?;
    let compose: ComposeFile = serde_yaml::from_str(&raw)?;
    let (service_name, svc) = compose.services.into_iter().next().ok_or_else(|| {
        AppError::InvalidInput(format!("compose has no services: {}", path.display()))
    })?;

    let parsed = parse_command_to_map(&svc.command);
    let (host_port, container_port) = parse_ports(&svc.ports);
    let (volumes_ro, extra_volumes) = parse_volumes(&svc.volumes);
    let memory_limit = parse_memory_limit(&svc.deploy);

    let config = InstanceConfig {
        name: String::new(),
        model: json_str(&parsed, "model").unwrap_or_default(),
        mmproj: json_opt_str(&parsed, "mmproj"),
        draft_model: json_opt_str(&parsed, "draft_model"),
        draft_max: json_opt_u32(&parsed, "draft_max"),
        draft_min: json_opt_u32(&parsed, "draft_min"),
        chat_template_file: json_opt_str(&parsed, "chat_template_file"),
        host: json_str(&parsed, "host").unwrap_or_else(|| "0.0.0.0".to_string()),
        port: json_opt_u16(&parsed, "port").unwrap_or(container_port),
        host_port,
        ctx_size: json_opt_u32(&parsed, "ctx_size").unwrap_or(262_144),
        parallel: json_opt_u32(&parsed, "parallel"),
        cont_batching: json_opt_bool(&parsed, "cont_batching").unwrap_or(false),
        cache_type_k: json_str(&parsed, "cache_type_k").unwrap_or_else(|| "q8_0".to_string()),
        cache_type_v: json_str(&parsed, "cache_type_v").unwrap_or_else(|| "q8_0".to_string()),
        n_gpu_layers: json_opt_u32(&parsed, "n_gpu_layers").unwrap_or(999),
        threads: json_opt_u32(&parsed, "threads").unwrap_or(8),
        threads_batch: json_opt_u32(&parsed, "threads_batch"),
        flash_attn: json_str(&parsed, "flash_attn").unwrap_or_else(|| "on".to_string()),
        no_mmap: json_opt_bool(&parsed, "no_mmap").unwrap_or(false),
        embedding: json_opt_bool(&parsed, "embedding").unwrap_or(false),
        reranking: json_opt_bool(&parsed, "reranking").unwrap_or(false),
        pooling: json_opt_str(&parsed, "pooling"),
        sampling: SamplingConfig {
            temp: json_opt_f64(&parsed, "temp").unwrap_or(1.0),
            top_p: json_opt_f64(&parsed, "top_p").unwrap_or(0.95),
            top_k: json_opt_u32(&parsed, "top_k").unwrap_or(20),
            min_p: json_opt_f64(&parsed, "min_p").unwrap_or(0.0),
        },
        batch: BatchConfig {
            batch_size: json_opt_u32(&parsed, "batch_size"),
            ubatch_size: json_opt_u32(&parsed, "ubatch_size"),
        },
        rope: RoPEConfig {
            scaling: json_opt_str(&parsed, "rope_scaling"),
            scale: json_opt_f64(&parsed, "rope_scale"),
            orig_ctx: json_opt_u32(&parsed, "yarn_orig_ctx"),
        },
        chat_template_kwargs: parsed
            .get("chat_template_kwargs")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"enable_thinking": true})),
        docker_image: svc.image,
        volumes_ro,
        ipc_host: svc.ipc.as_deref() == Some("host"),
        memory_limit,
        environment: parse_environment(&svc.environment),
        healthcheck: svc.healthcheck,
        logging: svc.logging.map(|l| LoggingConfig {
            driver: if l.driver.is_empty() {
                "json-file".to_string()
            } else {
                l.driver
            },
            max_size: l
                .options
                .get("max-size")
                .cloned()
                .unwrap_or_else(|| "50m".to_string()),
            max_file: l
                .options
                .get("max-file")
                .cloned()
                .unwrap_or_else(|| "3".to_string()),
        }),
        restart: svc.restart.unwrap_or_else(|| "unless-stopped".to_string()),
        service_name,
        extra_volumes,
        extra_args: svc.command.iter().skip(1).cloned().collect(),
        container_name: svc.container_name,
    };
    Ok((config.service_name.clone(), config))
}

pub fn instance_config_to_compose(
    config: &InstanceConfig,
    paths: &WorkspacePaths,
    compose_name: &str,
) -> AppResult<Value> {
    let mut command = vec!["llama-server".to_string()];
    add_arg(
        &mut command,
        "model",
        Some(Value::String(config.model.clone())),
    );
    add_arg(
        &mut command,
        "mmproj",
        config.mmproj.as_ref().map(|v| Value::String(v.clone())),
    );
    add_arg(
        &mut command,
        "draft-model",
        config
            .draft_model
            .as_ref()
            .map(|v| Value::String(v.clone())),
    );
    add_arg(&mut command, "draft-max", config.draft_max.map(Value::from));
    add_arg(&mut command, "draft-min", config.draft_min.map(Value::from));
    add_arg(
        &mut command,
        "chat-template-file",
        config
            .chat_template_file
            .as_ref()
            .map(|v| Value::String(v.clone())),
    );
    add_arg(
        &mut command,
        "host",
        Some(Value::String(config.host.clone())),
    );
    add_arg(&mut command, "port", Some(Value::from(config.port)));
    add_arg(&mut command, "ctx-size", Some(Value::from(config.ctx_size)));
    add_arg(&mut command, "parallel", config.parallel.map(Value::from));
    add_arg(
        &mut command,
        "cont-batching",
        config.cont_batching.then_some(Value::Bool(true)),
    );
    add_arg(
        &mut command,
        "cache-type-k",
        Some(Value::String(config.cache_type_k.clone())),
    );
    add_arg(
        &mut command,
        "cache-type-v",
        Some(Value::String(config.cache_type_v.clone())),
    );
    add_arg(
        &mut command,
        "n-gpu-layers",
        Some(Value::from(config.n_gpu_layers)),
    );
    add_arg(&mut command, "threads", Some(Value::from(config.threads)));
    add_arg(
        &mut command,
        "threads-batch",
        config.threads_batch.map(Value::from),
    );
    add_arg(
        &mut command,
        "flash-attn",
        Some(Value::String(config.flash_attn.clone())),
    );
    add_arg(
        &mut command,
        "no-mmap",
        config.no_mmap.then_some(Value::Bool(true)),
    );
    add_arg(
        &mut command,
        "embedding",
        config.embedding.then_some(Value::Bool(true)),
    );
    add_arg(
        &mut command,
        "reranking",
        config.reranking.then_some(Value::Bool(true)),
    );
    add_arg(
        &mut command,
        "pooling",
        config.pooling.as_ref().map(|v| Value::String(v.clone())),
    );
    add_arg(
        &mut command,
        "batch-size",
        config.batch.batch_size.map(Value::from),
    );
    add_arg(
        &mut command,
        "ubatch-size",
        config.batch.ubatch_size.map(Value::from),
    );
    add_arg(
        &mut command,
        "rope-scaling",
        config
            .rope
            .scaling
            .as_ref()
            .map(|v| Value::String(v.clone())),
    );
    add_arg(
        &mut command,
        "rope-scale",
        config.rope.scale.map(Value::from),
    );
    add_arg(
        &mut command,
        "yarn-orig-ctx",
        config.rope.orig_ctx.map(Value::from),
    );
    add_arg(
        &mut command,
        "temp",
        Some(Value::from(config.sampling.temp)),
    );
    add_arg(
        &mut command,
        "top-p",
        Some(Value::from(config.sampling.top_p)),
    );
    add_arg(
        &mut command,
        "top-k",
        Some(Value::from(config.sampling.top_k)),
    );
    add_arg(
        &mut command,
        "min-p",
        Some(Value::from(config.sampling.min_p)),
    );
    if config.chat_template_kwargs != Value::Null {
        match &config.chat_template_kwargs {
            Value::Object(map) => {
                // if only enable_thinking is present, map to --reasoning on|off
                if map.len() == 1 && map.get("enable_thinking").is_some() {
                    if let Some(Value::Bool(enabled)) = map.get("enable_thinking") {
                        command.push("--reasoning".to_string());
                        command.push(if *enabled { "on".to_string() } else { "off".to_string() });
                    } else {
                        // non-boolean value, fallback to JSON arg
                        command.push("--chat-template-kwargs".to_string());
                        command.push(serde_json::to_string(&config.chat_template_kwargs)?);
                    }
                } else {
                    // other keys present, keep JSON for backward compatibility
                    command.push("--chat-template-kwargs".to_string());
                    command.push(serde_json::to_string(&config.chat_template_kwargs)?);
                }
            }
            _ => {
                command.push("--chat-template-kwargs".to_string());
                command.push(serde_json::to_string(&config.chat_template_kwargs)?);
            }
        }
    }

    let mut volumes = vec![format!(
        "{}:/models{}",
        paths.models_dir.display(),
        if config.volumes_ro { ":ro" } else { "" }
    )];
    if config.chat_template_file.is_some() {
        volumes.push(format!("{}:/templates", paths.templates_dir.display()));
    }
    volumes.extend(config.extra_volumes.clone());

    let devices = DEFAULT_DEVICE_PATHS
        .iter()
        .map(|d| format!("{d}:{d}"))
        .collect::<Vec<_>>();
    let mut environment = vec![];
    if let Some(v) = &config.environment.hsa_override_gfx_version {
        environment.push(format!("HSA_OVERRIDE_GFX_VERSION={v}"));
    }
    if config.environment.rocblas_use_hipblaslt == Some(true) {
        environment.push("ROCBLAS_USE_HIPBLASLT=1".to_string());
    }
    if config.environment.rocm_allow_unsafe_asic_permit_default == Some(true) {
        environment.push("ROCM_ALLOW_UNSAFE_ASIC_PERMIT_DEFAULT=1".to_string());
    }
    if config.environment.ggml_hip_rocwmma_fattn == Some(true) {
        environment.push("GGML_HIP_ROCWMMA_FATTN=1".to_string());
    }
    for (k, v) in &config.environment.extra {
        environment.push(format!("{k}={v}"));
    }

    let mut service = Map::new();
    service.insert(
        "image".to_string(),
        Value::String(config.docker_image.clone()),
    );
    service.insert(
        "security_opt".to_string(),
        Value::Array(
            DEFAULT_SECURITY_OPT
                .iter()
                .map(|v| Value::String((*v).to_string()))
                .collect(),
        ),
    );
    service.insert(
        "ports".to_string(),
        Value::Array(vec![Value::String(format!(
            "{}:{}",
            config.host_port, config.port
        ))]),
    );
    service.insert(
        "volumes".to_string(),
        Value::Array(volumes.into_iter().map(Value::String).collect()),
    );
    service.insert(
        "devices".to_string(),
        Value::Array(devices.into_iter().map(Value::String).collect()),
    );
    service.insert(
        "group_add".to_string(),
        Value::Array(
            DEFAULT_GROUP_ADD
                .iter()
                .map(|v| Value::String((*v).to_string()))
                .collect(),
        ),
    );
    if !environment.is_empty() {
        service.insert(
            "environment".to_string(),
            Value::Array(environment.into_iter().map(Value::String).collect()),
        );
    }
    // Append any extra args preserved from the original compose command, avoiding duplicates
    let mut final_command = command;
    let mut existing = std::collections::HashSet::new();
    for t in &final_command {
        existing.insert(t.clone());
    }
    for token in &config.extra_args {
        let token_to_push = if token == "--repetition-penalty" {
            "--repeat-penalty".to_string()
        } else {
            token.clone()
        };
        if !existing.contains(&token_to_push) {
            final_command.push(token_to_push.clone());
            existing.insert(token_to_push);
        }
    }
    service.insert(
        "command".to_string(),
        Value::Array(final_command.into_iter().map(Value::String).collect()),
    );
    service.insert("restart".to_string(), Value::String(config.restart.clone()));
    if config.ipc_host {
        service.insert("ipc".to_string(), Value::String("host".to_string()));
    }
    if let Some(limit) = &config.memory_limit {
        service.insert(
            "deploy".to_string(),
            serde_json::json!({
                "resources": {
                    "limits": {
                        "memory": limit
                    }
                }
            }),
        );
    }
    if let Some(hc) = &config.healthcheck {
        service.insert("healthcheck".to_string(), serde_json::to_value(hc)?);
    }
    if let Some(log) = &config.logging {
        service.insert(
            "logging".to_string(),
            serde_json::json!({
                "driver": log.driver,
                "options": {
                    "max-size": log.max_size,
                    "max-file": log.max_file
                }
            }),
        );
    }
    if let Some(name) = &config.container_name {
        service.insert("container_name".to_string(), Value::String(name.clone()));
    }

    let compose = serde_json::json!({
        "name": compose_name,
        "services": {
            if config.service_name.is_empty() { DEFAULT_SERVICE_NAME } else { &config.service_name }: service
        }
    });
    Ok(compose)
}

pub fn write_compose(
    config: &InstanceConfig,
    path: &Path,
    paths: &WorkspacePaths,
) -> AppResult<()> {
    let compose_name = compose_project_name_for_instance(path, paths, config);
    let compose_json = instance_config_to_compose(config, paths, &compose_name)?;
    let yaml = serde_yaml::to_string(&compose_json)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, yaml)?;
    Ok(())
}

pub fn compose_project_name_for_instance(
    path: &Path,
    paths: &WorkspacePaths,
    config: &InstanceConfig,
) -> String {
    if let Some(parent) = path.parent() {
        if let Ok(rel) = parent.strip_prefix(&paths.instances_dir) {
            let parts = rel
                .iter()
                .map(|v| v.to_string_lossy().to_string())
                .collect::<Vec<_>>();
            if parts.len() >= 4 {
                return normalize_compose_project_name(&format!(
                    "{}-{}-{}",
                    parts[1], parts[2], parts[3]
                ));
            }
        }
    }
    let (model, quant) = model_quant_from_ref(&config.model)
        .unwrap_or_else(|| (sanitize_compose_segment(&config.name), "general".to_string()));
    normalize_compose_project_name(&format!("{model}-{quant}-general"))
}

fn model_quant_from_ref(model_ref: &str) -> Option<(String, String)> {
    let tail = model_ref
        .trim_start_matches("/models/")
        .trim_start_matches("models/")
        .trim_start_matches('/');
    let segments = tail
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>();
    if segments.len() < 3 {
        return None;
    }
    Some((
        sanitize_compose_segment(segments[1]),
        sanitize_compose_segment(segments[2]),
    ))
}

fn sanitize_compose_segment(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() || ch == '_' {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if next == '-' {
            if last_dash {
                continue;
            }
            last_dash = true;
        } else {
            last_dash = false;
        }
        out.push(next);
    }
    out.trim_matches('-').to_string()
}

fn normalize_compose_project_name(value: &str) -> String {
    let normalized = sanitize_compose_segment(value);
    if normalized.is_empty() {
        "manager-neo".to_string()
    } else {
        normalized
    }
}

fn add_arg(command: &mut Vec<String>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        match value {
            Value::Bool(true) => command.push(format!("--{key}")),
            Value::Bool(false) | Value::Null => {}
            Value::Number(_) | Value::String(_) | Value::Object(_) | Value::Array(_) => {
                command.push(format!("--{key}"));
                command.push(match value {
                    Value::String(v) => v,
                    other => other.to_string(),
                });
            }
        }
    }
}

fn parse_ports(ports: &[String]) -> (u16, u16) {
    if let Some(port) = ports.first() {
        let mut split = port.split(':');
        if let (Some(host), Some(rest)) = (split.next(), split.next()) {
            let container = rest.split('/').next().unwrap_or(rest);
            if let (Ok(host_port), Ok(container_port)) =
                (host.parse::<u16>(), container.parse::<u16>())
            {
                return (host_port, container_port);
            }
        }
    }
    (8080, 8080)
}

fn parse_volumes(volumes: &[String]) -> (bool, Vec<String>) {
    let mut volumes_ro = false;
    let mut extras = vec![];
    for volume in volumes {
        let is_models = volume.contains(":/models");
        if is_models {
            if volume.ends_with(":/models:ro") || volume.ends_with(":/models:rw:ro") {
                volumes_ro = true;
            }
            continue;
        }
        if !volume.contains(":/templates") {
            extras.push(volume.clone());
        }
    }
    (volumes_ro, extras)
}

fn parse_environment(environment: &[String]) -> EnvironmentConfig {
    let mut cfg = EnvironmentConfig::default();
    for item in environment {
        let Some((key, value)) = item.split_once('=') else {
            continue;
        };
        let lower = key.to_lowercase();
        match lower.as_str() {
            "hsa_override_gfx_version" => cfg.hsa_override_gfx_version = Some(value.to_string()),
            "rocblas_use_hipblaslt" => cfg.rocblas_use_hipblaslt = Some(is_truthy(value)),
            "rocm_allow_unsafe_asic_permit_default" => {
                cfg.rocm_allow_unsafe_asic_permit_default = Some(is_truthy(value))
            }
            "ggml_hip_rocwmma_fattn" => cfg.ggml_hip_rocwmma_fattn = Some(is_truthy(value)),
            _ => {
                cfg.extra.insert(key.to_string(), value.to_string());
            }
        }
    }
    cfg
}

fn parse_memory_limit(deploy: &Option<Value>) -> Option<String> {
    deploy
        .as_ref()
        .and_then(|v| v.get("resources"))
        .and_then(|v| v.get("limits"))
        .and_then(|v| v.get("memory"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn json_opt_u32(map: &HashMap<String, Value>, key: &str) -> Option<u32> {
    map.get(key)
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
}

fn json_opt_u16(map: &HashMap<String, Value>, key: &str) -> Option<u16> {
    map.get(key)
        .and_then(|v| v.as_u64())
        .and_then(|v| u16::try_from(v).ok())
}

fn json_opt_f64(map: &HashMap<String, Value>, key: &str) -> Option<f64> {
    map.get(key).and_then(Value::as_f64)
}

fn json_opt_bool(map: &HashMap<String, Value>, key: &str) -> Option<bool> {
    map.get(key).and_then(Value::as_bool)
}

fn json_str(map: &HashMap<String, Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn json_opt_str(map: &HashMap<String, Value>, key: &str) -> Option<String> {
    json_str(map, key)
}

fn is_truthy(value: &str) -> bool {
    !matches!(value, "0" | "false" | "False" | "FALSE")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_command_to_map;

    #[test]
    fn parse_command_supports_flags_and_json() {
        let parsed = parse_command_to_map(&[
            "llama-server".to_string(),
            "--model".to_string(),
            "/models/qwen.gguf".to_string(),
            "--no-mmap".to_string(),
            "--chat-template-kwargs".to_string(),
            r#"{"enable_thinking":true}"#.to_string(),
        ]);
        assert_eq!(parsed.get("model"), Some(&json!("/models/qwen.gguf")));
        assert_eq!(parsed.get("no_mmap"), Some(&json!(true)));
        assert_eq!(
            parsed.get("chat_template_kwargs"),
            Some(&json!({"enable_thinking": true}))
        );
    }
}
