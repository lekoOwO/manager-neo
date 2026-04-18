# manager-neo Organize Migration Playbook (AI Agent, Zero Context)

This document is a **single-source migration guide** for an AI Agent to reorganize any existing user workspace into manager-neo canonical layout.

## Goal

Normalize workspace to:

- `models/<family>/<model>/<QUANT>/...`
- `instances/<family>/<model>/<QUANT>/<role>/compose.yml`

Rules:

1. `family`: canonical slug (e.g. `qwen-3.5`, `step-3.5`).
2. `model`: lowercase, quant-free.
3. `QUANT`: uppercase quant token (`Q4_K_M`, `UD-Q4_K_XL`, `IQ4_XS`, `F16`, `GENERAL`, ...).
4. `role`: lowercase (`general` by default; use specific labels like `coding`, `no-thinking` when clearly indicated).

## Safety Requirements

1. Never mutate production files before a backup.
2. Stop running containers before moving files.
3. Keep a list of containers that were running, and restore only those.
4. If collision occurs (target already exists), merge safely and never overwrite silently.

## Inputs

Agent must know workspace root (`ROOT`), e.g. `/home/leko/llama`.

Key folders:

- `ROOT/models`
- `ROOT/instances`
- `ROOT/templates`

## Phase 1: Preflight

1. Build a full inventory:
   - all `compose.yml` under `ROOT`
   - all `.gguf` files under `ROOT/models`
2. Snapshot running containers:
   - map instance/container identity
3. Create rollback archive (or equivalent snapshot) for `models/`, `instances/`, `templates/`.

## Phase 2: Freeze Runtime

1. For each running instance, execute compose down from its compose directory.
2. Record down results and failures; abort if critical failures remain unresolved.

## Phase 3: Normalize Models

For each GGUF-containing model root:

1. Detect canonical `family`.
   - normalize aliases:
     - `qwen-3-5`, `qwen3.5`, `qwen35` -> `qwen-3.5`
     - `step-3-5`, `step3.5` -> `step-3.5`
2. Detect `QUANT`:
   - prefer explicit folder quant if valid
   - otherwise infer from GGUF filename token
   - fallback `GENERAL`
3. Detect canonical `model`:
   - use GGUF filename stem (exclude shard suffixes)
   - lowercase
   - remove quant suffix fragments
4. Move to `models/<family>/<model>/<QUANT>/`.
5. Preserve sharded sets and related mmproj files within same model+quant.
6. Produce mapping table: `old_model_ref -> new_model_ref`.

## Phase 4: Rewrite Compose Model References

For every compose file:

1. Rewrite `--model`, `--mmproj`, `--draft-model` paths using mapping table.
2. Ensure rewritten refs are under `/models/<family>/<model>/<QUANT>/...`.

## Phase 5: Normalize Instances

For every compose-backed instance:

1. Parse rewritten model ref to derive `<family>/<model>/<QUANT>`.
2. Derive `role`:
   - suffix match in instance naming (`-coding`, `-no-thinking`, etc.)
   - otherwise `general`
3. Move instance folder to:
   - `instances/<family>/<model>/<QUANT>/<role>/compose.yml`
4. If `<role>` already occupied:
   - use fallback subfolder with original instance name
   - never overwrite existing compose
5. Ensure compose sets stable `container_name` per logical instance.

## Phase 6: Post-Migration Validation

Hard checks:

1. All model GGUF roots match exactly 3 segments under `models/`.
2. All instance compose paths match exactly 4 segments under `instances/`.
3. Quant folders are uppercase.
4. Model folders are lowercase and quant-free.
5. Compose model refs path-prefix match their instance family/model/quant.
6. No legacy alias roots left (`qwen-3-5`, `step-3-5`) unless empty and removable.

## Phase 7: Restore Runtime

1. Start only instances that were running before migration.
2. Check health/status of restored instances.
3. Report any failed restarts explicitly.

## Expected Deliverables

Agent must output:

1. Stopped/restarted instance list.
2. Model move list.
3. Instance move list.
4. Updated compose reference list.
5. Validation summary (pass/fail + residual violations).
6. Manual follow-up items (if any collisions or ambiguous model naming remained).

## Failure Handling

Abort and report if:

1. Running containers cannot be safely stopped.
2. Compose rewrite would produce invalid or missing model refs.
3. Migration would overwrite existing non-identical files without safe merge policy.
