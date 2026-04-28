use crate::crypto;
use anyhow::Result;
use std::io::{self, Write};

fn prompt(label: &str) -> String {
    print!("{}: ", label);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

fn prompt_secret(label: &str) -> String {
    print!("{}: ", label);
    io::stdout().flush().unwrap();
    rpassword::read_password().unwrap_or_default()
}

pub async fn add_tenant_interactive(pool: &sqlx::PgPool) -> Result<()> {
    println!("\n─── Ajout d'un nouveau tenant ───\n");

    let id = prompt("ID tenant (ex: cerealog, client_a)");
    let name = prompt("Nom");
    let client_name = prompt("Nom du client");
    let sap_base_url = prompt("SAP Base URL");
    let sap_token_url = prompt("SAP Token URL");
    let client_id = prompt("Client ID OAuth");
    let client_secret = prompt_secret("Client Secret OAuth (masqué)");

    let secret_enc = crypto::encrypt(&client_secret)?;

    sqlx::query(
        "INSERT INTO tenants (id, name, client_name, shared_tenant, sap_base_url, sap_token_url, sap_client_id, sap_client_secret_enc, active)
         VALUES ($1, $2, $3, false, $4, $5, $6, $7, true)
         ON CONFLICT (id) DO UPDATE SET
           name = EXCLUDED.name,
           client_name = EXCLUDED.client_name,
           sap_base_url = EXCLUDED.sap_base_url,
           sap_token_url = EXCLUDED.sap_token_url,
           sap_client_id = EXCLUDED.sap_client_id,
           sap_client_secret_enc = EXCLUDED.sap_client_secret_enc"
    )
    .bind(&id)
    .bind(&name)
    .bind(&client_name)
    .bind(&sap_base_url)
    .bind(&sap_token_url)
    .bind(&client_id)
    .bind(&secret_enc)
    .execute(pool)
    .await?;

    println!("\n✓ Tenant '{}' ajouté/mis à jour avec succès.", id);
    Ok(())
}
