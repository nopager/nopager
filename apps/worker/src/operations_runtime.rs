use std::time::Duration;

use nopager_connectors::docker_ops::{DockerOperationsClient, DockerOperationsError};
use nopager_core::IncidentState;
use nopager_crypto::SecretCipher;
use nopager_db::{Database, IncidentTransition};
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

const PRIMARY_HEALTH_SIGNAL: &str = "primary_https_health";
const RESTART_COOLDOWN_MINUTES: i64 = 5;
const VERIFY_DELAY_SECONDS: i64 = 5;
const VERIFY_REPEAT_SECONDS: i64 = 10;

pub async fn should_handle(database: &Database, payload: &Value) -> anyhow::Result<bool> {
    let incident_id = incident_id(payload)?;
    let trigger_type =
        sqlx::query_scalar::<_, String>("SELECT trigger_type FROM incidents WHERE id = $1")
            .bind(incident_id)
            .fetch_one(database.pool())
            .await?;
    Ok(trigger_type == "HEALTH_CHECK")
}

pub async fn process_context(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let mut work = database.incident_work(incident_id).await?;
    if work.state == IncidentState::Open {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::Open,
                next: IncidentState::CollectingContext,
                actor: "operations-worker".into(),
                message: "Collecting bounded production operations evidence".into(),
                metadata: json!({ "pipeline": "production_operations" }),
            })
            .await?;
        work.state = IncidentState::CollectingContext;
    }
    if work.state != IncidentState::CollectingContext {
        return Ok(());
    }
    enqueue_job(
        database,
        "operations-plan",
        &format!("incident:{incident_id}:operations-plan"),
        incident_id,
        &json!({ "incidentId": incident_id }),
        3,
        0,
    )
    .await?;
    Ok(())
}

pub async fn process_plan(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let mut work = database.incident_work(incident_id).await?;
    if work.state == IncidentState::CollectingContext {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::CollectingContext,
                next: IncidentState::Diagnosing,
                actor: "operations-worker".into(),
                message: "AI operations reasoning started".into(),
                metadata: json!({}),
            })
            .await?;
        work.state = IncidentState::Diagnosing;
    }
    if work.state != IncidentState::Diagnosing {
        return Ok(());
    }

    let restart_enabled = env_true("NOPAGER_ALLOW_CONTAINER_RESTART");
    let target = configured_container_target();
    let docker = DockerOperationsClient::from_environment();
    let container_state = match target.as_deref() {
        Some(target) => docker.inspect_container(target).await.ok(),
        None => None,
    };

    let mut available_actions = vec![
        AvailableOperationsAction::new(
            OperationsActionKind::ObserveOnly,
            None,
            "Leave production unchanged when evidence is insufficient for a safe action",
        ),
        AvailableOperationsAction::new(
            OperationsActionKind::Escalate,
            None,
            "Escalate when the incident cannot be safely handled with configured capabilities",
        ),
    ];
    let deployment_recovery_available = deployment_recovery_configured(
        &work.repo_owner,
        &work.repo_name,
        work.vercel_project_id.as_deref(),
        &work.github_metadata,
    );
    if deployment_recovery_available {
        available_actions.push(AvailableOperationsAction::new(
            OperationsActionKind::DelegateDeploymentRecovery,
            None,
            "Use the deployment-recovery subsystem only when a code or deployment regression is supported by evidence",
        ));
    }
    if restart_enabled
        && let (Some(target), Some(_)) = (target.as_deref(), container_state.as_ref())
    {
        available_actions.push(AvailableOperationsAction::new(
            OperationsActionKind::RestartContainer,
            Some(target.to_owned()),
            "Restart exactly this configured Docker container through NoPager's trusted connector",
        ));
    }

    let mut verification_signals = vec![AvailableVerificationSignal::new(
        OperationsVerificationKind::ExternalHttp,
        PRIMARY_HEALTH_SIGNAL,
        "Independent external HTTP production health check",
    )];
    if let (Some(target), Some(_)) = (target.as_deref(), container_state.as_ref()) {
        verification_signals.push(AvailableVerificationSignal::new(
            OperationsVerificationKind::ContainerStatus,
            format!("docker:{target}"),
            "Configured Docker container runtime status",
        ));
    }

    let infrastructure = match (&target, &container_state) {
        (Some(target), Some(state)) => json!({
            "docker": {
                "targetId": target,
                "state": state,
                "restartCapabilityEnabled": restart_enabled
            }
        }),
        (Some(target), None) => json!({
            "docker": {
                "targetId": target,
                "status": "unavailable",
                "restartCapabilityEnabled": restart_enabled
            }
        }),
        (None, _) => json!({ "docker": { "configured": false } }),
    };

    let input = OperationsInput {
        incident_summary: work.title.clone(),
        trigger: work.trigger_context.clone(),
        health: work.trigger_context.clone(),
        infrastructure,
        deployment: work.deployment_context.clone(),
        available_actions,
        verification_signals,
    };
    let provider = provider_for(database, work.project_id).await?;
    let decision = provider.plan_operations(&input).await?;
    let decision_json = serde_json::to_value(&decision)?;

    database
        .record_audit_event(
            work.project_id,
            Some(incident_id),
            &format!("provider:{}", provider.id()),
            "model.operations_plan",
            &incident_id.to_string(),
            "success",
            &decision_json,
        )
        .await?;
    database
        .save_root_cause_summary(incident_id, &decision.summary)
        .await?;

    match decision.action {
        OperationsActionKind::DelegateDeploymentRecovery => {
            if !deployment_recovery_available {
                database
                    .escalate_incident(
                        incident_id,
                        "AI operations triage requested deployment recovery, but this protected app has no GitHub/Vercel deployment-recovery subsystem configured",
                    )
                    .await?;
                return Ok(());
            }
            database
                .record_audit_event(
                    work.project_id,
                    Some(incident_id),
                    "operations-worker",
                    "operations.delegate_deployment_recovery",
                    &incident_id.to_string(),
                    "success",
                    &json!({ "reason": decision.summary }),
                )
                .await?;
            enqueue_job(
                database,
                "diagnose",
                &format!("incident:{incident_id}:diagnose"),
                incident_id,
                &json!({ "incidentId": incident_id }),
                3,
                0,
            )
            .await?;
        }
        OperationsActionKind::ObserveOnly => {
            database
                .escalate_incident(
                    incident_id,
                    "AI operations triage selected observe-only because no safe autonomous production action was justified",
                )
                .await?;
        }
        OperationsActionKind::Escalate => {
            let reason = if decision.escalation_reason.trim().is_empty() {
                "AI operations triage requested human escalation"
            } else {
                decision.escalation_reason.as_str()
            };
            database.escalate_incident(incident_id, reason).await?;
        }
        OperationsActionKind::RestartContainer => {
            plan_container_restart(database, &work, incident_id, &decision, &decision_json).await?;
        }
        OperationsActionKind::RestartService | OperationsActionKind::RestartInstance => {
            anyhow::bail!("model selected an operations action that was not offered")
        }
    }
    Ok(())
}

async fn plan_container_restart(
    database: &Database,
    work: &nopager_db::IncidentWork,
    incident_id: Uuid,
    decision: &OperationsDecision,
    decision_json: &Value,
) -> anyhow::Result<()> {
    let target = decision.target_id.trim();
    let configured_target = configured_container_target()
        .ok_or_else(|| anyhow::anyhow!("Docker operations target is not configured"))?;
    if target != configured_target {
        anyhow::bail!("operations target does not match trusted configuration");
    }
    if !decision.verification_signals.iter().any(|signal| {
        signal.kind == OperationsVerificationKind::ExternalHttp
            && signal.source_id == PRIMARY_HEALTH_SIGNAL
    }) {
        database
            .escalate_incident(
                incident_id,
                "container restart was rejected because the AI plan did not select independent external health verification",
            )
            .await?;
        return Ok(());
    }

    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::Diagnosing,
            next: IncidentState::Planning,
            actor: "operations-worker".into(),
            message: "A bounded production operation was proposed".into(),
            metadata: decision_json.clone(),
        })
        .await?;

    let cooldown_clear = restart_cooldown_clear(database, work.project_id, target).await?;
    let policy = decide_operations(
        ActionRisk::Low,
        true,
        OperationsPolicyContext {
            mode: parse_safety_mode(&work.safety_mode)?,
            kill_switch_active: work.protection_paused,
            target_configured: true,
            action_enabled: env_true("NOPAGER_ALLOW_CONTAINER_RESTART"),
            verification_configured: true,
            cooldown_clear,
        },
    );
    let policy_name = policy_name(policy);
    let action_id = persist_operation_action(
        database,
        incident_id,
        "restart_container",
        target,
        decision_json,
        policy_name,
    )
    .await?;
    database
        .record_audit_event(
            work.project_id,
            Some(incident_id),
            "policy-engine",
            "operations.policy",
            &action_id.to_string(),
            policy_name,
            &json!({
                "action": "restart_container",
                "targetId": target,
                "cooldownClear": cooldown_clear,
                "safetyMode": work.safety_mode
            }),
        )
        .await?;

    match policy {
        PolicyDecision::Allow => {
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id,
                    expected: IncidentState::Planning,
                    next: IncidentState::Repairing,
                    actor: "policy-engine".into(),
                    message: "Low-risk container recovery allowed by experimental Autopilot".into(),
                    metadata: json!({ "operationActionId": action_id }),
                })
                .await?;
            enqueue_job(
                database,
                "operations-execute",
                &format!("incident:{incident_id}:operations-execute:{action_id}"),
                incident_id,
                &json!({ "incidentId": incident_id, "operationActionId": action_id }),
                1,
                0,
            )
            .await?;
        }
        PolicyDecision::RequireApproval => {
            mark_operation_status(database, action_id, "APPROVAL_REQUIRED", None).await?;
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id,
                    expected: IncidentState::Planning,
                    next: IncidentState::WaitingApproval,
                    actor: "policy-engine".into(),
                    message: "Safe Mode requires approval before the proposed production operation"
                        .into(),
                    metadata: json!({
                        "operationActionId": action_id,
                        "action": "restart_container",
                        "targetId": target
                    }),
                })
                .await?;
        }
        PolicyDecision::Block => {
            mark_operation_status(database, action_id, "BLOCKED", None).await?;
            if work.protection_paused {
                database
                    .transition_incident(IncidentTransition {
                        project_id: work.project_id,
                        incident_id,
                        expected: IncidentState::Planning,
                        next: IncidentState::Paused,
                        actor: "policy-engine".into(),
                        message: "Kill Switch blocked the proposed production operation".into(),
                        metadata: json!({ "operationActionId": action_id }),
                    })
                    .await?;
            } else {
                database
                    .escalate_incident(
                        incident_id,
                        "production operation was blocked by deterministic safety policy or restart cooldown",
                    )
                    .await?;
            }
        }
    }
    Ok(())
}

pub async fn process_execute(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let action_id = operation_action_id(payload)?;
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::Repairing {
        return Ok(());
    }
    if work.protection_paused {
        mark_operation_status(database, action_id, "BLOCKED", None).await?;
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::Repairing,
                next: IncidentState::Paused,
                actor: "operations-worker".into(),
                message: "Kill Switch activated before production operation execution".into(),
                metadata: json!({ "operationActionId": action_id }),
            })
            .await?;
        return Ok(());
    }

    let action = load_operation_action(database, action_id, incident_id).await?;
    if action.action_kind != "restart_container" {
        anyhow::bail!("unsupported persisted operations action")
    }
    match action.status.as_str() {
        "EXECUTED" => {
            begin_verification(database, &work, incident_id, action_id).await?;
            return Ok(());
        }
        "RUNNING" => {
            mark_operation_status(database, action_id, "AMBIGUOUS", None).await?;
            database
                .escalate_incident(
                    incident_id,
                    "a previous container restart may have executed before the worker stopped; NoPager will not repeat an ambiguous production mutation",
                )
                .await?;
            return Ok(());
        }
        "PLANNED" => {}
        _ => return Ok(()),
    }

    let claimed = sqlx::query(
        "UPDATE operations_actions SET status = 'RUNNING', started_at = now() WHERE id = $1 AND incident_id = $2 AND status = 'PLANNED'",
    )
    .bind(action_id)
    .bind(incident_id)
    .execute(database.pool())
    .await?
    .rows_affected();
    if claimed != 1 {
        return Ok(());
    }

    let docker = DockerOperationsClient::from_environment();
    let result = match docker.restart_container(&action.target_id).await {
        Ok(result) => result,
        Err(error) => {
            let error_class = docker_error_class(&error);
            mark_operation_status(
                database,
                action_id,
                "AMBIGUOUS",
                Some(&json!({ "errorClass": error_class })),
            )
            .await?;
            database
                .record_audit_event(
                    work.project_id,
                    Some(incident_id),
                    "operations-worker",
                    "operations.docker.restart",
                    &action.target_id,
                    "unknown",
                    &json!({ "errorClass": error_class }),
                )
                .await?;
            database
                .escalate_incident(
                    incident_id,
                    "Docker restart did not produce a safely provable outcome; NoPager refused to retry the mutation",
                )
                .await?;
            return Ok(());
        }
    };

    let execution = serde_json::to_value(&result)?;
    mark_operation_status(database, action_id, "EXECUTED", Some(&execution)).await?;
    database
        .record_audit_event(
            work.project_id,
            Some(incident_id),
            "operations-worker",
            "operations.docker.restart",
            &action.target_id,
            "success",
            &json!({
                "beforeStatus": result.before.status,
                "afterStatus": result.after.status,
                "operationActionId": action_id
            }),
        )
        .await?;
    begin_verification(database, &work, incident_id, action_id).await?;
    Ok(())
}

async fn begin_verification(
    database: &Database,
    work: &nopager_db::IncidentWork,
    incident_id: Uuid,
    action_id: Uuid,
) -> anyhow::Result<()> {
    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::Repairing,
            next: IncidentState::VerifyingProduction,
            actor: "operations-worker".into(),
            message: "Production operation executed; independently verifying recovery".into(),
            metadata: json!({ "operationActionId": action_id }),
        })
        .await?;
    enqueue_job(
        database,
        "operations-verify",
        &format!("incident:{incident_id}:operations-verify:{action_id}:0"),
        incident_id,
        &json!({
            "incidentId": incident_id,
            "operationActionId": action_id,
            "check": 0
        }),
        1,
        VERIFY_DELAY_SECONDS,
    )
    .await?;
    Ok(())
}

pub async fn process_verify(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let action_id = operation_action_id(payload)?;
    let check = payload.get("check").and_then(Value::as_u64).unwrap_or(0);
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::VerifyingProduction {
        return Ok(());
    }
    let action = load_operation_action(database, action_id, incident_id).await?;
    if action.status != "EXECUTED" {
        database
            .escalate_incident(
                incident_id,
                "production verification cannot proceed because the operation execution state is not provably complete",
            )
            .await?;
        return Ok(());
    }
    let decision: OperationsDecision = serde_json::from_value(action.plan_json.clone())?;
    let required = u64::from(decision.required_consecutive_successes.clamp(2, 3));

    let url = url::Url::parse(&work.health_check_url)?;
    let health = check_http(&url, 200, Duration::from_secs(10)).await;
    let health_ok = health.as_ref().is_ok_and(|observation| observation.success);
    let container = DockerOperationsClient::from_environment()
        .inspect_container(&action.target_id)
        .await;
    let container_ok = container.as_ref().is_ok_and(|state| state.running());

    if !health_ok || !container_ok {
        let verification = json!({
            "externalHealth": health_ok,
            "containerRunning": container_ok,
            "check": check + 1,
            "requiredConsecutiveSuccesses": required
        });
        mark_verification(database, action_id, "VERIFICATION_FAILED", &verification).await?;
        database
            .record_audit_event(
                work.project_id,
                Some(incident_id),
                "operations-worker",
                "operations.recovery.verify",
                &action.target_id,
                "failure",
                &verification,
            )
            .await?;
        database
            .escalate_incident(
                incident_id,
                "container restart completed but independent production health verification failed; NoPager will not loop or issue another restart",
            )
            .await?;
        return Ok(());
    }

    let successes = check + 1;
    if successes < required {
        enqueue_job(
            database,
            "operations-verify",
            &format!("incident:{incident_id}:operations-verify:{action_id}:{successes}"),
            incident_id,
            &json!({
                "incidentId": incident_id,
                "operationActionId": action_id,
                "check": successes
            }),
            1,
            VERIFY_REPEAT_SECONDS,
        )
        .await?;
        return Ok(());
    }

    let verification = json!({
        "externalHealth": true,
        "containerRunning": true,
        "consecutiveSuccesses": successes,
        "requiredConsecutiveSuccesses": required
    });
    mark_verification(database, action_id, "VERIFIED", &verification).await?;
    database
        .record_audit_event(
            work.project_id,
            Some(incident_id),
            "operations-worker",
            "operations.recovery.verify",
            &action.target_id,
            "success",
            &verification,
        )
        .await?;
    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::VerifyingProduction,
            next: IncidentState::Resolved,
            actor: "operations-worker".into(),
            message: "Production recovered and stayed healthy after the bounded container restart"
                .into(),
            metadata: verification,
        })
        .await?;
    Ok(())
}

#[derive(Debug)]
struct PersistedOperationAction {
    action_kind: String,
    target_id: String,
    plan_json: Value,
    status: String,
}

async fn persist_operation_action(
    database: &Database,
    incident_id: Uuid,
    action_kind: &str,
    target_id: &str,
    plan_json: &Value,
    policy_decision: &str,
) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    let inserted = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO operations_actions (id, incident_id, action_kind, target_id, plan_json, policy_decision) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (incident_id) DO NOTHING RETURNING id",
    )
    .bind(id)
    .bind(incident_id)
    .bind(action_kind)
    .bind(target_id)
    .bind(plan_json)
    .bind(policy_decision)
    .fetch_optional(database.pool())
    .await?;
    if let Some(id) = inserted {
        return Ok(id);
    }

    let row = sqlx::query(
        "SELECT id, action_kind, target_id FROM operations_actions WHERE incident_id = $1",
    )
    .bind(incident_id)
    .fetch_one(database.pool())
    .await?;
    let existing_id: Uuid = row.try_get("id")?;
    let existing_kind: String = row.try_get("action_kind")?;
    let existing_target: Option<String> = row.try_get("target_id")?;
    if existing_kind != action_kind || existing_target.as_deref() != Some(target_id) {
        anyhow::bail!("incident already contains a different production operation")
    }
    Ok(existing_id)
}

async fn load_operation_action(
    database: &Database,
    action_id: Uuid,
    incident_id: Uuid,
) -> anyhow::Result<PersistedOperationAction> {
    let row = sqlx::query(
        "SELECT action_kind, target_id, plan_json, status FROM operations_actions WHERE id = $1 AND incident_id = $2",
    )
    .bind(action_id)
    .bind(incident_id)
    .fetch_one(database.pool())
    .await?;
    Ok(PersistedOperationAction {
        action_kind: row.try_get("action_kind")?,
        target_id: row
            .try_get::<Option<String>, _>("target_id")?
            .ok_or_else(|| anyhow::anyhow!("operations target is missing"))?,
        plan_json: row.try_get("plan_json")?,
        status: row.try_get("status")?,
    })
}

async fn mark_operation_status(
    database: &Database,
    action_id: Uuid,
    status: &str,
    execution: Option<&Value>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE operations_actions SET status = $1, execution_json = COALESCE($2, execution_json), completed_at = CASE WHEN $1 IN ('BLOCKED', 'APPROVAL_REQUIRED', 'AMBIGUOUS', 'VERIFIED', 'VERIFICATION_FAILED') THEN now() ELSE completed_at END WHERE id = $3",
    )
    .bind(status)
    .bind(execution)
    .bind(action_id)
    .execute(database.pool())
    .await?;
    Ok(())
}

async fn mark_verification(
    database: &Database,
    action_id: Uuid,
    status: &str,
    verification: &Value,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE operations_actions SET status = $1, verification_json = $2, completed_at = now() WHERE id = $3",
    )
    .bind(status)
    .bind(verification)
    .bind(action_id)
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
        "SELECT NOT EXISTS(SELECT 1 FROM audit_events WHERE project_id = $1 AND action = 'operations.docker.restart' AND target = $2 AND outcome = 'success' AND created_at > now() - ($3 * interval '1 minute'))",
    )
    .bind(project_id)
    .bind(target)
    .bind(RESTART_COOLDOWN_MINUTES)
    .fetch_one(database.pool())
    .await?)
}

async fn enqueue_job(
    database: &Database,
    job_type: &str,
    idempotency_key: &str,
    incident_id: Uuid,
    payload: &Value,
    max_attempts: i32,
    delay_seconds: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO jobs (id, job_type, idempotency_key, correlation_id, payload_json, max_attempts, available_at) VALUES ($1, $2, $3, $4, $5, $6, now() + ($7 * interval '1 second')) ON CONFLICT (idempotency_key) DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .bind(job_type)
    .bind(idempotency_key)
    .bind(incident_id)
    .bind(payload)
    .bind(max_attempts)
    .bind(delay_seconds.max(0))
    .execute(database.pool())
    .await?;
    Ok(())
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
        .filter(|value| !value.trim().is_empty())
        .map(SecretString::from)
        .ok_or_else(|| anyhow::anyhow!("{name} is required for the selected provider"))
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

const fn policy_name(decision: PolicyDecision) -> &'static str {
    match decision {
        PolicyDecision::Allow => "allow",
        PolicyDecision::RequireApproval => "require_approval",
        PolicyDecision::Block => "block",
    }
}

fn configured_container_target() -> Option<String> {
    std::env::var("NOPAGER_DOCKER_TARGET")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn deployment_recovery_configured(
    repo_owner: &str,
    repo_name: &str,
    vercel_project_id: Option<&str>,
    github_metadata: &Value,
) -> bool {
    !repo_owner.trim().is_empty()
        && !repo_name.trim().is_empty()
        && vercel_project_id.is_some_and(|value| !value.trim().is_empty())
        && github_metadata
            .as_object()
            .is_some_and(|metadata| !metadata.is_empty())
}

fn env_true(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn docker_error_class(error: &DockerOperationsError) -> &'static str {
    match error {
        DockerOperationsError::InvalidTarget => "invalid_target",
        DockerOperationsError::Timeout => "timeout",
        DockerOperationsError::Io(_) => "io",
        DockerOperationsError::CommandFailed { .. } => "command_failed",
        DockerOperationsError::InvalidInspectOutput => "invalid_inspect_output",
        DockerOperationsError::ControlPlaneTarget => "control_plane_target",
        DockerOperationsError::RestartDidNotStart => "restart_did_not_start",
    }
}

fn incident_id(payload: &Value) -> anyhow::Result<Uuid> {
    required_string(payload, "incidentId")?
        .parse()
        .map_err(Into::into)
}

fn operation_action_id(payload: &Value) -> anyhow::Result<Uuid> {
    required_string(payload, "operationActionId")?
        .parse()
        .map_err(Into::into)
}

fn required_string<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{key} is missing"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_mode_is_not_silently_promoted_to_autopilot() {
        assert_eq!(parse_safety_mode("safe").unwrap(), SafetyMode::Safe);
        assert!(parse_safety_mode("autopilot").is_err());
    }

    #[test]
    fn docker_error_classes_do_not_expose_command_output() {
        let error = DockerOperationsError::CommandFailed {
            code: Some(1),
            message: "secret-looking stderr".into(),
        };
        assert_eq!(docker_error_class(&error), "command_failed");
    }

    #[test]
    fn verification_has_a_trusted_minimum_of_two_successes() {
        for model_value in [0_u8, 1, 2, 10] {
            assert!(u64::from(model_value.clamp(2, 3)) >= 2);
        }
    }

    #[test]
    fn operations_only_projects_cannot_delegate_to_deployment_recovery() {
        assert!(!deployment_recovery_configured("", "", None, &json!({})));
        assert!(!deployment_recovery_configured(
            "owner",
            "repo",
            None,
            &json!({ "repoId": 1 })
        ));
        assert!(deployment_recovery_configured(
            "owner",
            "repo",
            Some("vercel-project"),
            &json!({ "repoId": 1 })
        ));
    }
}
