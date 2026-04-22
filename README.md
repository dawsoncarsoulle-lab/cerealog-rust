# cerealog-rust

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

---

# SAP BTP Monitoring Tool (Rust TUI)

An interactive Terminal User Interface (TUI) dashboard built in **Rust**, designed for real-time monitoring of **SAP Integration Suite (Cloud Integration)** on SAP BTP.

This tool extracts, stores, and visualizes processing logs, artifact statuses, integration packages, and complex technical configurations with maximum performance.

---

## Features

### Monitoring & Analytics

- **Execution Logs**: Real-time visualization of _Message Processing Logs_ with automatic retrieval of error details for failed messages.
- **Artifacts & Packages**: Comprehensive inventory of runtime artifacts and design-time packages.
- **Advanced Statistics**: Sparkline charts (12h/24h activity) and BarCharts (7-day error trends) to identify failure patterns.
- **Analytics**: Global success rate calculation and status distribution (COMPLETED, FAILED, STARTED, etc.).

### Search & Audit

- **Dynamic Search Engine**: Instant filtering across all tables with visual **highlighting** of matches.
- **Detailed View (Popup)** : Deep analysis of an artifact, including its **externalized parameters** (configuration properties) and deployment errors.
- **SAP Tags**: Retrieval and merging of SAP metadata (_Industries, Keywords, Products, etc._) for improved classification.

### Performance & Robustness

- **Asynchronous Engine**: Powered by `Tokio` for non-blocking operations.
- **Concurrency Throttling**: Utilizes `Streams` with `buffer_unordered(50)` to handle hundreds of SAP API requests without saturation.
- **PostgreSQL Persistence**: Local storage via `SQLx` with `bulk update` transactions for maximum UI responsiveness.

---

## Technical Architecture

The project is modularized to ensure maintainability:

- **`api.rs`**: OData request manager, optimized HTTP client (connection pooling), and OAuth2 authentication.
- **`db.rs`**: Data Access Layer (DAL) managing PostgreSQL insertions and updates.
- **`models.rs`**: Data structure definitions for JSON deserialization (Serde) and SQL views.
- **`queries.rs`**: Centralized complex read queries for statistics and the interface.
- **`ui.rs`**: The core UI using `Ratatui`. Handles tab rendering, keyboard events, and the highlighting engine.
- **`main.rs`**: Main orchestrator managing the application lifecycle and data synchronization.

---

## Configuration & Installation

### Prerequisites

- **Rust** (Latest stable version)

Rust can be installed using the following command:

```zsh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

- **PostgreSQL** (With a database already created)

Execute the `table.sql` file to create the required tables.

- **SAP BTP Integration Suite** Access (Client ID / Secret with Monitoring scope)

### Environment Variables (`.env`)

Create a `.env` file at the root of the project:

```env
DATABASE_URL=postgres://user:password@localhost/sap_monitoring
CLIENT_ID=your_client_id
CLIENT_SECRET=your_client_secret
SAP_BASE_URL='https://xxx.it-cpi001.cfapps.eu10.hana.ondemand.com'
SAP_TOKEN_URL='https://xxx.authentication.eu10.hana.ondemand.com/oauth/token?grant_type=client_credentials&token_format=jwt'
```

### Database Initialization

Run the necessary SQL scripts to create the tables: `sap_monitoring_logs`, `integration_packages`, `runtime_artifacts`, `artifact_errors`, and `artifact_configurations`.

---

## Usage

### Launching

For optimal performance (highly recommended), use **release** mode:

```bash
cargo run --release
```

### Keyboard Shortcuts

| Key       | Action                                               |
| :-------- | :--------------------------------------------------- |
| **Tab**   | Navigate between tabs (Logs, Artifacts, Packages...) |
| **j / k** | Navigate through lists (Up / Down)                   |
| **Enter** | View artifact details (Properties / Error)           |
| **r**     | Force a full re-extraction from SAP                  |
| **f**     | Filter only FAILED logs                              |
| **+**     | Load 500 additional logs into history                |
| **Text**  | Type directly to search/filter                       |
| **Esc**   | Clear current filter / Close popup                   |
| **q**     | Quit application                                     |

---

## 📈 Network Optimizations

The tool uses a multi-wave data retrieval strategy to bypass SAP API limitations:

1.  **Global Extraction**: Parallel fetching of Logs, Packages, and Artifacts (`tokio::join!`).
2.  **Design/Runtime Linking**: Loops through packages to associate artifacts via the DesignTime API.
3.  **Deep Audit**: Massive extraction of configurations (`Configurations API`) using a parallel stream limited to 50 simultaneous requests to prevent IP rate-limiting.
