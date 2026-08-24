use nopager_db::Database;
use serde_json::Value;
use uuid::Uuid;

use super::RESTART_COOLDOWN_SECONDS;

pub(super) async fn ensure_operations_work(
    database: &Database,
    work: &nopager_db::IncidentWork,
    source_type: &str,
) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    let source_key = work.id.to_string();
    Ok(sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO operations_work (id, project_id, incident_id, source_type, source_key, status, idempotency_key) VALUES ($1, $2, $3, $4, $5, 'OPEN', $6) ON CONFLICT (project_id, source_type, source_key) DO UPDATE SET updated_at = now() RETURNING id",
    )
    .bind(id)
    .bind(work.project_id)
    .bind(work.id)
    .bind(source_type)
    .bind(&source_key)
    .bind(format!("incident:{}:operations", work.id))
    .fetch_one(database.pool())
    .await?)
}

pub(super) async fn append_work_event(
    database: &Database,
    work_id: Uuid,
    event_type: &str,
    actor: &str,
    message: &str,
    metadata: &Value,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO operations_work_events (id, work_id, event_type, actor, message, metadata_json) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(work_id)
    .bind(event_type)
    .bind(actor)
    .bind(message)
    .bind(metadata)
    .execute(database.pool())
    .await?;
    Ok(())
}

pub(super) async fn set_work_status(
    database: &Database,
    work_id: Uuid,
    status: &str,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE operations_work SET status = $1, updated_at = now() WHERE id = $2")
        .bind(status)
        .bind(work_id)
        .execute(database.pool())
        .await?;
    Ok(())
}

pub(super) async fn restart_cooldown_clear(
    database: &Database,
    project_id: Uuid,
    target: &str,
) -> anyhow::Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT NOT EXISTS(SELECT 1 FROM operations_executions e JOIN operations_work w ON w.id = e.work_id WHERE w.project_id = $1 AND e.action_kind = 'restart_container' AND e.target_id = $2 AND e.status IN ('STARTED', 'COMPLETED') AND e.created_at > now() - ($3 * interval '1 second'))",
    )
    .bind(project_id)
    .bind(target)
    .bind(RESTART_COOLDOWN_SECONDS)
    .fetch_one(database.pool())
    .await?)
}
