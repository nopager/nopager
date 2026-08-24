CREATE TABLE operations_work (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    incident_id uuid REFERENCES incidents(id) ON DELETE SET NULL,
    source_type text NOT NULL,
    source_key text NOT NULL,
    status text NOT NULL DEFAULT 'OPEN',
    owner_actor text,
    decision_json jsonb,
    policy_decision text,
    action_kind text,
    target_id text,
    idempotency_key text NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    UNIQUE(project_id, source_type, source_key)
);

CREATE INDEX operations_work_project_status_idx
    ON operations_work(project_id, status, created_at DESC);

CREATE INDEX operations_work_incident_idx
    ON operations_work(incident_id, created_at)
    WHERE incident_id IS NOT NULL;

CREATE TABLE operations_work_events (
    id uuid PRIMARY KEY,
    work_id uuid NOT NULL REFERENCES operations_work(id) ON DELETE CASCADE,
    event_type text NOT NULL,
    actor text NOT NULL,
    message text NOT NULL,
    metadata_json jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX operations_work_events_timeline_idx
    ON operations_work_events(work_id, created_at, id);

CREATE TABLE operations_executions (
    id uuid PRIMARY KEY,
    work_id uuid NOT NULL REFERENCES operations_work(id) ON DELETE CASCADE,
    action_kind text NOT NULL,
    target_id text,
    connector text NOT NULL,
    idempotency_key text NOT NULL UNIQUE,
    status text NOT NULL DEFAULT 'PENDING',
    result_json jsonb,
    started_at timestamptz,
    completed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX operations_executions_work_idx
    ON operations_executions(work_id, created_at);

CREATE TABLE operations_verification_samples (
    id uuid PRIMARY KEY,
    work_id uuid NOT NULL REFERENCES operations_work(id) ON DELETE CASCADE,
    execution_id uuid REFERENCES operations_executions(id) ON DELETE SET NULL,
    signal_kind text NOT NULL,
    source_id text NOT NULL,
    success boolean NOT NULL,
    metadata_json jsonb NOT NULL DEFAULT '{}'::jsonb,
    observed_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX operations_verification_samples_work_idx
    ON operations_verification_samples(work_id, observed_at, id);
