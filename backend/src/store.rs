use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value};

use crate::{
    compose::{compose_to_instance_config, write_compose},
    config::WorkspacePaths,
    error::{AppError, AppResult},
    types::{Instance, InstanceConfig, Model, ModelFile, Template, normalize_config_key_path},
};

const INSTANCE_EXCLUDES: [&str; 8] = [
    ".venv",
    "manager",
    "manager-neo",
    "templates",
    "models",
    "instances",
    ".git",
    ".cache",
];

pub fn discover_instances(paths: &WorkspacePaths) -> AppResult<Vec<Instance>> {
    if !paths.root.exists() && !paths.instances_dir.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut seen = HashSet::new();

    if paths.instances_dir.exists() {
        let mut compose_paths = Vec::new();
        collect_compose_paths(&paths.instances_dir, &mut compose_paths)?;
        for compose_path in compose_paths {
            let Some(path) = compose_path.parent().map(ToOwned::to_owned) else {
                continue;
            };
            let fallback_name = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| "instance".to_string());
            if let Ok((service_name, mut config)) = compose_to_instance_config(&compose_path) {
                let name = config
                    .container_name
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| infer_instance_name_from_path(&path, &fallback_name));
                if seen.contains(&name) {
                    continue;
                }
                config.name = name.clone();
                if config.service_name.is_empty() {
                    config.service_name = service_name;
                }
                seen.insert(name.clone());
                result.push(Instance { name, path, config });
            }
        }
    }

    // Legacy fallback: top-level instance folders under workspace root.
    if paths.root.exists() {
        for entry in fs::read_dir(&paths.root)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            if INSTANCE_EXCLUDES.iter().any(|name| name == &file_name) || seen.contains(&file_name)
            {
                continue;
            }
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let compose_path = path.join("compose.yml");
            if !compose_path.exists() {
                continue;
            }
            if let Ok((service_name, mut config)) = compose_to_instance_config(&compose_path) {
                config.name = file_name.clone();
                if config.service_name.is_empty() {
                    config.service_name = service_name;
                }
                seen.insert(file_name.clone());
                result.push(Instance {
                    name: file_name,
                    path,
                    config,
                });
            }
        }
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

fn collect_compose_paths(path: &Path, out: &mut Vec<PathBuf>) -> AppResult<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let current = entry.path();
        if current.is_dir() {
            collect_compose_paths(&current, out)?;
            continue;
        }
        if current
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "compose.yml")
        {
            out.push(current);
        }
    }
    Ok(())
}

fn infer_instance_name_from_path(path: &Path, fallback: &str) -> String {
    let role = fallback.to_ascii_lowercase();
    let role_like = matches!(
        role.as_str(),
        "general"
            | "coding"
            | "chat"
            | "instruct"
            | "tool"
            | "embedding"
            | "reranker"
            | "no-thinking"
    );
    if !role_like {
        return fallback.to_string();
    }
    let Some(model) = path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .map(|value| value.to_string_lossy().to_string())
    else {
        return fallback.to_string();
    };
    if role == "general" {
        model
    } else {
        format!("{model}-{fallback}")
    }
}

pub fn get_instance(paths: &WorkspacePaths, name: &str) -> AppResult<Option<Instance>> {
    Ok(discover_instances(paths)?
        .into_iter()
        .find(|instance| instance.name == name))
}

pub fn create_instance(
    paths: &WorkspacePaths,
    name: &str,
    mut config: InstanceConfig,
) -> AppResult<Instance> {
    if config.container_name.is_none() {
        config.container_name = Some(name.to_string());
    }
    let instance_path = canonical_instance_path(paths, name, &config.model);
    fs::create_dir_all(&instance_path)?;
    config.name = name.to_string();
    write_compose(&config, &instance_path.join("compose.yml"), paths)?;
    Ok(Instance {
        name: name.to_string(),
        path: instance_path,
        config,
    })
}

pub fn update_instance(
    paths: &WorkspacePaths,
    name: &str,
    mut config: InstanceConfig,
) -> AppResult<Instance> {
    let Some(instance) = get_instance(paths, name)? else {
        return Err(AppError::NotFound(format!("instance '{name}'")));
    };
    if config.container_name.is_none() {
        config.container_name = Some(name.to_string());
    }
    config.name = name.to_string();
    write_compose(&config, &instance.path.join("compose.yml"), paths)?;
    Ok(Instance {
        name: name.to_string(),
        path: instance.path,
        config,
    })
}

pub fn delete_instance(paths: &WorkspacePaths, name: &str) -> AppResult<bool> {
    let Some(instance) = get_instance(paths, name)? else {
        return Ok(false);
    };
    fs::remove_dir_all(instance.path)?;
    Ok(true)
}

fn canonical_instance_path(
    paths: &WorkspacePaths,
    instance_name: &str,
    model_ref: &str,
) -> PathBuf {
    let Some((family, model, quant)) = model_ref_parts(model_ref) else {
        return paths.instances_dir.join(instance_name);
    };
    let role = derive_role(instance_name, &model);
    let primary = paths
        .instances_dir
        .join(&family)
        .join(&model)
        .join(&quant)
        .join(&role);
    if !primary.join("compose.yml").exists() {
        return primary;
    }
    paths
        .instances_dir
        .join(family)
        .join(model)
        .join(quant)
        .join(role)
        .join(instance_name)
}

fn model_ref_parts(model_ref: &str) -> Option<(String, String, String)> {
    let trimmed = model_ref
        .trim_start_matches("/models/")
        .trim_start_matches('/');
    let parts = trimmed.split('/').collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }
    let family = canonical_family(parts[0]);
    let quant = canonical_quant(parts[2]);
    let model = normalize_model(parts[1], &quant);
    if family.is_empty() || model.is_empty() || quant.is_empty() {
        return None;
    }
    Some((family, model, quant))
}

fn canonical_family(value: &str) -> String {
    let base = value.to_ascii_lowercase().replace('_', "-");
    match base.as_str() {
        "qwen-3-5" | "qwen3-5" | "qwen3.5" | "qwen35" => "qwen-3.5".to_string(),
        "step-3-5" | "step3-5" | "step3.5" => "step-3.5".to_string(),
        _ => sanitize_slug(&base),
    }
}

fn canonical_quant(value: &str) -> String {
    value
        .trim_matches('/')
        .trim_end_matches(".gguf")
        .to_ascii_uppercase()
}

fn normalize_model(value: &str, quant: &str) -> String {
    let mut text = value.to_ascii_lowercase().replace([' ', '/'], "-");
    for variant in quant_suffix_variants(quant) {
        for sep in ["-", "_", "."] {
            let suffix = format!("{sep}{variant}");
            if text.ends_with(&suffix) {
                text = text.trim_end_matches(&suffix).to_string();
            }
        }
    }
    sanitize_slug(&text)
}

fn quant_suffix_variants(quant: &str) -> Vec<String> {
    let upper = quant.to_ascii_uppercase();
    vec![
        upper.clone(),
        upper.to_ascii_lowercase(),
        upper.replace('-', "_").to_ascii_lowercase(),
        upper.replace('-', "_").to_ascii_uppercase(),
        upper.replace(['-', '_'], "").to_ascii_lowercase(),
    ]
}

fn derive_role(instance_name: &str, model_name: &str) -> String {
    let name = sanitize_slug(instance_name);
    let model = sanitize_slug(model_name);
    if let Some(rest) = name.strip_prefix(&format!("{model}-")) {
        if !rest.is_empty() {
            return rest.to_string();
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

fn sanitize_slug(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' {
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

pub fn discover_models(paths: &WorkspacePaths) -> AppResult<Vec<Model>> {
    if !paths.models_dir.exists() {
        return Ok(Vec::new());
    }
    let mut model_roots = Vec::new();
    collect_model_roots(&paths.models_dir, &mut model_roots)?;
    let mut models = vec![];
    for dir in model_roots {
        let mut files = vec![];
        collect_gguf_files(&dir, &mut files)?;
        files.sort_by(|a, b| a.name.cmp(&b.name));
        if files.is_empty() {
            continue;
        }
        let rel_name = dir
            .strip_prefix(&paths.models_dir)
            .ok()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                dir.file_name()
                    .map(|v| v.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            });
        models.push(Model {
            name: rel_name,
            path: dir,
            files,
        });
    }
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

fn collect_gguf_files(path: &Path, files: &mut Vec<ModelFile>) -> AppResult<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let current = entry.path();
        if current.is_dir() {
            collect_gguf_files(&current, files)?;
            continue;
        }
        if current.extension().and_then(|v| v.to_str()) == Some("gguf") {
            let metadata = fs::metadata(&current)?;
            files.push(ModelFile {
                name: current
                    .file_name()
                    .map(|v| v.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown.gguf".to_string()),
                path: current,
                size_bytes: metadata.len(),
            });
        }
    }
    Ok(())
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

fn resolve_model_dir_by_name(paths: &WorkspacePaths, name: &str) -> PathBuf {
    let normalized = name.trim_matches('/');
    paths.models_dir.join(normalized)
}

pub fn delete_model(paths: &WorkspacePaths, name: &str) -> AppResult<bool> {
    let target = resolve_model_dir_by_name(paths, name);
    if !target.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(target)?;
    Ok(true)
}

pub fn rename_model(paths: &WorkspacePaths, name: &str, new_name: &str) -> AppResult<bool> {
    let source = resolve_model_dir_by_name(paths, name);
    if !source.exists() {
        return Ok(false);
    }
    let target = resolve_model_dir_by_name(paths, new_name);
    if target.exists() {
        return Err(AppError::InvalidInput(format!(
            "model target '{new_name}' already exists"
        )));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(source, target)?;
    Ok(true)
}

pub fn load_templates(paths: &WorkspacePaths) -> AppResult<Vec<Template>> {
    let templates_path = paths.manager_data_dir.join("templates.json");
    if !templates_path.exists() {
        return auto_detect_templates(paths);
    }
    let raw = fs::read_to_string(templates_path)?;
    let templates: Vec<Template> = serde_json::from_str(&raw)?;
    let (mut templates, changed) = normalize_templates(templates);
    if changed {
        save_templates(paths, &templates)?;
    }
    templates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(templates)
}

pub fn save_templates(paths: &WorkspacePaths, templates: &[Template]) -> AppResult<()> {
    fs::create_dir_all(&paths.manager_data_dir)?;
    let file = paths.manager_data_dir.join("templates.json");
    let raw = serde_json::to_string_pretty(templates)?;
    fs::write(file, raw)?;
    Ok(())
}

pub fn create_template(
    paths: &WorkspacePaths,
    name: &str,
    family: &str,
    description: &str,
    config: InstanceConfig,
) -> AppResult<Template> {
    let mut templates = load_templates(paths)?;
    let family = canonicalize_family_alias(family);
    templates.retain(|template| template.name != name);
    let template = Template {
        name: name.to_string(),
        family,
        description: description.to_string(),
        config,
        overrides: BTreeMap::new(),
    };
    templates.push(template.clone());
    save_templates(paths, &templates)?;
    Ok(template)
}

pub fn delete_template(paths: &WorkspacePaths, name: &str) -> AppResult<bool> {
    let mut templates = load_templates(paths)?;
    let before = templates.len();
    templates.retain(|template| template.name != name);
    if before == templates.len() {
        return Ok(false);
    }
    save_templates(paths, &templates)?;
    Ok(true)
}

pub fn instantiate_template(
    paths: &WorkspacePaths,
    template_name: &str,
    instance_name: &str,
    overrides: Option<HashMap<String, Value>>,
) -> AppResult<Option<Instance>> {
    let templates = load_templates(paths)?;
    let Some(template) = templates
        .iter()
        .find(|template| template.name == template_name)
    else {
        return Ok(None);
    };
    let config = template.resolve(overrides.as_ref())?;
    create_instance(paths, instance_name, config).map(Some)
}

pub fn set_template_override(
    paths: &WorkspacePaths,
    template_name: &str,
    variant_name: &str,
    key: &str,
    value: Value,
) -> AppResult<Template> {
    let mut templates = load_templates(paths)?;
    let Some(idx) = templates
        .iter()
        .position(|template| template.name == template_name)
    else {
        return Err(AppError::NotFound(format!("template '{template_name}'")));
    };

    {
        let template = &mut templates[idx];
        let normalized_key = normalize_config_key_path(key);
        if normalized_key.is_empty() {
            return Err(AppError::InvalidInput("empty override key".to_string()));
        }
        let override_map = template
            .overrides
            .entry(variant_name.to_string())
            .or_insert_with(HashMap::new);
        override_map.insert(normalized_key, value);
    }

    save_templates(paths, &templates)?;
    let updated_template = templates[idx].clone();

    if let Some(override_map) = updated_template.overrides.get(variant_name) {
        let mut resolved = updated_template.resolve(Some(override_map))?;
        resolved.name = variant_name.to_string();
        if get_instance(paths, variant_name)?.is_some() {
            update_instance(paths, variant_name, resolved)?;
        }
    }

    Ok(updated_template)
}

pub fn set_template_base_value(
    paths: &WorkspacePaths,
    template_name: &str,
    key: &str,
    value: Value,
) -> AppResult<Template> {
    let mut templates = load_templates(paths)?;
    let Some(idx) = templates
        .iter()
        .position(|template| template.name == template_name)
    else {
        return Err(AppError::NotFound(format!("template '{template_name}'")));
    };

    {
        let template = &mut templates[idx];
        let mut config_value = serde_json::to_value(&template.config)?;
        apply_key_path(&mut config_value, key, value)?;
        template.config = serde_json::from_value(config_value)?;
    }

    save_templates(paths, &templates)?;
    let updated_template = templates[idx].clone();

    for (variant_name, override_map) in &updated_template.overrides {
        if get_instance(paths, variant_name)?.is_some() {
            let mut resolved = updated_template.resolve(Some(override_map))?;
            resolved.name = variant_name.clone();
            update_instance(paths, variant_name, resolved)?;
        }
    }

    Ok(updated_template)
}

pub fn batch_apply_to_family(
    paths: &WorkspacePaths,
    template_name: &str,
    key: &str,
    value: Value,
) -> AppResult<Vec<String>> {
    let mut templates = load_templates(paths)?;
    let Some(template_idx) = templates
        .iter()
        .position(|template| template.name == template_name)
    else {
        return Ok(vec![]);
    };
    let normalized_key = normalize_config_key_path(key);
    if normalized_key.is_empty() {
        return Err(AppError::InvalidInput("empty override key".to_string()));
    }
    let template = &mut templates[template_idx];
    template.overrides.iter_mut().for_each(|(_, override_map)| {
        _ = override_map.insert(normalized_key.clone(), value.clone());
    });

    let variant_names = template.overrides.keys().cloned().collect::<Vec<_>>();
    save_templates(paths, &templates)?;
    let mut updated = vec![];
    for variant in variant_names {
        let Some(template_ref) = templates
            .iter()
            .find(|template| template.name == template_name)
        else {
            continue;
        };
        let Some(override_map) = template_ref.overrides.get(&variant) else {
            continue;
        };
        let mut resolved = template_ref.resolve(Some(override_map))?;
        resolved.name = variant.clone();
        if get_instance(paths, &variant)?.is_some() {
            update_instance(paths, &variant, resolved)?;
            updated.push(variant);
        }
    }
    Ok(updated)
}

pub fn auto_detect_templates(paths: &WorkspacePaths) -> AppResult<Vec<Template>> {
    let instances = discover_instances(paths)?;
    let mut families: BTreeMap<String, Vec<Instance>> = BTreeMap::new();
    for instance in instances {
        families
            .entry(infer_family(&instance.name))
            .or_default()
            .push(instance);
    }
    let mut templates = vec![];
    for (family, members) in families {
        let mut base = members
            .first()
            .map(|m| m.config.clone())
            .unwrap_or_default();
        base.name = family.clone();
        let mut overrides = BTreeMap::new();
        for member in members {
            let diff = config_diff(&base, &member.config)?;
            overrides.insert(member.name, diff);
        }
        templates.push(Template {
            name: family.clone(),
            family,
            description: "Auto-detected family".to_string(),
            config: base,
            overrides,
        });
    }
    Ok(templates)
}

fn infer_family(name: &str) -> String {
    let parts = name.split('-').collect::<Vec<_>>();
    for (idx, part) in parts.iter().enumerate() {
        if part.chars().any(|ch| ch.is_ascii_digit()) {
            return canonicalize_family_alias(&parts[..=idx].join("-"));
        }
    }
    canonicalize_family_alias(name)
}

fn canonicalize_family_alias(value: &str) -> String {
    let base = value.trim().to_ascii_lowercase().replace('_', "-");
    match base.as_str() {
        "qwen-3-5" | "qwen3-5" | "qwen3.5" | "qwen35" => "qwen-3.5".to_string(),
        "step-3-5" | "step3-5" | "step3.5" => "step-3.5".to_string(),
        _ => base,
    }
}

fn normalize_templates(input: Vec<Template>) -> (Vec<Template>, bool) {
    let mut changed = false;
    let mut merged: BTreeMap<String, Template> = BTreeMap::new();
    for mut template in input {
        let original_name = template.name.clone();
        let original_family = template.family.clone();
        let canonical_family = canonicalize_family_alias(&template.family);
        template.family = canonical_family.clone();
        if template.description == "Auto-detected family"
            || original_name == original_family
            || canonicalize_family_alias(&original_name) == canonical_family
        {
            template.name = canonical_family.clone();
        }
        if template.name != original_name || template.family != original_family {
            changed = true;
        }

        if let Some(existing) = merged.get_mut(&template.name) {
            for (variant, diff) in template.overrides {
                existing.overrides.entry(variant).or_insert(diff);
            }
            if existing.config.model.is_empty() && !template.config.model.is_empty() {
                existing.config = template.config;
            }
            if existing.description.is_empty() && !template.description.is_empty() {
                existing.description = template.description;
            }
            existing.family = canonical_family.clone();
            changed = true;
        } else {
            merged.insert(template.name.clone(), template);
        }
    }
    (merged.into_values().collect(), changed)
}

fn config_diff(base: &InstanceConfig, other: &InstanceConfig) -> AppResult<HashMap<String, Value>> {
    let base = serde_json::to_value(base)?;
    let other = serde_json::to_value(other)?;
    let mut diff = HashMap::new();
    flatten_diff("", &base, &other, &mut diff);
    Ok(diff)
}

fn flatten_diff(prefix: &str, left: &Value, right: &Value, diff: &mut HashMap<String, Value>) {
    match (left, right) {
        (Value::Object(lm), Value::Object(rm)) => {
            for (key, rv) in rm {
                let next = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                if let Some(lv) = lm.get(key) {
                    flatten_diff(&next, lv, rv, diff);
                } else {
                    diff.insert(next, rv.clone());
                }
            }
        }
        _ if left != right => {
            diff.insert(prefix.to_string(), right.clone());
        }
        _ => {}
    }
}

fn apply_key_path(root: &mut Value, key: &str, value: Value) -> AppResult<()> {
    let normalized = normalize_config_key_path(key);
    let parts: Vec<&str> = normalized.split('.').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return Err(AppError::InvalidInput("empty key path".to_string()));
    }
    let mut current = root;
    for part in &parts[..parts.len() - 1] {
        let map = current
            .as_object_mut()
            .ok_or_else(|| AppError::InvalidInput(format!("invalid path segment in '{key}'")))?;
        current = map
            .entry((*part).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    let map = current
        .as_object_mut()
        .ok_or_else(|| AppError::InvalidInput(format!("invalid path leaf in '{key}'")))?;
    map.insert(parts[parts.len() - 1].to_string(), value);
    Ok(())
}

pub fn get_port_map(paths: &WorkspacePaths) -> AppResult<BTreeMap<u16, String>> {
    let mut ports = BTreeMap::new();
    for instance in discover_instances(paths)? {
        ports.insert(instance.config.host_port, instance.name);
    }
    Ok(ports)
}

pub fn find_available_port(paths: &WorkspacePaths, start: u16, end: u16) -> AppResult<Option<u16>> {
    let used = get_port_map(paths)?
        .into_keys()
        .collect::<std::collections::HashSet<_>>();
    for port in start..end {
        if !used.contains(&port) {
            return Ok(Some(port));
        }
    }
    Ok(None)
}

pub fn ensure_workspace(paths: &WorkspacePaths) -> AppResult<()> {
    fs::create_dir_all(&paths.root)?;
    fs::create_dir_all(&paths.instances_dir)?;
    fs::create_dir_all(&paths.models_dir)?;
    fs::create_dir_all(&paths.templates_dir)?;
    fs::create_dir_all(&paths.manager_data_dir)?;
    Ok(())
}

#[allow(dead_code)]
fn _debug_path(path: &PathBuf) -> String {
    path.display().to_string()
}
