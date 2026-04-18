# Backend

## Stack

- Rust + Tokio
- Axum (REST API + MCP HTTP endpoint)
- Clap (CLI)
- Ratatui + Crossterm (TUI)
- Serde + serde_yaml (compose parsing/generation)

## Commands

```bash
cd backend

# CLI
cargo run -- instance list
cargo run -- model list
cargo run -- system metrics

# TUI
cargo run -- tui

# API + MCP (+ embedded frontend static assets)
cargo run -- serve --host 0.0.0.0 --port 9999
```

## API Surface

Base: `http://localhost:9999`

- `GET /api/instances`
- `GET /api/instances/memory-preview` (GGUF-based load memory estimate)
- `POST /api/instances`
- `PATCH /api/instances/{name}`
- `POST /api/instances/{name}/start|stop|restart`
- `GET /api/instances/{name}/status|health|logs`
- `GET /api/system/metrics` (CPU/RAM/GPU + `rocm-smi` snapshot)
- `GET /api/models`
- `POST /api/models/download`
- `PATCH /api/models/{name}` (rename model directory)
- `GET/POST/DELETE /api/templates...`
- `POST /api/templates/set-override`
- `POST /api/templates/set-base`
- `GET /api/ports`

## MCP Streamable HTTP

- `POST /mcp` with body:

```json
{
  "tool": "list_instances",
  "arguments": {}
}
```

- If request header contains `Accept: text/event-stream`, response is SSE.
- Otherwise returns JSON.

Tools include instance/model/template operations, status/ports queries, `system_metrics`, and `instance_memory_previews`.

## TUI Capabilities

The TUI now includes four workspaces:

1. **Instances**: lifecycle controls, parameter preview, GGUF-based memory load estimate, ad-hoc key editing, quick creation.
2. **Families**: template base parameter visualization, variant diff table, base/override editing, instantiate, batch apply.
3. **Models**: model directory/file inspection, download, rename, delete.
4. **System**: CPU/RAM bars and visualized GPU metrics summary.

Instance logs now open in a dedicated multi-line viewer with scroll and reload controls.

## Model Download Path

Model download uses:

```bash
uv run hf download <repo_id> --include <pattern>... --local-dir <target>
```

This mirrors previous `download.py` behavior while enforcing `uv`-mediated Python interop.
