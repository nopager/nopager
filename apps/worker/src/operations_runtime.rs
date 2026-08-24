use nopager_db::Database;
use serde_json::Value;
use uuid::Uuid;

mod config;
mod execution;
mod ledger;
mod planner;
mod provider;
mod verification;

pub(super) const OPERATIONS_PIPELINE: &str = "operations";
pub(super) const PRIMARY_HEALTH_SOURCE: &str = "primary_https_health";
pub(super) const RESTART_COOLDOWN_SECONDS: i64 = 300;
pub(super) const VERIFY_INTERVAL_SECONDS: i64 = 10;
pub(super) const OBSERVE_TIMEOUT_SECONDS: u64 = 60;

pub async fn is_health_incident(database: &Database, payload: &Value) -> anyhow::Result<bool> {
    let incident_id = incident_id(payload)?;
    let trigger_type =
        sqlx::query_scalar::<_, String>("SELECT trigger_type FROM incidents WHERE id = $1")
            .bind(incident_id)
            .fetch_one(database.pool())
            .await?;
    Ok(trigger_type == "HEALTH_CHECK")
}

pub async fn route_incident_context(database: &Database, payload: &Value) -> anyhow::Result<()> {
    planner::route_incident_context(database, payload).await
}

pub async fn plan_operations(database: &Database, payload: &Value) -> anyhow::Result<()> {
    planner::plan_operations(database, payload).await
}

pub async fn execute_operation(database: &Database, payload: &Value) -> anyhow::Result<()> {
    execution::execute_operation(database, payload).await
}

pub async fn verify_operation(database: &Database, payload: &Value) -> anyhow::Result<()> {
    verification::verify_operation(database, payload).await
}

pub fn routes_operations_job(payload: &Value) -> bool {
    payload.get("pipeline").and_then(Value::as_str) == Some(OPERATIONS_PIPELINE)
}

pub(super) fn incident_id(payload: &Value) -> anyhow::Result<Uuid> {
    required_string(payload, "incidentId")?
        .parse()
        .map_err(Into::into)
}

pub(super) fn work_id(payload: &Value) -> anyhow::Result<Uuid> {
    required_string(payload, "workId")?
        .parse()
        .map_err(Into::into)
}

pub(super) fn required_string<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{key} is missing"))
}

pub(super) fn bounded_error(value: &str) -> String {
    value.chars().take(2_000).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn operations_discriminator_is_explicit() {
        assert!(routes_operations_job(&json!({ "pipeline": "operations" })));
        assert!(!routes_operations_job(&json!({})));
    }

    #[test]
    fn errors_are_bounded_before_ledger_persistence() {
        assert_eq!(bounded_error(&"x".repeat(3_000)).len(), 2_000);
    }
}
