# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build
cargo run
cargo run -- --top <N>      # preload N logs on startup
cargo run -- --health       # check SAP + DB connectivity and exit
```

## Architecture

This is a Rust TUI app (`sap-extractor`) that fetches SAP BTP integration logs and displays them interactively.

**Data flow:** SAP BTP OData API → `api.rs` → PostgreSQL via `db.rs` → `refresh_views` → `App` state in `ui.rs` → ratatui renders

**Key files:**
- `src/main.rs` — CLI args (clap), DB connection, initial data load, TUI event loop
- `src/api.rs` — SAP BTP HTTP client: OAuth token, integration logs, packages, artifacts, errors (reqwest)
- `src/db.rs` — PostgreSQL persistence (sqlx): inserts logs, packages, artifacts, errors
- `src/models.rs` — serde/sqlx structs: `LogEntry`, `IntegrationPackage`, `RuntimeArtifact`, `ArtifactError`, OData wrappers
- `src/ui.rs` — ratatui `App` struct, `OverlayState` enum, `run_tui` render/event loop
