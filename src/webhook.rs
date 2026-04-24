//! Worker de webhooks Teams Adaptive Cards avec anti-spam (debouncing).
//!
//! Flux :
//!   1. Lire `pending_alerts`
//!   2. Grouper par `flow_name` + `error_type`
//!   3. Pour chaque groupe, construire une Adaptive Card Teams
//!   4. POST vers `WEBHOOK_URL` (env var)
//!   5. Supprimer les lignes envoyées

use crate::db::{delete_pending_alerts, fetch_pending_alerts, PendingAlert};
use anyhow::Result;
use std::collections::HashMap;

/// Point d'entrée appelé toutes les 60s par le worker dans main.rs.
pub async fn process_pending_alerts(pool: &sqlx::PgPool) -> Result<()> {
    let webhook_url = match std::env::var("WEBHOOK_URL") {
        Ok(u) if !u.is_empty() => u,
        _ => {
            log::debug!("WEBHOOK_URL non définie — webhook désactivé.");
            return Ok(());
        }
    };

    let alerts = fetch_pending_alerts(pool).await?;
    if alerts.is_empty() {
        return Ok(());
    }

    log::info!("{} alertes en attente à traiter.", alerts.len());

    // ── Grouper par (flow_name, error_type) ───────────────────────────────────
    let mut groups: HashMap<(String, String), Vec<PendingAlert>> = HashMap::new();
    for alert in alerts {
        groups
            .entry((alert.flow_name.clone(), alert.error_type.clone()))
            .or_default()
            .push(alert);
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let mut sent_ids: Vec<i64> = Vec::new();

    for ((flow, error_type), group) in &groups {
        let card = build_adaptive_card(flow, error_type, group);

        match client
            .post(&webhook_url)
            .header("Content-Type", "application/json")
            .body(card)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                log::info!(
                    "✓ Webhook envoyé : flow='{}' type='{}' ({} erreur(s))",
                    flow,
                    error_type,
                    group.len()
                );
                sent_ids.extend(group.iter().map(|a| a.id));
            }
            Ok(resp) => {
                log::warn!(
                    "Webhook HTTP {} pour flow='{}': {:?}",
                    resp.status(),
                    flow,
                    resp.text().await.unwrap_or_default()
                );
            }
            Err(e) => {
                log::warn!("Webhook réseau échoué pour flow='{}': {}", flow, e);
            }
        }
    }

    // ── Supprimer les alertes envoyées ────────────────────────────────────────
    if !sent_ids.is_empty() {
        delete_pending_alerts(pool, &sent_ids).await?;
        log::info!("{} alertes supprimées de pending_alerts.", sent_ids.len());
    }

    Ok(())
}

// ─── Construction de la Teams Adaptive Card ───────────────────────────────────

fn build_adaptive_card(flow: &str, error_type: &str, group: &[PendingAlert]) -> String {
    let count = group.len();
    let type_label = if error_type == "exec" {
        "Erreur d'Exécution (MPL FAILED)"
    } else {
        "Erreur de Déploiement BTP"
    };
    let icon = if error_type == "exec" {
        "⚠️"
    } else {
        "🔴"
    };

    // Résumé du premier snippet (le plus récent)
    let first_snippet: String = group
        .first()
        .map(|a| a.error_snippet.chars().take(250).collect())
        .unwrap_or_default();

    // Liste des GUIDs / IDs (max 5)
    let id_list: Vec<String> = group
        .iter()
        .take(5)
        .map(|a| {
            let ts = a.detected_at.format("%d/%m %H:%M").to_string();
            format!("• `{}` — {}", a.log_guid, ts)
        })
        .collect();
    let id_section = id_list.join("\\n");
    let more = if count > 5 {
        format!("… et {} autre(s)", count - 5)
    } else {
        String::new()
    };

    // Teams Adaptive Card v1.4 (format MessageCard simplifié pour compatibilité maximale)
    serde_json::json!({
        "@type": "MessageCard",
        "@context": "http://schema.org/extensions",
        "themeColor": if error_type == "exec" { "FFA500" } else { "CC0000" },
        "summary": format!("{} {} — {} erreur(s) sur {}", icon, type_label, count, flow),
        "sections": [
            {
                "activityTitle": format!("{} **{}**", icon, type_label),
                "activitySubtitle": format!("Flux : **{}** — {} occurrence(s) détectée(s)", flow, count),
                "activityImage": "https://raw.githubusercontent.com/microsoft/fluentui-emoji/main/assets/Warning/3D/warning_3d.png",
                "facts": [
                    { "name": "Flux", "value": flow },
                    { "name": "Type", "value": type_label },
                    { "name": "Occurrences", "value": count.to_string() },
                    { "name": "Première détection", "value": group.first().map(|a| a.detected_at.format("%d/%m/%Y %H:%M:%S").to_string()).unwrap_or_default() },
                ],
                "markdown": true
            },
            {
                "title": "Extrait de l'erreur",
                "text": format!("`{}`", first_snippet)
            },
            {
                "title": format!("Identifiants concernés ({}{})", id_section, if more.is_empty() { String::new() } else { format!("\\n{}", more) }),
                "text": ""
            }
        ],
        "potentialAction": [
            {
                "@type": "OpenUri",
                "name": "Ouvrir SAP BTP",
                "targets": [
                    { "os": "default", "uri": std::env::var("SAP_BASE_URL").unwrap_or_else(|_| "https://cockpit.btp.cloud.sap".to_string()) }
                ]
            }
        ]
    })
    .to_string()
}
