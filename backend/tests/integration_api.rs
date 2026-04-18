use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use manager_neo_backend::{
    api::{ApiState, build_router},
    config::WorkspacePaths,
    error::AppResult,
    runtime::{CommandOutput, DockerClient, ModelDownloader},
    service::AppService,
    types::ModelDownloadRequest,
};
use tower::ServiceExt;

struct MockDockerClient;

#[async_trait]
impl DockerClient for MockDockerClient {
    async fn compose(&self, _cwd: &Path, args: &[&str]) -> AppResult<CommandOutput> {
        if args.contains(&"ps") {
            return Ok(CommandOutput {
                code: 0,
                stdout: r#"[{"State":"running","Ports":"0.0.0.0:8080->8080/tcp"}]"#.to_string(),
                stderr: String::new(),
            });
        }
        Ok(CommandOutput {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

struct MockDownloader;

#[async_trait]
impl ModelDownloader for MockDownloader {
    async fn download(&self, _req: &ModelDownloadRequest, _models_dir: &Path) -> AppResult<String> {
        Ok("/tmp/mock-download".to_string())
    }
}

#[tokio::test]
async fn creates_and_lists_instances_via_api() {
    let temp = tempfile::tempdir().unwrap();
    let service = Arc::new(
        AppService::new(
            WorkspacePaths::new(temp.path().to_path_buf()),
            Arc::new(MockDockerClient),
            Arc::new(MockDownloader),
        )
        .unwrap(),
    );

    let app = build_router(ApiState { service });

    let create_req = Request::builder()
        .method("POST")
        .uri("/api/instances")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"name":"test-qwen","model":"qwen/Qwen.gguf","port":8080}"#,
        ))
        .unwrap();

    let create_resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(create_resp.status(), StatusCode::OK);

    let list_req = Request::builder()
        .method("GET")
        .uri("/api/instances")
        .body(Body::empty())
        .unwrap();
    let list_resp = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);

    let body = list_resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("test-qwen"));

    let preview_req = Request::builder()
        .method("GET")
        .uri("/api/instances/memory-preview")
        .body(Body::empty())
        .unwrap();
    let preview_resp = app.clone().oneshot(preview_req).await.unwrap();
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let body = preview_resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("test-qwen"));
}

#[tokio::test]
async fn mcp_stream_endpoint_returns_sse() {
    let temp = tempfile::tempdir().unwrap();
    let service = Arc::new(
        AppService::new(
            WorkspacePaths::new(temp.path().to_path_buf()),
            Arc::new(MockDockerClient),
            Arc::new(MockDownloader),
        )
        .unwrap(),
    );
    let app = build_router(ApiState { service });

    let req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from(r#"{"tool":"list_instances","arguments":{}}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    assert!(content_type.contains("text/event-stream"));
}
