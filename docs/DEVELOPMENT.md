# Development

## Prerequisites

- Rust toolchain
- Docker + docker compose
- `uv` (for `hf` CLI mediated model downloads)
- Node (managed by `fnm`) + `pnpm`

## Local Workflow

```bash
# backend
cd backend
cargo test
cargo run -- --help

# frontend
cd ../frontend
pnpm install
pnpm build
```

## Serving everything

```bash
cd backend
cargo run -- serve --port 9999
```

Then open `http://localhost:9999`.
