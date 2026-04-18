# Testing

## Matrix

- **Unit**: pure logic tests (compose parsing, template resolution)
- **Integration**: Axum router tests with mocked Docker/Downloader and temporary workspace
- **E2E**: binary smoke test (`--help`)

## Commands

```bash
cd backend
cargo test
```

## Isolation Rules

1. Tests use `tempfile::tempdir()` workspaces.
2. Docker calls are mocked through trait-based adapters.
3. No real production model or container state is mutated during tests.
