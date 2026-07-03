use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sqlx::PgPool;

use crate::{
    models::{
        ArtifactRow, ConfigurationRow, DataSet, ErrorRow, LogRow, PackageRow, PendingAlertRow,
        SmartAlertRow,
    },
    queries,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Logs,
    Packages,
    Artifacts,
    Errors,
    Configurations,
    Alerts,
}

impl Tab {
    pub const ALL: [Self; 7] = [
        Self::Overview,
        Self::Logs,
        Self::Packages,
        Self::Artifacts,
        Self::Errors,
        Self::Configurations,
        Self::Alerts,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Logs => "Logs",
            Self::Packages => "Packages",
            Self::Artifacts => "Artifacts",
            Self::Errors => "Errors",
            Self::Configurations => "Configurations",
            Self::Alerts => "Alerts",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Selections {
    pub logs: usize,
    pub packages: usize,
    pub artifacts: usize,
    pub errors: usize,
    pub configurations: usize,
    pub alerts: usize,
}

pub struct App {
    pub data: DataSet,
    pub active_tab: Tab,
    pub selections: Selections,
    pub initial_tenant: Option<String>,
    pub selected_tenant: Option<String>,
    pub limit: i64,
    pub refresh_every: Duration,
    pub last_refresh: Option<Instant>,
    pub search: String,
    pub search_input: String,
    pub editing_search: bool,
    pub detail_open: bool,
    pub status: String,
    pub should_quit: bool,
}

impl App {
    pub fn new(initial_tenant: Option<String>, limit: i64, refresh_seconds: u64) -> Self {
        Self {
            data: DataSet::default(),
            active_tab: Tab::Overview,
            selections: Selections::default(),
            selected_tenant: initial_tenant.clone(),
            initial_tenant,
            limit,
            refresh_every: Duration::from_secs(refresh_seconds),
            last_refresh: None,
            search: String::new(),
            search_input: String::new(),
            editing_search: false,
            detail_open: false,
            status: "Demarrage".to_string(),
            should_quit: false,
        }
    }

    pub fn tenant_filter(&self) -> Option<&str> {
        self.selected_tenant.as_deref()
    }

    pub async fn reload(&mut self, pool: &PgPool) {
        match queries::load_dataset(pool, self.tenant_filter(), None, self.limit).await {
            Ok(data) => {
                self.data = data;
                self.clamp_selections();
                self.last_refresh = Some(Instant::now());
                self.status = "Donnees rechargees".to_string();
            }
            Err(err) => {
                self.status = format!("Erreur DB: {err}");
            }
        }
    }

    pub fn needs_refresh(&self) -> bool {
        self.last_refresh
            .map(|last| last.elapsed() >= self.refresh_every)
            .unwrap_or(true)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.editing_search {
            return self.handle_search_key(key);
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('r') => return true,
            KeyCode::Enter => self.detail_open = true,
            KeyCode::Esc => {
                if self.detail_open {
                    self.detail_open = false;
                } else {
                    self.clear_search();
                }
            }
            KeyCode::Char('/') => {
                self.search_input = self.search.clone();
                self.editing_search = true;
                self.status = "Recherche".to_string();
            }
            KeyCode::Tab => self.next_tab(),
            KeyCode::BackTab => self.previous_tab(),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::Char('t') if self.initial_tenant.is_none() => {
                self.next_tenant();
                return true;
            }
            KeyCode::Char('g') if self.initial_tenant.is_none() => {
                self.selected_tenant = None;
                return true;
            }
            _ => {}
        }
        false
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.editing_search = false;
                self.clear_search();
                self.search_input.clear();
                false
            }
            KeyCode::Enter => {
                self.search = self.search_input.trim().to_string();
                self.editing_search = false;
                self.clamp_selections();
                false
            }
            KeyCode::Backspace => {
                self.search_input.pop();
                self.clamp_selections();
                false
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_input.clear();
                self.clamp_selections();
                false
            }
            KeyCode::Char(c) => {
                self.search_input.push(c);
                self.clamp_selections();
                false
            }
            _ => false,
        }
    }

    fn next_tab(&mut self) {
        let current = Tab::ALL
            .iter()
            .position(|tab| *tab == self.active_tab)
            .unwrap_or(0);
        self.active_tab = Tab::ALL[(current + 1) % Tab::ALL.len()];
    }

    fn previous_tab(&mut self) {
        let current = Tab::ALL
            .iter()
            .position(|tab| *tab == self.active_tab)
            .unwrap_or(0);
        self.active_tab = Tab::ALL[(current + Tab::ALL.len() - 1) % Tab::ALL.len()];
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.current_len();
        if len == 0 {
            return;
        }
        let selected = self.current_selection_mut();
        let next = (*selected as isize + delta).clamp(0, len.saturating_sub(1) as isize);
        *selected = next as usize;
    }

    fn next_tenant(&mut self) {
        if self.data.tenants.is_empty() {
            return;
        }

        let next = match self.selected_tenant.as_deref() {
            None => Some(self.data.tenants[0].id.clone()),
            Some(current) => {
                let idx = self
                    .data
                    .tenants
                    .iter()
                    .position(|tenant| tenant.id == current)
                    .unwrap_or(0);
                Some(
                    self.data.tenants[(idx + 1) % self.data.tenants.len()]
                        .id
                        .clone(),
                )
            }
        };
        self.selected_tenant = next;
    }

    fn current_len(&self) -> usize {
        self.current_match_count()
    }

    pub fn current_total_count(&self) -> usize {
        match self.active_tab {
            Tab::Overview => self.data.recent_logs.len(),
            Tab::Logs => self.data.logs.len(),
            Tab::Packages => self.data.packages.len(),
            Tab::Artifacts => self.data.artifacts.len(),
            Tab::Errors => self.data.errors.len(),
            Tab::Configurations => self.data.configurations.len(),
            Tab::Alerts => self.data.alerts.pending.len() + self.data.alerts.smart.len(),
        }
    }

    pub fn current_match_count(&self) -> usize {
        match self.active_tab {
            Tab::Overview => self.filtered_recent_log_indices().len(),
            Tab::Logs => self.filtered_log_indices().len(),
            Tab::Packages => self.filtered_package_indices().len(),
            Tab::Artifacts => self.filtered_artifact_indices().len(),
            Tab::Errors => self.filtered_error_indices().len(),
            Tab::Configurations => self.filtered_configuration_indices().len(),
            Tab::Alerts => self.filtered_alert_indices().len(),
        }
    }

    fn current_selection_mut(&mut self) -> &mut usize {
        match self.active_tab {
            Tab::Overview | Tab::Logs => &mut self.selections.logs,
            Tab::Packages => &mut self.selections.packages,
            Tab::Artifacts => &mut self.selections.artifacts,
            Tab::Errors => &mut self.selections.errors,
            Tab::Configurations => &mut self.selections.configurations,
            Tab::Alerts => &mut self.selections.alerts,
        }
    }

    fn clamp_selections(&mut self) {
        self.selections.logs = clamp(self.selections.logs, self.filtered_log_indices().len());
        self.selections.packages = clamp(
            self.selections.packages,
            self.filtered_package_indices().len(),
        );
        self.selections.artifacts = clamp(
            self.selections.artifacts,
            self.filtered_artifact_indices().len(),
        );
        self.selections.errors = clamp(self.selections.errors, self.filtered_error_indices().len());
        self.selections.configurations = clamp(
            self.selections.configurations,
            self.filtered_configuration_indices().len(),
        );
        self.selections.alerts = clamp(self.selections.alerts, self.filtered_alert_indices().len());
    }

    pub fn active_query(&self) -> &str {
        if self.editing_search {
            self.search_input.trim()
        } else {
            self.search.trim()
        }
    }

    pub fn search_active(&self) -> bool {
        !self.active_query().is_empty()
    }

    pub fn filtered_recent_log_indices(&self) -> Vec<usize> {
        filter_indices(&self.data.recent_logs, self.active_query(), log_fields)
    }

    pub fn filtered_log_indices(&self) -> Vec<usize> {
        filter_indices(&self.data.logs, self.active_query(), log_fields)
    }

    pub fn filtered_package_indices(&self) -> Vec<usize> {
        filter_indices(&self.data.packages, self.active_query(), package_fields)
    }

    pub fn filtered_artifact_indices(&self) -> Vec<usize> {
        filter_indices(&self.data.artifacts, self.active_query(), artifact_fields)
    }

    pub fn filtered_error_indices(&self) -> Vec<usize> {
        filter_indices(&self.data.errors, self.active_query(), error_fields)
    }

    pub fn filtered_configuration_indices(&self) -> Vec<usize> {
        filter_indices(
            &self.data.configurations,
            self.active_query(),
            configuration_fields,
        )
    }

    pub fn filtered_alert_indices(&self) -> Vec<usize> {
        let query = self.active_query();
        let terms = parse_query(query);
        if terms.is_empty() {
            return (0..self.current_total_count()).collect();
        }

        let pending = self
            .data
            .alerts
            .pending
            .iter()
            .enumerate()
            .filter_map(|(index, alert)| {
                row_matches(&pending_alert_fields(alert), &terms).then_some(index)
            });
        let offset = self.data.alerts.pending.len();
        let smart = self
            .data
            .alerts
            .smart
            .iter()
            .enumerate()
            .filter_map(|(index, alert)| {
                row_matches(&smart_alert_fields(alert), &terms).then_some(offset + index)
            });
        pending.chain(smart).collect()
    }

    pub fn selected_log(&self) -> Option<&LogRow> {
        let indices = self.filtered_log_indices();
        indices
            .get(self.selections.logs)
            .and_then(|index| self.data.logs.get(*index))
    }

    pub fn selected_recent_log(&self) -> Option<&LogRow> {
        let indices = self.filtered_recent_log_indices();
        indices
            .get(self.selections.logs)
            .and_then(|index| self.data.recent_logs.get(*index))
    }

    pub fn selected_package(&self) -> Option<&PackageRow> {
        let indices = self.filtered_package_indices();
        indices
            .get(self.selections.packages)
            .and_then(|index| self.data.packages.get(*index))
    }

    pub fn selected_artifact(&self) -> Option<&ArtifactRow> {
        let indices = self.filtered_artifact_indices();
        indices
            .get(self.selections.artifacts)
            .and_then(|index| self.data.artifacts.get(*index))
    }

    pub fn selected_error(&self) -> Option<&ErrorRow> {
        let indices = self.filtered_error_indices();
        indices
            .get(self.selections.errors)
            .and_then(|index| self.data.errors.get(*index))
    }

    pub fn selected_configuration(&self) -> Option<&ConfigurationRow> {
        let indices = self.filtered_configuration_indices();
        indices
            .get(self.selections.configurations)
            .and_then(|index| self.data.configurations.get(*index))
    }

    pub fn selected_alert_text(&self) -> Option<String> {
        let indices = self.filtered_alert_indices();
        let index = *indices.get(self.selections.alerts)?;
        let pending_len = self.data.alerts.pending.len();
        if index < pending_len {
            let alert = self.data.alerts.pending.get(index)?;
            Some(format!(
                "pending_alerts\ntenant_id: {}\nlog_guid: {}\nflow_name: {}\nerror_type: {}\ndetected_at: {}\n\n{}",
                alert.tenant_id,
                alert.log_guid,
                alert.flow_name.as_deref().unwrap_or("-"),
                alert.error_type.as_deref().unwrap_or("-"),
                crate::ui::fmt_dt(alert.detected_at),
                alert.error_snippet.as_deref().unwrap_or("-")
            ))
        } else {
            let alert = self.data.alerts.smart.get(index - pending_len)?;
            Some(format!(
                "smart_alerts\ntenant_id: {}\nflow_name: {}\nalert_type: {}\nstatus: {}\nlast_triggered_at: {}\n\n{}",
                alert.tenant_id,
                alert.flow_name,
                alert.alert_type,
                alert.status,
                crate::ui::fmt_dt(alert.last_triggered_at),
                alert.extra.as_deref().unwrap_or("-")
            ))
        }
    }

    fn clear_search(&mut self) {
        self.search.clear();
        self.search_input.clear();
        self.editing_search = false;
        self.clamp_selections();
    }
}

fn clamp(index: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        index.min(len - 1)
    }
}

#[derive(Debug)]
struct SearchTerm {
    key: Option<String>,
    value: String,
}

fn filter_indices<T>(
    items: &[T],
    query: &str,
    fields: fn(&T) -> Vec<(String, String)>,
) -> Vec<usize> {
    let terms = parse_query(query);
    if terms.is_empty() {
        return (0..items.len()).collect();
    }

    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| row_matches(&fields(item), &terms).then_some(index))
        .collect()
}

fn parse_query(query: &str) -> Vec<SearchTerm> {
    query
        .split_whitespace()
        .filter_map(|raw| {
            let raw = raw.trim();
            if raw.is_empty() {
                return None;
            }
            let (key, value) = raw
                .split_once(':')
                .map(|(key, value)| (Some(normalize_key(key)), value))
                .unwrap_or((None, raw));
            let value = value.trim().to_lowercase();
            (!value.is_empty()).then_some(SearchTerm { key, value })
        })
        .collect()
}

fn normalize_key(key: &str) -> String {
    match key.trim().to_lowercase().as_str() {
        "statut" | "etat" => "status",
        "flux" | "iflow" => "flow",
        "erreur" => "error",
        other => other,
    }
    .to_string()
}

fn row_matches(fields: &[(String, String)], terms: &[SearchTerm]) -> bool {
    terms.iter().all(|term| {
        fields.iter().any(|(key, value)| {
            let key_ok = term
                .key
                .as_ref()
                .map(|term_key| key == term_key)
                .unwrap_or(true);
            key_ok && value.to_lowercase().contains(&term.value)
        })
    })
}

fn text(value: Option<&str>) -> String {
    value.unwrap_or("").to_string()
}

fn log_fields(log: &LogRow) -> Vec<(String, String)> {
    vec![
        ("tenant".to_string(), log.tenant_id.clone()),
        ("status".to_string(), text(log.status.as_deref())),
        (
            "flow".to_string(),
            text(log.integration_flow_name.as_deref()),
        ),
        ("guid".to_string(), log.message_guid.clone()),
        ("error".to_string(), text(log.error_message.as_deref())),
    ]
}

fn package_fields(package: &PackageRow) -> Vec<(String, String)> {
    vec![
        ("tenant".to_string(), package.tenant_id.clone()),
        ("package".to_string(), package.id.clone()),
        ("id".to_string(), package.id.clone()),
        ("name".to_string(), text(package.name.as_deref())),
        ("version".to_string(), text(package.version.as_deref())),
        ("vendor".to_string(), text(package.vendor.as_deref())),
        ("tags".to_string(), text(package.tags.as_deref())),
    ]
}

fn artifact_fields(artifact: &ArtifactRow) -> Vec<(String, String)> {
    vec![
        ("tenant".to_string(), artifact.tenant_id.clone()),
        ("artifact".to_string(), artifact.id.clone()),
        ("id".to_string(), artifact.id.clone()),
        ("name".to_string(), text(artifact.name.as_deref())),
        ("status".to_string(), text(artifact.status.as_deref())),
        ("package".to_string(), text(artifact.package_id.as_deref())),
        ("type".to_string(), text(artifact.artifact_type.as_deref())),
    ]
}

fn error_fields(error: &ErrorRow) -> Vec<(String, String)> {
    vec![
        ("tenant".to_string(), error.tenant_id.clone()),
        ("artifact".to_string(), error.artifact_id.clone()),
        ("error".to_string(), text(error.error_message.as_deref())),
    ]
}

fn configuration_fields(config: &ConfigurationRow) -> Vec<(String, String)> {
    vec![
        ("tenant".to_string(), config.tenant_id.clone()),
        ("artifact".to_string(), config.artifact_id.clone()),
        ("config".to_string(), config.parameter_key.clone()),
        ("key".to_string(), config.parameter_key.clone()),
        ("value".to_string(), text(config.parameter_value.as_deref())),
        ("type".to_string(), text(config.data_type.as_deref())),
    ]
}

fn pending_alert_fields(alert: &PendingAlertRow) -> Vec<(String, String)> {
    vec![
        ("tenant".to_string(), alert.tenant_id.clone()),
        ("guid".to_string(), alert.log_guid.clone()),
        ("flow".to_string(), text(alert.flow_name.as_deref())),
        ("error".to_string(), text(alert.error_snippet.as_deref())),
        ("type".to_string(), text(alert.error_type.as_deref())),
    ]
}

fn smart_alert_fields(alert: &SmartAlertRow) -> Vec<(String, String)> {
    vec![
        ("tenant".to_string(), alert.tenant_id.clone()),
        ("flow".to_string(), alert.flow_name.clone()),
        ("status".to_string(), alert.status.clone()),
        ("type".to_string(), alert.alert_type.clone()),
        ("error".to_string(), text(alert.extra.as_deref())),
    ]
}
