-- Keep rollback state aligned with successful production deployments that were
-- created outside NoPager. A Vercel READY signal alone is not enough to call a
-- deployment known-good: wait until NoPager's production health check succeeds
-- after the deployment has been observed.
--
-- During an active incident we deliberately do nothing. In particular, a
-- NoPager repair being verified must keep the previous known-good deployment
-- until the worker's post-deploy watch explicitly promotes the repair.

CREATE FUNCTION nopager_refresh_external_known_good()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    candidate_id uuid;
BEGIN
    IF NEW.status <> 'HEALTHY' OR NEW.last_checked_at IS NULL THEN
        RETURN NEW;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM incidents
        WHERE project_id = NEW.project_id
          AND resolved_at IS NULL
    ) THEN
        RETURN NEW;
    END IF;

    SELECT id
    INTO candidate_id
    FROM deployments
    WHERE project_id = NEW.project_id
      AND environment = 'production'
      AND status = 'READY'
    ORDER BY created_at DESC, id DESC
    LIMIT 1;

    IF candidate_id IS NULL THEN
        RETURN NEW;
    END IF;

    UPDATE deployments
    SET known_good = (id = candidate_id)
    WHERE project_id = NEW.project_id
      AND environment = 'production'
      AND (known_good OR id = candidate_id);

    RETURN NEW;
END;
$$;

CREATE TRIGGER health_check_refreshes_external_known_good
AFTER UPDATE OF status, last_checked_at ON health_checks
FOR EACH ROW
EXECUTE FUNCTION nopager_refresh_external_known_good();
