En production, un tableau de bord d'alertes ne doit afficher **que les problèmes actuels non résolus**. Si tu gardes tout l'historique par défaut, les vraies urgences se noient dans la masse.

Voici le plan concret pour implémenter cette bascule magique : par défaut on n'affiche que les **alertes actives** (le dernier log du flux est FAILED), et si tu appuies sur la touche **`h`** (pour Historique), tu bascules sur **toutes les erreurs** depuis le début.

Voici les 3 fichiers à modifier pour mettre ça en place.

### 1. `models.rs` (Ajouter le nouveau canal de données)

On va ajouter une nouvelle liste dans ton `RefreshData` pour transporter ces "Erreurs Actuelles".
_Cherche la structure `RefreshData` tout en bas et ajoute `current_exec_errors` :_

```rust
pub struct RefreshData {
    pub logs: Vec<LogView>,
    pub exec_errors: Vec<LogView>,               // L'historique complet
    pub current_exec_errors: Vec<LogView>,       // <-- NOUVEAU : Uniquement les actives
    pub artifacts: Vec<ArtifactView>,
    // ... reste inchangé
```

### 2. `queries.rs` (La requête SQL intelligente)

On va créer la requête qui utilise une "Window Function" pour ne garder que le dernier statut de chaque flux.
_Ajoute cette nouvelle fonction en bas de `queries.rs` :_

```rust
/// Récupère uniquement les flux dont la TOUTE DERNIÈRE exécution est en échec
pub async fn fetch_current_exec_errors(pool: &sqlx::PgPool) -> anyhow::Result<Vec<LogView>> {
    let rows = sqlx::query_as(
        r#"
        WITH RankedLogs AS (
            SELECT
                status, parsed_date, error_message, message_guid, integration_flow_name,
                ROW_NUMBER() OVER(
                    PARTITION BY integration_flow_name
                    ORDER BY parsed_date DESC NULLS LAST
                ) as rn
            FROM sap_monitoring_logs
        )
        SELECT status, parsed_date, error_message, message_guid, integration_flow_name
        FROM RankedLogs
        WHERE rn = 1 AND status = 'FAILED'
        ORDER BY parsed_date DESC
        LIMIT 200
        "#
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

_Ensuite, au début du fichier dans `build_refresh_data`, ajoute-la au `tokio::join!` :_

```rust
    let (
        logs_res,
        exec_errors_res,
        current_exec_errors_res, // <-- NOUVEAU
        artifacts_res,
        // ...
    ) = tokio::join!(
        fetch_logs(pool, logs_limit),
        fetch_exec_errors(pool),
        fetch_current_exec_errors(pool), // <-- NOUVEAU
        fetch_artifacts(pool),
        // ...
    );

    // Puis dans le Ok(RefreshData { ... })
    Ok(RefreshData {
        logs: logs_res?,
        exec_errors: exec_errors_res?,
        current_exec_errors: current_exec_errors_res?, // <-- NOUVEAU
        // ...
```

### 3. `ui.rs` (La logique d'affichage et la touche `h`)

On doit apprendre à l'interface graphique à basculer entre les deux listes.

**A. Dans la structure `App` (vers la ligne 150) :**

```rust
    pub exec_errors: Vec<LogView>,
    pub current_exec_errors: Vec<LogView>, // <-- NOUVEAU
    pub show_all_exec_errors: bool,        // <-- NOUVEAU (Le flag pour basculer)
```

**B. Dans `App::new()` :**

```rust
            exec_errors: vec![],
            current_exec_errors: vec![], // <-- NOUVEAU
            show_all_exec_errors: false, // <-- NOUVEAU (Par défaut: seulement les actives)
```

**C. Dans `App::apply_refresh_data()` :**

```rust
        self.exec_errors = data.exec_errors;
        self.current_exec_errors = data.current_exec_errors; // <-- NOUVEAU
```

**D. Dans `App::apply_filters()` (La magie opère ici) :**
_Remplace le bloc qui filtre `self.filtered_exec_errors` par ceci :_

```rust
        // On choisit la source de données selon le bouton "Historique"
        let source_exec_errors = if self.show_all_exec_errors {
            &self.exec_errors
        } else {
            &self.current_exec_errors
        };

        self.filtered_exec_errors = source_exec_errors
            .iter()
            // ... garde exactement le même code .filter(|l| { ... }) qu'avant !
```

**E. Raccourci clavier (dans la longue boucle `match key.code` vers la ligne 500) :**
_Ajoute le raccourci `h` (pour Historique) :_

```rust
                    KeyCode::Char('h') if app.active_tab == Tab::ExecErrors => {
                        app.show_all_exec_errors = !app.show_all_exec_errors;
                        app.apply_filters();
                    }
```

**F. Affichage des titres (dans `draw_exec_errors_table` et `draw_footer`) :**
Pour que l'utilisateur comprenne dans quel mode il est, on va changer le titre de la fenêtre `draw_exec_errors_table` :

```rust
    let table_title = if app.show_all_exec_errors {
        "Erreurs d'Exécution — Historique Complet"
    } else {
        "Erreurs d'Exécution — Alertes Actives"
    };

    // Plus bas, remplace le texte en dur "Erreurs d'Exécution — MPL FAILED" par `table_title` dans le `table_block(...)`
```

Et dans `draw_footer`, tu peux ajouter l'astuce pour prévenir l'utilisateur que la touche existe :

```rust
    if app.active_tab == Tab::ExecErrors {
        spans.push(Span::styled(" h ", Style::default().fg(C_BG).bg(C_ACCENT)));
        spans.push(Span::styled(" hist/actif  ", Style::default().fg(C_TEXT_DIM)));
    }
```

### Le Résultat

Maintenant, quand tu lances le TUI et que tu vas sur l'onglet **Err.Exéc**, il ne t'affiche que les alertes vraiment en cours. Si le problème se résout sur BTP et qu'un nouveau log `COMPLETED` arrive, la ligne va disparaître d'elle-même au prochain refresh ! Et si tu veux auditer ce qu'il s'est passé hier, tu appuies sur `h` et boum, tu as tout l'historique ! C'est ultra professionnel.
