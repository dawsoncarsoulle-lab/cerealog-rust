-- SAP BTP Monitor backend schema
-- Idempotent PostgreSQL schema for the daemon/batch worker.

CREATE TABLE IF NOT EXISTS tenants (
    id                    VARCHAR(100) PRIMARY KEY,
    name                  TEXT NOT NULL,
    client_name           TEXT,
    shared_tenant         BOOLEAN DEFAULT false,
    sap_base_url          TEXT,
    sap_token_url         TEXT,
    sap_client_id         TEXT,
    sap_client_secret_enc TEXT,
    active                BOOLEAN DEFAULT true,
    created_at            TIMESTAMP DEFAULT NOW()
);

INSERT INTO tenants (id, name, client_name, shared_tenant, active)
VALUES ('cerealog', 'Cerealog', 'Cerealog', false, true)
ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS integration_packages (
    id            VARCHAR(255) PRIMARY KEY,
    name          TEXT,
    version       VARCHAR(50),
    vendor        TEXT,
    creation_date TIMESTAMP,
    tags          TEXT,
    tenant_id     VARCHAR(100) NOT NULL DEFAULT 'cerealog'
);

CREATE TABLE IF NOT EXISTS runtime_artifacts (
    id          VARCHAR(255) PRIMARY KEY,
    name        TEXT,
    status      VARCHAR(50),
    deployed_on TIMESTAMP,
    package_id  VARCHAR(255),
    tenant_id   VARCHAR(100) NOT NULL DEFAULT 'cerealog'
);

CREATE TABLE IF NOT EXISTS sap_monitoring_logs (
    message_guid          VARCHAR(255) PRIMARY KEY,
    status                VARCHAR(50),
    parsed_date           TIMESTAMP,
    error_message         TEXT,
    integration_flow_name TEXT,
    tenant_id             VARCHAR(100) NOT NULL DEFAULT 'cerealog'
);

CREATE TABLE IF NOT EXISTS artifact_errors (
    artifact_id   VARCHAR(255) PRIMARY KEY,
    error_message TEXT,
    error_time    TIMESTAMP,
    tenant_id     VARCHAR(100) NOT NULL DEFAULT 'cerealog'
);

CREATE TABLE IF NOT EXISTS artifact_configurations (
    id              BIGSERIAL PRIMARY KEY,
    artifact_id     VARCHAR(255) NOT NULL,
    parameter_key   TEXT NOT NULL,
    parameter_value TEXT,
    data_type       VARCHAR(50),
    tenant_id       VARCHAR(100) NOT NULL DEFAULT 'cerealog',
    UNIQUE(artifact_id, parameter_key)
);

CREATE TABLE IF NOT EXISTS pending_alerts (
    id            BIGSERIAL PRIMARY KEY,
    log_guid      TEXT NOT NULL,
    flow_name     TEXT NOT NULL DEFAULT '',
    error_type    TEXT NOT NULL DEFAULT 'exec',
    error_snippet TEXT NOT NULL DEFAULT '',
    tenant_id     VARCHAR(100) NOT NULL DEFAULT 'cerealog',
    detected_at   TIMESTAMP NOT NULL DEFAULT NOW(),
    CONSTRAINT    pending_alerts_guid_uq UNIQUE (log_guid)
);

CREATE TABLE IF NOT EXISTS smart_alerts (
    id                BIGSERIAL PRIMARY KEY,
    tenant_id         VARCHAR(100) NOT NULL,
    flow_name         TEXT NOT NULL,
    alert_type        TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'PENDING',
    last_triggered_at TIMESTAMP NOT NULL DEFAULT NOW(),
    extra             JSONB,
    CONSTRAINT smart_alerts_unique UNIQUE (tenant_id, flow_name, alert_type)
);

-- Migrations légères pour anciennes bases.
ALTER TABLE tenants ADD COLUMN IF NOT EXISTS sap_token_url TEXT;
ALTER TABLE tenants ADD COLUMN IF NOT EXISTS sap_client_id TEXT;
ALTER TABLE tenants ADD COLUMN IF NOT EXISTS sap_client_secret_enc TEXT;
ALTER TABLE tenants ADD COLUMN IF NOT EXISTS active BOOLEAN DEFAULT true;

ALTER TABLE integration_packages ADD COLUMN IF NOT EXISTS tenant_id VARCHAR(100) NOT NULL DEFAULT 'cerealog';
ALTER TABLE runtime_artifacts ADD COLUMN IF NOT EXISTS tenant_id VARCHAR(100) NOT NULL DEFAULT 'cerealog';
ALTER TABLE sap_monitoring_logs ADD COLUMN IF NOT EXISTS tenant_id VARCHAR(100) NOT NULL DEFAULT 'cerealog';
ALTER TABLE artifact_errors ADD COLUMN IF NOT EXISTS tenant_id VARCHAR(100) NOT NULL DEFAULT 'cerealog';
ALTER TABLE artifact_configurations ADD COLUMN IF NOT EXISTS tenant_id VARCHAR(100) NOT NULL DEFAULT 'cerealog';

-- Index prioritaires pour gros volumes / millions de lignes.
CREATE INDEX IF NOT EXISTS idx_logs_date_desc ON sap_monitoring_logs (parsed_date DESC);
CREATE INDEX IF NOT EXISTS idx_logs_tenant_date_desc ON sap_monitoring_logs (tenant_id, parsed_date DESC);
CREATE INDEX IF NOT EXISTS idx_logs_tenant_status_date_desc ON sap_monitoring_logs (tenant_id, status, parsed_date DESC);
CREATE INDEX IF NOT EXISTS idx_logs_tenant_flow_date_desc ON sap_monitoring_logs (tenant_id, integration_flow_name, parsed_date DESC);
CREATE INDEX IF NOT EXISTS idx_logs_failed_recent ON sap_monitoring_logs (tenant_id, integration_flow_name, parsed_date DESC) WHERE status = 'FAILED';
CREATE INDEX IF NOT EXISTS idx_packages_tenant ON integration_packages (tenant_id);
CREATE INDEX IF NOT EXISTS idx_artifacts_tenant ON runtime_artifacts (tenant_id);
CREATE INDEX IF NOT EXISTS idx_artifacts_package ON runtime_artifacts (package_id);
CREATE INDEX IF NOT EXISTS idx_artifact_errors_tenant_time ON artifact_errors (tenant_id, error_time DESC);
CREATE INDEX IF NOT EXISTS idx_pending_alerts_detected ON pending_alerts (detected_at ASC);
CREATE INDEX IF NOT EXISTS idx_smart_alerts_status_time ON smart_alerts (status, last_triggered_at ASC);
