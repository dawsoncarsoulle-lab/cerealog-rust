# SAP BTP Monitor — Daemon / Backend Worker

Cette version supprime complètement le TUI (`ratatui`, `crossterm`) et transforme le projet en worker backend compilable en binaire.

## Modes principaux

### Airflow / BashOperator recommandé

```bash
./sap-extractor --once --top 10000 --page-size 1000 --tenant cerealog
```

Le process exécute un cycle, écrit en base PostgreSQL, traite les webhooks, puis quitte avec :

- code `0` si tous les tenants ont réussi ;
- code non nul si au moins un tenant échoue.

Exemple Airflow :

```python
from airflow.operators.bash import BashOperator

sync_sap_btp = BashOperator(
    task_id="sync_sap_btp",
    bash_command="/opt/sap-monitor/sap-extractor --once --top 10000 --page-size 1000 --tenant cerealog",
)
```

### Daemon long-running

```bash
./sap-extractor --daemon --interval-seconds 300 --top 5000
```

Le process garde les tokens OAuth en mémoire et relance un cycle toutes les 5 minutes.

## Commandes utiles

```bash
# Vérifier DB + OAuth SAP
./sap-extractor --health

# Ajouter un tenant chiffré dans PostgreSQL
./sap-extractor --add-tenant

# Backfill massif sans récupérer ErrorInformation/$value pour chaque FAILED
./sap-extractor --once --full --top 1000000 --page-size 5000 --skip-error-details

# Logs uniquement
./sap-extractor --once --logs-only --top 50000

# Métadonnées uniquement
./sap-extractor --once --metadata-only

# Plusieurs tenants en parallèle, mais limité à 4 tenants simultanés
./sap-extractor --once --tenant-concurrency 4 --top 10000
```

## Options de performance

- `--top` : volume maximum de MPL par tenant/cycle.
- `--page-size` : taille de page SAP OData. Maximum volontaire : `5000`.
- `--log-concurrency` : concurrence pour les appels `ErrorInformation/$value` des MPL FAILED.
- `--metadata-concurrency` : concurrence packages/artifacts/configurations.
- `--tenant-concurrency` : nombre de tenants synchronisés en parallèle. `0` = tous.
- `--db-connections` : taille du pool PostgreSQL.
- `--skip-error-details` : très important pour les backfills massifs, car sinon chaque MPL FAILED peut déclencher un appel HTTP additionnel.

## Notes d'architecture

- Les insertions SQL sont batchées pour éviter la limite PostgreSQL de paramètres par requête.
- Le client HTTP `reqwest` est partagé et garde un pool de connexions.
- Les logs MPL sont récupérés par pagination `$top` + `$skip`.
- Les alertes Teams restent disponibles via `pending_alerts` et `smart_alerts`.
- Le fichier `table.sql` contient les index recommandés pour les gros volumes.


## Correction HTTP/OData v0.4.1

Les endpoints SAP OData sont maintenant appelés avec `$format=json` sur toutes les ressources JSON (`MessageProcessingLogs`, `IntegrationPackages`, `IntegrationRuntimeArtifacts`, configurations, designtime artifacts).

En cas de réponse non JSON, le daemon n'affiche plus seulement `expected value at line 1 column 1`. Il affiche maintenant : tenant, endpoint logique, status HTTP, `Content-Type`, URL appelée et les 600 premiers caractères du body reçu.

La récupération des logs insère désormais page par page en PostgreSQL. Même avec `--top 1000000`, le daemon ne garde plus le million de logs en RAM avant insertion.

Test conseillé après rebuild :

```bash
cargo build --release
./target/release/sap-extractor --once --logs-only --top 100 --page-size 100 --skip-error-details
```

Puis augmenter progressivement :

```bash
./target/release/sap-extractor --once --logs-only --top 50000 --page-size 1000 --skip-error-details
./target/release/sap-extractor --once --full --top 1000000 --page-size 5000 --skip-error-details
```


## Note pagination SAP CPI

Certains tenants SAP CPI plafonnent `MessageProcessingLogs` à 1000 lignes par page, même si `--page-size` demande 5000. Depuis la v0.4.2, le daemon continue correctement la pagination avec `$skip += lignes_reçues` au lieu de stopper dès qu'une page est plus petite que la taille demandée.

Commande sûre pour 50 000 dernières logs :

```bash
./sap-extractor --once --full --logs-only --top 50000 --page-size 5000 --skip-error-details --skip-webhooks
```

En cas de throttling ou comportement SAP bizarre, utiliser `--page-size 1000`, qui correspond souvent au plafond réel SAP :

```bash
./sap-extractor --once --full --logs-only --top 50000 --page-size 1000 --skip-error-details --skip-webhooks
```

## v0.4.3 — insertion PostgreSQL optimisée

Les logs MPL sont maintenant insérés avec une stratégie bulk :

1. `COPY FROM STDIN` vers une table temporaire PostgreSQL ;
2. `INSERT ... SELECT ... ON CONFLICT` vers `sap_monitoring_logs`.

Cette approche réduit fortement l'overhead SQL sur les gros volumes par rapport à `INSERT VALUES` batché.

### Backfill recommandé pour benchmark / historique

```bash
RUST_LOG=warn ./target/release/sap-extractor \
  --once \
  --full \
  --logs-only \
  --top 50000 \
  --page-size 1000 \
  --skip-error-details \
  --skip-alerts \
  --skip-webhooks
```

Notes :

- `--skip-alerts` désactive la création de `pending_alerts` pendant le cycle.
- `--skip-webhooks` empêche l'envoi Teams.
- `--skip-error-details` évite les appels `ErrorInformation/$value`, très coûteux pour les backfills.
- SAP CPI/BTP peut plafonner les pages MPL à 1000 lignes ; le daemon continue maintenant à paginer jusqu'à `--top` ou page vide.

### Run incrémental Airflow recommandé

```bash
./target/release/sap-extractor \
  --once \
  --logs-only \
  --top 5000 \
  --page-size 1000 \
  --skip-error-details
```


## Backfill ultra-rapide

Pour comparer au script Python async ou charger un gros historique, utilise le mode full + logs only + pages SAP parallèles.

```bash
RUST_LOG=warn ./target/release/sap-extractor \
  --once \
  --full \
  --logs-only \
  --top 100000 \
  --page-size 1000 \
  --page-concurrency 4 \
  --insert-buffer-rows 50000 \
  --skip-error-details \
  --skip-alerts \
  --skip-webhooks
```

Notes :

- `--page-concurrency` ne s'applique qu'au backfill `--full` sans filtre incrémental.
- Le mode incrémental Airflow reste séquentiel pour éviter les trous/doublons liés à `$skip` pendant que SAP reçoit de nouveaux logs.
- `--insert-buffer-rows` groupe plusieurs pages SAP avant le `COPY` PostgreSQL, donc beaucoup moins de merges `ON CONFLICT`.
- SAP CPI semble plafonner `MessageProcessingLogs` à 1000 résultats par page, donc garde `--page-size 1000` pour le mode parallèle.

## Correctif v0.4.5

Le mode backfill parallèle peut recevoir deux fois le même `message_guid` si SAP renvoie des pages instables ou si de nouveaux logs arrivent pendant le scan. La version `0.4.5` déduplique maintenant la table temporaire avec `DISTINCT ON (message_guid)` avant le `ON CONFLICT`, ce qui évite l'erreur PostgreSQL :

```text
ON CONFLICT DO UPDATE command cannot affect row a second time
```

## v0.4.6 — Correction smart alerts NUMERIC/FLOAT8

PostgreSQL renvoie `EXTRACT(...)` et certaines divisions `COUNT / 6.0` en `NUMERIC`.
La version 0.4.6 caste explicitement ces valeurs en `double precision` pour éviter :

```text
error occurred while decoding column 2: mismatched types; Rust type `f64` ... is not compatible with SQL type `NUMERIC`
```

Cette correction touche uniquement les requêtes de smart alerts après ingestion.

## v0.4.7 — Backfill par curseur temporel

Pour les gros volumes, le mode `offset` peut ralentir quand `$skip` devient profond. Le mode `cursor` evite cela en paginant avec `LogStart < derniere_date_vue`, toujours avec `$skip=0` :

```bash
RUST_LOG=info ./target/release/sap-extractor \
  --once \
  --tenant pierre_import \
  --full \
  --logs-only \
  --top 250000 \
  --page-size 1000 \
  --backfill-strategy cursor \
  --insert-buffer-rows 50000 \
  --skip-error-details \
  --skip-alerts \
  --skip-webhooks
```

`offset` reste disponible et peut etre plus rapide sur des volumes moyens :

```bash
--backfill-strategy offset --page-concurrency 5
```

Recommandation :
- backfill test ou moyen volume : `offset` + `--page-concurrency 4..6` ;
- backfill profond / gros historique : `cursor` ;
- runs Airflow reguliers : incremental sans `--full`.
