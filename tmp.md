C'est une excellente décision ! Passer sur Apache Airflow, c'est littéralement entrer dans la cour des grands pour l'ingénierie de la donnée. Les deux documentations que tu as (l'officielle et le blog de Stéphane Robert) sont d'excellentes références.

Pour t'éviter de te perdre dans les dizaines d'options d'installation, voici le guide **le plus direct et le plus propre** pour installer Airflow sur ton WSL spécialement pour le développement local de ton projet SAP.

Nous allons utiliser l'installation standard via **Python (pip)** avec un environnement virtuel. C'est la méthode la plus légère pour coder et tester tes DAGs rapidement avant de penser à Docker.

Voici ta feuille de route exacte :

### 🛠️ Étape 1 : Préparer ton système WSL

Airflow est un gros logiciel Python. Il faut s'assurer que ton Ubuntu/Pop!\_OS a les bons outils. Ouvre ton terminal et tape :

```bash
sudo apt update
sudo apt install -y python3 python3-pip python3-venv libpq-dev
```

### 📦 Étape 2 : Créer ton espace de travail

On ne va pas installer Airflow "en vrac" sur ton PC. On va créer un dossier dédié et un environnement virtuel (pour isoler les paquets Python).

```bash
# 1. Créer un dossier pour ton nouveau projet
mkdir ~/airflow-sap
cd ~/airflow-sap

# 2. Créer l'environnement virtuel Python
python3 -m venv venv

# 3. L'activer (à faire à chaque fois que tu ouvres un nouveau terminal !)
source venv/bin/activate
```

_(Ton terminal devrait maintenant afficher `(venv)` au début de la ligne)._

### ⬇️ Étape 3 : L'installation d'Airflow (La méthode officielle)

Airflow est très capricieux avec les versions de ses dépendances. La documentation officielle recommande d'utiliser un fichier de "contraintes" pour éviter que l'installation ne plante.

Copie-colle ce bloc entier dans ton terminal (cela va détecter ta version de Python et installer la version stable d'Airflow) :

```bash
# Définir le dossier où Airflow va stocker sa base de données locale et tes DAGs
export AIRFLOW_HOME=~/airflow-sap/airflow

# Récupérer la version d'Airflow et de ton Python
AIRFLOW_VERSION=2.9.1
PYTHON_VERSION="$(python --version | cut -d " " -f 2 | cut -d "." -f 1-2)"

# URL du fichier de contraintes officiel d'Apache
CONSTRAINT_URL="https://raw.githubusercontent.com/apache/airflow/constraints-${AIRFLOW_VERSION}/constraints-${PYTHON_VERSION}.txt"

# Installation magique
pip install "apache-airflow==${AIRFLOW_VERSION}" --constraint "${CONSTRAINT_URL}"
```

_(Laisse tourner, ça peut prendre 1 à 2 minutes)._

### 🚀 Étape 4 : Le lancement magique (`standalone`)

Pour le développement local, Airflow a créé une commande géniale qui initialise la base de données (SQLite par défaut), crée un utilisateur et lance tous les services d'un coup.

```bash
airflow standalone
```

**⚠️ ATTENTION : Regarde bien ce qui s'affiche dans ton terminal !**
Au milieu des logs, Airflow va générer un mot de passe aléatoire pour le compte `admin`. Cherche une ligne qui ressemble à ça et **copie le mot de passe** :
`admin | <ton-mot-de-passe-généré>`

### 🌐 Étape 5 : Connecte-toi à l'interface !

1. Ouvre ton navigateur web (sur ton Windows).
2. Va à l'adresse : **`http://localhost:8080`**
3. Connecte-toi avec l'identifiant `admin` et le mot de passe que tu viens de copier.

Bienvenue dans Apache Airflow ! Tu verras plein de "DAGs" d'exemples pré-installés.

---

### 🐍 Étape 6 : Préparer ton premier DAG SAP BTP

Maintenant que le moteur tourne, il faut lui donner ton code.
Laisse le terminal avec `airflow standalone` tourner, et **ouvre un nouveau terminal WSL**.

```bash
# 1. Retourne dans ton dossier et active l'environnement
cd ~/airflow-sap
source venv/bin/activate
export AIRFLOW_HOME=~/airflow-sap/airflow

# 2. Crée le dossier où Airflow ira lire tes scripts Python
mkdir -p $AIRFLOW_HOME/dags

# 3. Crée ton premier fichier DAG
touch $AIRFLOW_HOME/dags/sap_extractor_dag.py
```

Ouvre ce fichier `sap_extractor_dag.py` dans VSCode, et voici le squelette de base que tu vas devoir remplir pour reproduire ce que tu as fait en Rust :

```python
from airflow import DAG
from airflow.operators.python import PythonOperator
from datetime import datetime, timedelta

# 1. Définition des paramètres par défaut
default_args = {
    'owner': 'dawson',
    'depends_on_past': False,
    'email_on_failure': False,
    'email_on_retry': False,
    'retries': 1,
    'retry_delay': timedelta(minutes=1),
}

# 2. Création du DAG (planification)
# Ici, il se lancera toutes les 5 minutes (*/5 * * * *)
with DAG(
    'sap_btp_monitoring_sync',
    default_args=default_args,
    description='Rapatriement des logs SAP BTP B2B',
    schedule_interval='*/5 * * * *',
    start_date=datetime(2023, 1, 1),
    catchup=False,
    tags=['sap', 'monitoring'],
) as dag:

    # 3. Tes fonctions Python (La logique métier)
    def fetch_sap_token():
        print("Récupération du token OAuth2...")
        # TODO: Ton code Python avec la librairie 'requests'

    def fetch_and_insert_logs():
        print("Extraction des logs et insertion en DB...")
        # TODO: Appels API SAP OData et insertion (avec 'psycopg2' ou 'SQLAlchemy')

    def send_teams_alert():
        print("Vérification des erreurs et envoi du Webhook...")
        # TODO: Envoyer le payload JSON si erreur

    # 4. Les "Tâches" (Tasks)
    task_get_token = PythonOperator(
        task_id='get_oauth_token',
        python_callable=fetch_sap_token,
    )

    task_extract_logs = PythonOperator(
        task_id='extract_and_insert_logs',
        python_callable=fetch_and_insert_logs,
    )

    task_alerting = PythonOperator(
        task_id='send_alerts',
        python_callable=send_teams_alert,
    )

    # 5. L'ordre d'exécution (Le Graphe / DAG)
    task_get_token >> task_extract_logs >> task_alerting
```

Dès que tu sauvegarderas ce fichier, si tu rafraîchis la page web d'Airflow (`localhost:8080`), ton DAG `sap_btp_monitoring_sync` apparaîtra dans la liste ! Tu pourras cliquer sur "Play" pour le lancer manuellement et voir chaque tâche passer au vert.
