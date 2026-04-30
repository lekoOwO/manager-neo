use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{delete, get, post},
};
use futures_util::stream;
use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use serde_json::Value;

static FRONTEND_DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../frontend/dist");

use crate::{
    error::{AppError, AppResult},
    mcp::{McpToolRequest, dispatch_tool},
    service::{AppService, InstanceCreateInput, ModelRenameInput, TemplateCreateInput},
    types::ModelDownloadRequest,
};

#[derive(Clone)]
pub struct ApiState {
    pub service: Arc<AppService>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstanceEditRequest {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TemplateInstantiateRequest {
    pub template_name: String,
    pub instance_name: String,
    #[serde(default)]
    pub overrides: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BatchApplyRequest {
    pub template_name: String,
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TemplateOverrideRequest {
    pub template_name: String,
    pub variant_name: String,
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TemplateBaseEditRequest {
    pub template_name: String,
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TemplateFromModelRequest {
    pub name: String,
    pub family: String,
    #[serde(default)]
    pub description: String,
    pub model_ref: String,
    #[serde(default)]
    pub mmproj: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LogsQuery {
    #[serde(default = "default_tail")]
    pub tail: usize,
}

const fn default_tail() -> usize {
    100
}

pub fn build_router(state: ApiState) -> Router {
    Router::new()
        .route("/api/instances", get(list_instances).post(create_instance))
        .route("/api/instances/hierarchy", get(list_instances_hierarchy))
        .route(
            "/api/instances/memory-preview",
            get(instance_memory_previews),
        )
        .route(
            "/api/instances/{name}",
            get(get_instance)
                .patch(edit_instance)
                .delete(delete_instance),
        )
        .route("/api/instances/{name}/start", post(start_instance))
        .route("/api/instances/{name}/stop", post(stop_instance))
        .route("/api/instances/{name}/restart", post(restart_instance))
        .route("/api/instances/{name}/status", get(instance_status))
        .route("/api/instances/{name}/health", get(instance_health))
        .route("/api/instances/{name}/logs", get(instance_logs))
        .route("/api/status", get(all_instances_status))
        .route("/api/models", get(list_models))
        .route("/api/models/hierarchy", get(list_models_hierarchy))
        .route("/api/models/download/plan", post(plan_model_download))
        .route(
            "/api/models/download/tasks",
            post(start_model_download_task).get(list_model_download_tasks),
        )
        .route(
            "/api/models/download/tasks/{id}",
            get(get_model_download_task),
        )
        .route("/api/models/download", post(download_model))
        .route(
            "/api/models/{name}",
            delete(delete_model).patch(rename_model),
        )
        .route("/api/templates", get(list_templates).post(create_template))
        .route("/api/templates/hierarchy", get(list_templates_hierarchy))
        .route("/api/templates/{name}", delete(delete_template))
        .route(
            "/api/templates/from-model",
            post(create_template_from_model),
        )
        .route("/api/templates/instantiate", post(instantiate_template))
        .route("/api/templates/batch-apply", post(batch_apply))
        .route("/api/templates/set-override", post(set_template_override))
        .route("/api/templates/set-base", post(set_template_base))
        .route("/api/templates/scan", post(scan_templates))
        .route("/api/ports", get(port_map))
        .route("/api/system/metrics", get(system_metrics))
        .route("/mcp", post(mcp_http))
        .route("/mcp/tools", get(mcp_tools))
        .fallback(get(serve_embedded_frontend))
        .with_state(state)
}

pub async fn serve(state: ApiState, host: String, port: u16) -> AppResult<()> {
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|err| AppError::InvalidInput(format!("invalid bind address: {err}")))?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let app = build_router(state);
    axum::serve(listener, app)
        .await
        .map_err(|err| AppError::Message(err.to_string()))
}

async fn serve_embedded_frontend(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let normalized = if path.is_empty() { "index.html" } else { path };
    let file = FRONTEND_DIST.get_file(normalized).or_else(|| {
        if normalized.contains('.') {
            None
        } else {
            FRONTEND_DIST.get_file("index.html")
        }
    });

    let Some(file) = file else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };

    let content_type = mime_guess::from_path(file.path())
        .first_or_octet_stream()
        .to_string();
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .body(Body::from(file.contents().to_vec()))
        .unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "response build failed").into_response()
        })
}

async fn list_instances(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    let instances = state.service.list_instances()?;
    let hierarchy = state.service.list_instances_hierarchy()?;
    let mut quant_map: HashMap<String, String> = HashMap::new();
    for item in hierarchy {
        quant_map.insert(item.name.clone(), item.quant.clone());
    }
    let mapped = instances
        .into_iter()
        .map(|inst| {
            let quant = quant_map.get(&inst.name).cloned().unwrap_or_else(|| {
                inst.config
                    .model
                    .rsplit('/')
                    .next()
                    .unwrap_or("GENERAL")
                    .to_string()
            });
            let display_name = crate::service::strip_quant_suffix_from_name(&inst.name, &quant);
            let mut value = serde_json::to_value(&inst).unwrap_or(serde_json::Value::Null);
            if let serde_json::Value::Object(map) = &mut value {
                map.insert(
                    "display_name".to_string(),
                    serde_json::Value::String(display_name),
                );
            }
            value
        })
        .collect::<Vec<_>>();
    Ok(Json(serde_json::Value::Array(mapped)))
}

async fn list_instances_hierarchy(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    let rows = state.service.list_instances_hierarchy()?;
    let mapped = rows
        .into_iter()
        .map(|item| {
            let display = crate::service::strip_quant_suffix_from_name(&item.name, &item.quant);
            let mut value = serde_json::to_value(&item).unwrap_or(serde_json::Value::Null);
            if let serde_json::Value::Object(map) = &mut value {
                map.insert(
                    "display_name".to_string(),
                    serde_json::Value::String(display),
                );
            }
            value
        })
        .collect::<Vec<_>>();
    Ok(Json(serde_json::Value::Array(mapped)))
}

async fn instance_memory_previews(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.instance_memory_previews()?,
    )?))
}

async fn get_instance(
    Path(name): Path<String>,
    State(state): State<ApiState>,
) -> AppResult<Json<Value>> {
    let inst = state.service.get_instance(&name)?;
    let quant = inst
        .config
        .model
        .rsplit('/')
        .next()
        .unwrap_or("GENERAL")
        .to_string();
    let display_name = crate::service::strip_quant_suffix_from_name(&inst.name, &quant);
    let mut value = serde_json::to_value(&inst)?;
    if let Value::Object(map) = &mut value {
        map.insert("display_name".to_string(), Value::String(display_name));
    }
    Ok(Json(value))
}

async fn create_instance(
    State(state): State<ApiState>,
    Json(req): Json<InstanceCreateInput>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.create_instance(req)?,
    )?))
}

async fn delete_instance(
    Path(name): Path<String>,
    State(state): State<ApiState>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        serde_json::json!({ "deleted": state.service.delete_instance(&name)? }),
    ))
}

async fn edit_instance(
    Path(name): Path<String>,
    State(state): State<ApiState>,
    Json(req): Json<InstanceEditRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.edit_instance(&name, &req.key, req.value)?,
    )?))
}

async fn start_instance(
    Path(name): Path<String>,
    State(state): State<ApiState>,
) -> AppResult<Json<Value>> {
    state.service.start_instance(&name).await?;
    Ok(Json(serde_json::json!({ "started": true })))
}

async fn stop_instance(
    Path(name): Path<String>,
    State(state): State<ApiState>,
) -> AppResult<Json<Value>> {
    state.service.stop_instance(&name).await?;
    Ok(Json(serde_json::json!({ "stopped": true })))
}

async fn restart_instance(
    Path(name): Path<String>,
    State(state): State<ApiState>,
) -> AppResult<Json<Value>> {
    state.service.restart_instance(&name).await?;
    Ok(Json(serde_json::json!({ "restarted": true })))
}

async fn instance_status(
    Path(name): Path<String>,
    State(state): State<ApiState>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.instance_status(&name).await?,
    )?))
}

async fn instance_health(
    Path(name): Path<String>,
    State(state): State<ApiState>,
) -> AppResult<Json<Value>> {
    Ok(Json(state.service.health_check(&name).await?))
}

async fn instance_logs(
    Path(name): Path<String>,
    Query(query): Query<LogsQuery>,
    State(state): State<ApiState>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::json!({
        "logs": state.service.instance_logs(&name, query.tail).await?
    })))
}

async fn all_instances_status(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.all_instances_status().await?,
    )?))
}

async fn list_models(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(state.service.list_models()?)?))
}

async fn list_models_hierarchy(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.list_models_hierarchy()?,
    )?))
}

async fn download_model(
    State(state): State<ApiState>,
    Json(req): Json<ModelDownloadRequest>,
) -> AppResult<Json<Value>> {
    let path = state.service.download_model(req).await?;
    Ok(Json(
        serde_json::json!({ "downloaded": true, "path": path }),
    ))
}

async fn plan_model_download(
    State(state): State<ApiState>,
    Json(req): Json<ModelDownloadRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.plan_model_download(req).await?,
    )?))
}

async fn start_model_download_task(
    State(state): State<ApiState>,
    Json(req): Json<ModelDownloadRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.start_model_download_task(req).await?,
    )?))
}

async fn list_model_download_tasks(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.list_model_download_tasks().await,
    )?))
}

async fn get_model_download_task(
    Path(id): Path<String>,
    State(state): State<ApiState>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.get_model_download_task(&id).await?,
    )?))
}

async fn delete_model(
    Path(name): Path<String>,
    State(state): State<ApiState>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        serde_json::json!({ "deleted": state.service.delete_model(&name)? }),
    ))
}

async fn rename_model(
    Path(name): Path<String>,
    State(state): State<ApiState>,
    Json(req): Json<ModelRenameInput>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        serde_json::json!({ "renamed": state.service.rename_model(&name, &req.name)? }),
    ))
}

async fn list_templates(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(state.service.list_templates()?)?))
}

async fn list_templates_hierarchy(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.list_templates_hierarchy()?,
    )?))
}

async fn create_template(
    State(state): State<ApiState>,
    Json(req): Json<TemplateCreateInput>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.create_template(req)?,
    )?))
}

async fn create_template_from_model(
    State(state): State<ApiState>,
    Json(req): Json<TemplateFromModelRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.create_template_from_model(
            &req.name,
            &req.family,
            &req.description,
            &req.model_ref,
            req.mmproj,
        )?,
    )?))
}

async fn delete_template(
    Path(name): Path<String>,
    State(state): State<ApiState>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        serde_json::json!({ "deleted": state.service.delete_template(&name)? }),
    ))
}

async fn instantiate_template(
    State(state): State<ApiState>,
    Json(req): Json<TemplateInstantiateRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.instantiate_template(
            &req.template_name,
            &req.instance_name,
            req.overrides,
        )?,
    )?))
}

async fn batch_apply(
    State(state): State<ApiState>,
    Json(req): Json<BatchApplyRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::json!({
        "updated": state.service.batch_apply(&req.template_name, &req.key, req.value)?
    })))
}

async fn set_template_override(
    State(state): State<ApiState>,
    Json(req): Json<TemplateOverrideRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.set_template_override(
            &req.template_name,
            &req.variant_name,
            &req.key,
            req.value,
        )?,
    )?))
}

async fn set_template_base(
    State(state): State<ApiState>,
    Json(req): Json<TemplateBaseEditRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state
            .service
            .set_template_base_value(&req.template_name, &req.key, req.value)?,
    )?))
}

async fn scan_templates(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(state.service.scan_templates()?)?))
}

async fn port_map(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(state.service.port_map()?)?))
}

async fn system_metrics(State(state): State<ApiState>) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(
        state.service.system_metrics().await?,
    )?))
}

async fn mcp_tools() -> Json<Value> {
    Json(serde_json::json!({
        "tools": [
            "list_instances","get_instance","create_instance","delete_instance","edit_instance",
            "instance_memory_previews",
            "start_instance","stop_instance","restart_instance","instance_status","instance_health",
            "instance_logs","all_instances_status","list_models","download_model","delete_model","rename_model",
            "plan_model_download","start_model_download_task","list_model_download_tasks","get_model_download_task",
            "list_templates","create_template","delete_template","instantiate_template","batch_apply",
            "set_template_override","set_template_base","get_port_map","scan_templates","system_metrics"
        ]
    }))
}

async fn mcp_http(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<McpToolRequest>,
) -> Result<Response, AppError> {
    let result = dispatch_tool(state.service.clone(), req).await?;
    let accept_stream = headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"));
    if accept_stream {
        let payload = serde_json::to_string(&result)?;
        let stream = stream::once(async move {
            Ok::<Event, std::convert::Infallible>(Event::default().data(payload))
        });
        let sse = Sse::new(stream).keep_alive(KeepAlive::default());
        return Ok(sse.into_response());
    }
    Ok(Json(result).into_response())
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::InvalidInput(_) => StatusCode::BAD_REQUEST,
            AppError::CommandFailed(_) => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(serde_json::json!({ "error": self.to_string() }));
        (status, body).into_response()
    }
}
