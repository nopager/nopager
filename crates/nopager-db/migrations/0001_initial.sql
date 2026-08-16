CREATE TABLE projects (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    slug text NOT NULL UNIQUE,
    repo_owner text NOT NULL,
    repo_name text NOT NULL,
    production_url text NOT NULL,
    status text NOT NULL DEFAULT 'ACTIVE',
    safety_mode text NOT NULL DEFAULT 'safe',
    protection_paused boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE integrations (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    type text NOT NULL,
    external_account_id text,
    external_project_id text,
    encrypted_credentials text NOT NULL,
    metadata_json jsonb NOT NULL DEFAULT '{}'::jsonb,
    status text NOT NULL DEFAULT 'PENDING',
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(project_id, type)
);

CREATE TABLE health_checks (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    url text NOT NULL,
    method text NOT NULL DEFAULT 'GET',
    interval_seconds integer NOT NULL DEFAULT 60 CHECK (interval_seconds >= 10),
    timeout_ms integer NOT NULL DEFAULT 10000 CHECK (timeout_ms BETWEEN 100 AND 60000),
    expected_status integer NOT NULL DEFAULT 200 CHECK (expected_status BETWEEN 100 AND 599),
    consecutive_failures integer NOT NULL DEFAULT 0,
    consecutive_successes integer NOT NULL DEFAULT 0,
    status text NOT NULL DEFAULT 'UNKNOWN',
    last_checked_at timestamptz,
    UNIQUE(project_id, url)
);

CREATE TABLE incidents (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    deduplication_key text NOT NULL,
    trigger_type text NOT NULL,
    status text NOT NULL,
    severity text NOT NULL,
    title text NOT NULL,
    root_cause_summary text,
    autonomous_resolution boolean NOT NULL DEFAULT false,
    opened_at timestamptz NOT NULL DEFAULT now(),
    resolved_at timestamptz,
    current_attempt_id uuid,
    UNIQUE(project_id, deduplication_key)
);

CREATE INDEX incidents_project_status_idx ON incidents(project_id, status, opened_at DESC);

CREATE TABLE incident_events (
    id uuid PRIMARY KEY,
    incident_id uuid NOT NULL REFERENCES incidents(id) ON DELETE CASCADE,
    type text NOT NULL,
    actor text NOT NULL,
    message text NOT NULL,
    metadata_json jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX incident_events_timeline_idx ON incident_events(incident_id, created_at, id);

CREATE TABLE repair_attempts (
    id uuid PRIMARY KEY,
    incident_id uuid NOT NULL REFERENCES incidents(id) ON DELETE CASCADE,
    attempt_number integer NOT NULL CHECK (attempt_number > 0),
    base_commit_sha text NOT NULL,
    diagnosis_json jsonb,
    plan_json jsonb,
    patch_diff text,
    patch_fingerprint text,
    risk_level text,
    sandbox_status text NOT NULL DEFAULT 'PENDING',
    test_status text NOT NULL DEFAULT 'PENDING',
    preview_url text,
    status text NOT NULL DEFAULT 'PENDING',
    started_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    UNIQUE(incident_id, attempt_number),
    UNIQUE(incident_id, patch_fingerprint)
);

ALTER TABLE incidents
    ADD CONSTRAINT incidents_current_attempt_fk
    FOREIGN KEY (current_attempt_id) REFERENCES repair_attempts(id) ON DELETE SET NULL;

CREATE TABLE model_runs (
    id uuid PRIMARY KEY,
    incident_id uuid NOT NULL REFERENCES incidents(id) ON DELETE CASCADE,
    repair_attempt_id uuid REFERENCES repair_attempts(id) ON DELETE SET NULL,
    provider text NOT NULL,
    model text NOT NULL,
    purpose text NOT NULL,
    input_token_count bigint,
    output_token_count bigint,
    latency_ms bigint NOT NULL,
    status text NOT NULL,
    error_class text,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE deployments (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider_deployment_id text NOT NULL,
    environment text NOT NULL,
    commit_sha text NOT NULL,
    url text NOT NULL,
    status text NOT NULL,
    known_good boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(project_id, provider_deployment_id)
);

CREATE TABLE policies (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL UNIQUE REFERENCES projects(id) ON DELETE CASCADE,
    safety_mode text NOT NULL DEFAULT 'safe',
    allowed_actions_json jsonb NOT NULL DEFAULT '[]'::jsonb,
    required_checks_json jsonb NOT NULL DEFAULT '[]'::jsonb,
    max_repair_attempts integer NOT NULL DEFAULT 3 CHECK (max_repair_attempts BETWEEN 1 AND 10),
    max_risk_level text NOT NULL DEFAULT 'medium'
);

CREATE TABLE audit_events (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    incident_id uuid REFERENCES incidents(id) ON DELETE SET NULL,
    actor text NOT NULL,
    action text NOT NULL,
    target text NOT NULL,
    outcome text NOT NULL,
    metadata_json jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX audit_events_project_idx ON audit_events(project_id, created_at DESC);

CREATE TABLE webhook_deliveries (
    id uuid PRIMARY KEY,
    provider text NOT NULL,
    external_delivery_id text NOT NULL,
    event_type text NOT NULL,
    metadata_json jsonb NOT NULL DEFAULT '{}'::jsonb,
    received_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(provider, external_delivery_id)
);

CREATE TABLE jobs (
    id uuid PRIMARY KEY,
    job_type text NOT NULL,
    idempotency_key text NOT NULL UNIQUE,
    correlation_id uuid,
    payload_json jsonb NOT NULL,
    status text NOT NULL DEFAULT 'PENDING',
    attempt integer NOT NULL DEFAULT 0,
    max_attempts integer NOT NULL DEFAULT 3 CHECK (max_attempts > 0),
    available_at timestamptz NOT NULL DEFAULT now(),
    locked_at timestamptz,
    locked_by text,
    last_error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz
);

CREATE INDEX jobs_claim_idx ON jobs(status, available_at, created_at)
    WHERE status IN ('PENDING', 'RETRY');

