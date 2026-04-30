use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bare_metal_gguf::{GgufDtype, GgufFile, GgufMetadataValue, GgufTensorInfo, parse_gguf_file};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::{
    process::Command,
    sync::{RwLock, mpsc::UnboundedSender},
    time::sleep,
};

use crate::{
    compose,
    config::WorkspacePaths,
    error::{AppError, AppResult},
    runtime::{DockerClient, DownloadProgress, ModelDownloader},
    store,
    types::{
        Instance, InstanceConfig, InstanceStatus, Model, ModelDownloadRequest, Template,
        normalize_config_key_path,
    },
};

#[derive(Clone)]
pub struct AppService {
    pub paths: WorkspacePaths,
    docker: Arc<dyn DockerClient>,
    downloader: Arc<dyn ModelDownloader>,
    download_tasks: Arc<RwLock<HashMap<String, ModelDownloadTaskStatus>>>,
    download_seq: Arc<AtomicU64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstanceCreateInput {
    pub name: String,
    pub model: String,
    #[serde(default)]
    pub mmproj: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default = "default_ctx_size")]
    pub ctx_size: u32,
    #[serde(default = "default_threads")]
    pub threads: u32,
    #[serde(default = "default_gpu_layers")]
    pub gpu_layers: u32,
    #[serde(default = "default_thinking")]
    pub thinking: bool,
    #[serde(default)]
    pub parallel: Option<u32>,
}

const fn default_ctx_size() -> u32 {
    262_144
}
const fn default_threads() -> u32 {
    8
}
const fn default_gpu_layers() -> u32 {
    999
}
const fn default_thinking() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateCreateInput {
    pub name: String,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub from_instance: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelRenameInput {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub unix_time: u64,
    pub cpu: CpuMetrics,
    pub ram: RamMetrics,
    pub rocm: RocmMetrics,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub usage_percent: f64,
    pub cores: usize,
    pub load_1: f64,
    pub load_5: f64,
    pub load_15: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RamMetrics {
    pub total_mb: u64,
    pub used_mb: u64,
    pub free_mb: u64,
    pub available_mb: u64,
    pub usage_percent: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RocmMetrics {
    pub available: bool,
    pub devices: Vec<GpuDeviceMetrics>,
    pub raw: String,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GpuDeviceMetrics {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub utilization_percent: Option<f64>,
    #[serde(default)]
    pub memory_use_percent: Option<f64>,
    #[serde(default)]
    pub temperature_c: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstanceMemoryPreview {
    pub name: String,
    pub model_ref: String,
    #[serde(default)]
    pub gguf_path: Option<String>,
    #[serde(default)]
    pub architecture: Option<String>,
    pub model_bytes: u64,
    pub kv_cache_bytes: u64,
    pub overhead_bytes: u64,
    pub estimated_total_bytes: u64,
    pub context_size: u32,
    pub parallel: u32,
    pub cache_type_k: String,
    pub cache_type_v: String,
    #[serde(default)]
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEstimateDebug {
    pub preview: InstanceMemoryPreview,
    #[serde(default)]
    pub details: Option<MemoryEstimateDetails>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEstimateDetails {
    pub architecture_raw: String,
    pub architecture_profile: String,
    pub layers: u64,
    pub full_context_layers: u64,
    pub reduced_context_layers: u64,
    #[serde(default)]
    pub reduced_context_size: Option<u32>,
    pub embedding_length: u64,
    pub head_count_query: u64,
    pub head_count_kv: u64,
    pub context_size: u32,
    pub context_size_per_slot: u32,
    pub parallel: u32,
    pub cache_type_k: String,
    pub cache_type_v: String,
    pub bytes_per_element_k: f64,
    pub bytes_per_element_v: f64,
    pub per_layer_per_token_bytes: f64,
    pub per_slot_kv_bytes: u64,
    pub total_kv_bytes: u64,
    pub model_bytes: u64,
    pub overhead_bytes: u64,
    pub estimated_total_bytes: u64,
    pub formulas: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayoutMove {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayoutMigrationReport {
    pub stopped_instances: Vec<String>,
    pub restarted_instances: Vec<String>,
    pub model_moves: Vec<LayoutMove>,
    pub instance_moves: Vec<LayoutMove>,
    pub updated_instances: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstanceHierarchyItem {
    pub name: String,
    pub family: String,
    pub model: String,
    pub quant: String,
    pub variant: String,
    pub model_ref: String,
    #[serde(default)]
    pub mmproj_ref: Option<String>,
    pub path: String,
    pub host_port: u16,
    pub config: InstanceConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelHierarchyItem {
    pub key: String,
    pub family: String,
    pub model: String,
    pub quant: String,
    pub path: String,
    pub file_count: usize,
    pub total_size_bytes: u64,
    #[serde(default)]
    pub files: Vec<ModelFileRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelFileRecord {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelDownloadPlan {
    pub repo_id: String,
    #[serde(default)]
    pub patterns: Vec<String>,
    pub target_relative_dir: String,
    pub target_absolute_dir: String,
    pub script_path: String,
    pub family: String,
    pub model: String,
    pub quant: String,
    #[serde(default)]
    pub selected_files: Vec<String>,
    #[serde(default)]
    pub selected_mmproj_files: Vec<String>,
    pub selected_model_file: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelDownloadTaskStatus {
    pub id: String,
    pub repo_id: String,
    #[serde(default)]
    pub patterns: Vec<String>,
    pub phase: String,
    pub running: bool,
    pub progress_percent: f64,
    pub latest_message: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub output_path: Option<String>,
    pub started_at: u64,
    pub updated_at: u64,
    pub plan: ModelDownloadPlan,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateHierarchyItem {
    pub name: String,
    pub family: String,
    pub model: String,
    pub quant: String,
    pub description: String,
    pub variant_count: usize,
    pub variants: Vec<String>,
    pub config: InstanceConfig,
    pub overrides: BTreeMap<String, HashMap<String, Value>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemoryArchitectureProfile {
    Gemma4,
    Qwen35,
    Qwen3,
    Llama,
    Standard,
}

impl MemoryArchitectureProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Gemma4 => "gemma4",
            Self::Qwen35 => "qwen35",
            Self::Qwen3 => "qwen3",
            Self::Llama => "llama",
            Self::Standard => "standard",
        }
    }
}

const MEMORY_ARCHITECTURE_PROFILES: [&str; 5] = ["gemma4", "qwen35", "qwen3", "llama", "standard"];

impl AppService {
    pub fn new(
        paths: WorkspacePaths,
        docker: Arc<dyn DockerClient>,
        downloader: Arc<dyn ModelDownloader>,
    ) -> AppResult<Self> {
        Self::new_with_layout_enforcement(paths, docker, downloader, true)
    }

    pub fn new_with_layout_enforcement(
        paths: WorkspacePaths,
        docker: Arc<dyn DockerClient>,
        downloader: Arc<dyn ModelDownloader>,
        enforce_canonical_layout: bool,
    ) -> AppResult<Self> {
        store::ensure_workspace(&paths)?;
        if enforce_canonical_layout {
            enforce_canonical_workspace_layout(&paths)?;
        }
        Ok(Self {
            paths,
            docker,
            downloader,
            download_tasks: Arc::new(RwLock::new(HashMap::new())),
            download_seq: Arc::new(AtomicU64::new(1)),
        })
    }

    pub fn list_instances(&self) -> AppResult<Vec<Instance>> {
        store::discover_instances(&self.paths)
    }

    pub fn list_instances_hierarchy(&self) -> AppResult<Vec<InstanceHierarchyItem>> {
        let mut rows = self
            .list_instances()?
            .into_iter()
            .map(|instance| {
                let (family, model, quant) = model_ref_to_layout_parts(&instance.config.model)
                    .unwrap_or_else(|| {
                        (
                            infer_family_slug_from_name(&instance.name),
                            normalize_model_name(&instance.name, "GENERAL"),
                            "GENERAL".to_string(),
                        )
                    });
                let variant = derive_instance_variant_from_path(&self.paths, &instance.path)
                    .unwrap_or_else(|| derive_instance_role(&instance.name, &model));
                InstanceHierarchyItem {
                    name: instance.name.clone(),
                    family,
                    model,
                    quant,
                    variant,
                    model_ref: instance.config.model.clone(),
                    mmproj_ref: instance.config.mmproj.clone(),
                    path: instance.path.display().to_string(),
                    host_port: instance.config.host_port,
                    config: instance.config,
                }
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(rows)
    }

    pub fn instance_memory_previews(&self) -> AppResult<Vec<InstanceMemoryPreview>> {
        let instances = self.list_instances()?;
        Ok(instances
            .iter()
            .map(|instance| estimate_instance_memory_preview(&self.paths, instance))
            .collect())
    }

    pub fn instance_memory_debug(&self, name: &str) -> AppResult<MemoryEstimateDebug> {
        let instance = self.get_instance(name)?;
        Ok(estimate_memory_preview_with_details(
            &self.paths,
            &instance.name,
            &instance.config,
        ))
    }

    pub fn memory_debug_for_config(
        &self,
        name: &str,
        config: InstanceConfig,
    ) -> AppResult<MemoryEstimateDebug> {
        Ok(estimate_memory_preview_with_details(
            &self.paths,
            name,
            &config,
        ))
    }

    pub fn memory_estimator_architectures(&self) -> Vec<String> {
        MEMORY_ARCHITECTURE_PROFILES
            .iter()
            .map(|value| (*value).to_string())
            .collect()
    }

    pub fn get_instance(&self, name: &str) -> AppResult<Instance> {
        store::get_instance(&self.paths, name)?
            .ok_or_else(|| AppError::NotFound(format!("instance '{name}'")))
    }

    pub fn create_instance(&self, req: InstanceCreateInput) -> AppResult<Instance> {
        let port = match req.port {
            Some(port) => port,
            None => store::find_available_port(&self.paths, 8080, 8192)?.ok_or_else(|| {
                AppError::InvalidInput("no available port in range 8080-8191".to_string())
            })?,
        };
        let config = InstanceConfig {
            name: req.name.clone(),
            model: ensure_model_prefix(&req.model),
            mmproj: req.mmproj.map(|v| ensure_model_prefix(&v)),
            host_port: port,
            ctx_size: req.ctx_size,
            threads: req.threads,
            n_gpu_layers: req.gpu_layers,
            parallel: req.parallel,
            chat_template_kwargs: serde_json::json!({ "enable_thinking": req.thinking }),
            ..InstanceConfig::default()
        };
        store::create_instance(&self.paths, &req.name, config)
    }

    pub fn delete_instance(&self, name: &str) -> AppResult<bool> {
        store::delete_instance(&self.paths, name)
    }

    pub fn edit_instance(&self, name: &str, key: &str, value: Value) -> AppResult<Instance> {
        let mut instance = self.get_instance(name)?;
        let mut config_value = serde_json::to_value(&instance.config)?;
        apply_edit(&mut config_value, key, value)?;
        instance.config = serde_json::from_value(config_value)?;
        store::update_instance(&self.paths, name, instance.config)
    }

    pub async fn start_instance(&self, name: &str) -> AppResult<()> {
        let instance = self.get_instance(name)?;
        let projects = compose_project_names_for_instance(&self.paths, &instance);
        let project = projects
            .first()
            .cloned()
            .unwrap_or_else(|| compose_project_name(&instance.name));
        let args = vec!["-p", project.as_str(), "up", "-d"];
        let output = self.docker.compose(&instance.path, &args).await?;
        if output.code != 0 {
            return Err(AppError::CommandFailed(output.stderr));
        }
        Ok(())
    }

    pub async fn stop_instance(&self, name: &str) -> AppResult<()> {
        let instance = self.get_instance(name)?;
        let projects = compose_project_names_for_instance(&self.paths, &instance);
        let mut last_err = None::<String>;
        let mut any_ok = false;
        for project in projects {
            let args = vec!["-p", project.as_str(), "down"];
            let output = self.docker.compose(&instance.path, &args).await?;
            if output.code == 0 {
                any_ok = true;
            } else if !output.stderr.trim().is_empty() {
                last_err = Some(output.stderr);
            }
        }
        if !any_ok {
            return Err(AppError::CommandFailed(
                last_err.unwrap_or_else(|| format!("failed to stop instance '{name}'")),
            ));
        }
        Ok(())
    }

    pub async fn restart_instance(&self, name: &str) -> AppResult<()> {
        self.stop_instance(name).await?;
        self.start_instance(name).await
    }

    pub async fn instance_status(&self, name: &str) -> AppResult<InstanceStatus> {
        let instance = self.get_instance(name)?;
        let projects = compose_project_names_for_instance(&self.paths, &instance);
        let mut fallback = None::<InstanceStatus>;
        let mut last_error = None::<String>;
        for project in projects {
            let args = vec!["-p", project.as_str(), "ps", "--format", "json"];
            let output = self.docker.compose(&instance.path, &args).await?;
            if output.code != 0 {
                if !output.stderr.trim().is_empty() {
                    last_error = Some(output.stderr);
                }
                continue;
            }
            let parsed = parse_status_output(name, &output.stdout);
            let lowered = parsed.status.to_ascii_lowercase();
            if lowered.contains("run") || lowered.contains("up") {
                return Ok(parsed);
            }
            if fallback.is_none() {
                fallback = Some(parsed);
            }
        }
        if let Some(status) = fallback {
            return Ok(status);
        }
        Ok(InstanceStatus {
            name: name.to_string(),
            status: "error".to_string(),
            ports: None,
            error: last_error.or_else(|| Some("compose status unavailable".to_string())),
            raw: None,
        })
    }

    pub async fn instance_logs(&self, name: &str, tail: usize) -> AppResult<String> {
        let instance = self.get_instance(name)?;
        let tail_text = tail.to_string();
        let projects = compose_project_names_for_instance(&self.paths, &instance);
        let mut last_err = None::<String>;
        for project in projects {
            let args = vec!["-p", project.as_str(), "logs", "--tail", tail_text.as_str()];
            let output = self.docker.compose(&instance.path, &args).await?;
            if output.code == 0 {
                return Ok(output.stdout);
            }
            if !output.stderr.trim().is_empty() {
                last_err = Some(output.stderr);
            }
        }
        Err(AppError::CommandFailed(last_err.unwrap_or_else(|| {
            format!("failed to fetch logs for instance '{name}'")
        })))
    }

    pub async fn health_check(&self, name: &str) -> AppResult<Value> {
        let instance = self.get_instance(name)?;
        let url = format!("http://127.0.0.1:{}/health", instance.config.host_port);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;
        match client.get(&url).send().await {
            Ok(response) => {
                let status = response.status();
                let json = response.json::<Value>().await.unwrap_or(Value::Null);
                Ok(serde_json::json!({
                    "name": name,
                    "status": if status.is_success() { "healthy" } else { "unhealthy" },
                    "response": json
                }))
            }
            Err(err) => Ok(serde_json::json!({
                "name": name,
                "status": "unhealthy",
                "error": err.to_string()
            })),
        }
    }

    pub async fn all_instances_status(&self) -> AppResult<Vec<InstanceStatus>> {
        let instances = self.list_instances()?;
        let mut statuses = Vec::with_capacity(instances.len());
        for instance in instances {
            statuses.push(self.instance_status(&instance.name).await?);
        }
        Ok(statuses)
    }

    pub fn list_models(&self) -> AppResult<Vec<Model>> {
        store::discover_models(&self.paths)
    }

    pub fn list_models_hierarchy(&self) -> AppResult<Vec<ModelHierarchyItem>> {
        let mut rows = self
            .list_models()?
            .into_iter()
            .map(|model| {
                let rel = model
                    .path
                    .strip_prefix(&self.paths.models_dir)
                    .ok()
                    .map(|v| {
                        v.iter()
                            .map(|p| p.to_string_lossy().to_string())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let family = rel
                    .first()
                    .cloned()
                    .map(|v| canonicalize_family_slug(&sanitize_segment(&v)))
                    .unwrap_or_else(|| "unknown".to_string());
                let folder_quant = rel.get(2).cloned().unwrap_or_else(|| "GENERAL".to_string());
                let folder_quant = normalize_quant_slug(&folder_quant).unwrap_or(folder_quant);
                let fallback_model_name = rel.get(1).cloned().unwrap_or_else(|| model.name.clone());
                let model_name = {
                    let normalized = normalize_model_name(&fallback_model_name, &folder_quant);
                    if normalized.is_empty() {
                        sanitize_segment(&fallback_model_name)
                    } else {
                        normalized
                    }
                };

                // Prefer quant detected from primary *non-mmproj* gguf filename.
                let primary_non_mmproj = model
                    .files
                    .iter()
                    .filter(|file| {
                        file.path.extension().and_then(|ext| ext.to_str()) == Some("gguf")
                    })
                    .filter(|file| !is_mmproj_filename(&file.name))
                    .max_by_key(|f| f.size_bytes);
                let detected_quant = primary_non_mmproj
                    .and_then(|primary| detect_quant_from_filename(&primary.name));
                let quant = detected_quant.unwrap_or_else(|| folder_quant.clone());
                let files = model
                    .files
                    .into_iter()
                    .map(|file| ModelFileRecord {
                        name: file.name,
                        path: file.path.display().to_string(),
                        size_bytes: file.size_bytes,
                    })
                    .collect::<Vec<_>>();
                ModelHierarchyItem {
                    key: format!("{family}/{model_name}/{quant}"),
                    family,
                    model: model_name,
                    quant,
                    path: model.path.display().to_string(),
                    file_count: files.len(),
                    total_size_bytes: files.iter().map(|f| f.size_bytes).sum(),
                    files,
                }
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            a.family
                .cmp(&b.family)
                .then(a.model.cmp(&b.model))
                .then(a.quant.cmp(&b.quant))
        });
        Ok(rows)
    }

    pub async fn plan_model_download(
        &self,
        req: ModelDownloadRequest,
    ) -> AppResult<ModelDownloadPlan> {
        let (plan, _) = self.normalize_download_request(req).await?;
        Ok(plan)
    }

    pub async fn download_model(&self, req: ModelDownloadRequest) -> AppResult<String> {
        let (_, normalized_req) = self.normalize_download_request(req).await?;
        self.downloader
            .download(&normalized_req, &self.paths.models_dir)
            .await
    }

    pub async fn download_model_with_progress(
        &self,
        req: ModelDownloadRequest,
        progress: Option<UnboundedSender<DownloadProgress>>,
    ) -> AppResult<String> {
        let (plan, normalized_req) = self.normalize_download_request(req).await?;
        if let Some(tx) = &progress {
            let _ = tx.send(DownloadProgress {
                percent: Some(0.0),
                message: format!(
                    "planned target: /models/{} ({}/{}/{})",
                    plan.target_relative_dir, plan.family, plan.model, plan.quant
                ),
            });
        }
        self.downloader
            .download_with_progress(&normalized_req, &self.paths.models_dir, progress)
            .await
    }

    pub async fn start_model_download_task(
        &self,
        req: ModelDownloadRequest,
    ) -> AppResult<ModelDownloadTaskStatus> {
        let (plan, normalized_req) = self.normalize_download_request(req).await?;
        let task_id = format!(
            "dl-{}-{}",
            now_unix_millis(),
            self.download_seq.fetch_add(1, Ordering::Relaxed)
        );
        let now = now_unix_secs();
        let task = ModelDownloadTaskStatus {
            id: task_id.clone(),
            repo_id: normalized_req.repo_id.clone(),
            patterns: normalized_req.patterns.clone().unwrap_or_default(),
            phase: "queued".to_string(),
            running: true,
            progress_percent: 0.0,
            latest_message: format!(
                "queued -> /models/{} ({}/{}/{})",
                plan.target_relative_dir, plan.family, plan.model, plan.quant
            ),
            error: None,
            output_path: None,
            started_at: now,
            updated_at: now,
            plan: plan.clone(),
        };
        {
            let mut tasks = self.download_tasks.write().await;
            tasks.insert(task_id.clone(), task.clone());
        }

        let tasks = Arc::clone(&self.download_tasks);
        let downloader = Arc::clone(&self.downloader);
        let models_dir = self.paths.models_dir.clone();
        tokio::spawn(async move {
            let (progress_tx, mut progress_rx) =
                tokio::sync::mpsc::unbounded_channel::<DownloadProgress>();
            let progress_tasks = Arc::clone(&tasks);
            let progress_id = task_id.clone();
            let progress_worker = tokio::spawn(async move {
                while let Some(progress) = progress_rx.recv().await {
                    let mut guard = progress_tasks.write().await;
                    if let Some(task) = guard.get_mut(&progress_id) {
                        task.phase = "downloading".to_string();
                        if let Some(percent) = progress.percent {
                            task.progress_percent =
                                task.progress_percent.max(percent.clamp(0.0, 99.5));
                        } else {
                            task.progress_percent = (task.progress_percent + 0.4).min(95.0);
                        }
                        task.latest_message = progress.message;
                        task.updated_at = now_unix_secs();
                    }
                }
            });

            let result = downloader
                .download_with_progress(&normalized_req, &models_dir, Some(progress_tx))
                .await;
            let _ = progress_worker.await;

            let mut guard = tasks.write().await;
            if let Some(task) = guard.get_mut(&task_id) {
                task.running = false;
                task.updated_at = now_unix_secs();
                match result {
                    Ok(path) => {
                        task.phase = "completed".to_string();
                        task.progress_percent = 100.0;
                        task.latest_message = format!("download completed: {}", path);
                        task.output_path = Some(path);
                        task.error = None;
                    }
                    Err(err) => {
                        task.phase = "failed".to_string();
                        task.latest_message = err.to_string();
                        task.error = Some(err.to_string());
                    }
                }
            }
        });

        Ok(task)
    }

    pub async fn list_model_download_tasks(&self) -> Vec<ModelDownloadTaskStatus> {
        let mut items = self
            .download_tasks
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(b.id.cmp(&a.id)));
        items
    }

    pub async fn get_model_download_task(&self, id: &str) -> AppResult<ModelDownloadTaskStatus> {
        self.download_tasks
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("download task '{id}'")))
    }

    async fn normalize_download_request(
        &self,
        req: ModelDownloadRequest,
    ) -> AppResult<(ModelDownloadPlan, ModelDownloadRequest)> {
        let plan = plan_model_download_request(&self.paths, &req).await?;
        let mut normalized = req.clone();
        normalized.local_dir = Some(plan.target_relative_dir.clone());
        Ok((plan, normalized))
    }

    pub fn delete_model(&self, name: &str) -> AppResult<bool> {
        store::delete_model(&self.paths, name)
    }

    pub fn rename_model(&self, name: &str, new_name: &str) -> AppResult<bool> {
        store::rename_model(&self.paths, name, new_name)
    }

    pub fn list_templates(&self) -> AppResult<Vec<Template>> {
        store::load_templates(&self.paths)
    }

    pub fn list_templates_hierarchy(&self) -> AppResult<Vec<TemplateHierarchyItem>> {
        let mut rows = self
            .list_templates()?
            .into_iter()
            .map(|template| {
                let (family_from_model, model, quant) =
                    model_ref_to_layout_parts(&template.config.model).unwrap_or_else(|| {
                        (
                            infer_family_slug_from_name(&template.family),
                            normalize_model_name(&template.name, "GENERAL"),
                            "GENERAL".to_string(),
                        )
                    });
                let mut variants = template.overrides.keys().cloned().collect::<Vec<_>>();
                variants.sort();
                TemplateHierarchyItem {
                    name: template.name,
                    family: if template.family.trim().is_empty() {
                        family_from_model
                    } else {
                        canonicalize_family_slug(&template.family)
                    },
                    model,
                    quant,
                    description: template.description,
                    variant_count: variants.len(),
                    variants,
                    config: template.config,
                    overrides: template.overrides,
                }
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            a.family
                .cmp(&b.family)
                .then(a.model.cmp(&b.model))
                .then(a.quant.cmp(&b.quant))
                .then(a.name.cmp(&b.name))
        });
        Ok(rows)
    }

    pub fn create_template(&self, req: TemplateCreateInput) -> AppResult<Template> {
        let config = if let Some(source) = req.from_instance {
            self.get_instance(&source)?.config
        } else {
            InstanceConfig {
                name: req.name.clone(),
                ..InstanceConfig::default()
            }
        };
        store::create_template(
            &self.paths,
            &req.name,
            req.family.as_deref().unwrap_or(&req.name),
            &req.description,
            config,
        )
    }

    pub fn create_template_from_model(
        &self,
        name: &str,
        family: &str,
        description: &str,
        model_ref: &str,
        mmproj: Option<String>,
    ) -> AppResult<Template> {
        let config = InstanceConfig {
            name: name.to_string(),
            model: ensure_model_prefix(model_ref),
            mmproj: mmproj.as_deref().map(ensure_model_prefix),
            ..InstanceConfig::default()
        };
        store::create_template(&self.paths, name, family, description, config)
    }

    pub fn delete_template(&self, name: &str) -> AppResult<bool> {
        store::delete_template(&self.paths, name)
    }

    pub fn instantiate_template(
        &self,
        template_name: &str,
        instance_name: &str,
        overrides: Option<HashMap<String, Value>>,
    ) -> AppResult<Option<Instance>> {
        store::instantiate_template(&self.paths, template_name, instance_name, overrides)
    }

    pub fn batch_apply(
        &self,
        template_name: &str,
        key: &str,
        value: Value,
    ) -> AppResult<Vec<String>> {
        store::batch_apply_to_family(&self.paths, template_name, key, value)
    }

    pub fn set_template_override(
        &self,
        template_name: &str,
        variant_name: &str,
        key: &str,
        value: Value,
    ) -> AppResult<Template> {
        store::set_template_override(&self.paths, template_name, variant_name, key, value)
    }

    pub fn set_template_base_value(
        &self,
        template_name: &str,
        key: &str,
        value: Value,
    ) -> AppResult<Template> {
        store::set_template_base_value(&self.paths, template_name, key, value)
    }

    pub fn scan_templates(&self) -> AppResult<Vec<Template>> {
        let templates = store::auto_detect_templates(&self.paths)?;
        store::save_templates(&self.paths, &templates)?;
        Ok(templates)
    }

    pub fn port_map(&self) -> AppResult<std::collections::BTreeMap<u16, String>> {
        store::get_port_map(&self.paths)
    }

    pub async fn system_metrics(&self) -> AppResult<SystemMetrics> {
        let cpu = sample_cpu_metrics().await?;
        let ram = read_ram_metrics()?;
        let rocm = collect_rocm_metrics().await;
        let unix_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();
        Ok(SystemMetrics {
            unix_time,
            cpu,
            ram,
            rocm,
        })
    }

    pub async fn migrate_workspace_layout(&self) -> AppResult<LayoutMigrationReport> {
        let mut report = LayoutMigrationReport {
            stopped_instances: Vec::new(),
            restarted_instances: Vec::new(),
            model_moves: Vec::new(),
            instance_moves: Vec::new(),
            updated_instances: Vec::new(),
            warnings: Vec::new(),
        };

        let statuses = self.all_instances_status().await?;
        let running_instances = statuses
            .iter()
            .filter(|status| {
                let lower = status.status.to_ascii_lowercase();
                lower.contains("running") || lower.contains("up")
            })
            .map(|status| status.name.clone())
            .collect::<Vec<_>>();

        for name in &running_instances {
            match self.stop_instance(name).await {
                Ok(_) => report.stopped_instances.push(name.clone()),
                Err(err) => report
                    .warnings
                    .push(format!("failed to stop '{name}' before migration: {err}")),
            }
        }

        let model_prefix_map = migrate_models_layout(&self.paths, &mut report)?;
        migrate_instances_layout(&self.paths, &model_prefix_map, &mut report)?;
        cleanup_alias_directories(&self.paths, &mut report)?;

        for name in running_instances {
            match self.start_instance(&name).await {
                Ok(_) => report.restarted_instances.push(name),
                Err(err) => report
                    .warnings
                    .push(format!("failed to restart '{name}' after migration: {err}")),
            }
        }

        Ok(report)
    }

    pub fn backfill_compose_project_names(&self) -> AppResult<Vec<String>> {
        let instances = self.list_instances()?;
        let mut updated = Vec::with_capacity(instances.len());
        for instance in instances {
            let compose_path = instance.path.join("compose.yml");
            compose::write_compose(&instance.config, &compose_path, &self.paths)?;
            updated.push(compose_path.display().to_string());
        }
        Ok(updated)
    }
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

fn now_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis()
}

async fn plan_model_download_request(
    paths: &WorkspacePaths,
    req: &ModelDownloadRequest,
) -> AppResult<ModelDownloadPlan> {
    if req.repo_id.trim().is_empty() {
        return Err(AppError::InvalidInput("repo_id is required".to_string()));
    }

    let repo_files = fetch_hf_repo_files(&req.repo_id).await?;
    let selected_files = select_repo_files(&repo_files, req.patterns.as_ref());
    if selected_files.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "no files matched repo '{}' with patterns {:?}",
            req.repo_id, req.patterns
        )));
    }

    let mut selected_ggufs = selected_files
        .iter()
        .filter(|file| file.to_ascii_lowercase().ends_with(".gguf"))
        .cloned()
        .collect::<Vec<_>>();
    if selected_ggufs.is_empty() {
        return Err(AppError::InvalidInput(
            "selected files do not contain any gguf file".to_string(),
        ));
    }
    selected_ggufs.sort();
    let selected_mmproj_files = selected_ggufs
        .iter()
        .filter(|file| is_mmproj_filename(file))
        .cloned()
        .collect::<Vec<_>>();
    let selected_model_files = selected_ggufs
        .iter()
        .filter(|file| !is_mmproj_filename(file))
        .cloned()
        .collect::<Vec<_>>();
    if selected_model_files.is_empty() {
        return Err(AppError::InvalidInput(
            "selected gguf files are mmproj-only; include at least one model gguf file".to_string(),
        ));
    }

    let mut quants = selected_model_files
        .iter()
        .filter_map(|file| detect_quant_from_filename(file))
        .collect::<BTreeSet<_>>();
    let selected_model_file = selected_model_files
        .iter()
        .max_by_key(|file| model_file_primary_rank(file))
        .cloned()
        .unwrap_or_else(|| selected_model_files[0].clone());

    let (family, model, quant, target_relative_dir) = if let Some(local_dir_raw) = &req.local_dir {
        let (family, model, quant, local_dir) = normalize_download_local_dir(local_dir_raw)?;
        if quants.len() > 1 && !quants.contains(&quant) {
            return Err(AppError::InvalidInput(format!(
                "download quant ambiguity: selected files suggest quants {:?}, but local_dir quant is '{}'",
                quants, quant
            )));
        }
        (family, model, quant, local_dir)
    } else {
        if quants.len() > 1 {
            return Err(AppError::InvalidInput(format!(
                "download would mix multiple quants {:?}; narrow patterns or provide --local-dir <family>/<model>/<QUANT>",
                quants
            )));
        }
        let quant = if let Some(quant) = quants.pop_first() {
            quant
        } else {
            return Err(AppError::InvalidInput(
                "cannot infer quant from selected model gguf files; provide --local-dir <family>/<model>/<QUANT>".to_string(),
            ));
        };

        let repo_slug = req
            .repo_id
            .split('/')
            .next_back()
            .unwrap_or("model")
            .to_string();
        let inferred_model =
            infer_model_name_from_gguf_filename(repo_file_basename(&selected_model_file), &quant);
        let mut model = if inferred_model.is_empty() {
            normalize_model_name(&repo_slug, &quant)
        } else {
            inferred_model
        };
        if model.is_empty() {
            model = sanitize_segment(&repo_slug);
        }
        let family_seed = if model.is_empty() {
            repo_slug
        } else {
            model.clone()
        };
        let family = infer_family_slug_from_name(&family_seed);
        let target_relative_dir = format!("{}/{}/{}", family, model, quant);
        (family, model, quant, target_relative_dir)
    };

    let target_absolute_dir = paths.models_dir.join(&target_relative_dir);
    let script_path = target_absolute_dir.join("download-model.sh");
    Ok(ModelDownloadPlan {
        repo_id: req.repo_id.clone(),
        patterns: req.patterns.clone().unwrap_or_default(),
        target_relative_dir,
        target_absolute_dir: target_absolute_dir.display().to_string(),
        script_path: script_path.display().to_string(),
        family,
        model,
        quant,
        selected_files,
        selected_mmproj_files,
        selected_model_file,
    })
}

fn normalize_download_local_dir(input: &str) -> AppResult<(String, String, String, String)> {
    let mut local = input.trim().replace('\\', "/");
    if let Some(stripped) = local.strip_prefix("/models/") {
        local = stripped.to_string();
    } else if let Some(stripped) = local.strip_prefix("models/") {
        local = stripped.to_string();
    }
    local = local.trim_matches('/').to_string();
    let segments = local
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>();
    if segments.len() != 3 {
        return Err(AppError::InvalidInput(
            "local_dir must be '<family>/<model>/<QUANT>'".to_string(),
        ));
    }
    let family = canonicalize_family_slug(&sanitize_segment(segments[0]));
    let quant = normalize_quant_slug(segments[2]).ok_or_else(|| {
        AppError::InvalidInput(format!(
            "cannot parse quant from local_dir segment '{}'",
            segments[2]
        ))
    })?;
    let model = normalize_model_name(segments[1], &quant);
    if model.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "cannot parse model from local_dir segment '{}'",
            segments[1]
        )));
    }
    let normalized = format!("{}/{}/{}", family, model, quant);
    Ok((family, model, quant, normalized))
}

async fn fetch_hf_repo_files(repo_id: &str) -> AppResult<Vec<String>> {
    let url = format!("https://huggingface.co/api/models/{repo_id}");
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await?
        .error_for_status()?;
    let payload = response.json::<Value>().await?;
    let files = payload
        .get("siblings")
        .and_then(Value::as_array)
        .map(|siblings| {
            siblings
                .iter()
                .filter_map(|item| item.get("rfilename").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if files.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "repository '{}' has no discoverable files via Hugging Face API",
            repo_id
        )));
    }
    Ok(files)
}

fn select_repo_files(repo_files: &[String], patterns: Option<&Vec<String>>) -> Vec<String> {
    let Some(patterns) = patterns else {
        return repo_files.to_vec();
    };
    if patterns.is_empty() {
        return repo_files.to_vec();
    }
    repo_files
        .iter()
        .filter(|path| {
            let name = repo_file_basename(path);
            patterns
                .iter()
                .any(|pattern| wildcard_match(pattern, path) || wildcard_match(pattern, name))
        })
        .cloned()
        .collect::<Vec<_>>()
}

fn repo_file_basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pat = pattern.to_ascii_lowercase().chars().collect::<Vec<_>>();
    let txt = text.to_ascii_lowercase().chars().collect::<Vec<_>>();
    let mut dp = vec![vec![false; txt.len() + 1]; pat.len() + 1];
    dp[0][0] = true;
    for i in 1..=pat.len() {
        if pat[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=pat.len() {
        for j in 1..=txt.len() {
            dp[i][j] = match pat[i - 1] {
                '*' => dp[i - 1][j] || dp[i][j - 1],
                '?' => dp[i - 1][j - 1],
                ch => dp[i - 1][j - 1] && ch == txt[j - 1],
            };
        }
    }
    dp[pat.len()][txt.len()]
}

fn model_file_primary_rank(path: &str) -> (u8, usize) {
    let lower = path.to_ascii_lowercase();
    let base = repo_file_basename(path).to_ascii_lowercase();
    let shard_hint = if lower.contains("-00001-of-") || lower.contains("_00001-of-") {
        3
    } else if lower.contains(".i1-") || lower.contains(".part1") || lower.contains("-part1") {
        2
    } else {
        1
    };
    (shard_hint, base.len())
}

fn is_mmproj_filename(name: &str) -> bool {
    name.to_ascii_lowercase().contains("mmproj")
}

fn ensure_model_prefix(path: &str) -> String {
    if path.starts_with("/models/") {
        path.to_string()
    } else {
        format!("/models/{}", path.trim_start_matches('/'))
    }
}

fn compose_project_name(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('-');
        }
    }
    let project = out.trim_matches('-').to_string();
    if project.is_empty() {
        "manager-neo".to_string()
    } else {
        project
    }
}

fn compose_project_names_for_instance(paths: &WorkspacePaths, instance: &Instance) -> Vec<String> {
    let compose_path = instance.path.join("compose.yml");
    let canonical =
        compose::compose_project_name_for_instance(&compose_path, paths, &instance.config);
    let legacy = compose_project_name(&instance.name);
    if canonical == legacy {
        vec![canonical]
    } else {
        vec![canonical, legacy]
    }
}

fn enforce_canonical_workspace_layout(paths: &WorkspacePaths) -> AppResult<()> {
    let mut violations = Vec::<String>::new();
    validate_models_layout(paths, &mut violations)?;
    validate_instances_layout(paths, &mut violations)?;
    if violations.is_empty() {
        return Ok(());
    }

    let mut message = vec![
        format!(
            "workspace layout is not canonical under '{}'",
            paths.root.display()
        ),
        "please run migration first:".to_string(),
        format!(
            "  manager-neo --root {} layout migrate",
            paths.root.display()
        ),
        "non-canonical entries:".to_string(),
    ];
    for item in violations.iter().take(16) {
        message.push(format!("  - {item}"));
    }
    if violations.len() > 16 {
        message.push(format!("  ... and {} more", violations.len() - 16));
    }
    Err(AppError::InvalidInput(message.join("\n")))
}

fn validate_models_layout(paths: &WorkspacePaths, violations: &mut Vec<String>) -> AppResult<()> {
    if !paths.models_dir.exists() {
        return Ok(());
    }
    let mut roots = Vec::new();
    collect_model_roots(&paths.models_dir, &mut roots)?;
    for root in roots {
        let Ok(rel) = root.strip_prefix(&paths.models_dir) else {
            continue;
        };
        let parts = rel
            .iter()
            .map(|v| v.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        if parts.len() != 3 {
            violations.push(format!(
                "model path must be /models/<family>/<model>/<QUANT>: {}",
                root.display()
            ));
            continue;
        }
        let family = &parts[0];
        let model = &parts[1];
        let quant = &parts[2];
        let expected_family = canonicalize_family_slug(&sanitize_segment(family));
        if family != &expected_family {
            violations.push(format!(
                "model family alias not allowed (expected '{}'): {}",
                expected_family,
                root.display()
            ));
        }
        let expected_quant = normalize_quant_slug(quant);
        if expected_quant.as_deref() != Some(quant) {
            violations.push(format!(
                "model quant must be canonical uppercase (expected '{}'): {}",
                expected_quant.unwrap_or_else(|| "UNKNOWN".to_string()),
                root.display()
            ));
        }
        let expected_model = normalize_model_name(model, quant);
        if model != &expected_model {
            violations.push(format!(
                "model name must be lowercase and quant-free (expected '{}'): {}",
                expected_model,
                root.display()
            ));
        }
    }
    Ok(())
}

fn validate_instances_layout(
    paths: &WorkspacePaths,
    violations: &mut Vec<String>,
) -> AppResult<()> {
    if !paths.instances_dir.exists() {
        return Ok(());
    }
    let mut compose_files = Vec::<PathBuf>::new();
    collect_compose_files(&paths.instances_dir, &mut compose_files)?;
    for compose_path in compose_files {
        let parent = match compose_path.parent() {
            Some(value) => value,
            None => continue,
        };
        let Ok(rel) = parent.strip_prefix(&paths.instances_dir) else {
            continue;
        };
        let parts = rel
            .iter()
            .map(|v| v.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        if parts.len() != 4 {
            violations.push(format!(
                "instance path must be /instances/<family>/<model>/<QUANT>/<role>/compose.yml: {}",
                compose_path.display()
            ));
            continue;
        }
        let family = &parts[0];
        let model = &parts[1];
        let quant = &parts[2];
        let role = &parts[3];

        let expected_family = canonicalize_family_slug(&sanitize_segment(family));
        if family != &expected_family {
            violations.push(format!(
                "instance family alias not allowed (expected '{}'): {}",
                expected_family,
                compose_path.display()
            ));
        }
        let expected_quant = normalize_quant_slug(quant);
        if expected_quant.as_deref() != Some(quant) {
            violations.push(format!(
                "instance quant must be canonical uppercase (expected '{}'): {}",
                expected_quant.unwrap_or_else(|| "UNKNOWN".to_string()),
                compose_path.display()
            ));
        }
        let expected_model = normalize_model_name(model, quant);
        if model != &expected_model {
            violations.push(format!(
                "instance model must be lowercase and quant-free (expected '{}'): {}",
                expected_model,
                compose_path.display()
            ));
        }
        if role != &sanitize_segment(role) {
            violations.push(format!(
                "instance role must be lowercase slug: {}",
                compose_path.display()
            ));
        }

        if let Ok((_, config)) = compose::compose_to_instance_config(&compose_path) {
            let refs = [&config.model]
                .into_iter()
                .chain(config.mmproj.as_ref())
                .chain(config.draft_model.as_ref());
            for model_ref in refs {
                if !model_ref.starts_with("/models/") {
                    continue;
                }
                if let Some((rf_family, rf_model, rf_quant)) = model_ref_to_layout_parts(model_ref)
                {
                    if rf_family != *family || rf_model != *model || rf_quant != *quant {
                        violations.push(format!(
                            "compose model ref mismatches instance path '{}': {}",
                            model_ref,
                            compose_path.display()
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn collect_compose_files(path: &Path, out: &mut Vec<PathBuf>) -> AppResult<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let current = entry.path();
        if current.is_dir() {
            collect_compose_files(&current, out)?;
            continue;
        }
        if current
            .file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|name| name == "compose.yml")
        {
            out.push(current);
        }
    }
    Ok(())
}

fn migrate_models_layout(
    paths: &WorkspacePaths,
    report: &mut LayoutMigrationReport,
) -> AppResult<HashMap<String, String>> {
    let mut remap = HashMap::new();
    if !paths.models_dir.exists() {
        return Ok(remap);
    }
    let mut roots = Vec::new();
    collect_model_roots(&paths.models_dir, &mut roots)?;
    for model_dir in roots {
        let rel = match model_dir.strip_prefix(&paths.models_dir) {
            Ok(path) => path.to_path_buf(),
            Err(_) => continue,
        };
        let rel_segments = rel
            .iter()
            .map(|v| v.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        if rel_segments.is_empty() {
            continue;
        }

        let mut gguf_names = Vec::new();
        collect_gguf_names(&model_dir, &mut gguf_names)?;
        let quant_from_second = rel_segments
            .get(1)
            .and_then(|value| normalize_quant_slug(value));
        let quant = rel_segments
            .get(2)
            .and_then(|value| normalize_quant_slug(value))
            .or_else(|| quant_from_second.clone())
            .or_else(|| {
                gguf_names
                    .iter()
                    .find_map(|name| detect_quant_from_filename(name))
            })
            .unwrap_or_else(|| "GENERAL".to_string());

        let family_seed = rel_segments
            .first()
            .cloned()
            .unwrap_or_else(|| "family".to_string());
        let family = infer_family_slug_from_name(&family_seed);

        let model_segment = rel_segments
            .get(1)
            .cloned()
            .unwrap_or_else(|| rel_segments[0].clone());
        let generic_model_segment = normalize_model_name(&model_segment, &quant) == family;
        if rel_segments.len() >= 3 && generic_model_segment {
            let mut inferred_models = Vec::<String>::new();
            for name in &gguf_names {
                if name.to_ascii_lowercase().contains("mmproj") {
                    continue;
                }
                let inferred = infer_model_name_from_gguf_filename(name, &quant);
                if inferred.is_empty()
                    || inferred_models.iter().any(|existing| existing == &inferred)
                {
                    continue;
                }
                inferred_models.push(inferred);
            }
            if inferred_models.len() > 1 {
                for name in &gguf_names {
                    if name.to_ascii_lowercase().contains("mmproj") {
                        continue;
                    }
                    let inferred = infer_model_name_from_gguf_filename(name, &quant);
                    if inferred.is_empty() {
                        continue;
                    }
                    let source = model_dir.join(name);
                    if !source.exists() {
                        continue;
                    }
                    let target_dir = paths.models_dir.join(&family).join(&inferred).join(&quant);
                    fs::create_dir_all(&target_dir)?;
                    let target = target_dir.join(name);
                    if !target.exists() {
                        fs::rename(&source, &target)?;
                    }
                    report.model_moves.push(LayoutMove {
                        from: source.display().to_string(),
                        to: target.display().to_string(),
                    });
                    let old_rel = source
                        .strip_prefix(&paths.models_dir)
                        .ok()
                        .map(|v| v.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let new_rel = target
                        .strip_prefix(&paths.models_dir)
                        .ok()
                        .map(|v| v.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !old_rel.is_empty() && !new_rel.is_empty() {
                        remap.insert(
                            format!("/models/{}", old_rel.trim_start_matches('/')),
                            format!("/models/{}", new_rel.trim_start_matches('/')),
                        );
                    }
                }
                for name in &gguf_names {
                    if !name.to_ascii_lowercase().contains("mmproj") {
                        continue;
                    }
                    let source = model_dir.join(name);
                    if !source.exists() {
                        continue;
                    }
                    for inferred in &inferred_models {
                        let target = paths
                            .models_dir
                            .join(&family)
                            .join(inferred)
                            .join(&quant)
                            .join(name);
                        if target.exists() {
                            continue;
                        }
                        fs::copy(&source, &target)?;
                    }
                }
                continue;
            }
        }

        let model_from_file = gguf_names
            .iter()
            .find(|name| !name.to_ascii_lowercase().contains("mmproj"))
            .map(|name| infer_model_name_from_gguf_filename(name, &quant))
            .filter(|name| !name.is_empty());
        let mut model_seed = if rel_segments.len() >= 2 && quant_from_second.is_some() {
            rel_segments[0].clone()
        } else {
            rel_segments
                .get(1)
                .cloned()
                .unwrap_or_else(|| rel_segments[0].clone())
        };
        if normalize_model_name(&model_seed, &quant) == canonicalize_family_slug(&family_seed)
            || model_seed.eq_ignore_ascii_case("general")
            || model_seed.eq_ignore_ascii_case("embedding")
            || model_seed.eq_ignore_ascii_case("reranker")
        {
            if let Some(file_model) = model_from_file {
                model_seed = file_model;
            }
        }
        let mut model = normalize_model_name(&model_seed, &quant);
        if model.is_empty() {
            model = sanitize_segment(&model_seed);
        }

        let target_dir = paths.models_dir.join(&family).join(&model).join(&quant);
        if target_dir == model_dir {
            continue;
        }

        fs::create_dir_all(target_dir.parent().unwrap_or(&paths.models_dir))?;
        move_directory_merge(&model_dir, &target_dir, &mut report.warnings)?;
        report.model_moves.push(LayoutMove {
            from: model_dir.display().to_string(),
            to: target_dir.display().to_string(),
        });
        remap.insert(
            format!("/models/{}", rel.to_string_lossy().trim_start_matches('/')),
            format!(
                "/models/{}/{}/{}",
                family.trim_start_matches('/'),
                model.trim_start_matches('/'),
                quant.trim_start_matches('/')
            ),
        );
    }

    Ok(remap)
}

fn migrate_instances_layout(
    paths: &WorkspacePaths,
    model_prefix_map: &HashMap<String, String>,
    report: &mut LayoutMigrationReport,
) -> AppResult<()> {
    let instances = store::discover_instances(paths)?;
    for instance in instances {
        let mut config = instance.config.clone();
        let mut changed = false;
        let mut moved = false;

        if let Some(rewritten) = rewrite_model_reference(&config.model, model_prefix_map) {
            if rewritten != config.model {
                config.model = rewritten;
                changed = true;
            }
        }
        if let Some(mmproj) = config.mmproj.as_ref() {
            if let Some(rewritten) = rewrite_model_reference(mmproj, model_prefix_map) {
                if &rewritten != mmproj {
                    config.mmproj = Some(rewritten);
                    changed = true;
                }
            }
        }
        if let Some(draft_model) = config.draft_model.as_ref() {
            if let Some(rewritten) = rewrite_model_reference(draft_model, model_prefix_map) {
                if &rewritten != draft_model {
                    config.draft_model = Some(rewritten);
                    changed = true;
                }
            }
        }
        if config.container_name.as_deref() != Some(&instance.name) {
            config.container_name = Some(instance.name.clone());
            changed = true;
        }
        let (family, mut model_name, quant) = model_ref_to_layout_parts(&config.model)
            .map(|(family, model, quant)| {
                (
                    canonicalize_family_slug(&family),
                    normalize_model_name(&model, &quant),
                    normalize_quant_slug(&quant).unwrap_or_else(|| "GENERAL".to_string()),
                )
            })
            .unwrap_or_else(|| {
                (
                    infer_family_slug_from_name(&instance.name),
                    normalize_model_name(&instance.name, "GENERAL"),
                    "GENERAL".to_string(),
                )
            });
        if model_name.is_empty() {
            model_name = sanitize_segment(&instance.name);
        }
        if (model_name == family || model_name == "general")
            && config.model.to_ascii_lowercase().ends_with(".gguf")
        {
            if let Some(file_name) = config.model.rsplit('/').next() {
                let inferred = infer_model_name_from_gguf_filename(file_name, &quant);
                if !inferred.is_empty() {
                    model_name = inferred;
                }
            }
        }
        let role = derive_instance_role(&instance.name, &model_name);
        let target_dir = paths
            .instances_dir
            .join(&family)
            .join(&model_name)
            .join(&quant)
            .join(&role);

        let current_dir = instance.path.clone();
        let effective_dir;
        if current_dir != target_dir {
            if target_dir.exists() {
                let fallback = target_dir.join(sanitize_segment(&instance.name));
                if fallback.exists() {
                    report.warnings.push(format!(
                        "instance move skipped (both target and fallback exist): {}",
                        fallback.display()
                    ));
                    effective_dir = current_dir.clone();
                } else {
                    fs::create_dir_all(fallback.parent().unwrap_or(&paths.instances_dir))?;
                    fs::rename(&current_dir, &fallback)?;
                    moved = true;
                    effective_dir = fallback.clone();
                    report.instance_moves.push(LayoutMove {
                        from: current_dir.display().to_string(),
                        to: fallback.display().to_string(),
                    });
                }
            } else {
                fs::create_dir_all(target_dir.parent().unwrap_or(&paths.instances_dir))?;
                fs::rename(&current_dir, &target_dir)?;
                moved = true;
                effective_dir = target_dir.clone();
                report.instance_moves.push(LayoutMove {
                    from: current_dir.display().to_string(),
                    to: target_dir.display().to_string(),
                });
            }
        } else {
            effective_dir = target_dir.clone();
        }

        if changed || moved {
            let compose_path = effective_dir.join("compose.yml");
            compose::write_compose(&config, &compose_path, paths)?;
            report.updated_instances.push(instance.name.clone());
        }
    }
    Ok(())
}

fn rewrite_model_reference(
    path: &str,
    model_prefix_map: &HashMap<String, String>,
) -> Option<String> {
    if !path.starts_with("/models/") {
        return Some(ensure_model_prefix(path));
    }
    for (legacy, modern) in model_prefix_map {
        if path == legacy {
            return Some(modern.clone());
        }
        if let Some(rest) = path.strip_prefix(legacy) {
            if rest.starts_with('/') {
                return Some(canonicalize_model_ref_layout(&format!(
                    "{}{}",
                    modern.trim_end_matches('/'),
                    rest
                )));
            }
        }
    }
    Some(canonicalize_model_ref_layout(path))
}

fn infer_family_slug_from_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.contains("gemma-4") || lower.starts_with("gemma4") {
        "gemma-4".to_string()
    } else if lower.contains("qwen-3.6")
        || lower.contains("qwen-3-6")
        || lower.starts_with("qwen3.6")
        || lower.starts_with("qwen3-6")
        || lower.starts_with("qwen36")
    {
        "qwen-3.6".to_string()
    } else if lower.contains("qwen-3.5")
        || lower.contains("qwen-3-5")
        || lower.starts_with("qwen3.5")
        || lower.starts_with("qwen3-5")
        || lower.starts_with("qwen35")
    {
        "qwen-3.5".to_string()
    } else if lower.contains("qwen-3") || lower.starts_with("qwen3") {
        "qwen-3".to_string()
    } else if lower.contains("llama-4") {
        "llama-4".to_string()
    } else if lower.contains("llama-3") {
        "llama-3".to_string()
    } else if lower.contains("step-3.5")
        || lower.contains("step-3-5")
        || lower.starts_with("step3.5")
        || lower.starts_with("step3-5")
    {
        "step-3.5".to_string()
    } else {
        canonicalize_family_slug(&sanitize_segment(name))
    }
}

fn detect_quant_from_filename(name: &str) -> Option<String> {
    if is_mmproj_filename(name) {
        return None;
    }
    if let Some(detected) = detect_quant_like_slug_from_text(name) {
        return Some(detected);
    }
    let stem_key = quant_compare_key(name);
    let mut tokens = quant_tokens()
        .iter()
        .map(|token| (*token, quant_compare_key(token)))
        .collect::<Vec<_>>();
    tokens.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (token, token_key) in tokens {
        if stem_key.contains(&token_key) {
            return Some(token.to_string());
        }
    }
    None
}

fn sanitize_segment(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if normalized == '-' {
            if last_dash {
                continue;
            }
            last_dash = true;
        } else {
            last_dash = false;
        }
        out.push(normalized);
    }
    out.trim_matches('-').to_string()
}

fn model_ref_to_layout_parts(model_ref: &str) -> Option<(String, String, String)> {
    let trimmed = model_ref
        .trim_start_matches("/models/")
        .trim_start_matches('/');
    let segments = trimmed.split('/').collect::<Vec<_>>();
    if segments.len() < 3 {
        return None;
    }
    Some((
        canonicalize_family_slug(&sanitize_segment(segments[0])),
        segments[1].to_string(),
        segments[2].to_string(),
    ))
}

fn canonicalize_family_slug(value: &str) -> String {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace("--", "-");
    match normalized.as_str() {
        "qwen-3-5" | "qwen3-5" | "qwen3.5" | "qwen35" => "qwen-3.5".to_string(),
        "step-3-5" | "step3-5" | "step3.5" => "step-3.5".to_string(),
        _ => normalized,
    }
}

fn canonicalize_model_ref_layout(model_ref: &str) -> String {
    let prefix = "/models/";
    if !model_ref.starts_with(prefix) {
        return model_ref.to_string();
    }
    let segments = model_ref[prefix.len()..]
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return model_ref.to_string();
    }
    if segments.len() < 3 {
        let family = canonicalize_family_slug(segments[0]);
        if segments.len() == 1 {
            return format!("{prefix}{family}");
        }
        return format!("{prefix}{family}/{}", segments[1..].join("/"));
    }
    let family = canonicalize_family_slug(segments[0]);
    let quant = normalize_quant_slug(segments[2]).unwrap_or_else(|| "GENERAL".to_string());
    let model = normalize_model_name(segments[1], &quant);
    let mut out = vec![family, model, quant];
    out.extend(segments[3..].iter().map(|value| value.to_string()));
    format!("{prefix}{}", out.join("/"))
}

fn normalize_quant_slug(value: &str) -> Option<String> {
    if value.trim().is_empty() {
        return None;
    }
    let raw = value.trim();
    let input = value
        .trim()
        .trim_matches('/')
        .trim_end_matches(".gguf")
        .trim_end_matches(".GGUF");
    let input_key = quant_compare_key(input);
    for token in quant_tokens() {
        if quant_compare_key(token) == input_key {
            return Some((*token).to_string());
        }
    }
    if let Some(normalized) = normalize_quant_like_slug(input) {
        return Some(normalized);
    }
    if raw.ends_with(".gguf") || raw.ends_with(".GGUF") {
        let detected = detect_quant_from_filename(input)?;
        return Some(detected);
    }
    None
}

fn normalize_model_name(value: &str, quant: &str) -> String {
    let mut text = value
        .trim()
        .trim_end_matches(".gguf")
        .to_ascii_lowercase()
        .replace([' ', '/'], "-");
    while text.contains("--") {
        text = text.replace("--", "-");
    }
    let quant_variants = quant_suffix_variants(quant);
    for variant in quant_variants {
        for sep in ["-", "_", "."] {
            let suffix = format!("{sep}{variant}");
            if text.ends_with(&suffix) {
                text = text.trim_end_matches(&suffix).to_string();
            }
        }
    }
    if text.ends_with("-gguf") {
        text = text.trim_end_matches("-gguf").to_string();
    }
    text.trim_matches('-').to_string()
}

fn infer_model_name_from_gguf_filename(file_name: &str, quant: &str) -> String {
    let mut stem = file_name
        .trim_end_matches(".gguf")
        .trim_end_matches(".GGUF")
        .to_string();
    if let Some((base, _)) = stem.split_once("-00001-of-") {
        stem = base.to_string();
    } else if let Some((base, _)) = stem.split_once("-00002-of-") {
        stem = base.to_string();
    } else if let Some((base, _)) = stem.split_once("-00003-of-") {
        stem = base.to_string();
    }
    normalize_model_name(&stem, quant)
}

fn quant_tokens() -> &'static [&'static str] {
    &[
        "UD-Q4_K_XL",
        "UD-Q4_K_M",
        "UD-IQ4_XS",
        "UD-IQ4_NL",
        "IQ4_XS",
        "IQ4_NL",
        "IQ3_M",
        "IQ3_S",
        "IQ2_XS",
        "IQ2_XXS",
        "Q8_0",
        "Q8_1",
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
        "Q4_0",
        "F32",
        "F16",
        "BF16",
    ]
}

fn quant_compare_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

fn detect_quant_like_slug_from_text(value: &str) -> Option<String> {
    let stem = value
        .trim()
        .trim_end_matches(".gguf")
        .trim_end_matches(".GGUF");
    let stem = strip_shard_suffix(stem);
    let mut best: Option<String> = None;
    for (idx, ch) in stem.char_indices() {
        if idx != 0 && !matches!(ch, '-' | '_' | '.' | ' ') {
            continue;
        }
        let start = if idx == 0 { idx } else { idx + ch.len_utf8() };
        let Some(candidate) = stem.get(start..) else {
            continue;
        };
        if let Some(normalized) = normalize_quant_like_slug(candidate) {
            if best
                .as_ref()
                .is_none_or(|current| normalized.len() > current.len())
            {
                best = Some(normalized);
            }
        }
    }
    best
}

fn strip_shard_suffix(value: &str) -> &str {
    for marker in ["-00001-of-", "-00002-of-", "-00003-of-"] {
        if let Some((base, _)) = value.split_once(marker) {
            return base;
        }
    }
    value
}

fn normalize_quant_like_slug(value: &str) -> Option<String> {
    let mut upper = value.trim().trim_matches('/').to_ascii_uppercase();
    upper = upper.replace('-', "_");
    if matches!(upper.as_str(), "F16" | "BF16" | "F32") {
        return Some(upper);
    }

    let (has_ud_prefix, core) = if let Some(rest) = upper.strip_prefix("UD_") {
        (true, rest)
    } else if let Some(rest) = upper.strip_prefix("UD") {
        if rest.starts_with('Q') || rest.starts_with("IQ") {
            (true, rest)
        } else {
            (false, upper.as_str())
        }
    } else {
        (false, upper.as_str())
    };

    let normalized_core = normalize_quant_core(core)?;
    if has_ud_prefix {
        Some(format!("UD-{normalized_core}"))
    } else {
        Some(normalized_core)
    }
}

fn normalize_quant_core(value: &str) -> Option<String> {
    let (prefix, rest) = value
        .strip_prefix("IQ")
        .map(|rest| ("IQ", rest))
        .or_else(|| value.strip_prefix('Q').map(|rest| ("Q", rest)))?;

    let digit_count = rest.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }
    let (digits, suffix) = rest.split_at(digit_count);
    if suffix.is_empty() {
        return Some(format!("{prefix}{digits}"));
    }
    if !suffix.starts_with('_') {
        return None;
    }

    let groups = suffix
        .trim_start_matches('_')
        .split('_')
        .collect::<Vec<_>>();
    if groups.is_empty()
        || groups
            .iter()
            .any(|group| group.is_empty() || !is_quant_suffix_component(group))
    {
        return None;
    }
    Some(format!("{prefix}{digits}_{}", groups.join("_")))
}

fn is_quant_suffix_component(value: &str) -> bool {
    matches!(
        value,
        "K" | "S" | "M" | "L" | "XL" | "XS" | "XXS" | "NL" | "P" | "0" | "1"
    )
}

fn quant_suffix_variants(quant: &str) -> Vec<String> {
    let upper = quant.to_ascii_uppercase();
    let mut variants = vec![upper.clone(), upper.to_ascii_lowercase()];
    // underscore-normalized
    variants.push(upper.replace('-', "_").to_ascii_lowercase());
    variants.push(upper.replace('-', "_").to_ascii_uppercase());
    // hyphen-normalized (covers iq4-xs style)
    variants.push(upper.replace('_', "-").to_ascii_lowercase());
    variants.push(upper.replace('_', "-").to_ascii_uppercase());
    // concatenated form
    variants.push(upper.replace(['-', '_'], "").to_ascii_lowercase());
    variants
}

pub fn strip_quant_suffix_from_name(name: &str, quant: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let upper = quant.to_ascii_uppercase();
    let hyphen = upper.replace('_', "-").to_ascii_lowercase();
    let underscore = upper.replace('-', "_").to_ascii_lowercase();
    let compact = upper.replace(['-', '_'], "").to_ascii_lowercase();
    let mut candidates = vec![
        hyphen.clone(),
        underscore.clone(),
        compact.clone(),
        upper.to_ascii_lowercase(),
    ];
    if hyphen.starts_with("ud-") {
        candidates.push(hyphen.trim_start_matches("ud-").to_string());
    }
    for cand in candidates {
        let hyphen_cand = format!("-{}", cand);
        if lower.ends_with(&hyphen_cand) {
            if let Some(idx) = lower.rfind(&hyphen_cand) {
                return name[..idx].to_string();
            }
        }
    }
    name.to_string()
}

fn derive_instance_role(instance_name: &str, model_name: &str) -> String {
    let name = sanitize_segment(instance_name);
    let model = sanitize_segment(model_name);
    if let Some(suffix) = name.strip_prefix(&format!("{model}-")) {
        if !suffix.is_empty() {
            return suffix.to_string();
        }
    }
    if name.ends_with("-coding") {
        return "coding".to_string();
    }
    if name.ends_with("-general") {
        return "general".to_string();
    }
    if name.ends_with("-no-thinking") {
        return "no-thinking".to_string();
    }
    "general".to_string()
}

fn derive_instance_variant_from_path(
    paths: &WorkspacePaths,
    instance_path: &Path,
) -> Option<String> {
    let rel = instance_path.strip_prefix(&paths.instances_dir).ok()?;
    let segments = rel
        .iter()
        .map(|v| v.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if segments.len() < 4 {
        return None;
    }
    Some(segments[3].clone())
}

fn collect_model_roots(path: &Path, roots: &mut Vec<PathBuf>) -> AppResult<bool> {
    let mut has_direct_gguf = false;
    let mut has_nested_model = false;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let current = entry.path();
        if current.is_dir() {
            if collect_model_roots(&current, roots)? {
                has_nested_model = true;
            }
            continue;
        }
        if current.extension().and_then(|v| v.to_str()) == Some("gguf") {
            has_direct_gguf = true;
        }
    }
    if has_direct_gguf {
        roots.push(path.to_path_buf());
        return Ok(true);
    }
    Ok(has_nested_model)
}

fn collect_gguf_names(path: &Path, out: &mut Vec<String>) -> AppResult<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let current = entry.path();
        if current.is_dir() {
            collect_gguf_names(&current, out)?;
            continue;
        }
        if current.extension().and_then(|v| v.to_str()) == Some("gguf") {
            if let Some(name) = current.file_name().and_then(|v| v.to_str()) {
                out.push(name.to_string());
            }
        }
    }
    Ok(())
}

fn move_directory_merge(from: &Path, to: &Path, warnings: &mut Vec<String>) -> AppResult<()> {
    if from == to {
        return Ok(());
    }
    if to.starts_with(from) {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let source = entry.path();
            if source == to {
                continue;
            }
            let target = to.join(entry.file_name());
            if target.exists() {
                warnings.push(format!(
                    "model move skipped existing target: {}",
                    target.display()
                ));
                continue;
            }
            fs::rename(&source, &target)?;
        }
        return Ok(());
    }
    if !to.exists() {
        fs::rename(from, to)?;
        return Ok(());
    }
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if target.exists() {
            warnings.push(format!(
                "model move skipped existing target: {}",
                target.display()
            ));
            continue;
        }
        fs::rename(&source, &target)?;
    }
    let _ = fs::remove_dir(from);
    Ok(())
}

fn cleanup_alias_directories(
    paths: &WorkspacePaths,
    report: &mut LayoutMigrationReport,
) -> AppResult<()> {
    let aliases = [
        paths.instances_dir.join("qwen-3-5"),
        paths.instances_dir.join("step-3-5"),
        paths.models_dir.join("qwen-3-5"),
        paths.models_dir.join("step-3-5"),
    ];
    for alias in aliases {
        if !alias.exists() {
            continue;
        }
        remove_empty_tree(&alias)?;
        if alias.exists() {
            report.warnings.push(format!(
                "alias directory still has content, kept as-is: {}",
                alias.display()
            ));
        }
    }
    remove_empty_tree(&paths.instances_dir)?;
    remove_empty_tree(&paths.models_dir)?;
    Ok(())
}

fn remove_empty_tree(path: &Path) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            remove_empty_tree(&child)?;
        }
    }
    if fs::read_dir(path)?.next().is_none() {
        fs::remove_dir(path)?;
    }
    Ok(())
}

fn is_empty_tree(path: &Path) -> AppResult<bool> {
    if !path.exists() {
        return Ok(true);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            if !is_empty_tree(&child)? {
                return Ok(false);
            }
        } else {
            return Ok(false);
        }
    }
    Ok(true)
}

fn simulate_cleanup_alias_directories(
    paths: &WorkspacePaths,
    report: &mut LayoutMigrationReport,
) -> AppResult<()> {
    let aliases = [
        paths.instances_dir.join("qwen-3-5"),
        paths.instances_dir.join("step-3-5"),
        paths.models_dir.join("qwen-3-5"),
        paths.models_dir.join("step-3-5"),
    ];
    for alias in aliases {
        if !alias.exists() {
            continue;
        }
        let empty = is_empty_tree(&alias)?;
        if empty {
            report.warnings.push(format!(
                "alias directory would be removed: {}",
                alias.display()
            ));
        } else {
            report.warnings.push(format!(
                "alias directory still has content and would be kept: {}",
                alias.display()
            ));
        }
    }

    if is_empty_tree(&paths.instances_dir)? {
        report.warnings.push(format!(
            "instances dir '{}' would be removed (empty)",
            paths.instances_dir.display()
        ));
    }
    if is_empty_tree(&paths.models_dir)? {
        report.warnings.push(format!(
            "models dir '{}' would be removed (empty)",
            paths.models_dir.display()
        ));
    }
    Ok(())
}

fn simulate_migrate_models_layout(
    paths: &WorkspacePaths,
    report: &mut LayoutMigrationReport,
) -> AppResult<HashMap<String, String>> {
    let mut remap = HashMap::new();
    if !paths.models_dir.exists() {
        return Ok(remap);
    }
    let mut roots = Vec::new();
    collect_model_roots(&paths.models_dir, &mut roots)?;
    for model_dir in roots {
        let rel = match model_dir.strip_prefix(&paths.models_dir) {
            Ok(path) => path.to_path_buf(),
            Err(_) => continue,
        };
        let rel_segments = rel
            .iter()
            .map(|v| v.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        if rel_segments.is_empty() {
            continue;
        }

        let mut gguf_names = Vec::new();
        collect_gguf_names(&model_dir, &mut gguf_names)?;
        let quant_from_second = rel_segments
            .get(1)
            .and_then(|value| normalize_quant_slug(value));
        let quant = rel_segments
            .get(2)
            .and_then(|value| normalize_quant_slug(value))
            .or_else(|| quant_from_second.clone())
            .or_else(|| {
                gguf_names
                    .iter()
                    .find_map(|name| detect_quant_from_filename(name))
            })
            .unwrap_or_else(|| "GENERAL".to_string());

        let family_seed = rel_segments
            .first()
            .cloned()
            .unwrap_or_else(|| "family".to_string());
        let family = infer_family_slug_from_name(&family_seed);

        let model_segment = rel_segments
            .get(1)
            .cloned()
            .unwrap_or_else(|| rel_segments[0].clone());
        let generic_model_segment = normalize_model_name(&model_segment, &quant) == family;
        if rel_segments.len() >= 3 && generic_model_segment {
            let mut inferred_models = Vec::<String>::new();
            for name in &gguf_names {
                if name.to_ascii_lowercase().contains("mmproj") {
                    continue;
                }
                let inferred = infer_model_name_from_gguf_filename(name, &quant);
                if inferred.is_empty()
                    || inferred_models.iter().any(|existing| existing == &inferred)
                {
                    continue;
                }
                inferred_models.push(inferred);
            }
            if inferred_models.len() > 1 {
                for name in &gguf_names {
                    if name.to_ascii_lowercase().contains("mmproj") {
                        continue;
                    }
                    let inferred = infer_model_name_from_gguf_filename(name, &quant);
                    if inferred.is_empty() {
                        continue;
                    }
                    let source = model_dir.join(name);
                    if !source.exists() {
                        continue;
                    }
                    let target_dir = paths.models_dir.join(&family).join(&inferred).join(&quant);
                    let target = target_dir.join(name);
                    if target.exists() {
                        report.warnings.push(format!(
                            "model move skipped existing target: {}",
                            target.display()
                        ));
                        continue;
                    }
                    report.model_moves.push(LayoutMove {
                        from: source.display().to_string(),
                        to: target.display().to_string(),
                    });
                    let old_rel = source
                        .strip_prefix(&paths.models_dir)
                        .ok()
                        .map(|v| v.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let new_rel = target
                        .strip_prefix(&paths.models_dir)
                        .ok()
                        .map(|v| v.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !old_rel.is_empty() && !new_rel.is_empty() {
                        remap.insert(
                            format!("/models/{}", old_rel.trim_start_matches('/')),
                            format!("/models/{}", new_rel.trim_start_matches('/')),
                        );
                    }
                }
                continue;
            }
        }

        let model_from_file = gguf_names
            .iter()
            .find(|name| !name.to_ascii_lowercase().contains("mmproj"))
            .map(|name| infer_model_name_from_gguf_filename(name, &quant))
            .filter(|name| !name.is_empty());
        let mut model_seed = if rel_segments.len() >= 2 && quant_from_second.is_some() {
            rel_segments[0].clone()
        } else {
            rel_segments
                .get(1)
                .cloned()
                .unwrap_or_else(|| rel_segments[0].clone())
        };
        if normalize_model_name(&model_seed, &quant) == canonicalize_family_slug(&family_seed)
            || model_seed.eq_ignore_ascii_case("general")
            || model_seed.eq_ignore_ascii_case("embedding")
            || model_seed.eq_ignore_ascii_case("reranker")
        {
            if let Some(file_model) = model_from_file {
                model_seed = file_model;
            }
        }
        let mut model = normalize_model_name(&model_seed, &quant);
        if model.is_empty() {
            model = sanitize_segment(&model_seed);
        }

        let target_dir = paths.models_dir.join(&family).join(&model).join(&quant);
        if target_dir == model_dir {
            continue;
        }

        // simulate move_directory_merge behavior
        if target_dir.starts_with(&model_dir) {
            for entry in fs::read_dir(&model_dir)? {
                let entry = entry?;
                let source = entry.path();
                if source == target_dir {
                    continue;
                }
                let target = target_dir.join(entry.file_name());
                if target.exists() {
                    report.warnings.push(format!(
                        "model move skipped existing target: {}",
                        target.display()
                    ));
                    continue;
                }
                report.model_moves.push(LayoutMove {
                    from: source.display().to_string(),
                    to: target.display().to_string(),
                });
            }
            remap.insert(
                format!("/models/{}", rel.to_string_lossy().trim_start_matches('/')),
                format!(
                    "/models/{}/{}/{}",
                    family.trim_start_matches('/'),
                    model.trim_start_matches('/'),
                    quant.trim_start_matches('/')
                ),
            );
            continue;
        }

        if !target_dir.exists() {
            report.model_moves.push(LayoutMove {
                from: model_dir.display().to_string(),
                to: target_dir.display().to_string(),
            });
            remap.insert(
                format!("/models/{}", rel.to_string_lossy().trim_start_matches('/')),
                format!(
                    "/models/{}/{}/{}",
                    family.trim_start_matches('/'),
                    model.trim_start_matches('/'),
                    quant.trim_start_matches('/')
                ),
            );
            continue;
        }

        for entry in fs::read_dir(&model_dir)? {
            let entry = entry?;
            let source = entry.path();
            let target = target_dir.join(entry.file_name());
            if target.exists() {
                report.warnings.push(format!(
                    "model move skipped existing target: {}",
                    target.display()
                ));
                continue;
            }
            report.model_moves.push(LayoutMove {
                from: source.display().to_string(),
                to: target.display().to_string(),
            });
        }
        remap.insert(
            format!("/models/{}", rel.to_string_lossy().trim_start_matches('/')),
            format!(
                "/models/{}/{}/{}",
                family.trim_start_matches('/'),
                model.trim_start_matches('/'),
                quant.trim_start_matches('/')
            ),
        );
    }

    Ok(remap)
}

fn simulate_migrate_instances_layout(
    paths: &WorkspacePaths,
    model_prefix_map: &HashMap<String, String>,
    report: &mut LayoutMigrationReport,
) -> AppResult<()> {
    let instances = store::discover_instances(paths)?;
    for instance in instances {
        let mut config = instance.config.clone();
        let mut changed = false;
        let mut moved = false;

        if let Some(rewritten) = rewrite_model_reference(&config.model, model_prefix_map) {
            if rewritten != config.model {
                config.model = rewritten;
                changed = true;
            }
        }
        if let Some(mmproj) = config.mmproj.as_ref() {
            if let Some(rewritten) = rewrite_model_reference(mmproj, model_prefix_map) {
                if &rewritten != mmproj {
                    config.mmproj = Some(rewritten);
                    changed = true;
                }
            }
        }
        if let Some(draft_model) = config.draft_model.as_ref() {
            if let Some(rewritten) = rewrite_model_reference(draft_model, model_prefix_map) {
                if &rewritten != draft_model {
                    config.draft_model = Some(rewritten);
                    changed = true;
                }
            }
        }
        if config.container_name.as_deref() != Some(&instance.name) {
            config.container_name = Some(instance.name.clone());
            changed = true;
        }
        let (family, mut model_name, quant) = model_ref_to_layout_parts(&config.model)
            .map(|(family, model, quant)| {
                (
                    canonicalize_family_slug(&family),
                    normalize_model_name(&model, &quant),
                    normalize_quant_slug(&quant).unwrap_or_else(|| "GENERAL".to_string()),
                )
            })
            .unwrap_or_else(|| {
                (
                    infer_family_slug_from_name(&instance.name),
                    normalize_model_name(&instance.name, "GENERAL"),
                    "GENERAL".to_string(),
                )
            });
        if model_name.is_empty() {
            model_name = sanitize_segment(&instance.name);
        }
        if (model_name == family || model_name == "general")
            && config.model.to_ascii_lowercase().ends_with(".gguf")
        {
            if let Some(file_name) = config.model.rsplit('/').next() {
                let inferred = infer_model_name_from_gguf_filename(file_name, &quant);
                if !inferred.is_empty() {
                    model_name = inferred;
                }
            }
        }
        let role = derive_instance_role(&instance.name, &model_name);
        let target_dir = paths
            .instances_dir
            .join(&family)
            .join(&model_name)
            .join(&quant)
            .join(&role);

        let current_dir = instance.path.clone();
        if current_dir != target_dir {
            if target_dir.exists() {
                let fallback = target_dir.join(sanitize_segment(&instance.name));
                if fallback.exists() {
                    report.warnings.push(format!(
                        "instance move skipped (both target and fallback exist): {}",
                        fallback.display()
                    ));
                } else {
                    report.instance_moves.push(LayoutMove {
                        from: current_dir.display().to_string(),
                        to: fallback.display().to_string(),
                    });
                    moved = true;
                }
            } else {
                report.instance_moves.push(LayoutMove {
                    from: current_dir.display().to_string(),
                    to: target_dir.display().to_string(),
                });
                moved = true;
            }
        }

        if changed || moved {
            report.updated_instances.push(instance.name.clone());
        }
    }
    Ok(())
}

impl AppService {
    pub async fn migrate_workspace_layout_dry_run(&self) -> AppResult<LayoutMigrationReport> {
        let mut report = LayoutMigrationReport {
            stopped_instances: Vec::new(),
            restarted_instances: Vec::new(),
            model_moves: Vec::new(),
            instance_moves: Vec::new(),
            updated_instances: Vec::new(),
            warnings: Vec::new(),
        };

        let statuses = self.all_instances_status().await?;
        let running_instances = statuses
            .iter()
            .filter(|status| {
                let lower = status.status.to_ascii_lowercase();
                lower.contains("running") || lower.contains("up")
            })
            .map(|status| status.name.clone())
            .collect::<Vec<_>>();

        // simulate stops
        report.stopped_instances = running_instances.clone();

        let model_prefix_map = simulate_migrate_models_layout(&self.paths, &mut report)?;
        simulate_migrate_instances_layout(&self.paths, &model_prefix_map, &mut report)?;
        simulate_cleanup_alias_directories(&self.paths, &mut report)?;

        // simulate restarts
        report.restarted_instances = running_instances.clone();
        report
            .warnings
            .push("dry-run: no filesystem changes or docker actions performed.".to_string());

        Ok(report)
    }
}

fn estimate_instance_memory_preview(
    paths: &WorkspacePaths,
    instance: &Instance,
) -> InstanceMemoryPreview {
    estimate_memory_preview_with_details(paths, &instance.name, &instance.config).preview
}

#[derive(Clone, Debug)]
struct KvEstimateComputation {
    architecture_raw: String,
    architecture_profile: MemoryArchitectureProfile,
    layers: u64,
    full_context_layers: u64,
    reduced_context_layers: u64,
    reduced_context_size: Option<u32>,
    embedding_length: u64,
    head_count_query: u64,
    head_count_kv: u64,
    context_size_per_slot: u32,
    bytes_per_element_k: f64,
    bytes_per_element_v: f64,
    per_layer_per_token_bytes: f64,
    per_slot_kv_bytes: u64,
}

fn estimate_memory_preview_with_details(
    paths: &WorkspacePaths,
    name: &str,
    config: &InstanceConfig,
) -> MemoryEstimateDebug {
    let mut warnings = Vec::new();
    let model_path = resolve_model_path(paths, &config.model);
    let gguf_path = match find_primary_gguf_path(&model_path) {
        Ok(path) => path,
        Err(err) => {
            warnings.push(err);
            None
        }
    };

    let mut architecture = None;
    let mut model_bytes = match sum_gguf_sizes(&model_path) {
        Ok(size) => size,
        Err(err) => {
            warnings.push(err);
            0
        }
    };
    let mut kv_details = None::<KvEstimateComputation>;

    if let Some(path) = gguf_path.as_ref() {
        match parse_gguf_file(path) {
            Ok(gguf) => {
                architecture = gguf
                    .metadata
                    .get("general.architecture")
                    .and_then(GgufMetadataValue::as_str)
                    .map(ToString::to_string);
                kv_details = estimate_kv_cache_details(&gguf, config, &mut warnings);
            }
            Err(err) => {
                warnings.push(format!("gguf parse failed: {err}"));
            }
        }
    } else {
        warnings.push(format!(
            "model reference '{}' did not resolve to a gguf file",
            config.model
        ));
    }

    if model_bytes == 0 {
        if let Some(path) = gguf_path.as_ref() {
            model_bytes = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            if model_bytes > 0 {
                warnings.push("using primary gguf file size for model bytes".to_string());
            }
        }
    }

    let kv_cache_bytes = kv_details
        .as_ref()
        .map_or(0, |detail| detail.per_slot_kv_bytes);
    let parallel_u32 = config.parallel.unwrap_or(1).max(1);
    let parallel = u64::from(parallel_u32);
    let total_kv_bytes = kv_cache_bytes.saturating_mul(parallel);
    let overhead_bytes =
        estimate_runtime_overhead_bytes(model_bytes.saturating_add(total_kv_bytes));
    let estimated_total_bytes = model_bytes
        .saturating_add(total_kv_bytes)
        .saturating_add(overhead_bytes);
    let warning = if warnings.is_empty() {
        None
    } else {
        Some(warnings.join("; "))
    };

    let details = kv_details.map(|detail| {
        let kv_formula = match detail.architecture_profile {
            MemoryArchitectureProfile::Gemma4 => {
                "Per-slot KV = Σ_layer [ C_slot(layer) * (H_kv(layer)*K_dim(layer)*BytePerElem_k + H_kv(layer)*V_dim(layer)*BytePerElem_v) ]"
            }
            MemoryArchitectureProfile::Qwen35 | MemoryArchitectureProfile::Qwen3 => {
                "Per-slot KV = L_effective(25% of L)*per_layer_per_token*C_slot"
            }
            MemoryArchitectureProfile::Llama | MemoryArchitectureProfile::Standard => {
                "Per-slot KV = L*per_layer_per_token*C_slot"
            }
        };
        MemoryEstimateDetails {
            architecture_raw: detail.architecture_raw,
            architecture_profile: detail.architecture_profile.as_str().to_string(),
            layers: detail.layers,
            full_context_layers: detail.full_context_layers,
            reduced_context_layers: detail.reduced_context_layers,
            reduced_context_size: detail.reduced_context_size,
            embedding_length: detail.embedding_length,
            head_count_query: detail.head_count_query,
            head_count_kv: detail.head_count_kv,
            context_size: config.ctx_size,
            context_size_per_slot: detail.context_size_per_slot,
            parallel: parallel_u32,
            cache_type_k: config.cache_type_k.clone(),
            cache_type_v: config.cache_type_v.clone(),
            bytes_per_element_k: detail.bytes_per_element_k,
            bytes_per_element_v: detail.bytes_per_element_v,
            per_layer_per_token_bytes: detail.per_layer_per_token_bytes,
            per_slot_kv_bytes: detail.per_slot_kv_bytes,
            total_kv_bytes,
            model_bytes,
            overhead_bytes,
            estimated_total_bytes,
            formulas: vec![
                "Weight Memory = Model_Size_Bytes".to_string(),
                "C_slot = floor(Context_Size / parallel), minimum 1".to_string(),
                "Per-layer-per-token (generic) = H_kv * (E / H_q) * (BytePerElem_k + BytePerElem_v)".to_string(),
                kv_formula.to_string(),
                "Total Memory = Model_Size + (Per-slot KV * parallel) + 5% overhead".to_string(),
            ],
        }
    });

    let preview = InstanceMemoryPreview {
        name: name.to_string(),
        model_ref: config.model.clone(),
        gguf_path: gguf_path.map(|path| path.display().to_string()),
        architecture,
        model_bytes,
        kv_cache_bytes,
        overhead_bytes,
        estimated_total_bytes,
        context_size: config.ctx_size,
        parallel: parallel_u32,
        cache_type_k: config.cache_type_k.clone(),
        cache_type_v: config.cache_type_v.clone(),
        warning,
    };

    MemoryEstimateDebug { preview, details }
}

fn estimate_kv_cache_details(
    gguf: &GgufFile,
    config: &InstanceConfig,
    warnings: &mut Vec<String>,
) -> Option<KvEstimateComputation> {
    let metadata = &gguf.metadata;
    let architecture = metadata
        .get("general.architecture")
        .and_then(GgufMetadataValue::as_str)
        .unwrap_or("llama")
        .to_string();
    let profile = detect_architecture_profile(&architecture);

    let block_count = metadata_first_u64(
        metadata,
        &[
            format!("{}.block_count", architecture),
            "llama.block_count".to_string(),
        ],
    )
    .or_else(|| infer_block_count_from_tensors(&gguf.tensors));

    let embedding_length = metadata_first_u64(
        metadata,
        &[
            format!("{}.embedding_length", architecture),
            "llama.embedding_length".to_string(),
        ],
    );

    let key_length = metadata_first_u64(
        metadata,
        &[
            format!("{}.attention.key_length", architecture),
            "llama.attention.key_length".to_string(),
        ],
    );
    let value_length = metadata_first_u64(
        metadata,
        &[
            format!("{}.attention.value_length", architecture),
            "llama.attention.value_length".to_string(),
        ],
    );
    let key_length_swa = metadata_first_u64(
        metadata,
        &[format!("{}.attention.key_length_swa", architecture)],
    );
    let value_length_swa = metadata_first_u64(
        metadata,
        &[format!("{}.attention.value_length_swa", architecture)],
    );
    let sliding_window = metadata_first_u64(
        metadata,
        &[format!("{}.attention.sliding_window", architecture)],
    )
    .unwrap_or(2048) as u32;

    let head_count_q_keys = vec![
        format!("{}.attention.head_count", architecture),
        "llama.attention.head_count".to_string(),
    ];
    let head_count_q = metadata_first_u64(metadata, &head_count_q_keys).or_else(|| {
        metadata_first_array_u64(metadata, &head_count_q_keys)
            .and_then(|values| values.into_iter().max())
    });

    let head_count_kv_keys = vec![
        format!("{}.attention.head_count_kv", architecture),
        "llama.attention.head_count_kv".to_string(),
    ];
    let head_count_kv = metadata_first_u64(metadata, &head_count_kv_keys)
        .or_else(|| {
            metadata_first_array_u64(metadata, &head_count_kv_keys)
                .and_then(|values| values.into_iter().max())
        })
        .or(head_count_q);

    let layers = block_count.unwrap_or_else(|| {
        warnings.push("missing block_count metadata, fallback to 32 layers".to_string());
        32
    });

    let head_q = head_count_q.unwrap_or_else(|| {
        warnings.push("missing attention.head_count metadata, fallback to 32".to_string());
        32
    });

    let head_kv = head_count_kv.unwrap_or(head_q).max(1);

    let embedding_length = embedding_length.unwrap_or_else(|| {
        warnings.push("missing embedding_length metadata, fallback to 4096".to_string());
        4096
    });

    let layers_f = layers as f64;
    let head_kv_f = head_kv as f64;
    let embedding_length_f = embedding_length as f64;
    let head_q_f = head_q as f64;
    let parallel_u32 = config.parallel.unwrap_or(1).max(1);
    let context_per_slot = (config.ctx_size.max(1) / parallel_u32).max(1);
    let context_f = f64::from(context_per_slot);

    let k_type = config.cache_type_k.trim().to_ascii_lowercase();
    let v_type = config.cache_type_v.trim().to_ascii_lowercase();

    let bpk = if k_type == "q8_0" {
        34.0 / 32.0
    } else if k_type == "f16" || k_type == "fp16" || k_type == "float16" {
        2.0
    } else {
        cache_type_bytes_per_element(&config.cache_type_k).unwrap_or_else(|| {
            warnings.push(format!(
                "unknown cache_type_k '{}', fallback to f16",
                config.cache_type_k
            ));
            2.0
        })
    };

    let bpv = if v_type == "q8_0" {
        34.0 / 32.0
    } else if v_type == "f16" || v_type == "fp16" || v_type == "float16" {
        2.0
    } else {
        cache_type_bytes_per_element(&config.cache_type_v).unwrap_or_else(|| {
            warnings.push(format!(
                "unknown cache_type_v '{}', fallback to f16",
                config.cache_type_v
            ));
            2.0
        })
    };

    let (
        head_count_kv_report,
        per_layer_per_token,
        full_context_layers,
        reduced_context_layers,
        reduced_context_size,
        kv_bytes,
    ) = if profile == MemoryArchitectureProfile::Gemma4 {
        let kv_per_layer = metadata_first_array_u64(
            metadata,
            &[format!("{}.attention.head_count_kv", architecture)],
        );
        let swa_pattern = metadata_first_array_bool(
            metadata,
            &[format!("{}.attention.sliding_window_pattern", architecture)],
        );

        if let (Some(kv_per_layer), Some(swa_pattern)) = (kv_per_layer, swa_pattern) {
            if kv_per_layer.len() == layers as usize && swa_pattern.len() == layers as usize {
                let fallback_head_dim = (embedding_length / head_q).max(1);
                let key_full = key_length.unwrap_or(fallback_head_dim).max(1);
                let value_full = value_length.unwrap_or(key_full).max(1);
                let key_swa = key_length_swa.unwrap_or(key_full).max(1);
                let value_swa = value_length_swa.unwrap_or(value_full).max(1);
                let mut kv_total = 0.0_f64;
                let mut full_layers = 0_u64;
                let mut reduced_layers = 0_u64;
                let mut head_kv_max = 1_u64;
                let mut weighted_cells = 0.0_f64;
                for idx in 0..layers as usize {
                    let layer_head_kv = kv_per_layer[idx].max(1);
                    head_kv_max = head_kv_max.max(layer_head_kv);
                    let is_swa = swa_pattern[idx];
                    let (ctx_layer, key_dim, value_dim) = if is_swa {
                        reduced_layers += 1;
                        (f64::from(sliding_window), key_swa as f64, value_swa as f64)
                    } else {
                        full_layers += 1;
                        (context_f, key_full as f64, value_full as f64)
                    };
                    weighted_cells += ctx_layer;
                    kv_total += ctx_layer
                        * ((layer_head_kv as f64) * key_dim * bpk
                            + (layer_head_kv as f64) * value_dim * bpv);
                }
                let avg_per_layer_per_token = if weighted_cells > 0.0 {
                    kv_total / weighted_cells
                } else {
                    0.0
                };
                (
                    head_kv_max,
                    avg_per_layer_per_token,
                    full_layers,
                    reduced_layers,
                    Some(sliding_window),
                    kv_total,
                )
            } else {
                warnings.push(format!(
                    "gemma4 per-layer metadata length mismatch (head_count_kv={}, sliding_pattern={}, layers={}), using heuristic split",
                    kv_per_layer.len(),
                    swa_pattern.len(),
                    layers
                ));
                let mut global_layers = ((layers as f64) * 0.2).round() as u64;
                if global_layers == 0 {
                    global_layers = 1;
                }
                if global_layers > layers {
                    global_layers = layers;
                }
                let sliding_layers = layers.saturating_sub(global_layers);
                let heuristic = head_kv_f * (embedding_length_f / head_q_f) * (bpk + bpv);
                (
                    head_kv,
                    heuristic,
                    global_layers,
                    sliding_layers,
                    Some(2048),
                    (global_layers as f64) * heuristic * context_f
                        + (sliding_layers as f64) * heuristic * 2048.0,
                )
            }
        } else {
            warnings
                .push("gemma4 per-layer metadata missing, using heuristic layer split".to_string());
            let mut global_layers = ((layers as f64) * 0.2).round() as u64;
            if global_layers == 0 {
                global_layers = 1;
            }
            if global_layers > layers {
                global_layers = layers;
            }
            let sliding_layers = layers.saturating_sub(global_layers);
            let heuristic = head_kv_f * (embedding_length_f / head_q_f) * (bpk + bpv);
            (
                head_kv,
                heuristic,
                global_layers,
                sliding_layers,
                Some(2048),
                (global_layers as f64) * heuristic * context_f
                    + (sliding_layers as f64) * heuristic * 2048.0,
            )
        }
    } else {
        let heuristic = head_kv_f * (embedding_length_f / head_q_f) * (bpk + bpv);
        match profile {
            MemoryArchitectureProfile::Qwen35 | MemoryArchitectureProfile::Qwen3 => {
                let mut effective_layers = ((layers as f64) * 0.25).round() as u64;
                if effective_layers == 0 {
                    effective_layers = 1;
                }
                if effective_layers > layers {
                    effective_layers = layers;
                }
                let excluded_layers = layers.saturating_sub(effective_layers);
                (
                    head_kv,
                    heuristic,
                    effective_layers,
                    excluded_layers,
                    None,
                    (effective_layers as f64) * heuristic * context_f,
                )
            }
            MemoryArchitectureProfile::Llama | MemoryArchitectureProfile::Standard => (
                head_kv,
                heuristic,
                layers,
                0,
                None,
                layers_f * heuristic * context_f,
            ),
            MemoryArchitectureProfile::Gemma4 => unreachable!(),
        }
    };

    if !kv_bytes.is_finite() || kv_bytes <= 0.0 {
        return None;
    }

    Some(KvEstimateComputation {
        architecture_raw: architecture,
        architecture_profile: profile,
        layers,
        full_context_layers,
        reduced_context_layers,
        reduced_context_size,
        embedding_length,
        head_count_query: head_q,
        head_count_kv: head_count_kv_report,
        context_size_per_slot: context_per_slot,
        bytes_per_element_k: bpk,
        bytes_per_element_v: bpv,
        per_layer_per_token_bytes: per_layer_per_token,
        per_slot_kv_bytes: kv_bytes.ceil() as u64,
    })
}

fn detect_architecture_profile(architecture: &str) -> MemoryArchitectureProfile {
    let normalized = architecture
        .chars()
        .filter(|value| value.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();

    if normalized.contains("gemma4") {
        MemoryArchitectureProfile::Gemma4
    } else if normalized.contains("qwen35") {
        MemoryArchitectureProfile::Qwen35
    } else if normalized.contains("qwen3") {
        MemoryArchitectureProfile::Qwen3
    } else if normalized.contains("qwen") {
        MemoryArchitectureProfile::Qwen3
    } else if normalized.contains("llama") {
        MemoryArchitectureProfile::Llama
    } else {
        MemoryArchitectureProfile::Standard
    }
}

fn metadata_first_u64(
    metadata: &HashMap<String, GgufMetadataValue>,
    keys: &[String],
) -> Option<u64> {
    for key in keys {
        if let Some(value) = metadata.get(key).and_then(metadata_value_to_u64) {
            return Some(value);
        }
    }
    None
}

fn metadata_first_array_u64(
    metadata: &HashMap<String, GgufMetadataValue>,
    keys: &[String],
) -> Option<Vec<u64>> {
    for key in keys {
        let Some(values) = metadata.get(key).and_then(GgufMetadataValue::as_array) else {
            continue;
        };
        let mut out = Vec::with_capacity(values.len());
        for value in values {
            out.push(metadata_value_to_u64(value)?);
        }
        return Some(out);
    }
    None
}

fn metadata_first_array_bool(
    metadata: &HashMap<String, GgufMetadataValue>,
    keys: &[String],
) -> Option<Vec<bool>> {
    for key in keys {
        let Some(values) = metadata.get(key).and_then(GgufMetadataValue::as_array) else {
            continue;
        };
        let mut out = Vec::with_capacity(values.len());
        for value in values {
            out.push(metadata_value_to_bool(value)?);
        }
        return Some(out);
    }
    None
}

fn metadata_value_to_u64(value: &GgufMetadataValue) -> Option<u64> {
    match value {
        GgufMetadataValue::Uint8(v) => Some(u64::from(*v)),
        GgufMetadataValue::Uint16(v) => Some(u64::from(*v)),
        GgufMetadataValue::Uint32(v) => Some(u64::from(*v)),
        GgufMetadataValue::Uint64(v) => Some(*v),
        GgufMetadataValue::Int8(v) if *v >= 0 => Some(*v as u64),
        GgufMetadataValue::Int16(v) if *v >= 0 => Some(*v as u64),
        GgufMetadataValue::Int32(v) if *v >= 0 => Some(*v as u64),
        GgufMetadataValue::Int64(v) if *v >= 0 => Some(*v as u64),
        GgufMetadataValue::Float32(v) if *v >= 0.0 => Some(*v as u64),
        GgufMetadataValue::Float64(v) if *v >= 0.0 => Some(*v as u64),
        _ => None,
    }
}

fn metadata_value_to_bool(value: &GgufMetadataValue) -> Option<bool> {
    match value {
        GgufMetadataValue::Bool(value) => Some(*value),
        _ => None,
    }
}

fn cache_type_bytes_per_element(cache_type: &str) -> Option<f64> {
    let dtype = match cache_type
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .as_str()
    {
        "f16" | "fp16" | "float16" => Some(GgufDtype::F16),
        "bf16" => Some(GgufDtype::BF16),
        "f32" | "fp32" | "float32" => Some(GgufDtype::F32),
        "q4_0" => Some(GgufDtype::Q4_0),
        "q4_1" => Some(GgufDtype::Q4_1),
        "q5_0" => Some(GgufDtype::Q5_0),
        "q5_1" => Some(GgufDtype::Q5_1),
        "q8_0" => Some(GgufDtype::Q8_0),
        "q8_1" => Some(GgufDtype::Q8_1),
        "q4k" | "q4_k" => Some(GgufDtype::Q4K),
        "q5k" | "q5_k" => Some(GgufDtype::Q5K),
        "q6k" | "q6_k" => Some(GgufDtype::Q6K),
        "q8k" | "q8_k" => Some(GgufDtype::Q8K),
        "iq2_xxs" | "iq2xxs" => Some(GgufDtype::IQ2XXS),
        "iq2_xs" | "iq2xs" => Some(GgufDtype::IQ2XS),
        "iq2_s" | "iq2s" => Some(GgufDtype::IQ2S),
        "iq3_xxs" | "iq3xxs" => Some(GgufDtype::IQ3XXS),
        "iq3_s" | "iq3s" => Some(GgufDtype::IQ3S),
        "iq1_s" | "iq1s" => Some(GgufDtype::IQ1S),
        "iq1_m" | "iq1m" => Some(GgufDtype::IQ1M),
        "iq4_nl" | "iq4nl" => Some(GgufDtype::IQ4NL),
        "iq4_xs" | "iq4xs" => Some(GgufDtype::IQ4XS),
        _ => None,
    }?;
    Some(dtype.type_size() as f64 / dtype.block_size() as f64)
}

fn estimate_runtime_overhead_bytes(base_bytes: u64) -> u64 {
    // Simple 5% runtime overhead estimate as requested
    ((base_bytes as f64) * 0.05).round() as u64
}

fn resolve_model_path(paths: &WorkspacePaths, model_ref: &str) -> PathBuf {
    if let Some(stripped) = model_ref.strip_prefix("/models/") {
        return paths.models_dir.join(stripped.trim_start_matches('/'));
    }
    if let Some(stripped) = model_ref.strip_prefix("models/") {
        return paths.models_dir.join(stripped.trim_start_matches('/'));
    }
    let path = PathBuf::from(model_ref);
    if path.is_absolute() {
        path
    } else {
        paths.root.join(path)
    }
}

fn find_primary_gguf_path(path: &Path) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    if path.is_file() {
        if path.extension().and_then(|ext| ext.to_str()) == Some("gguf") {
            return Ok(Some(path.to_path_buf()));
        }
        return Ok(None);
    }
    let mut files = Vec::new();
    collect_gguf_files(path, &mut files).map_err(|err| {
        format!(
            "failed to walk model directory '{}' for gguf files: {err}",
            path.display()
        )
    })?;
    let mut preferred = files
        .iter()
        .filter(|candidate| {
            candidate
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| !is_mmproj_filename(name))
        })
        .cloned()
        .collect::<Vec<_>>();
    if preferred.is_empty() {
        preferred = files;
    }
    Ok(preferred.into_iter().max_by_key(|candidate| {
        let hint = gguf_primary_hint_score(candidate);
        let size = fs::metadata(candidate).map(|meta| meta.len()).unwrap_or(0);
        (hint, size)
    }))
}

fn gguf_primary_hint_score(path: &Path) -> u8 {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.contains("-00001-of-") || name.contains("_00001-of-") {
        return 3;
    }
    if name.contains(".i1-") || name.contains(".part1") || name.contains("-part1") {
        return 2;
    }
    1
}

fn collect_gguf_files(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let current = entry.path();
        if current.is_dir() {
            collect_gguf_files(&current, out)?;
            continue;
        }
        if current.extension().and_then(|ext| ext.to_str()) == Some("gguf") {
            out.push(current);
        }
    }
    Ok(())
}

fn sum_gguf_sizes(path: &Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    if path.is_file() {
        if let Some(total) = sum_sharded_gguf_file_series(path)? {
            return Ok(total);
        }
        return Ok(fs::metadata(path).map(|meta| meta.len()).unwrap_or(0));
    }
    let mut files = Vec::new();
    collect_gguf_files(path, &mut files).map_err(|err| {
        format!(
            "failed to walk model directory '{}' for gguf size fallback: {err}",
            path.display()
        )
    })?;
    let mut non_mmproj = files
        .iter()
        .filter(|candidate| {
            candidate
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| !is_mmproj_filename(name))
        })
        .collect::<Vec<_>>();
    if non_mmproj.is_empty() {
        non_mmproj = files.iter().collect::<Vec<_>>();
    }
    Ok(non_mmproj
        .iter()
        .filter_map(|candidate| fs::metadata(candidate).ok().map(|meta| meta.len()))
        .sum())
}

fn sum_sharded_gguf_file_series(path: &Path) -> Result<Option<u64>, String> {
    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        return Ok(None);
    };
    let Some((prefix, suffix)) = gguf_shard_name_pattern(file_name) else {
        return Ok(None);
    };
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let mut total = 0u64;
    let mut count = 0u32;
    for entry in fs::read_dir(parent).map_err(|err| {
        format!(
            "failed to walk sharded gguf directory '{}' for size sum: {err}",
            parent.display()
        )
    })? {
        let entry = entry.map_err(|err| {
            format!(
                "failed to read entry in sharded gguf directory '{}': {err}",
                parent.display()
            )
        })?;
        let candidate = entry.path();
        if candidate.extension().and_then(|ext| ext.to_str()) != Some("gguf") {
            continue;
        }
        let Some(name) = candidate.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let name_lc = name.to_ascii_lowercase();
        if name_lc.starts_with(&prefix) && name_lc.ends_with(&suffix) {
            total =
                total.saturating_add(fs::metadata(&candidate).map(|meta| meta.len()).unwrap_or(0));
            count += 1;
        }
    }
    if count > 1 { Ok(Some(total)) } else { Ok(None) }
}

fn gguf_shard_name_pattern(name: &str) -> Option<(String, String)> {
    let lower = name.to_ascii_lowercase();
    let of_pos = lower.find("-of-")?;
    let prefix_pos = lower[..of_pos].rfind("-000")?;
    let prefix = lower[..prefix_pos].to_string();
    let suffix = lower[of_pos + 4..].to_string();
    if prefix.is_empty() || suffix.is_empty() {
        return None;
    }
    Some((prefix, suffix))
}

fn infer_block_count_from_tensors(tensors: &[GgufTensorInfo]) -> Option<u64> {
    let mut max_block = None::<u64>;
    for tensor in tensors {
        let Some(rest) = tensor.name.strip_prefix("blk.") else {
            continue;
        };
        let Some(index_part) = rest.split('.').next() else {
            continue;
        };
        let Ok(index) = index_part.parse::<u64>() else {
            continue;
        };
        max_block = Some(max_block.map_or(index, |value| value.max(index)));
    }
    max_block.map(|value| value + 1)
}

fn apply_edit(root: &mut Value, key: &str, value: Value) -> AppResult<()> {
    let mut current = root;
    let normalized = normalize_config_key_path(key);
    let parts: Vec<&str> = normalized
        .split('.')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(AppError::InvalidInput("empty edit key".to_string()));
    }
    for part in &parts[..parts.len() - 1] {
        let map = current
            .as_object_mut()
            .ok_or_else(|| AppError::InvalidInput(format!("invalid edit path: {key}")))?;
        current = map
            .entry((*part).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    let map = current
        .as_object_mut()
        .ok_or_else(|| AppError::InvalidInput(format!("invalid edit leaf: {key}")))?;
    map.insert(parts[parts.len() - 1].to_string(), value);
    Ok(())
}

fn parse_status_output(name: &str, output: &str) -> InstanceStatus {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return InstanceStatus {
            name: name.to_string(),
            status: "stopped".to_string(),
            ports: None,
            error: None,
            raw: None,
        };
    }
    let parsed = serde_json::from_str::<Value>(trimmed).ok().or_else(|| {
        let lines = trimmed
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            None
        } else {
            Some(Value::Array(lines))
        }
    });
    if let Some(value) = parsed {
        let container = value
            .as_array()
            .and_then(|arr| arr.first().cloned())
            .or_else(|| value.as_object().map(|_| value.clone()));
        if let Some(container) = container {
            let status = container
                .get("State")
                .or_else(|| container.get("state"))
                .or_else(|| container.get("Status"))
                .or_else(|| container.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_lowercase();
            let ports = container
                .get("Ports")
                .or_else(|| container.get("ports"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            return InstanceStatus {
                name: name.to_string(),
                status,
                ports,
                error: None,
                raw: Some(container),
            };
        }
    }
    let lowered = trimmed.to_lowercase();
    let status = if lowered.contains("running") {
        "running"
    } else if lowered.contains("exited") || lowered.contains("stopped") {
        "stopped"
    } else {
        "unknown"
    };
    InstanceStatus {
        name: name.to_string(),
        status: status.to_string(),
        ports: None,
        error: None,
        raw: Some(Value::String(trimmed.to_string())),
    }
}

#[derive(Clone, Copy)]
struct CpuSample {
    idle: u64,
    total: u64,
}

async fn sample_cpu_metrics() -> AppResult<CpuMetrics> {
    let start = read_cpu_sample()?;
    sleep(Duration::from_millis(220)).await;
    let end = read_cpu_sample()?;
    let total_delta = end.total.saturating_sub(start.total) as f64;
    let idle_delta = end.idle.saturating_sub(start.idle) as f64;
    let usage_percent = if total_delta <= f64::EPSILON {
        0.0
    } else {
        ((total_delta - idle_delta) / total_delta * 100.0).clamp(0.0, 100.0)
    };
    let (load_1, load_5, load_15) = read_loadavg().unwrap_or((0.0, 0.0, 0.0));
    let cores = std::thread::available_parallelism()
        .map(|v| v.get())
        .unwrap_or(1);
    Ok(CpuMetrics {
        usage_percent,
        cores,
        load_1,
        load_5,
        load_15,
    })
}

fn read_cpu_sample() -> AppResult<CpuSample> {
    let stat = fs::read_to_string("/proc/stat")?;
    let first = stat
        .lines()
        .next()
        .ok_or_else(|| AppError::InvalidInput("cannot read /proc/stat".to_string()))?;
    let parts = first
        .split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse::<u64>().ok())
        .collect::<Vec<_>>();
    if parts.len() < 4 {
        return Err(AppError::InvalidInput(
            "unexpected /proc/stat cpu format".to_string(),
        ));
    }
    let idle = parts[3] + parts.get(4).copied().unwrap_or(0);
    let total = parts.iter().sum();
    Ok(CpuSample { idle, total })
}

fn read_loadavg() -> AppResult<(f64, f64, f64)> {
    let loadavg = fs::read_to_string("/proc/loadavg")?;
    let mut parts = loadavg.split_whitespace();
    let one = parts
        .next()
        .ok_or_else(|| AppError::InvalidInput("missing loadavg 1m".to_string()))?
        .parse::<f64>()
        .map_err(|_| AppError::InvalidInput("invalid loadavg 1m".to_string()))?;
    let five = parts
        .next()
        .ok_or_else(|| AppError::InvalidInput("missing loadavg 5m".to_string()))?
        .parse::<f64>()
        .map_err(|_| AppError::InvalidInput("invalid loadavg 5m".to_string()))?;
    let fifteen = parts
        .next()
        .ok_or_else(|| AppError::InvalidInput("missing loadavg 15m".to_string()))?
        .parse::<f64>()
        .map_err(|_| AppError::InvalidInput("invalid loadavg 15m".to_string()))?;
    Ok((one, five, fifteen))
}

fn read_ram_metrics() -> AppResult<RamMetrics> {
    let meminfo = fs::read_to_string("/proc/meminfo")?;
    let mut total_kb = 0u64;
    let mut free_kb = 0u64;
    let mut available_kb = 0u64;
    for line in meminfo.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let kb = value
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            match key {
                "MemTotal" => total_kb = kb,
                "MemFree" => free_kb = kb,
                "MemAvailable" => available_kb = kb,
                _ => {}
            }
        }
    }
    if total_kb == 0 {
        return Err(AppError::InvalidInput(
            "MemTotal missing from /proc/meminfo".to_string(),
        ));
    }
    if available_kb == 0 {
        available_kb = free_kb;
    }
    let used_kb = total_kb.saturating_sub(available_kb);
    let usage_percent = ((used_kb as f64 / total_kb as f64) * 100.0).clamp(0.0, 100.0);
    Ok(RamMetrics {
        total_mb: total_kb / 1024,
        used_mb: used_kb / 1024,
        free_mb: free_kb / 1024,
        available_mb: available_kb / 1024,
        usage_percent,
    })
}

async fn collect_rocm_metrics() -> RocmMetrics {
    let args = [
        "--showproductname",
        "--showuse",
        "--showmemuse",
        "--showtemp",
        "--json",
    ];
    let result = run_command_capture("rocm-smi", &args).await;
    match result {
        Ok(raw) => {
            let parsed_json = serde_json::from_str::<Value>(&raw).ok();
            let devices = parsed_json
                .as_ref()
                .map(parse_rocm_devices_from_json)
                .unwrap_or_default();
            RocmMetrics {
                available: true,
                devices,
                raw,
                error: None,
            }
        }
        Err(err) => {
            let fallback = run_command_capture(
                "rocm-smi",
                &[
                    "--showproductname",
                    "--showuse",
                    "--showmemuse",
                    "--showtemp",
                ],
            )
            .await;
            match fallback {
                Ok(raw) => RocmMetrics {
                    available: true,
                    devices: parse_rocm_devices_from_text(&raw),
                    raw,
                    error: Some(err),
                },
                Err(fallback_err) => RocmMetrics {
                    available: false,
                    devices: vec![],
                    raw: String::new(),
                    error: Some(format!("{err}; {fallback_err}")),
                },
            }
        }
    }
}

async fn run_command_capture(cmd: &str, args: &[&str]) -> Result<String, String> {
    match Command::new(cmd).args(args).output().await {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        }
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).to_string()),
        Err(err) => Err(err.to_string()),
    }
}

fn parse_rocm_devices_from_json(value: &Value) -> Vec<GpuDeviceMetrics> {
    let mut devices = vec![];
    let Some(object) = value.as_object() else {
        return devices;
    };
    for (key, val) in object {
        let key_lower = key.to_lowercase();
        if !key_lower.contains("card") && !key_lower.contains("gpu") {
            continue;
        }
        let Some(map) = val.as_object() else {
            continue;
        };
        let name = find_field(map, &["Card series", "Card model", "Product Name"]);
        let utilization_percent = find_percent_field(map, &["GPU use", "GPU use (%)"]);
        let memory_use_percent = find_percent_field(map, &["GPU memory use", "GPU memory use (%)"]);
        let temperature_c = find_numeric_field(
            map,
            &["Temperature", "Sensor edge", "Temperature (Sensor edge)"],
        );
        devices.push(GpuDeviceMetrics {
            id: key.clone(),
            name,
            utilization_percent,
            memory_use_percent,
            temperature_c,
        });
    }
    devices
}

fn parse_rocm_devices_from_text(text: &str) -> Vec<GpuDeviceMetrics> {
    let mut map: HashMap<String, GpuDeviceMetrics> = HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let Some((left, right)) = trimmed.split_once(':') else {
            continue;
        };
        if !left.to_lowercase().contains("gpu[") {
            continue;
        }
        let id = left.split_whitespace().next().unwrap_or(left).to_string();
        let device = map.entry(id.clone()).or_insert_with(|| GpuDeviceMetrics {
            id,
            name: None,
            utilization_percent: None,
            memory_use_percent: None,
            temperature_c: None,
        });
        let rhs = right.trim();
        let lower = trimmed.to_lowercase();
        if lower.contains("gpu use") {
            device.utilization_percent = parse_first_number(rhs);
        } else if lower.contains("memory use") {
            device.memory_use_percent = parse_first_number(rhs);
        } else if lower.contains("temp") {
            device.temperature_c = parse_first_number(rhs);
        } else if lower.contains("card series") || lower.contains("product name") {
            device.name = Some(rhs.to_string());
        }
    }
    map.into_values().collect()
}

fn find_field(map: &serde_json::Map<String, Value>, needles: &[&str]) -> Option<String> {
    for (key, value) in map {
        let key_lower = key.to_lowercase();
        if needles
            .iter()
            .any(|needle| key_lower.contains(&needle.to_lowercase()))
        {
            if let Some(text) = value.as_str() {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn find_percent_field(map: &serde_json::Map<String, Value>, needles: &[&str]) -> Option<f64> {
    for (key, value) in map {
        let key_lower = key.to_lowercase();
        if needles
            .iter()
            .any(|needle| key_lower.contains(&needle.to_lowercase()))
        {
            if let Some(percent) = parse_value_as_number(value) {
                return Some(percent);
            }
        }
    }
    None
}

fn find_numeric_field(map: &serde_json::Map<String, Value>, needles: &[&str]) -> Option<f64> {
    find_percent_field(map, needles)
}

fn parse_value_as_number(value: &Value) -> Option<f64> {
    if let Some(number) = value.as_f64() {
        return Some(number);
    }
    value.as_str().and_then(parse_first_number)
}

fn parse_first_number(text: &str) -> Option<f64> {
    let mut out = String::new();
    let mut started = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == '.' || (ch == '-' && !started) {
            started = true;
            out.push(ch);
        } else if started {
            break;
        }
    }
    if out.is_empty() {
        None
    } else {
        out.parse::<f64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_quant_from_filename, normalize_quant_slug};

    #[test]
    fn normalizes_extended_ud_quant_without_truncating() {
        assert_eq!(
            normalize_quant_slug("UD-Q6_K_XL").as_deref(),
            Some("UD-Q6_K_XL")
        );
        assert_eq!(
            normalize_quant_slug("ud-q6_k_xl").as_deref(),
            Some("UD-Q6_K_XL")
        );
        assert_eq!(
            normalize_quant_slug("UDQ6_K_XL").as_deref(),
            Some("UD-Q6_K_XL")
        );
    }

    #[test]
    fn detects_extended_quant_from_filename() {
        assert_eq!(
            detect_quant_from_filename("Qwen3.6-27B-UD-Q6_K_XL.gguf").as_deref(),
            Some("UD-Q6_K_XL")
        );
        assert_eq!(
            detect_quant_from_filename("Qwen3.6-27B-UD-Q6_K_XL-00001-of-00002.gguf").as_deref(),
            Some("UD-Q6_K_XL")
        );
    }

    #[test]
    fn does_not_treat_model_names_as_quant() {
        assert_eq!(normalize_quant_slug("qwen3.6-27b"), None);
        assert_eq!(detect_quant_from_filename("Qwen3.6-27B.gguf"), None);
        assert_eq!(normalize_quant_slug("Q4_K_MODEL"), None);
    }
}
