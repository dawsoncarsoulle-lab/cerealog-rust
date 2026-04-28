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
