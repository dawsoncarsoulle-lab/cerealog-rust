import asyncio
import aiohttp
import time
import os
from dotenv import load_dotenv

# Charger les variables d'environnement

load_dotenv()

BASE_URL = os.getenv("SAP_BASE_URL")
TOKEN_URL = os.getenv("SAP_TOKEN_URL")
CLIENT_ID = os.getenv("CLIENT_ID")
CLIENT_SECRET = os.getenv("CLIENT_SECRET")
TOP = 500 # On s'aligne sur ton benchmark de 500 logs
CONCURRENCY = 20

async def get_token(session):
auth = aiohttp.BasicAuth(CLIENT_ID, CLIENT_SECRET)
async with session.post(TOKEN_URL, auth=auth) as response:
response.raise_for_status()
data = await response.json()
return data["access_token"]

async def fetch_odata(session, token, url):
headers = {"Authorization": f"Bearer {token}", "Accept": "application/json"}
async with session.get(url, headers=headers) as response:
if response.status == 200:
data = await response.json()
return data.get("d", {}).get("results", [])
return []

async def fetch_configs(session, token, package_ids):
"""Récupère les configurations de manière concurrente avec un Sémaphore"""
sem = asyncio.Semaphore(CONCURRENCY)

    async def fetch_single_package(pkg_id):
        async with sem:
            url = f"{BASE_URL}/api/v1/IntegrationPackages('{pkg_id}')/IntegrationDesigntimeArtifacts"
            return await fetch_odata(session, token, url)

    # Récupérer les artifacts des packages
    tasks = [fetch_single_package(pid) for pid in package_ids]
    results = await asyncio.gather(*tasks)

    # Extraire tous les IDs d'artifacts
    all_art_ids = [art["Id"] for pkg_arts in results for art in pkg_arts if "Id" in art]

    async def fetch_single_config(art_id):
        async with sem:
            url = f"{BASE_URL}/api/v1/IntegrationDesigntimeArtifacts(Id='{art_id}',Version='active')/Configurations"
            return await fetch_odata(session, token, url)

    # Récupérer les configs
    config_tasks = [fetch_single_config(aid) for aid in all_art_ids]
    await asyncio.gather(*config_tasks)

async def main():
print("🚀 Démarrage du benchmark Python Asynchrone (aiohttp)...")
start_time = time.perf_counter()

    # Configuration du pool TCP (équivalent au pool_max_idle_per_host de reqwest)
    connector = aiohttp.TCPConnector(limit_per_host=50)

    async with aiohttp.ClientSession(connector=connector) as session:
        # 1. Auth
        token = await get_token(session)
        auth_time = time.perf_counter()
        print(f"🔑 Token OAuth obtenu en {auth_time - start_time:.3f}s")

        # URLs optimisées avec $select (pour être à armes égales avec Rust)
        logs_url = f"{BASE_URL}/api/v1/MessageProcessingLogs?$select=MessageGuid,Status,LogStart,IntegrationFlowName&$orderby=LogStart desc&$top={TOP}"
        packages_url = f"{BASE_URL}/api/v1/IntegrationPackages" # Sans select comme vu précédemment
        artifacts_url = f"{BASE_URL}/api/v1/IntegrationRuntimeArtifacts" # Sans select

        # 2. Parallélisation Logs, Packages et Artifacts (L'équivalent du tokio::join!)
        logs_task = fetch_odata(session, token, logs_url)
        packages_task = fetch_odata(session, token, packages_url)
        artifacts_task = fetch_odata(session, token, artifacts_url)

        logs, packages, artifacts = await asyncio.gather(logs_task, packages_task, artifacts_task)

        # 3. Configurations séquentielles (dépendantes des packages)
        package_ids = [p["Id"] for p in packages if "Id" in p]
        await fetch_configs(session, token, package_ids)

    end_time = time.perf_counter()

    print("\n" + "="*50)
    print(f"📊 RÉSULTATS DU BENCHMARK PYTHON")
    print("="*50)
    print(f"📦 Logs extraits      : {len(logs)}")
    print(f"📦 Packages extraits  : {len(packages)}")
    print(f"📦 Artifacts extraits : {len(artifacts)}")
    print(f"⏱️  TEMPS TOTAL        : {end_time - start_time:.3f} secondes")
    print("="*50)

if **name** == "**main**": # Optimisation asyncio pour Windows/Linux
if os.name == 'nt':
asyncio.set_event_loop_policy(asyncio.WindowsSelectorEventLoopPolicy())
asyncio.run(main())
