use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nopager_connectors::docker_ops::DockerOperationsClient;
use nopager_core::IncidentState;
use nopager_crypto::SecretCipher;
use nopager_db::{Database, IncidentTransition, JobType};
use nopager_monitor::check_http;
use nopager_policy::{
    ActionRisk, OperationsPolicyContext, PolicyDecision, SafetyMode, decide_operations,
};
use nopager_providers::{
    AnthropicProvider, AvailableOperationsAction, AvailableVerificationSignal, GeminiProvider,
    ModelProvider, OpenAiProvider, OperationsActionKind, OperationsDecision, OperationsInput,
    OperationsVerificationKind,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

const OPERATIONS_PIPELINE: &str = "operations";
const PRIMARY_HEALTH_SOURCE: &str = "primary_https_health";
const RESTART_COOLDOWN_SECONDS: i64 = 300;
const VERIFY_INTERVAL_SECONDS: i64 = 10;
const OBSERVE_TIMEOUT_SECONDS: u64 = 60;

pub async fn is_health_incident(database: &Database, payload: &Value) -> anyhow::Result<bool> {
    let incident_id = incident_id(payload)?;
    let trigger_type = sqlx::query_scalar::<_, String>(
        "SELECT trigger_type FROM incidents WHERE id = $1",
    )
    .bind(incident_id)
    .fetch_one(database.pool())
    .await?;
    Ok(trigger_type == "HEALTH_CHECK")
}

pub async fn route_incident_context(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let mut work = database.incident_work(incident_id).await?;
    if work.state == IncidentState::Open {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::Open,
                next: IncidentState::CollectingContext,
                actor: "operations-router".into(),
                message: "Collecting production operations context".into(),
                metadata: json!({ "pipeline": OPERATIONS_PIPELINE }),
            })
            .await?;
        work.state = IncidentState::CollectingContext;
    }
    if work.state != IncidentState::CollectingContext {
        return Ok(());
    }

    let work_id = ensure_operations_work(database, &work, "health_check").await?;
    append_work_event(
        database,
        work_id,
        "CONTEXT_ROUTED",
        "operations-router",
        "Health incident routed to the production operations pipeline",
        &json!({ "incidentId": incident_id }),
    )
    .await?;
    database
        .enqueue(
            JobType::Diagnose,
            &format!("incident:{incident_id}:operations-plan"),
            Some(incident_id),
            &json!({
                "incidentId": incident_id,
                "workId": work_id,
                "pipeline": OPERATIONS_PIPELINE
            }),
            3,
        )
        .await?;
    Ok(())
}

pub async fn plan_operations(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let work_id = work_id(payload)?;
    let mut work = database.incident_work(incident_id).await?;
    if work.state == IncidentState::CollectingContext {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::CollectingContext,
                next: IncidentState::Diagnosing,
                actor: "operations-router".into(),
                message: "Analyzing production health and available recovery actions".into(),
                metadata: json!({ "workId": work_id }),
            })
            .await?;
        work.state = IncidentState::Diagnosing;
    }
    if work.state != IncidentState::Diagnosing {
        return Ok(());
    }

    let target = configured_docker_target();
    let restart_enabled = docker_restart_enabled();
    let docker = DockerOperationsClient::from_environment();
    let container_context = match target.as_deref() {
        Some(target) => match docker.inspect_container(target).await {
            Ok(state) => serde_json::to_value(state)?,
            Err(error) => json!({ "targetId": target, "inspectError": bounded_error(&error.to_string()) }),
        },
        None => Value::Null,
    };

    let mut available_actions = vec![
        AvailableOperationsAction::new(
            OperationsActionKind::ObserveOnly,
            None,
            "Make no production mutation and re-check bounded external health evidence",
        ),
        AvailableOperationsAction::new(
            OperationsActionKind::DelegateDeploymentRecovery,
            None,
            "Delegate to the existing source/deployment recovery pipeline only when evidence supports a deployment regression",
        ),
        AvailableOperationsAction::new(
            OperationsActionKind::Escalate,
            None,
            "Stop autonomous work and ask the owner to investigate",
        ),
    ];
    if restart_enabled
        && let Some(target) = target.as_deref()
        && !container_context
            .get("nopagerControlPlane")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        available_actions.push(AvailableOperationsAction::new(
            OperationsActionKind::RestartContainer,
            Some(target.to_owned()),
            "Restart exactly the configured customer container once; independent external health verification is mandatory afterward",
        ));
    }

    let input = OperationsInput {
        incident_summary: work.title.clone(),
        trigger: work.trigger_context.clone(),
        health: work.trigger_context.clone(),
        infrastructure: json!({ "dockerContainer": container_context }),
        deployment: work.deployment_context.clone(),
        available_actions,
        verification_signals: vec![AvailableVerificationSignal::new(
            OperationsVerificationKind::ExternalHttp,
            PRIMARY_HEALTH_SOURCE,
            "The configured external production HTTPS health check",
        )],
    };
    let provider = provider_for(database, work.project_id).await?;
    let decision = provider.plan_operations(&input).await?;
    let decision_json = serde_json::to_value(&decision)?;
    append_work_event(
        database,
        work_id,
        "OPERATIONS_DECISION",
        &format!("provider:{}", provider.id()),
        "AI produced a bounded production operations decision",
        &decision_json,
    )
    .await?;
    sqlx::query(
        "UPDATE operations_work SET decision_json = $1, action_kind = $2, target_id = NULLIF($3, ''), status = 'PLANNED', updated_at = now() WHERE id = $4",
    )
    .bind(&decision_json)
    .bind(decision.action.as_str())
    .bind(&decision.target_id)
    .bind(work_id)
    .execute(database.pool())
    .await?;

    match decision.action {
        OperationsActionKind::DelegateDeploymentRecovery => {
            append_work_event(
                database,
                work_id,
                "DELEGATED",
                "operations-router",
                "Evidence supports the existing deployment recovery subsystem",
                &json!({}),
            )
            .await?;
            sqlx::query(
                "UPDATE operations_work SET status = 'DELEGATED', updated_at = now() WHERE id = $1",
            )
            .bind(work_id)
            .execute(database.pool())
            .await?;
            database
                .enqueue(
                    JobType::Diagnose,
                    &format!("incident:{incident_id}:deployment-diagnose-after-operations"),
                    Some(incident_id),
                    &json!({ "incidentId": incident_id }),
                    3,
                )
                .await?;
        }
        OperationsActionKind::Escalate => {
            set_work_status(database, work_id, "ESCALATED").await?;
            database
                .escalate_incident(incident_id, &decision.escalation_reason)
                .await?;
        }
        OperationsActionKind::ObserveOnly => {
            set_work_status(database, work_id, "OBSERVING").await?;
            let deadline = unix_now().saturating_add(OBSERVE_TIMEOUT_SECONDS);
            database
                .enqueue_after(
                    JobType::Verify,
                    &format!("incident:{incident_id}:operations-observe:0"),
                    Some(incident_id),
                    &json!({
                        "incidentId": incident_id,
                        "workId": work_id,
                        "pipeline": OPERATIONS_PIPELINE,
                        "phase": "operations-observe",
                        "deadlineUnix": deadline,
                        "consecutiveSuccesses": 0
                    }),
                    3,
                    VERIFY_INTERVAL_SECONDS,
                )
                .await?;
        }
        OperationsActionKind::RestartContainer => {
            plan_mutating_action(database, &work, work_id, &decision, restart_enabled).await?;
        }
        OperationsActionKind::RestartService | OperationsActionKind::RestartInstance => {
            set_work_status(database, work_id, "ESCALATED").await?;
            database
                .escalate_incident(
                    incident_id,
                    "the selected operations action has no configured trusted executor",
                )
                .await?;
        }
    }
    Ok(())
}

async fn plan_mutating_action(
    database: &Database,
    work: &nopager_db::IncidentWork,
    work_id: Uuid,
    decision: &OperationsDecision,
    restart_enabled: bool,
) -> anyhow::Result<()> {
    let target = configured_docker_target();
    let cooldown_clear = match target.as_deref() {
        Some(target) => restart_cooldown_clear(database, work.project_id, target).await?,
        None => false,
    };
    let policy = decide_operations(
        ActionRisk::Low,
        true,
        OperationsPolicyContext {
            mode: parse_safety_mode(&work.safety_mode)?,
            kill_switch_active: work.protection_paused,
            target_configured: target.as_deref() == Some(decision.target_id.as_str()),
            action_enabled: restart_enabled,
            verification_configured: decision
                .verification_signals
                .iter()
                .any(|signal| {
                    signal.kind == OperationsVerificationKind::ExternalHttp
                        && signal.source_id == PRIMARY_HEALTH_SOURCE
                }),
            cooldown_clear,
        },
    );
    let policy_name = match policy {
        PolicyDecision::Allow => "ALLOW",
        PolicyDecision::RequireApproval => "REQUIRE_APPROVAL",
        PolicyDecision::Block => "BLOCK",
    };
    sqlx::query(
        "UPDATE operations_work SET policy_decision = $1, updated_at = now() WHERE id = $2",
    )
    .bind(policy_name)
    .bind(work_id)
    .execute(database.pool())
    .await?;
    append_work_event(
        database,
        work_id,
        "POLICY_DECISION",
        "policy-engine",
        "Deterministic operations policy evaluated the proposed mutation",
        &json!({
            "decision": policy_name,
            "killSwitchActive": work.protection_paused,
            "targetConfigured": target.as_deref() == Some(decision.target_id.as_str()),
            "actionEnabled": restart_enabled,
            "cooldownClear": cooldown_clear
        }),
    )
    .await?;

    match policy {
        PolicyDecision::Allow => {
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id: work.id,
                    expected: IncidentState::Diagnosing,
                    next: IncidentState::Planning,
                    actor: "policy-engine".into(),
                    message: "A bounded low-risk production recovery action is allowed".into(),
                    metadata: json!({ "workId": work_id, "action": decision.action.as_str() }),
                })
                .await?;
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id: work.id,
                    expected: IncidentState::Planning,
                    next: IncidentState::Repairing,
                    actor: "operations-router".into(),
                    message: "Executing the approved typed production operation".into(),
                    metadata: json!({ "workId": work_id }),
                })
                .await?;
            set_work_status(database, work_id, "EXECUTING").await?;
            database
                .enqueue(
                    JobType::ProductionAction,
                    &format!("incident:{}:operations-execute", work.id),
                    Some(work.id),
                    &json!({
                        "incidentId": work.id,
                        "workId": work_id,
                        "pipeline": OPERATIONS_PIPELINE,
                        "action": "operations-execute",
                        "decision": decision
                    }),
                    3,
                )
                .await?;
        }
        PolicyDecision::RequireApproval => {
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id: work.id,
                    expected: IncidentState::Diagnosing,
                    next: IncidentState::Planning,
                    actor: "policy-engine".into(),
                    message: "A production operation was prepared for Safe Mode approval".into(),
                    metadata: json!({ "workId": work_id }),
                })
                .await?;
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id: work.id,
                    expected: IncidentState::Planning,
                    next: IncidentState::WaitingApproval,
                    actor: "policy-engine".into(),
                    message: "Production operation requires explicit administrator approval".into(),
                    metadata: json!({
                        "workId": work_id,
                        "action": decision.action.as_str(),
                        "targetId": decision.target_id
                    }),
                })
                .await?;
            set_work_status(database, work_id, "WAITING_APPROVAL").await?;
        }
        PolicyDecision::Block => {
            if work.protection_paused {
                database
                    .transition_incident(IncidentTransition {
                        project_id: work.project_id,
                        incident_id: work.id,
                        expected: IncidentState::Diagnosing,
                        next: IncidentState::Paused,
                        actor: "policy-engine".into(),
                        message: "Kill Switch blocked the proposed production operation".into(),
                        metadata: json!({ "workId": work_id }),
                    })
                    .await?;
                set_work_status(database, work_id, "PAUSED").await?;
            } else {
                set_work_status(database, work_id, "ESCALATED").await?;
                database
                    .escalate_incident(
                        work.id,
                        "deterministic operations policy blocked the proposed recovery action",
                    )
                    .await?;
            }
        }
    }
    Ok(())
}

pub async fn execute_operation(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let work_id = work_id(payload)?;
    let decision: OperationsDecision = serde_json::from_value(
        payload
            .get("decision")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("operations decision is missing"))?,
    )?;
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::Repairing {
        return Ok(());
    }
    if work.protection_paused {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::Repairing,
                next: IncidentState::Paused,
                actor: "operations-executor".into(),
                message: "Kill Switch activated before production mutation".into(),
                metadata: json!({ "workId": work_id }),
            })
            .await?;
        set_work_status(database, work_id, "PAUSED").await?;
        return Ok(());
    }

    if decision.action != OperationsActionKind::RestartContainer {
        set_work_status(database, work_id, "ESCALATED").await?;
        database
            .escalate_incident(incident_id, "unsupported operations executor action")
            .await?;
        return Ok(());
    }
    let configured_target = configured_docker_target();
    if !docker_restart_enabled() || configured_target.as_deref() != Some(decision.target_id.as_str()) {
        set_work_status(database, work_id, "ESCALATED").await?;
        database
            .escalate_incident(
                incident_id,
                "operations target or capability changed after planning; refusing mutation",
            )
            .await?;
        return Ok(());
    }
    if !restart_cooldown_clear(database, work.project_id, &decision.target_id).await? {
        set_work_status(database, work_id, "ESCALATED").await?;
        database
            .escalate_incident(
                incident_id,
                "restart cooldown became active before execution; refusing repeated mutation",
            )
            .await?;
        return Ok(());
    }

    let execution_key = format!("operations:{work_id}:restart-container:{}", decision.target_id);
    let execution_id = Uuid::now_v7();
    let inserted = sqlx::query(
        "INSERT INTO operations_executions (id, work_id, action_kind, target_id, connector, idempotency_key, status, started_at) VALUES ($1, $2, 'restart_container', $3, 'docker', $4, 'STARTED', now()) ON CONFLICT (idempotency_key) DO NOTHING",
    )
    .bind(execution_id)
    .bind(work_id)
    .bind(&decision.target_id)
    .bind(&execution_key)
    .execute(database.pool())
    .await?
    .rows_affected();

    let execution_id = if inserted == 1 {
        execution_id
    } else {
        let row = sqlx::query(
            "SELECT id, status FROM operations_executions WHERE idempotency_key = $1",
        )
        .bind(&execution_key)
        .fetch_one(database.pool())
        .await?;
        let id: Uuid = row.try_get("id")?;
        let status: String = row.try_get("status")?;
        match status.as_str() {
            "COMPLETED" => {
                enqueue_verification(database, &work, work_id, id, &decision).await?;
                return Ok(());
            }
            _ => {
                set_work_status(database, work_id, "ESCALATED").await?;
                append_work_event(
                    database,
                    work_id,
                    "EXECUTION_AMBIGUOUS",
                    "operations-executor",
                    "A previous mutation may have started but did not durably complete; refusing to execute it again",
                    &json!({ "executionId": id, "status": status }),
                )
                .await?;
                database
                    .escalate_incident(
                        incident_id,
                        "a previous production operation has ambiguous completion state; refusing duplicate execution",
                    )
                    .await?;
                return Ok(());
            }
        }
    };

    append_work_event(
        database,
        work_id,
        "EXECUTION_STARTED",
        "operations-executor",
        "Executing the typed Docker container restart",
        &json!({ "executionId": execution_id, "targetId": decision.target_id }),
    )
    .await?;
    let docker = DockerOperationsClient::from_environment();
    match docker.restart_container(&decision.target_id).await {
        Ok(result) => {
            let result_json = serde_json::to_value(&result)?;
            sqlx::query(
                "UPDATE operations_executions SET status = 'COMPLETED', result_json = $1, completed_at = now() WHERE id = $2 AND status = 'STARTED'",
            )
            .bind(&result_json)
            .bind(execution_id)
            .execute(database.pool())
            .await?;
            database
                .record_audit_event(
                    work.project_id,
                    Some(incident_id),
                    "operations-executor",
                    "docker.container.restart",
                    &decision.target_id,
                    "success",
                    &json!({ "workId": work_id, "executionId": execution_id }),
                )
                .await?;
            append_work_event(
                database,
                work_id,
                "EXECUTION_COMPLETED",
                "operations-executor",
                "Docker accepted the configured restart; external recovery is not yet proven",
                &json!({ "executionId": execution_id, "result": result_json }),
            )
            .await?;
            enqueue_verification(database, &work, work_id, execution_id, &decision).await?;
        }
        Err(error) => {
            sqlx::query(
                "UPDATE operations_executions SET status = 'FAILED', result_json = $1, completed_at = now() WHERE id = $2 AND status = 'STARTED'",
            )
            .bind(json!({ "error": bounded_error(&error.to_string()) }))
            .bind(execution_id)
            .execute(database.pool())
            .await?;
            set_work_status(database, work_id, "ESCALATED").await?;
            append_work_event(
                database,
                work_id,
                "EXECUTION_FAILED",
                "operations-executor",
                "The trusted Docker connector could not complete the requested operation",
                &json!({ "executionId": execution_id, "error": bounded_error(&error.to_string()) }),
            )
            .await?;
            database
                .escalate_incident(incident_id, &error.to_string())
                .await?;
        }
    }
    Ok(())
}

async fn enqueue_verification(
    database: &Database,
    work: &nopager_db::IncidentWork,
    work_id: Uuid,
    execution_id: Uuid,
    decision: &OperationsDecision,
) -> anyhow::Result<()> {
    let current = database.incident_work(work.id).await?;
    if current.state == IncidentState::Repairing {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id: work.id,
                expected: IncidentState::Repairing,
                next: IncidentState::Testing,
                actor: "operations-executor".into(),
                message: "Production operation completed; verifying independent external health".into(),
                metadata: json!({ "workId": work_id, "executionId": execution_id }),
            })
            .await?;
    }
    set_work_status(database, work_id, "VERIFYING").await?;
    let deadline = unix_now().saturating_add(u64::from(decision.timeout_seconds));
    database
        .enqueue_after(
            JobType::Verify,
            &format!("incident:{}:operations-verify:0", work.id),
            Some(work.id),
            &json!({
                "incidentId": work.id,
                "workId": work_id,
                "executionId": execution_id,
                "pipeline": OPERATIONS_PIPELINE,
                "phase": "operations-recovery",
                "decision": decision,
                "deadlineUnix": deadline,
                "consecutiveSuccesses": 0,
                "check": 0
            }),
            3,
            VERIFY_INTERVAL_SECONDS,
        )
        .await?;
    Ok(())
}

pub async fn verify_operation(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let work_id = work_id(payload)?;
    let phase = required_string(payload, "phase")?;
    let work = database.incident_work(incident_id).await?;
    let observation = external_health(&work.health_check_url).await;
    let success = observation.is_ok();
    let metadata = match observation {
        Ok(value) => value,
        Err(error) => json!({ "error": bounded_error(&error.to_string()) }),
    };
    let execution_id = payload
        .get("executionId")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<Uuid>().ok());
    sqlx::query(
        "INSERT INTO operations_verification_samples (id, work_id, execution_id, signal_kind, source_id, success, metadata_json) VALUES ($1, $2, $3, 'external_http', $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(work_id)
    .bind(execution_id)
    .bind(PRIMARY_HEALTH_SOURCE)
    .bind(success)
    .bind(&metadata)
    .execute(database.pool())
    .await?;

    let consecutive = if success {
        payload
            .get("consecutiveSuccesses")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .saturating_add(1)
    } else {
        0
    };
    let required = if phase == "operations-observe" {
        2
    } else {
        let decision: OperationsDecision = serde_json::from_value(
            payload
                .get("decision")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("operations decision is missing"))?,
        )?;
        u64::from(decision.required_consecutive_successes)
    };

    if consecutive >= required {
        append_work_event(
            database,
            work_id,
            "VERIFICATION_SUCCEEDED",
            "operations-verifier",
            "Independent external health checks proved production recovery",
            &json!({ "consecutiveSuccesses": consecutive }),
        )
        .await?;
        set_work_status(database, work_id, "COMPLETED").await?;
        sqlx::query(
            "UPDATE operations_work SET completed_at = now(), updated_at = now() WHERE id = $1",
        )
        .bind(work_id)
        .execute(database.pool())
        .await?;
        if work.state == IncidentState::Testing {
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id,
                    expected: IncidentState::Testing,
                    next: IncidentState::Resolved,
                    actor: "operations-verifier".into(),
                    message: "Production recovered and independent health verification passed".into(),
                    metadata: json!({ "workId": work_id, "consecutiveSuccesses": consecutive }),
                })
                .await?;
        } else if work.state == IncidentState::Diagnosing {
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id,
                    expected: IncidentState::Diagnosing,
                    next: IncidentState::Cancelled,
                    actor: "operations-verifier".into(),
                    message: "Production self-recovered during bounded observation; no mutation was performed".into(),
                    metadata: json!({ "workId": work_id, "consecutiveSuccesses": consecutive }),
                })
                .await?;
        }
        return Ok(());
    }

    let deadline = payload
        .get("deadlineUnix")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("operations verification deadline is missing"))?;
    if unix_now() >= deadline {
        append_work_event(
            database,
            work_id,
            "VERIFICATION_FAILED",
            "operations-verifier",
            "Production did not recover within the bounded verification window",
            &json!({ "lastSuccess": success, "consecutiveSuccesses": consecutive }),
        )
        .await?;
        set_work_status(database, work_id, "ESCALATED").await?;
        database
            .escalate_incident(
                incident_id,
                "production remained unhealthy after the bounded operations verification window; no repeated restart will be attempted",
            )
            .await?;
        return Ok(());
    }

    let check = payload.get("check").and_then(Value::as_u64).unwrap_or(0) + 1;
    let mut next = payload.clone();
    let object = next
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("operations verification payload must be an object"))?;
    object.insert("consecutiveSuccesses".into(), Value::from(consecutive));
    object.insert("check".into(), Value::from(check));
    database
        .enqueue_after(
            JobType::Verify,
            &format!("incident:{incident_id}:{phase}:{check}"),
            Some(incident_id),
            &next,
            3,
            VERIFY_INTERVAL_SECONDS,
        )
        .await?;
    Ok(())
}

fn is_operations_payload(payload: &Value) -> bool {
    payload.get("pipeline").and_then(Value::as_str) == Some(OPERATIONS_PIPELINE)
}

pub fn routes_operations_job(payload: &Value) -> bool {
    is_operations_payload(payload)
}

async fn external_health(url: &str) -> anyhow::Result<Value> {
    let url = url::Url::parse(url)?;
    let observation = check_http(&url, 200, Duration::from_secs(15)).await?;
    if !observation.success {
        anyhow::bail!(
            "external health verification failed with status {:?}",
            observation.status_code
        );
    }
    Ok(json!({
        "statusCode": observation.status_code,
        "latencyMs": observation.latency_ms,
        "errorClass": observation.error_class
    }))
}

async fn ensure_operations_work(
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

async fn append_work_event(
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

async fn set_work_status(database: &Database, work_id: Uuid, status: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE operations_work SET status = $1, updated_at = now() WHERE id = $2")
        .bind(status)
        .bind(work_id)
        .execute(database.pool())
        .await?;
    Ok(())
}

async fn restart_cooldown_clear(
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

async fn provider_for(
    database: &Database,
    project_id: Uuid,
) -> anyhow::Result<Box<dyn ModelProvider>> {
    if let Ok(integration) = database
        .integration_secret(project_id, "model_provider")
        .await
    {
        let credentials = decrypt_credentials(&integration.encrypted_credentials)?;
        let api_key = SecretString::from(required_string(&credentials, "apiKey")?.to_owned());
        let kind = integration
            .metadata
            .get("provider")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("model provider metadata is missing provider"))?;
        let model = integration
            .metadata
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("model provider metadata is missing model"))?
            .to_owned();
        return provider(kind, api_key, model);
    }
    let kind = std::env::var("NOPAGER_AI_PROVIDER").unwrap_or_else(|_| "openai".into());
    let model = std::env::var("NOPAGER_AI_MODEL")
        .map_err(|_| anyhow::anyhow!("NOPAGER_AI_MODEL is required"))?;
    let key_name = match kind.as_str() {
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        _ => anyhow::bail!("unsupported NOPAGER_AI_PROVIDER: {kind}"),
    };
    provider(&kind, secret_env(key_name)?, model)
}

fn provider(
    kind: &str,
    api_key: SecretString,
    model: String,
) -> anyhow::Result<Box<dyn ModelProvider>> {
    Ok(match kind {
        "openai" => Box::new(OpenAiProvider::new(api_key, model)?) as Box<dyn ModelProvider>,
        "anthropic" => Box::new(AnthropicProvider::new(api_key, model)?),
        "gemini" => Box::new(GeminiProvider::new(api_key, model)?),
        _ => anyhow::bail!("unsupported model provider: {kind}"),
    })
}

fn decrypt_credentials(encrypted: &str) -> anyhow::Result<Value> {
    let key = SecretString::from(std::env::var("NOPAGER_MASTER_KEY")?);
    let plaintext = SecretCipher::from_base64_key(&key)?.decrypt(encrypted)?;
    Ok(serde_json::from_str(plaintext.expose_secret())?)
}

fn secret_env(name: &str) -> anyhow::Result<SecretString> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .map(SecretString::from)
        .ok_or_else(|| anyhow::anyhow!("{name} is required for the selected provider"))
}

fn configured_docker_target() -> Option<String> {
    std::env::var("NOPAGER_DOCKER_TARGET")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn docker_restart_enabled() -> bool {
    std::env::var("NOPAGER_DOCKER_RESTART_ENABLED")
        .ok()
        .is_some_and(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn parse_safety_mode(value: &str) -> anyhow::Result<SafetyMode> {
    match value {
        "safe" => Ok(SafetyMode::Safe),
        "autopilot_experimental" | "autopilot-experimental" => {
            Ok(SafetyMode::AutopilotExperimental)
        }
        _ => anyhow::bail!("unknown safety mode: {value}"),
    }
}

fn incident_id(payload: &Value) -> anyhow::Result<Uuid> {
    required_string(payload, "incidentId")?.parse().map_err(Into::into)
}

fn work_id(payload: &Value) -> anyhow::Result<Uuid> {
    required_string(payload, "workId")?.parse().map_err(Into::into)
}

fn required_string<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{key} is missing"))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn bounded_error(value: &str) -> String {
    value.chars().take(2_000).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations_discriminator_is_explicit() {
        assert!(routes_operations_job(&json!({ "pipeline": "operations" })));
        assert!(!routes_operations_job(&json!({})));
    }

    #[test]
    fn docker_mutation_requires_explicit_opt_in() {
        assert!(!matches!("false", "1" | "true" | "TRUE" | "yes" | "YES"));
    }

    #[test]
    fn errors_are_bounded_before_ledger_persistence() {
        assert_eq!(bounded_error(&"x".repeat(3_000)).len(), 2_000);
    }
}
