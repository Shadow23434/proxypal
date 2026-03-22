# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Core commands

- Install deps: `pnpm install`
- Run desktop app in dev mode (frontend + Tauri backend): `pnpm tauri dev`
- Run frontend only: `pnpm dev`
- Frontend build: `pnpm build`
- Desktop production build: `pnpm tauri build`

### Checks and formatting

- Fast TS check (uses `tsgo` if available, falls back to `tsc`): `pnpm check:ts`
- Full frontend check (typecheck + lint + format check): `pnpm check`
- Run checks in parallel: `pnpm check:parallel`
- Lint: `pnpm lint`
- Lint with fixes: `pnpm lint:fix`
- Format: `pnpm format`
- Format check: `pnpm format:check`
- Rust backend check: `cargo check --manifest-path src-tauri/Cargo.toml`

### Tests

- Run all tests once: `pnpm test`
- Watch mode: `pnpm test:watch`
- Run one test file: `pnpm vitest run src/i18n/locale.test.ts`
- Run one test case by name: `pnpm vitest run src/i18n/locale.test.ts -t "<test name>"`

## Architecture (big picture)

ProxyPal is a Tauri v2 desktop app that runs a local proxy sidecar and exposes a local OpenAI-compatible endpoint (default `http://localhost:8317/v1`) so coding tools can use existing provider subscriptions.

### Frontend (`src/`)

- `src/App.tsx` is the app shell and page switcher (dashboard/settings/api-keys/auth-files/logs/analytics).
- `src/stores/app.ts` is the main runtime orchestrator:
  - initializes config + proxy/auth state,
  - registers Tauri event listeners,
  - handles OAuth callback completion,
  - handles tray toggle events,
  - optionally auto-starts proxy,
  - syncs usage stats on startup.
- `src/lib/tauri/*` provides typed wrappers for Tauri `invoke` calls and event listeners; pages/stores should call these wrappers instead of raw `invoke` directly.

### Backend (`src-tauri/src/`)

- `lib.rs` wires the Tauri app:
  - plugin registration,
  - command registration,
  - tray/deep-link behavior,
  - startup migration/cleanup,
  - shared state initialization.
- `commands/*` are the IPC domains (proxy/auth/config/models/usage/logs/ssh/cloudflare/etc.).
- `state.rs` defines shared `AppState` with mutex-protected runtime state (proxy/auth/config/process handles and counters).
- `config.rs` defines `AppConfig`, defaults, migrations, and persistent paths under OS config dir (`config.json`, `auth.json`, `history.json`, `aggregate.json`).

### Proxy lifecycle and data flow

- `commands/proxy.rs` builds `proxy-config.yaml` from `AppConfig`, appends optional `proxy-config-custom.yaml`, then spawns the sidecar (`cli-proxy-api`).
- Management API calls are local (`127.0.0.1`) and authenticated with `management_key` from config.
- Frontend start/stop actions flow through Tauri commands and status events back to `appStore`.

## Repository-specific implementation rules

- Use `pnpm` (both `pnpm-lock.yaml` and `bun.lock` exist, but project scripts are pnpm-based).
- Frontend conventions from `src/AGENTS.md`:
  - use Solid signals/memos patterns,
  - use `class` (not `className`),
  - keep global state in `stores/`.
- Backend conventions from `src-tauri/AGENTS.md`:
  - Tauri IPC functions use `#[tauri::command]` and `Result<T, String>`,
  - shared mutable state goes through `State<AppState>`,
  - IPC structs use serde camelCase.
- If changing config schema (`AppConfig`), also update defaults/migration behavior and frontend config typing (`src/lib/tauri/config.ts`).
