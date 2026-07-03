use crate::app::App;

use super::fmt_dt;

pub fn tenant_label(app: &App) -> String {
    match app.selected_tenant.as_deref() {
        Some(id) => app
            .data
            .tenants
            .iter()
            .find(|tenant| tenant.id == id)
            .map(|tenant| {
                let client = tenant.client_name.as_deref().unwrap_or("-");
                let shared = if tenant.shared_tenant.unwrap_or(false) {
                    "shared"
                } else {
                    "dedicated"
                };
                let state = if tenant.active.unwrap_or(false) {
                    "active"
                } else {
                    "inactive"
                };
                format!(
                    "{} - {} ({client}, {shared}, {state}, created_at {})",
                    tenant.id,
                    tenant.name,
                    fmt_dt(tenant.created_at)
                )
            })
            .unwrap_or_else(|| id.to_string()),
        None => "global".to_string(),
    }
}
