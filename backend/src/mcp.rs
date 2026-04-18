use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    error::{AppError, AppResult},
    service::{AppService, InstanceCreateInput, ModelRenameInput, TemplateCreateInput},
    types::ModelDownloadRequest,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpToolRequest {
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

pub async fn dispatch_tool(service: Arc<AppService>, request: McpToolRequest) -> AppResult<Value> {
    match request.tool.as_str() {
        "list_instances" => Ok(serde_json::to_value(service.list_instances()?)?),
        "instance_memory_previews" => {
            Ok(serde_json::to_value(service.instance_memory_previews()?)?)
        }
        "get_instance" => {
            let name = required_str(&request.arguments, "name")?;
            Ok(serde_json::to_value(service.get_instance(&name)?)?)
        }
        "create_instance" => {
            let req: InstanceCreateInput = serde_json::from_value(request.arguments)?;
            Ok(serde_json::to_value(service.create_instance(req)?)?)
        }
        "delete_instance" => {
            let name = required_str(&request.arguments, "name")?;
            Ok(serde_json::json!({ "deleted": service.delete_instance(&name)? }))
        }
        "edit_instance" => {
            let name = required_str(&request.arguments, "name")?;
            let key = required_str(&request.arguments, "key")?;
            let value = request
                .arguments
                .get("value")
                .cloned()
                .ok_or_else(|| AppError::InvalidInput("missing field: value".to_string()))?;
            Ok(serde_json::to_value(
                service.edit_instance(&name, &key, value)?,
            )?)
        }
        "start_instance" => {
            let name = required_str(&request.arguments, "name")?;
            service.start_instance(&name).await?;
            Ok(serde_json::json!({ "started": true, "name": name }))
        }
        "stop_instance" => {
            let name = required_str(&request.arguments, "name")?;
            service.stop_instance(&name).await?;
            Ok(serde_json::json!({ "stopped": true, "name": name }))
        }
        "restart_instance" => {
            let name = required_str(&request.arguments, "name")?;
            service.restart_instance(&name).await?;
            Ok(serde_json::json!({ "restarted": true, "name": name }))
        }
        "instance_status" => {
            let name = required_str(&request.arguments, "name")?;
            Ok(serde_json::to_value(service.instance_status(&name).await?)?)
        }
        "instance_health" => {
            let name = required_str(&request.arguments, "name")?;
            service.health_check(&name).await
        }
        "instance_logs" => {
            let name = required_str(&request.arguments, "name")?;
            let tail = request
                .arguments
                .get("tail")
                .and_then(Value::as_u64)
                .and_then(|v| usize::try_from(v).ok())
                .unwrap_or(100);
            Ok(serde_json::json!({ "logs": service.instance_logs(&name, tail).await? }))
        }
        "all_instances_status" => Ok(serde_json::to_value(service.all_instances_status().await?)?),
        "list_models" => Ok(serde_json::to_value(service.list_models()?)?),
        "download_model" => {
            let req: ModelDownloadRequest = serde_json::from_value(request.arguments)?;
            let path = service.download_model(req).await?;
            Ok(serde_json::json!({ "downloaded": true, "path": path }))
        }
        "plan_model_download" => {
            let req: ModelDownloadRequest = serde_json::from_value(request.arguments)?;
            Ok(serde_json::to_value(service.plan_model_download(req).await?)?)
        }
        "start_model_download_task" => {
            let req: ModelDownloadRequest = serde_json::from_value(request.arguments)?;
            Ok(serde_json::to_value(
                service.start_model_download_task(req).await?,
            )?)
        }
        "list_model_download_tasks" => {
            Ok(serde_json::to_value(service.list_model_download_tasks().await)?)
        }
        "get_model_download_task" => {
            let id = required_str(&request.arguments, "id")?;
            Ok(serde_json::to_value(
                service.get_model_download_task(&id).await?,
            )?)
        }
        "delete_model" => {
            let name = required_str(&request.arguments, "name")?;
            Ok(serde_json::json!({ "deleted": service.delete_model(&name)? }))
        }
        "rename_model" => {
            let name = required_str(&request.arguments, "name")?;
            let req: ModelRenameInput = serde_json::from_value(request.arguments)?;
            Ok(serde_json::json!({ "renamed": service.rename_model(&name, &req.name)? }))
        }
        "list_templates" => Ok(serde_json::to_value(service.list_templates()?)?),
        "create_template" => {
            let req: TemplateCreateInput = serde_json::from_value(request.arguments)?;
            Ok(serde_json::to_value(service.create_template(req)?)?)
        }
        "delete_template" => {
            let name = required_str(&request.arguments, "name")?;
            Ok(serde_json::json!({ "deleted": service.delete_template(&name)? }))
        }
        "instantiate_template" => {
            let template_name = required_str(&request.arguments, "template_name")?;
            let instance_name = required_str(&request.arguments, "instance_name")?;
            let overrides = request
                .arguments
                .get("overrides")
                .and_then(Value::as_object)
                .map(to_hashmap);
            Ok(serde_json::to_value(service.instantiate_template(
                &template_name,
                &instance_name,
                overrides,
            )?)?)
        }
        "batch_apply" => {
            let template_name = required_str(&request.arguments, "template_name")?;
            let key = required_str(&request.arguments, "key")?;
            let value = request
                .arguments
                .get("value")
                .cloned()
                .ok_or_else(|| AppError::InvalidInput("missing field: value".to_string()))?;
            Ok(serde_json::json!({
                "updated": service.batch_apply(&template_name, &key, value)?
            }))
        }
        "set_template_override" => {
            let template_name = required_str(&request.arguments, "template_name")?;
            let variant_name = required_str(&request.arguments, "variant_name")?;
            let key = required_str(&request.arguments, "key")?;
            let value = request
                .arguments
                .get("value")
                .cloned()
                .ok_or_else(|| AppError::InvalidInput("missing field: value".to_string()))?;
            Ok(serde_json::to_value(service.set_template_override(
                &template_name,
                &variant_name,
                &key,
                value,
            )?)?)
        }
        "set_template_base" => {
            let template_name = required_str(&request.arguments, "template_name")?;
            let key = required_str(&request.arguments, "key")?;
            let value = request
                .arguments
                .get("value")
                .cloned()
                .ok_or_else(|| AppError::InvalidInput("missing field: value".to_string()))?;
            Ok(serde_json::to_value(service.set_template_base_value(
                &template_name,
                &key,
                value,
            )?)?)
        }
        "get_port_map" => Ok(serde_json::to_value(service.port_map()?)?),
        "scan_templates" => Ok(serde_json::to_value(service.scan_templates()?)?),
        "system_metrics" => Ok(serde_json::to_value(service.system_metrics().await?)?),
        other => Err(AppError::InvalidInput(format!("unknown MCP tool: {other}"))),
    }
}

fn required_str(input: &Value, key: &str) -> AppResult<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| AppError::InvalidInput(format!("missing field: {key}")))
}

fn to_hashmap(map: &serde_json::Map<String, Value>) -> HashMap<String, Value> {
    map.iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<HashMap<_, _>>()
}
