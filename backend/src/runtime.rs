use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

use async_trait::async_trait;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc::UnboundedSender,
};

use crate::{
    error::{AppError, AppResult},
    types::ModelDownloadRequest,
};

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait DockerClient: Send + Sync {
    async fn compose(&self, cwd: &Path, args: &[&str]) -> AppResult<CommandOutput>;
}

#[derive(Default)]
pub struct DockerComposeClient;

#[async_trait]
impl DockerClient for DockerComposeClient {
    async fn compose(&self, cwd: &Path, args: &[&str]) -> AppResult<CommandOutput> {
        let mut cmd = Command::new("docker");
        cmd.args(["compose"]);
        cmd.args(args);
        cmd.current_dir(cwd);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let output = cmd.output().await?;
        Ok(CommandOutput {
            code: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

#[async_trait]
pub trait ModelDownloader: Send + Sync {
    async fn download(&self, req: &ModelDownloadRequest, models_dir: &Path) -> AppResult<String>;

    async fn download_with_progress(
        &self,
        req: &ModelDownloadRequest,
        models_dir: &Path,
        progress: Option<UnboundedSender<DownloadProgress>>,
    ) -> AppResult<String> {
        let _ = progress;
        self.download(req, models_dir).await
    }
}

#[derive(Default)]
pub struct HfCliDownloader;

#[derive(Clone, Debug)]
pub struct DownloadProgress {
    pub percent: Option<f64>,
    pub message: String,
}

#[async_trait]
impl ModelDownloader for HfCliDownloader {
    async fn download(&self, req: &ModelDownloadRequest, models_dir: &Path) -> AppResult<String> {
        self.download_with_progress(req, models_dir, None).await
    }

    async fn download_with_progress(
        &self,
        req: &ModelDownloadRequest,
        models_dir: &Path,
        progress: Option<UnboundedSender<DownloadProgress>>,
    ) -> AppResult<String> {
        if req.repo_id.trim().is_empty() {
            return Err(AppError::InvalidInput("repo_id is required".to_string()));
        }
        let target_dir = resolve_download_target_dir(req, models_dir);
        fs::create_dir_all(&target_dir)?;
        let script_path = save_download_script(req, &target_dir)?;
        send_progress(
            &progress,
            Some(0.0),
            format!("saved download script: {}", script_path.display()),
        );

        let mut cmd = Command::new("uv");
        cmd.args(build_hf_download_args(req, &target_dir));
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        send_progress(&progress, Some(1.0), "starting model download".to_string());

        let mut child = cmd.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Message("failed to capture downloader stdout".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::Message("failed to capture downloader stderr".to_string()))?;

        let progress_stdout = progress.clone();
        let progress_stderr = progress.clone();
        let stdout_task = tokio::spawn(async move {
            read_download_stream(stdout, progress_stdout).await;
        });
        let stderr_task = tokio::spawn(async move {
            read_download_stream(stderr, progress_stderr).await;
        });

        let status = child.wait().await?;
        let _ = stdout_task.await;
        let _ = stderr_task.await;
        if !status.success() {
            return Err(AppError::CommandFailed(
                "model download command failed; check downloader output for details".to_string(),
            ));
        }
        send_progress(&progress, Some(100.0), "download completed".to_string());
        Ok(target_dir.display().to_string())
    }
}

pub fn resolve_download_target_dir(req: &ModelDownloadRequest, models_dir: &Path) -> PathBuf {
    req.local_dir
        .as_ref()
        .map(|value| {
            let mut rel = value.trim().replace('\\', "/");
            if let Some(stripped) = rel.strip_prefix("/models/") {
                rel = stripped.to_string();
            } else if let Some(stripped) = rel.strip_prefix("models/") {
                rel = stripped.to_string();
            }
            rel = rel.trim_matches('/').to_string();
            if rel.is_empty() {
                models_dir.to_path_buf()
            } else {
                models_dir.join(rel)
            }
        })
        .unwrap_or_else(|| {
            let repo_name = req.repo_id.split('/').next_back().unwrap_or("model");
            models_dir.join(repo_name.to_lowercase())
        })
}

pub fn download_script_path(target_dir: &Path) -> PathBuf {
    target_dir.join("download-model.sh")
}

fn save_download_script(req: &ModelDownloadRequest, target_dir: &Path) -> AppResult<PathBuf> {
    let script_path = download_script_path(target_dir);
    let args = build_hf_download_args(req, target_dir)
        .into_iter()
        .map(|part| shell_quote(&part))
        .collect::<Vec<_>>()
        .join(" ");
    let content = format!("#!/usr/bin/env bash\nset -euo pipefail\nuv {args}\n");
    fs::write(&script_path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms)?;
    }
    Ok(script_path)
}

fn build_hf_download_args(req: &ModelDownloadRequest, target_dir: &Path) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "hf".to_string(),
        "download".to_string(),
        req.repo_id.clone(),
    ];
    if let Some(patterns) = &req.patterns {
        for pattern in patterns {
            args.push("--include".to_string());
            args.push(pattern.clone());
        }
    }
    args.push("--local-dir".to_string());
    args.push(target_dir.display().to_string());
    args
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-' | '.' | ':' | '='))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

async fn read_download_stream(
    stream: impl tokio::io::AsyncRead + Unpin,
    progress: Option<UnboundedSender<DownloadProgress>>,
) {
    let mut reader = BufReader::new(stream).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let percent = extract_percent(text);
        send_progress(&progress, percent, text.to_string());
    }
}

fn extract_percent(line: &str) -> Option<f64> {
    for token in line.split_whitespace() {
        let cleaned =
            token.trim_matches(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '%'));
        let Some(raw) = cleaned.strip_suffix('%') else {
            continue;
        };
        if let Ok(value) = raw.parse::<f64>() {
            return Some(value.clamp(0.0, 100.0));
        }
    }
    None
}

fn send_progress(
    progress: &Option<UnboundedSender<DownloadProgress>>,
    percent: Option<f64>,
    message: String,
) {
    if let Some(tx) = progress {
        let _ = tx.send(DownloadProgress { percent, message });
    }
}
