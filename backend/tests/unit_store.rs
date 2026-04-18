use std::collections::HashMap;

use manager_neo_backend::{
    config::WorkspacePaths,
    store,
    types::{InstanceConfig, Template},
};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn template_instantiation_uses_temp_workspace_only() {
    let temp = tempdir().unwrap();
    let paths = WorkspacePaths::new(temp.path().to_path_buf());
    store::ensure_workspace(&paths).unwrap();

    let template = Template {
        name: "qwen".to_string(),
        family: "qwen".to_string(),
        description: "test".to_string(),
        config: InstanceConfig {
            model: "/models/base.gguf".to_string(),
            ..InstanceConfig::default()
        },
        overrides: std::collections::BTreeMap::from([(
            "qwen-variant".to_string(),
            HashMap::from([("model".to_string(), json!("/models/variant.gguf"))]),
        )]),
    };

    store::save_templates(&paths, &[template]).unwrap();
    let created = store::instantiate_template(
        &paths,
        "qwen",
        "qwen-variant",
        Some(HashMap::from([("ctx_size".to_string(), json!(8192))])),
    )
    .unwrap()
    .unwrap();

    assert_eq!(created.name, "qwen-variant");
    assert_eq!(created.config.ctx_size, 8192);
    assert!(
        paths
            .instances_dir
            .join("qwen-variant/compose.yml")
            .exists()
    );
}

#[test]
fn template_override_update_writes_diff_and_updates_instance() {
    let temp = tempdir().unwrap();
    let paths = WorkspacePaths::new(temp.path().to_path_buf());
    store::ensure_workspace(&paths).unwrap();

    let base_config = InstanceConfig {
        model: "/models/base.gguf".to_string(),
        ..InstanceConfig::default()
    };
    store::create_instance(&paths, "qwen-v1", base_config.clone()).unwrap();
    let template = Template {
        name: "qwen".to_string(),
        family: "qwen".to_string(),
        description: "test".to_string(),
        config: base_config,
        overrides: std::collections::BTreeMap::new(),
    };
    store::save_templates(&paths, &[template]).unwrap();

    let updated =
        store::set_template_override(&paths, "qwen", "qwen-v1", "sampling.temp", json!(0.5))
            .unwrap();
    assert_eq!(
        updated
            .overrides
            .get("qwen-v1")
            .unwrap()
            .get("sampling.temp"),
        Some(&json!(0.5))
    );

    let inst = store::get_instance(&paths, "qwen-v1").unwrap().unwrap();
    assert_eq!(inst.config.sampling.temp, 0.5);
}

#[test]
fn model_rename_works_in_temp_workspace() {
    let temp = tempdir().unwrap();
    let paths = WorkspacePaths::new(temp.path().to_path_buf());
    store::ensure_workspace(&paths).unwrap();
    std::fs::create_dir_all(paths.models_dir.join("old-model")).unwrap();

    let renamed = store::rename_model(&paths, "old-model", "new-model").unwrap();
    assert!(renamed);
    assert!(!paths.models_dir.join("old-model").exists());
    assert!(paths.models_dir.join("new-model").exists());
}
