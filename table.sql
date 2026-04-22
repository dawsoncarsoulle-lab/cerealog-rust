-- 1. Table des packages d'intégration
CREATE TABLE IF NOT EXISTS integration_packages (
    id VARCHAR(255) PRIMARY KEY,
    name TEXT,
    version VARCHAR(50),
    vendor TEXT,
    creation_date TIMESTAMP,
    tags TEXT
);

-- 2. Table des artifacts de runtime
CREATE TABLE IF NOT EXISTS runtime_artifacts (
    id VARCHAR(255) PRIMARY KEY,
    name TEXT,
    status VARCHAR(50),
    deployed_on TIMESTAMP,
    package_id VARCHAR(255) REFERENCES integration_packages(id) ON DELETE SET NULL
);

-- 3. Table des logs de traitement (Message Processing Logs)
CREATE TABLE IF NOT EXISTS sap_monitoring_logs (
    message_guid VARCHAR(255) PRIMARY KEY,
    status VARCHAR(50),
    parsed_date TIMESTAMP,
    error_message TEXT,
    integration_flow_name TEXT
);

-- 4. Table des erreurs de déploiement des artifacts
CREATE TABLE IF NOT EXISTS artifact_errors (
    artifact_id VARCHAR(255) PRIMARY KEY REFERENCES runtime_artifacts(id) ON DELETE CASCADE,
    error_message TEXT,
    error_time TIMESTAMP
);

-- 5. Table des configurations / propriétés (Externalized Parameters)
CREATE TABLE IF NOT EXISTS artifact_configurations (
    id SERIAL PRIMARY KEY,
    artifact_id VARCHAR(255) NOT NULL REFERENCES runtime_artifacts(id) ON DELETE CASCADE,
    parameter_key TEXT NOT NULL,
    parameter_value TEXT,
    data_type VARCHAR(50),
    UNIQUE(artifact_id, parameter_key)
);

-- ─── INDEX POUR LES PERFORMANCES ───
CREATE INDEX IF NOT EXISTS idx_logs_date ON sap_monitoring_logs (parsed_date DESC);
CREATE INDEX IF NOT EXISTS idx_logs_status_date ON sap_monitoring_logs (status, parsed_date);
CREATE INDEX IF NOT EXISTS idx_logs_flow_name ON sap_monitoring_logs (integration_flow_name);
CREATE INDEX IF NOT EXISTS idx_artifacts_package ON runtime_artifacts (package_id);
