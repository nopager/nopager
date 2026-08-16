ALTER TABLE repair_attempts
    ADD COLUMN repair_branch text,
    ADD COLUMN repair_commit_sha text,
    ADD COLUMN pull_request_number bigint,
    ADD COLUMN pull_request_url text,
    ADD COLUMN validation_json jsonb NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN preview_deployment_id text;

