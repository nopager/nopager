ALTER TABLE incidents DROP CONSTRAINT incidents_project_id_deduplication_key_key;

CREATE UNIQUE INDEX incidents_active_dedup_idx
    ON incidents(project_id, deduplication_key)
    WHERE status NOT IN ('RESOLVED', 'ROLLED_BACK', 'FAILED', 'ESCALATED', 'CANCELLED', 'IGNORED', 'DUPLICATE');

