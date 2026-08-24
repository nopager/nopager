CREATE TABLE operations_actions (
    id uuid PRIMARY KEY,
    incident_id uuid NOT NULL REFERENCES incidents(id) ON DELETE CASCADE,
    action_kind text NOT NULL,
    target_id text,
    plan_json jsonb NOT NULL,
    policy_decision text NOT NULL,
    status text NOT NULL DEFAULT 'PLANNED',
    execution_json jsonb,
    verification_json jsonb,
    started_at timestamptz,
    completed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(incident_id)
);

CREATE INDEX operations_actions_incident_idx
    ON operations_actions(incident_id, created_at DESC);

CREATE INDEX operations_actions_status_idx
    ON operations_actions(status, created_at)
    WHERE status IN ('PLANNED', 'RUNNING', 'EXECUTED');
