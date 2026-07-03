from __future__ import annotations

import os
import signal
import subprocess
from datetime import timedelta
from pathlib import Path

import pendulum
from airflow import DAG
from airflow.exceptions import AirflowException
from airflow.models import Variable
from airflow.operators.python import PythonOperator


DAG_DIR = Path(__file__).resolve().parent
BINARY_PATH = DAG_DIR / "bin" / "cerealog-tui"


def run_cerealog_tui() -> None:
    database_url = Variable.get("CEREALOG_DATABASE_URL", default_var=os.getenv("DATABASE_URL"))
    if not database_url:
        raise AirflowException(
            "CEREALOG_DATABASE_URL Airflow variable or DATABASE_URL environment variable is required"
        )

    tenant = Variable.get("CEREALOG_TENANT", default_var="")
    limit = int(Variable.get("CEREALOG_LIMIT", default_var="500"))
    refresh_seconds = int(Variable.get("CEREALOG_REFRESH_SECONDS", default_var="10"))
    timeout_seconds = int(Variable.get("CEREALOG_RUN_TIMEOUT_SECONDS", default_var="60"))

    if not BINARY_PATH.exists():
        raise AirflowException(f"Binary not found: {BINARY_PATH}")

    command = [
        str(BINARY_PATH),
        "--limit",
        str(max(limit, 1)),
        "--refresh-seconds",
        str(max(refresh_seconds, 1)),
    ]
    if tenant:
        command.extend(["--tenant", tenant])

    env = os.environ.copy()
    env["DATABASE_URL"] = database_url
    env.setdefault("TERM", "xterm-256color")

    process = subprocess.Popen(
        command,
        env=env,
        cwd=str(DAG_DIR),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        preexec_fn=os.setsid,
    )

    try:
        output, _ = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGTERM)
        output, _ = process.communicate(timeout=10)
        print(output or "")
        return

    print(output or "")
    if process.returncode != 0:
        raise AirflowException(f"cerealog-tui failed with exit code {process.returncode}")


with DAG(
    dag_id="cerealog_tui_runner",
    description="Run the cerealog-tui binary from Airflow with a configurable timeout.",
    start_date=pendulum.datetime(2026, 6, 26, tz="Europe/Paris"),
    schedule=None,
    catchup=False,
    max_active_runs=1,
    default_args={
        "owner": "cerealog",
        "retries": 0,
        "execution_timeout": timedelta(minutes=5),
    },
    tags=["cerealog", "sap-btp"],
) as dag:
    run_cerealog_tui_task = PythonOperator(
        task_id="run_cerealog_tui",
        python_callable=run_cerealog_tui,
    )
