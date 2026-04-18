# Architecture

## Goal

`manager-neo` manages local LLM instances under `~/llama` using Rust-first infrastructure:
- **CLI** for scripting and Unix-style automation
- **TUI** for interactive operations and live instance status
- **Web API** for remote/UI orchestration
- **MCP Streamable HTTP** for AI-agent tool integration

## Runtime Layout

```text
~/llama
├── <instance-name>/compose.yml
├── models/
├── templates/
└── manager-neo/
    ├── backend/
    └── frontend/
```

## Data Flow

```text
CLI / TUI / Web API / MCP
        │
        ▼
    AppService
  (business logic)
        │
  ┌─────┴──────────┐
  ▼                ▼
Store (FS/YAML)   Runtime adapters
                  - docker compose
                  - uv run hf download
```

## Key Design Choices

1. **File-system source of truth**: instances are discovered from `compose.yml`.
2. **Template + override model**: base config for a model family, variant-level diffs.
3. **Isolation-friendly architecture**: runtime adapters are trait-based and mockable for tests.
4. **Streamable MCP**: `/mcp` supports JSON responses and SSE mode via `Accept: text/event-stream`.
