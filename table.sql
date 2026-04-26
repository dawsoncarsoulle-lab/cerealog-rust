CREATE TABLE IF NOT EXISTS integration_packages (
    id VARCHAR(255) PRIMARY KEY,
    name TEXT,
    version VARCHAR(50),
    vendor TEXT,
    creation_date TIMESTAMP,
    tags TEXT
);

CREATE TABLE IF NOT EXISTS runtime_artifacts (
    id VARCHAR(255) PRIMARY KEY,
    name TEXT,
    status VARCHAR(50),
    deployed_on TIMESTAMP,
    package_id VARCHAR(255) REFERENCES integration_packages(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS sap_monitoring_logs (
    message_guid VARCHAR(255) PRIMARY KEY,
    status VARCHAR(50),
    parsed_date TIMESTAMP,
    error_message TEXT,
    integration_flow_name TEXT
);

CREATE TABLE IF NOT EXISTS artifact_errors (
    artifact_id VARCHAR(255) PRIMARY KEY REFERENCES runtime_artifacts(id) ON DELETE CASCADE,
    error_message TEXT,
    error_time TIMESTAMP
);

CREATE TABLE IF NOT EXISTS artifact_configurations (
    id SERIAL PRIMARY KEY,
    artifact_id VARCHAR(255) NOT NULL REFERENCES runtime_artifacts(id) ON DELETE CASCADE,
    parameter_key TEXT NOT NULL,
    parameter_value TEXT,
    data_type VARCHAR(50),
    UNIQUE(artifact_id, parameter_key)
);

CREATE INDEX IF NOT EXISTS idx_logs_date ON sap_monitoring_logs (parsed_date DESC);
CREATE INDEX IF NOT EXISTS idx_logs_status_date ON sap_monitoring_logs (status, parsed_date);
CREATE INDEX IF NOT EXISTS idx_logs_flow_name ON sap_monitoring_logs (integration_flow_name);
CREATE INDEX IF NOT EXISTS idx_artifacts_package ON runtime_artifacts (package_id);


ALTER TABLE integration_packages ADD COLUMN tenant_id VARCHAR(100) NOT NULL DEFAULT 'cerealog';
ALTER TABLE runtime_artifacts ADD COLUMN tenant_id VARCHAR(100) NOT NULL DEFAULT 'cerealog';
ALTER TABLE sap_monitoring_logs ADD COLUMN tenant_id VARCHAR(100) NOT NULL DEFAULT 'cerealog';
ALTER TABLE artifact_errors ADD COLUMN tenant_id VARCHAR(100) NOT NULL DEFAULT 'cerealog';
ALTER TABLE artifact_configurations ADD COLUMN tenant_id VARCHAR(100) NOT NULL DEFAULT 'cerealog';

CREATE TABLE IF NOT EXISTS tenants (
    id VARCHAR(100) PRIMARY KEY,
    name TEXT NOT NULL,
    client_name TEXT,
    shared_tenant BOOLEAN DEFAULT false,
    sap_base_url TEXT,
    sap_token_url TEXT,
    active BOOLEAN DEFAULT true,
    created_at TIMESTAMP DEFAULT NOW()
);

INSERT INTO tenants (id, name, client_name, shared_tenant, active)
VALUES ('cerealog', 'Cerealog', 'Cerealog', false, true)
ON CONFLICT (id) DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_packages_tenant ON integration_packages (tenant_id);
CREATE INDEX IF NOT EXISTS idx_artifacts_tenant ON runtime_artifacts (tenant_id);
CREATE INDEX IF NOT EXISTS idx_logs_tenant ON sap_monitoring_logs (tenant_id);

ALTER TABLE integration_packages ADD CONSTRAINT fk_packages_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id);
ALTER TABLE runtime_artifacts ADD CONSTRAINT fk_artifacts_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id);
ALTER TABLE sap_monitoring_logs ADD CONSTRAINT fk_logs_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE tenants ADD COLUMN sap_client_id TEXT;
ALTER TABLE tenants ADD COLUMN sap_client_secret_enc TEXT;

DROP TABLE IF EXISTS smart_alerts;

CREATE TABLE smart_alerts (
    id                BIGSERIAL PRIMARY KEY,
    tenant_id         VARCHAR(100) NOT NULL,
    flow_name         TEXT NOT NULL,
    alert_type        TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'PENDING',
    last_triggered_at TIMESTAMP NOT NULL DEFAULT NOW(),
    extra             JSONB,
    CONSTRAINT smart_alerts_unique UNIQUE (tenant_id, flow_name, alert_type)
);
