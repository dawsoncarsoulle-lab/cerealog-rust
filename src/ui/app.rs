use crate::config::UserConfig;
use crate::db::{ArtifactView, ErrorView, LogView, PackageView};
use crate::models::{RefreshData, Stats};
use crate::ui::tabs::Tab;
use chrono::NaiveDate;
use ratatui::widgets::TableState;
use std::time::Instant;

// ─── Search Helpers ────────────────────────────────────────────────────────
/// Parse a search query into free terms and key:value filters.
/// Tokens containing a colon are split on the first colon into a key and value.
/// All other tokens are treated as free text. Returned values keep their original case.
fn parse_query(query: &str) -> (Vec<String>, Vec<(String, String)>) {
    let mut free_terms = Vec::new();
    let mut kv_pairs = Vec::new();
    for token in query.split_whitespace() {
        if let Some(idx) = token.find(':') {
            let (k, rest) = token.split_at(idx);
            let v = rest[1..].to_string();
            kv_pairs.push((k.to_string(), v));
        } else {
            free_terms.push(token.to_string());
        }
    }
    (free_terms, kv_pairs)
}

/// Normalize known search keys to canonical names. Unknown keys return None and are ignored.
/// This helper allows aliases in multiple languages (e.g., "statut" → "status", "flux" → "flow").
fn unify_key(key: &str) -> Option<String> {
    match key.to_lowercase().as_str() {
        "status" | "statut" | "etat" => Some("status".to_string()),
        "flow" | "flux" | "iflow" | "integration_flow" | "integrationflow" | "integration" => {
            Some("flow".to_string())
        }
        "tenant" | "tenant_id" | "tenantid" | "ten" => Some("tenant".to_string()),
        "guid" | "message_guid" | "messageguid" | "msg_guid" | "id" => Some("guid".to_string()),
        "error" | "err" | "error_message" | "errormessage" => Some("error".to_string()),
        _ => None,
    }
}

/// Normalize statuses and synonyms to a canonical lowercase string.
/// For example, "error", "failed", "erreur" all map to "failed", and
/// "ok", "completed", "success" map to "completed".
fn unify_status(s: &str) -> String {
    match s.to_lowercase().as_str() {
        "failed" | "fail" | "error" | "erreur" | "failure" => "failed".to_string(),
        "ok" | "completed" | "success" | "succeeded" | "done" => "completed".to_string(),
        "canceled" | "cancelled" => "canceled".to_string(),
        other => other.to_string(),
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum PackageFocus {
    List,
    Artifacts,
    Activity,
}

pub struct App {
    pub active_tab: Tab,
    pub table_states: [TableState; 6],
    pub package_focus: PackageFocus,
    pub pkg_art_state: TableState,
    pub pkg_log_state: TableState,

    pub logs: Vec<LogView>,
    pub exec_errors: Vec<LogView>,
    pub active_exec_errors: Vec<LogView>,
    pub artifacts: Vec<ArtifactView>,
    pub packages: Vec<PackageView>,
    pub deploy_errors: Vec<ErrorView>,
    pub configs: std::collections::HashMap<String, Vec<(String, String)>>,
    pub error_sparkline: Vec<u64>,
    pub error_barchart: Vec<(String, u64)>,
    pub activity_sparkline: Vec<u64>,
    pub top_errors_barchart: Vec<(String, u64)>,
    pub status_counts: Vec<(String, u64)>,
    pub exec_errors_history_mode: bool,

    pub filtered_logs: Vec<LogView>,
    pub filtered_exec_errors: Vec<LogView>,
    pub filtered_active_exec_errors: Vec<LogView>,
    pub filtered_artifacts: Vec<ArtifactView>,
    pub filtered_packages: Vec<PackageView>,
    pub filtered_deploy_errors: Vec<ErrorView>,
    pub tenant_filter: Option<String>,

    pub stats: Stats,
    pub search_query: String,
    pub search_active: bool,
    pub date_filter: Option<(NaiveDate, NaiveDate)>,
    pub logs_limit: u32,
    pub overlay: OverlayState,
    pub last_tick: Instant,
    pub last_refresh: Instant,
    pub user_config: UserConfig,
    pub refreshing: bool,
    pub refresh_spinner: u8,
}

// ─── Overlay ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub enum OverlayState {
    Hidden,
    TenantFilter {
        selected: usize,
    },
    Done {
        message: String,
    },
    Error {
        message: String,
    },
    ArtifactDetail {
        name: String,
        status: String,
        error: Option<String>,
        configs: Vec<(String, String)>,
    },
    LogDetail {
        guid: String,
        status: String,
        date: String,
        flow: String,
        error: String,
    },
    Calendar(CalendarState),
}

// ─── Calendar ────────────────────────────────────────────────────────────────

use chrono::{Datelike, Days};

#[derive(Clone)]
pub struct CalendarState {
    pub displayed_month: NaiveDate,
    pub cursor: NaiveDate,
    pub phase: u8,
    pub date_start: Option<NaiveDate>,
    pub date_end: Option<NaiveDate>,
}

impl CalendarState {
    pub fn new() -> Self {
        let today = chrono::Local::now().date_naive();
        Self {
            displayed_month: today.with_day(1).unwrap_or(today),
            cursor: today,
            phase: 0,
            date_start: None,
            date_end: None,
        }
    }

    pub fn prev_month(&mut self) {
        let d = self.displayed_month;
        self.displayed_month = if d.month() == 1 {
            NaiveDate::from_ymd_opt(d.year() - 1, 12, 1).unwrap_or(d)
        } else {
            NaiveDate::from_ymd_opt(d.year(), d.month() - 1, 1).unwrap_or(d)
        };
    }

    pub fn next_month(&mut self) {
        let d = self.displayed_month;
        self.displayed_month = if d.month() == 12 {
            NaiveDate::from_ymd_opt(d.year() + 1, 1, 1).unwrap_or(d)
        } else {
            NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1).unwrap_or(d)
        };
    }

    pub fn cursor_left(&mut self) {
        if let Some(d) = self.cursor.checked_sub_days(Days::new(1)) {
            self.cursor = d;
            self.sync_month();
        }
    }

    pub fn cursor_right(&mut self) {
        if let Some(d) = self.cursor.checked_add_days(Days::new(1)) {
            self.cursor = d;
            self.sync_month();
        }
    }

    pub fn cursor_up(&mut self) {
        if let Some(d) = self.cursor.checked_sub_days(Days::new(7)) {
            self.cursor = d;
            self.sync_month();
        }
    }

    pub fn cursor_down(&mut self) {
        if let Some(d) = self.cursor.checked_add_days(Days::new(7)) {
            self.cursor = d;
            self.sync_month();
        }
    }

    fn sync_month(&mut self) {
        if let Some(first) = NaiveDate::from_ymd_opt(self.cursor.year(), self.cursor.month(), 1) {
            self.displayed_month = first;
        }
    }

    pub fn confirm(&mut self) -> bool {
        if self.phase == 0 {
            self.date_start = Some(self.cursor);
            self.date_end = None;
            self.phase = 1;
            false
        } else {
            let end = self.cursor;
            let start = self.date_start.unwrap_or(end);
            if end < start {
                self.date_start = Some(end);
                self.date_end = Some(start);
            } else {
                self.date_end = Some(end);
            }
            true
        }
    }
}

// ─── App impl ────────────────────────────────────────────────────────────────

impl App {
    pub fn new(user_config: UserConfig) -> Self {
        let logs_limit = user_config.logs_limit;
        let make_state = || {
            let mut s = TableState::default();
            s.select(Some(0));
            s
        };
        Self {
            active_tab: Tab::Logs,
            table_states: [
                make_state(),
                make_state(),
                make_state(),
                make_state(),
                make_state(),
                make_state(),
            ],
            package_focus: PackageFocus::List,
            pkg_art_state: make_state(),
            pkg_log_state: make_state(),
            logs: vec![],
            exec_errors: vec![],
            active_exec_errors: vec![],
            artifacts: vec![],
            packages: vec![],
            deploy_errors: vec![],
            configs: std::collections::HashMap::new(),
            error_sparkline: vec![],
            error_barchart: vec![],
            activity_sparkline: vec![],
            top_errors_barchart: vec![],
            status_counts: vec![],
            filtered_logs: vec![],
            filtered_exec_errors: vec![],
            filtered_active_exec_errors: vec![],
            filtered_artifacts: vec![],
            filtered_packages: vec![],
            filtered_deploy_errors: vec![],
            stats: Stats::default(),
            search_query: String::new(),
            search_active: false,
            date_filter: None,
            tenant_filter: None,
            logs_limit,
            overlay: OverlayState::Hidden,
            last_tick: Instant::now(),
            last_refresh: Instant::now(),
            user_config,
            refreshing: false,
            refresh_spinner: 0,
            exec_errors_history_mode: false,
        }
    }

    pub fn tab_index(tab: Tab) -> usize {
        match tab {
            Tab::Logs => 0,
            Tab::Artifacts => 1,
            Tab::Packages => 2,
            Tab::DeployErrors => 3,
            Tab::ExecErrors => 4,
            Tab::Analytics => 5,
        }
    }

    pub fn table_state(&mut self) -> &mut TableState {
        let i = Self::tab_index(self.active_tab);
        &mut self.table_states[i]
    }

    pub fn selected(&self) -> Option<usize> {
        let i = Self::tab_index(self.active_tab);
        self.table_states[i].selected()
    }

    pub fn select(&mut self, idx: Option<usize>) {
        let i = Self::tab_index(self.active_tab);
        self.table_states[i].select(idx);
    }

    pub fn apply_refresh_data(&mut self, data: RefreshData) {
        self.logs = data.logs;
        self.exec_errors = data.exec_errors;
        self.active_exec_errors = data.active_exec_errors;
        self.artifacts = data.artifacts;
        self.packages = data.packages;
        self.deploy_errors = data.deploy_errors;
        self.configs = data.configs;
        self.stats = data.stats;
        self.error_sparkline = data.error_sparkline;
        self.error_barchart = data.error_barchart;
        self.activity_sparkline = data.activity_sparkline;
        self.top_errors_barchart = data.top_errors_barchart;
        self.status_counts = data.status_counts;
        self.last_refresh = Instant::now();
        self.apply_filters();
    }

    pub fn apply_filters(&mut self) {
        // Smart search (version B): parse key:value pairs, fuzzy matching and synonyms
        // We keep the original order of items (chronological order) while filtering.
        let raw_query = self.search_query.trim();
        let (free_terms, mut filters) = parse_query(raw_query);

        // canonicalize keys and drop unknowns
        filters = filters
            .into_iter()
            .filter_map(|(k, v)| unify_key(&k).map(|key| (key, v)))
            .collect();

        // Prepare date and tenant filters
        let date_range = self.date_filter;
        let tenant_filter = self.tenant_filter.clone();
        let tenant_ok = |tenant: Option<&str>| -> bool {
            tenant_filter
                .as_deref()
                .map(|expected| tenant == Some(expected))
                .unwrap_or(true)
        };
        let date_ok = |d: Option<chrono::NaiveDateTime>| -> bool {
            match (date_range, d) {
                (Some((start, end)), Some(dt)) => {
                    let day = dt.date();
                    day >= start && day <= end
                }
                (Some(_), None) => false,
                (None, _) => true,
            }
        };

        // Helper: check if a log matches all filters
        let log_filter_ok = |l: &LogView| -> bool {
            for (key, val) in &filters {
                let expected = val.to_lowercase();
                match key.as_str() {
                    "status" => {
                        // Canonicalize both sides
                        let cur = l.status.as_deref().unwrap_or("").to_lowercase();
                        if unify_status(&cur) != unify_status(&expected) {
                            return false;
                        }
                    }
                    "flow" => {
                        let cur = l
                            .integration_flow_name
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase();
                        if !cur.contains(&expected) {
                            return false;
                        }
                    }
                    "tenant" => {
                        let cur = l.tenant_id.as_deref().unwrap_or("").to_lowercase();
                        if !cur.contains(&expected) {
                            return false;
                        }
                    }
                    "guid" => {
                        let cur = l.message_guid.as_deref().unwrap_or("").to_lowercase();
                        if !cur.contains(&expected) {
                            return false;
                        }
                    }
                    "error" => {
                        let cur = l.error_message.as_deref().unwrap_or("").to_lowercase();
                        if !cur.contains(&expected) {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
            true
        };

        // Helper: check if all free terms appear somewhere in the log
        let log_free_ok = |l: &LogView| -> bool {
            if free_terms.is_empty() {
                return true;
            }
            for term in &free_terms {
                let t = term.to_lowercase();
                let mut found = false;
                if l.status
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                } else if l
                    .error_message
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                } else if l
                    .message_guid
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                } else if l
                    .integration_flow_name
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                } else if l
                    .tenant_id
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                }
                if !found {
                    return false;
                }
            }
            true
        };

        let log_matches = |l: &&LogView| -> bool {
            log_filter_ok(*l)
                && log_free_ok(*l)
                && date_ok(l.parsed_date)
                && tenant_ok(l.tenant_id.as_deref())
        };

        // Filter logs preserving order
        self.filtered_logs = self.logs.iter().filter(log_matches).cloned().collect();
        self.filtered_exec_errors = self
            .exec_errors
            .iter()
            .filter(log_matches)
            .cloned()
            .collect();
        self.filtered_active_exec_errors = self
            .active_exec_errors
            .iter()
            .filter(log_matches)
            .cloned()
            .collect();

        // Artifacts: only apply free terms, date and tenant filters
        let artifact_free_ok = |a: &ArtifactView| -> bool {
            if free_terms.is_empty() {
                return true;
            }
            for term in &free_terms {
                let t = term.to_lowercase();
                let mut found = false;
                if a.name.as_deref().unwrap_or("").to_lowercase().contains(&t) {
                    found = true;
                } else if a
                    .status
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                } else if a.id.as_deref().unwrap_or("").to_lowercase().contains(&t) {
                    found = true;
                } else if a
                    .package_id
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                } else if a
                    .tenant_id
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                }
                if !found {
                    return false;
                }
            }
            true
        };
        self.filtered_artifacts = self
            .artifacts
            .iter()
            .filter(|a| {
                artifact_free_ok(a) && date_ok(a.deployed_on) && tenant_ok(a.tenant_id.as_deref())
            })
            .cloned()
            .collect();

        // Packages: only apply free terms, date and tenant filters
        let package_free_ok = |p: &PackageView| -> bool {
            if free_terms.is_empty() {
                return true;
            }
            for term in &free_terms {
                let t = term.to_lowercase();
                let mut found = false;
                if p.id.as_deref().unwrap_or("").to_lowercase().contains(&t) {
                    found = true;
                } else if p.name.as_deref().unwrap_or("").to_lowercase().contains(&t) {
                    found = true;
                } else if p
                    .vendor
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                } else if p.tags.as_deref().unwrap_or("").to_lowercase().contains(&t) {
                    found = true;
                } else if p
                    .tenant_id
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                }
                if !found {
                    return false;
                }
            }
            true
        };
        self.filtered_packages = self
            .packages
            .iter()
            .filter(|p| {
                package_free_ok(p) && date_ok(p.creation_date) && tenant_ok(p.tenant_id.as_deref())
            })
            .cloned()
            .collect();

        // Deploy errors: apply free terms and filters for tenant and date
        let deploy_free_ok = |e: &ErrorView| -> bool {
            if free_terms.is_empty() {
                return true;
            }
            for term in &free_terms {
                let t = term.to_lowercase();
                let mut found = false;
                if e.artifact_id.to_lowercase().contains(&t) {
                    found = true;
                } else if e
                    .error_message
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                } else if e
                    .tenant_id
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&t)
                {
                    found = true;
                }
                if !found {
                    return false;
                }
            }
            true
        };
        self.filtered_deploy_errors = self
            .deploy_errors
            .iter()
            .filter(|e| {
                deploy_free_ok(e) && date_ok(e.error_time) && tenant_ok(e.tenant_id.as_deref())
            })
            .cloned()
            .collect();

        let len = self.current_len();
        if len == 0 {
            self.select(None);
        } else if let Some(i) = self.selected() {
            if i >= len {
                self.select(Some(len - 1));
            }
        } else {
            self.select(Some(0));
        }
    }

    pub fn current_len(&self) -> usize {
        match self.active_tab {
            Tab::Logs => self.filtered_logs.len(),
            Tab::Artifacts => self.filtered_artifacts.len(),
            Tab::Packages => self.filtered_packages.len(),
            Tab::DeployErrors => self.filtered_deploy_errors.len(),
            Tab::ExecErrors => {
                if self.exec_errors_history_mode {
                    self.filtered_exec_errors.len()
                } else {
                    self.filtered_active_exec_errors.len()
                }
            }
            Tab::Analytics => 0,
        }
    }

    pub fn next_row(&mut self) {
        if self.active_tab == Tab::Packages && self.package_focus != PackageFocus::List {
            match self.package_focus {
                PackageFocus::Artifacts => {
                    let i = self.pkg_art_state.selected().unwrap_or(0);
                    self.pkg_art_state.select(Some(i + 1));
                }
                PackageFocus::Activity => {
                    let i = self.pkg_log_state.selected().unwrap_or(0);
                    self.pkg_log_state.select(Some(i + 1));
                }
                _ => {}
            }
            return;
        }
        let len = self.current_len();
        if len == 0 {
            return;
        }
        let i = self.selected().unwrap_or(0);
        self.select(Some((i + 1).min(len - 1)));
    }

    pub fn prev_row(&mut self) {
        if self.active_tab == Tab::Packages && self.package_focus != PackageFocus::List {
            match self.package_focus {
                PackageFocus::Artifacts => {
                    let i = self.pkg_art_state.selected().unwrap_or(0);
                    self.pkg_art_state.select(Some(i.saturating_sub(1)));
                }
                PackageFocus::Activity => {
                    let i = self.pkg_log_state.selected().unwrap_or(0);
                    self.pkg_log_state.select(Some(i.saturating_sub(1)));
                }
                _ => {}
            }
            return;
        }
        let i = self.selected().unwrap_or(0);
        self.select(Some(i.saturating_sub(1)));
    }

    pub fn next_tab(&mut self) {
        self.package_focus = PackageFocus::List;
        let tabs = Tab::all();
        let i = (self.active_tab.index() + 1) % tabs.len();
        self.active_tab = tabs[i];
    }

    pub fn prev_tab(&mut self) {
        self.package_focus = PackageFocus::List;
        let tabs = Tab::all();
        let i = self.active_tab.index();
        self.active_tab = tabs[if i == 0 { tabs.len() - 1 } else { i - 1 }];
    }

    pub fn tick(&mut self) {
        if self.refreshing {
            self.refresh_spinner = self.refresh_spinner.wrapping_add(1);
        }
    }

    pub fn padded_sparkline(data: &[u64], num_points: usize) -> Vec<u64> {
        if data.len() >= num_points {
            data[data.len() - num_points..].to_vec()
        } else {
            let mut p = vec![0u64; num_points - data.len()];
            p.extend_from_slice(data);
            p
        }
    }
    pub fn available_tenants(&self) -> Vec<String> {
        let mut tenants = std::collections::HashSet::new();

        for log in &self.logs {
            if let Some(t) = &log.tenant_id {
                tenants.insert(t.clone());
            }
        }
        for art in &self.artifacts {
            if let Some(t) = &art.tenant_id {
                tenants.insert(t.clone());
            }
        }
        for pkg in &self.packages {
            if let Some(t) = &pkg.tenant_id {
                tenants.insert(t.clone());
            }
        }

        let mut result: Vec<String> = tenants.into_iter().collect();
        result.sort();
        result
    }
}
