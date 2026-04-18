use std::{env, path::PathBuf};

use crate::error::{AppError, AppResult};

#[derive(Clone, Debug)]
pub struct WorkspacePaths {
    pub root: PathBuf,
    pub instances_dir: PathBuf,
    pub models_dir: PathBuf,
    pub templates_dir: PathBuf,
    pub manager_data_dir: PathBuf,
}

impl WorkspacePaths {
    pub fn from_env() -> AppResult<Self> {
        let root = if let Ok(path) = env::var("MANAGER_NEO_WORKDIR") {
            PathBuf::from(path)
        } else {
            env::current_exe()?
                .parent()
                .map(PathBuf::from)
                .ok_or_else(|| {
                    AppError::InvalidInput(
                        "cannot determine executable directory for default workspace".into(),
                    )
                })?
        };
        Ok(Self::new(root))
    }

    pub fn new(root: PathBuf) -> Self {
        let manager_data_dir = root.join(".manager-neo").join("data");
        Self {
            instances_dir: root.join("instances"),
            models_dir: root.join("models"),
            templates_dir: root.join("templates"),
            root,
            manager_data_dir,
        }
    }
}
