# Frontend

## Stack

- Vite + Vue 3 + TypeScript
- PrimeVue v4
- Tailwind CSS (no PrimeFlex)
- PrimeVue composables: `useToast`, `useConfirm`

## Theme & Layout

- Nexus Industrial dark-only shell (grid background, 1px border protocol, hard-edge components)
- PrimeVue global PT overrides for:
  - Card / Panel (hazard strip + corner markers)
  - Button (bordered hard switch behavior)
  - ProgressBar (striped fill)
  - Menu/Tab style segmented switching
- Runtime controls:
  - Compact top-right settings popover (Preset + Menu mode)
  - Preset: Aura / Lara / Nora
  - Menu mode: Static / Overlay

## Setup

```bash
cd frontend
pnpm install
pnpm dev
pnpm build
```

## Screens

- Instances
  - Real system telemetry cards (CPU/RAM/GPU)
  - Structured parameter visualization (Runtime/Execution/Sampling/Cache)
  - Integrated per-instance logs panel
- Models (CRUD + download + file view)
- Families/Templates (base parameter + variant diff editing)
- Ports (signal derived from real instance status, not static labels)

All pages call backend APIs at `/api/*` and use toast/confirm flows for mutating actions.
