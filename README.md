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

Un tableau de bord interactif en terminal (TUI) conçu en **Rust** pour le monitoring en temps réel de **SAP Integration Suite (Cloud Integration)** sur SAP BTP.

Cet outil permet d'extraire, de stocker et de visualiser les logs de traitement, l'état des artifacts, les packages d'intégration et leurs configurations techniques complexes avec une performance maximale.

---

## Fonctionnalités

### Monitoring & Analytics

- **Logs d'exécution** : Visualisation en temps réel des _Message Processing Logs_ avec récupération automatique des détails d'erreurs pour les messages échoués.
- **Artifacts & Packages** : Inventaire complet des artifacts de runtime et des packages de design-time.
- **Statistiques Avancées** : Graphiques Sparkline (activité sur 12h/24h) et BarCharts (erreurs sur 7j) pour identifier les tendances de pannes.
- **Analytics** : Calcul du taux de succès global et répartition par statuts (COMPLETED, FAILED, STARTED, etc.).

### Recherche & Audit

- **Moteur de recherche dynamique** : Filtrage instantané sur tous les tableaux avec **surlignage (highlighting)** des correspondances.
- **Vue détaillée (Popup)** : Analyse approfondie d'un artifact incluant ses **propriétés de configuration** (Externalized Parameters) et ses erreurs de déploiement.
- **Tags SAP** : Récupération et fusion des métadonnées SAP (_Industries, Keywords, Products, etc._) pour un meilleur classement.

### Performance & Robustesse

- **Moteur Asynchrone** : Propulsé par `Tokio` pour des opérations non-bloquantes.
- **Limitation de Concurrence** : Utilisation de `Streams` avec `buffer_unordered(50)` pour traiter les centaines de requêtes API SAP sans saturation.
- **Persistance PostgreSQL** : Stockage local avec `SQLx` et transactions groupées (`bulk update`) pour une réactivité maximale de l'UI.

---

## Architecture Technique

Le projet est découpé en modules spécialisés pour garantir la maintenabilité :

- **`api.rs`** : Gestionnaire de requêtes OData, client HTTP optimisé (connection pooling) et authentification OAuth2.
- **`db.rs`** : Couche d'accès aux données (DAL) gérant les insertions et les mises à jour en base PostgreSQL.
- **`models.rs`** : Définitions des structures de données pour le désérialisation JSON (Serde) et les vues SQL.
- **`queries.rs`** : Centralisation des requêtes de lecture complexes pour les statistiques et l'interface.
- **`ui.rs`** : Cœur de l'interface utilisateur utilisant `Ratatui`. Gère le rendu des onglets, les événements clavier et le moteur de surlignage.
- **`main.rs`** : Orchestrateur principal gérant le cycle de vie de l'application et la synchronisation des données.

---

## ⚙️ Configuration & Installation

### Pré-requis

- **Rust** (dernière version stable)
- **PostgreSQL** (avec une base de données créée)
- Accès **SAP BTP Integration Suite** (Client ID / Secret avec scope Monitoring)

### Variables d'environnement (`.env`)

Crée un fichier `.env` à la racine du projet :

```env
DATABASE_URL=postgres://user:password@localhost/sap_monitoring
CLIENT_ID=votre_client_id
CLIENT_SECRET=votre_client_secret
```

### Initialisation de la Base de Données

Exécutez les scripts SQL nécessaires pour créer les tables : `sap_monitoring_logs`, `integration_packages`, `runtime_artifacts`, `artifact_errors` et `artifact_configurations`.

---

## 🚀 Utilisation

### Lancement

Pour des performances optimales (fortement recommandé), utilisez le mode **release** :

```bash
cargo run --release
```

### Raccourcis Clavier

| Touche     | Action                                                    |
| :--------- | :-------------------------------------------------------- |
| **Tab**    | Naviguer entre les onglets (Logs, Artifacts, Packages...) |
| **j / k**  | Naviguer dans les listes (Haut / Bas)                     |
| **Entrée** | Voir le détail d'un artifact (Propriétés / Erreur)        |
| **r**      | Forcer une re-extraction complète depuis SAP              |
| **f**      | Filtrer uniquement les logs en échec (FAILED)             |
| **+**      | Charger 500 logs supplémentaires dans l'historique        |
| **Texte**  | Taper directement pour rechercher/filtrer                 |
| **Esc**    | Effacer le filtre actuel / Fermer un popup                |
| **q**      | Quitter l'application                                     |

---

## 📈 Optimisations Réseau

L'outil utilise une stratégie de récupération de données en plusieurs vagues pour contourner les limitations de l'API SAP :

1.  **Extraction Global** : Logs, Packages et Artifacts en parallèle (`tokio::join!`).
2.  **Lien Design/Runtime** : Bouclage sur les packages pour associer les artifacts via l'API DesignTime.
3.  **Audit Profond** : Extraction massive des configurations (`Configurations API`) en flux parallèle limité à 50 requêtes simultanées pour éviter le bannissement IP.

---

### Références des fichiers sources

- Logique API et Réseau : `src/api.rs`
- Gestion de la Base de données : `src/db.rs`
- Interface Utilisateur (Ratatui) : `src/ui.rs`
- Orchestration des données : `src/main.rs`
- Modèles de données : `src/models.rs`
- Requêtes de lecture : `src/queries.rs`
