# cerealog-tui

TUI read-only pour consulter les donnees SAP BTP deja synchronisees dans PostgreSQL par le daemon Rust separe.

## Installation

```bash
cargo build --release
```

## Configuration

Le TUI lit uniquement `DATABASE_URL` depuis l'environnement ou un fichier `.env`.

```bash
DATABASE_URL=postgres://cerealog_reader:password@localhost:5432/cerealog
```

## User PostgreSQL read-only

Exemple de creation d'un utilisateur limite a la lecture :

```sql
CREATE USER cerealog_reader WITH PASSWORD 'change-me';
GRANT CONNECT ON DATABASE cerealog TO cerealog_reader;
GRANT USAGE ON SCHEMA public TO cerealog_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO cerealog_reader;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO cerealog_reader;
```

## Lancement

Vue globale :

```bash
cargo run
```

Filtrer sur un tenant :

```bash
cargo run -- --tenant cerealog
```

Options :

```bash
cargo run -- --tenant cerealog --limit 500 --refresh-seconds 10
```

## Touches clavier

- `q` quitter
- `r` recharger depuis PostgreSQL
- `Tab` / `Shift+Tab` changer d'onglet
- `Up` / `Down` naviguer dans les listes
- `/` rechercher, `Enter` appliquer, `Esc` annuler
- `t` passer au tenant suivant en mode global
- `g` revenir a la vue globale

## Verification

```bash
cargo fmt --check
cargo check
cargo run -- --tenant cerealog
```
