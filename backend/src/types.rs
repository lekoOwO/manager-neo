use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{AppError, AppResult};

pub const DOCKER_IMAGE_DEFAULT: &str = "docker.io/kyuz0/amd-strix-halo-toolboxes:rocm7-nightlies";
pub const DEFAULT_SERVICE_NAME: &str = "llm";

const LLAMA_CONFIG_ROOTS: &[&str] = &[
    "model",
    "mmproj",
    "draft_model",
    "draft_max",
    "draft_min",
    "chat_template_file",
    "ctx_size",
    "parallel",
    "cont_batching",
    "cache_type_k",
    "cache_type_v",
    "n_gpu_layers",
    "gpu_layers",
    "threads",
    "threads_batch",
    "flash_attn",
    "no_mmap",
    "embedding",
    "reranking",
    "pooling",
    "sampling",
    "batch",
    "rope",
    "chat_template_kwargs",
];

const COMPOSE_CONFIG_ROOTS: &[&str] = &[
    "host",
    "port",
    "host_port",
    "docker_image",
    "volumes_ro",
    "ipc_host",
    "memory_limit",
    "environment",
    "healthcheck",
    "logging",
    "restart",
    "service_name",
    "extra_volumes",
    "container_name",
];

const META_CONFIG_ROOTS: &[&str] = &["name"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigKeySource {
    Llama,
    Compose,
    Meta,
    Other,
}

impl ConfigKeySource {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Llama => "llama",
            Self::Compose => "compose",
            Self::Meta => "meta",
            Self::Other => "other",
        }
    }
}

pub fn normalize_config_key_path(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    for prefix in ["llama.", "compose.", "meta.", "other."] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let rest = rest.trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    trimmed.to_string()
}

pub fn config_key_source(key: &str) -> ConfigKeySource {
    let normalized = normalize_config_key_path(key);
    let root = normalized
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if root.is_empty() {
        return ConfigKeySource::Other;
    }
    if LLAMA_CONFIG_ROOTS.iter().any(|candidate| *candidate == root) {
        ConfigKeySource::Llama
    } else if COMPOSE_CONFIG_ROOTS.iter().any(|candidate| *candidate == root) {
        ConfigKeySource::Compose
    } else if META_CONFIG_ROOTS.iter().any(|candidate| *candidate == root) {
        ConfigKeySource::Meta
    } else {
        ConfigKeySource::Other
    }
}

pub fn display_config_key(key: &str) -> String {
    let normalized = normalize_config_key_path(key);
    if normalized.is_empty() {
        return normalized;
    }
    format!("{}.{}", config_key_source(&normalized).prefix(), normalized)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RoPEConfig {
    pub scaling: Option<String>,
    pub scale: Option<f64>,
    pub orig_ctx: Option<u32>,
}

impl Default for RoPEConfig {
    fn default() -> Self {
        Self {
            scaling: None,
            scale: None,
            orig_ctx: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SamplingConfig {
    #[serde(default = "default_temp")]
    pub temp: f64,
    #[serde(default = "default_top_p")]
    pub top_p: f64,
    #[serde(default = "default_top_k")]
    pub top_k: u32,
    #[serde(default)]
    pub min_p: f64,
}

const fn default_temp() -> f64 {
    1.0
}

const fn default_top_p() -> f64 {
    0.95
}

const fn default_top_k() -> u32 {
    20
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            temp: default_temp(),
            top_p: default_top_p(),
            top_k: default_top_k(),
            min_p: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct BatchConfig {
    pub batch_size: Option<u32>,
    pub ubatch_size: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct HealthCheckConfig {
    #[serde(default)]
    pub test: Vec<String>,
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub retries: Option<u32>,
    #[serde(default)]
    pub start_period: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct LoggingConfig {
    #[serde(default = "default_log_driver")]
    pub driver: String,
    #[serde(default = "default_log_max_size")]
    pub max_size: String,
    #[serde(default = "default_log_max_file")]
    pub max_file: String,
}

fn default_log_driver() -> String {
    "json-file".to_string()
}

fn default_log_max_size() -> String {
    "50m".to_string()
}

fn default_log_max_file() -> String {
    "3".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct EnvironmentConfig {
    pub hsa_override_gfx_version: Option<String>,
    pub rocblas_use_hipblaslt: Option<bool>,
    pub rocm_allow_unsafe_asic_permit_default: Option<bool>,
    pub ggml_hip_rocwmma_fattn: Option<bool>,
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct InstanceConfig {
    #[serde(default)]
    pub name: String,
    pub model: String,
    #[serde(default)]
    pub mmproj: Option<String>,
    #[serde(default)]
    pub draft_model: Option<String>,
    #[serde(default)]
    pub draft_max: Option<u32>,
    #[serde(default)]
    pub draft_min: Option<u32>,
    #[serde(default)]
    pub chat_template_file: Option<String>,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_container_port")]
    pub port: u16,
    #[serde(default = "default_host_port")]
    pub host_port: u16,
    #[serde(default = "default_ctx_size")]
    pub ctx_size: u32,
    #[serde(default)]
    pub parallel: Option<u32>,
    #[serde(default)]
    pub cont_batching: bool,
    #[serde(default = "default_cache_type")]
    pub cache_type_k: String,
    #[serde(default = "default_cache_type")]
    pub cache_type_v: String,
    #[serde(default = "default_n_gpu_layers")]
    pub n_gpu_layers: u32,
    #[serde(default = "default_threads")]
    pub threads: u32,
    #[serde(default)]
    pub threads_batch: Option<u32>,
    #[serde(default = "default_flash_attn")]
    pub flash_attn: String,
    #[serde(default = "default_no_mmap")]
    pub no_mmap: bool,
    #[serde(default)]
    pub embedding: bool,
    #[serde(default)]
    pub reranking: bool,
    #[serde(default)]
    pub pooling: Option<String>,
    #[serde(default)]
    pub sampling: SamplingConfig,
    #[serde(default)]
    pub batch: BatchConfig,
    #[serde(default)]
    pub rope: RoPEConfig,
    #[serde(default = "default_chat_template_kwargs")]
    pub chat_template_kwargs: Value,
    #[serde(default = "default_docker_image")]
    pub docker_image: String,
    #[serde(default)]
    pub volumes_ro: bool,
    #[serde(default)]
    pub ipc_host: bool,
    #[serde(default)]
    pub memory_limit: Option<String>,
    #[serde(default)]
    pub environment: EnvironmentConfig,
    #[serde(default)]
    pub healthcheck: Option<HealthCheckConfig>,
    #[serde(default)]
    pub logging: Option<LoggingConfig>,
    #[serde(default = "default_restart")]
    pub restart: String,
    #[serde(default = "default_service_name")]
    pub service_name: String,
    #[serde(default)]
    pub extra_volumes: Vec<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub container_name: Option<String>,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

const fn default_container_port() -> u16 {
    8080
}

const fn default_host_port() -> u16 {
    8080
}

const fn default_ctx_size() -> u32 {
    262_144
}

fn default_cache_type() -> String {
    "q8_0".to_string()
}

const fn default_n_gpu_layers() -> u32 {
    999
}

const fn default_threads() -> u32 {
    8
}

fn default_flash_attn() -> String {
    "on".to_string()
}

const fn default_no_mmap() -> bool {
    true
}

fn default_chat_template_kwargs() -> Value {
    serde_json::json!({ "enable_thinking": true })
}

fn default_docker_image() -> String {
    DOCKER_IMAGE_DEFAULT.to_string()
}

fn default_restart() -> String {
    "unless-stopped".to_string()
}

fn default_service_name() -> String {
    DEFAULT_SERVICE_NAME.to_string()
}

impl Default for InstanceConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            model: String::new(),
            mmproj: None,
            draft_model: None,
            draft_max: None,
            draft_min: None,
            chat_template_file: None,
            host: default_host(),
            port: default_container_port(),
            host_port: default_host_port(),
            ctx_size: default_ctx_size(),
            parallel: None,
            cont_batching: false,
            cache_type_k: default_cache_type(),
            cache_type_v: default_cache_type(),
            n_gpu_layers: default_n_gpu_layers(),
            threads: default_threads(),
            threads_batch: None,
            flash_attn: default_flash_attn(),
            no_mmap: default_no_mmap(),
            embedding: false,
            reranking: false,
            pooling: None,
            sampling: SamplingConfig::default(),
            batch: BatchConfig::default(),
            rope: RoPEConfig::default(),
            chat_template_kwargs: default_chat_template_kwargs(),
            docker_image: default_docker_image(),
            volumes_ro: false,
            ipc_host: false,
            memory_limit: None,
            environment: EnvironmentConfig::default(),
            healthcheck: None,
            logging: None,
            restart: default_restart(),
            service_name: default_service_name(),
            extra_volumes: Vec::new(),
            extra_args: Vec::new(),
            container_name: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Instance {
    pub name: String,
    pub path: PathBuf,
    pub config: InstanceConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelFile {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

impl ModelFile {
    pub fn size_human(&self) -> String {
        human_size(self.size_bytes)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Model {
    pub name: String,
    pub path: PathBuf,
    pub files: Vec<ModelFile>,
}

impl Model {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size_bytes).sum()
    }

    pub fn total_size_human(&self) -> String {
        human_size(self.total_size())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Template {
    pub name: String,
    pub family: String,
    #[serde(default)]
    pub description: String,
    pub config: InstanceConfig,
    #[serde(default)]
    pub overrides: BTreeMap<String, HashMap<String, Value>>,
}

impl Template {
    pub fn resolve(
        &self,
        variant_overrides: Option<&HashMap<String, Value>>,
    ) -> AppResult<InstanceConfig> {
        let mut value = serde_json::to_value(&self.config)?;
        if let Some(overrides) = variant_overrides {
            for (key, patch) in overrides {
                apply_dot_notation(&mut value, key, patch.clone())?;
            }
        }
        serde_json::from_value(value).map_err(Into::into)
    }
}

fn apply_dot_notation(root: &mut Value, key: &str, value: Value) -> AppResult<()> {
    let normalized = normalize_config_key_path(key);
    let parts: Vec<&str> = normalized.split('.').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return Err(AppError::InvalidInput("empty override key".into()));
    }
    let mut current = root;
    for part in &parts[..parts.len() - 1] {
        let map = current.as_object_mut().ok_or_else(|| {
            AppError::InvalidInput(format!("cannot apply override on non-object path: {key}"))
        })?;
        current = map
            .entry((*part).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    let leaf = current
        .as_object_mut()
        .ok_or_else(|| AppError::InvalidInput(format!("cannot set leaf for key: {key}")))?;
    leaf.insert(parts[parts.len() - 1].to_string(), value);
    Ok(())
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = bytes as f64;
    let mut unit = 0usize;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct InstanceStatus {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub ports: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub raw: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct ModelDownloadRequest {
    pub repo_id: String,
    #[serde(default)]
    pub patterns: Option<Vec<String>>,
    #[serde(default)]
    pub local_dir: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::{
        ConfigKeySource, InstanceConfig, Template, config_key_source, display_config_key,
        normalize_config_key_path,
    };

    #[test]
    fn template_override_updates_nested_values() {
        let mut overrides = std::collections::BTreeMap::new();
        overrides.insert(
            "variant-a".to_string(),
            HashMap::from([
                ("sampling.temp".to_string(), json!(0.7)),
                ("ctx_size".to_string(), json!(8192)),
            ]),
        );
        let template = Template {
            name: "qwen".to_string(),
            family: "qwen".to_string(),
            description: String::new(),
            config: InstanceConfig {
                model: "/models/qwen.gguf".to_string(),
                ..InstanceConfig::default()
            },
            overrides,
        };
        let resolved = template
            .resolve(template.overrides.get("variant-a"))
            .unwrap();
        assert_eq!(resolved.sampling.temp, 0.7);
        assert_eq!(resolved.ctx_size, 8192);
    }

    #[test]
    fn source_prefixed_keys_are_supported() {
        let mut overrides = std::collections::BTreeMap::new();
        overrides.insert(
            "variant-a".to_string(),
            HashMap::from([
                ("llama.sampling.temp".to_string(), json!(0.66)),
                ("compose.host_port".to_string(), json!(18080)),
            ]),
        );
        let template = Template {
            name: "qwen".to_string(),
            family: "qwen".to_string(),
            description: String::new(),
            config: InstanceConfig {
                model: "/models/qwen.gguf".to_string(),
                ..InstanceConfig::default()
            },
            overrides,
        };
        let resolved = template
            .resolve(template.overrides.get("variant-a"))
            .unwrap();
        assert!((resolved.sampling.temp - 0.66).abs() < 1e-9);
        assert_eq!(resolved.host_port, 18080);
    }

    #[test]
    fn key_source_helpers_classify_and_display() {
        assert_eq!(
            normalize_config_key_path("llama.sampling.temp"),
            "sampling.temp"
        );
        assert_eq!(normalize_config_key_path("sampling.temp"), "sampling.temp");
        assert_eq!(display_config_key("sampling.temp"), "llama.sampling.temp");
        assert_eq!(display_config_key("compose.host_port"), "compose.host_port");
        assert_eq!(config_key_source("service_name"), ConfigKeySource::Compose);
    }
}
