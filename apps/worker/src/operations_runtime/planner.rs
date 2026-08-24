use std::time::{SystemTime, UNIX_EPOCH};

use nopager_connectors::docker_ops::DockerOperationsClient;
use nopager_core::IncidentState;
use nopager_db::{Database, IncidentTransition, JobType};
use nopager_policy::{
    ActionRisk, OperationsPolicyContext, PolicyDecision, decide_operations,
};
use nopager_providers::{
    AvailableOperationsAction, AvailableVerificationSignal, ModelProvider, OperationsActionKind,
    OperationsDecision, OperationsInput, OperationsVerificationKind,
};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    OBSERVE_TIMEOUT_SECONDS, OPERATIONS_PIPELINE, PRIMARY_HEALTH_SOURCE, VERIFY_INTERVAL_SECONDS,
    bounded_error, incident_id, work_id,
};
use super::{config, ledger, provider};

pub(super) async fn route_incident_context(
    database: &Database,
    payload: &Value,
) -> anyhow::Result<()> {
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

    let work_id = ledger::ensure_operations_work(database, &work, "health_check").await?;
    ledger::append_work_event(
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

pub(super) async fn plan_operations(
    database: &Database,
    payload: &Value,
) -> anyhow::Result<()> {
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

    let target = config::configured_docker_target();
    let restart_enabled = config::docker_restart_enabled();
    let docker = DockerOperationsClient::from_environment();
    let container_context = match target.as_deref() {
        Some(target) => match docker.inspect_container(target).await {
            Ok(state) => serde_json::to_value(state)?,
            Err(error) => {
                json!({ "targetId": target, "inspectError": bounded_error(&error.to_string()) })
            }
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
            "Delegate to source/deployment recovery only when evidence supports a deployment regression",
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
            "Restart exactly the configured customer container once; external health verification is mandatory afterward",
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
    let provider = provider::provider_for(database, work.project_id).await?;
    let decision = provider.plan_operations(&input).await?;
    persist_decision(database, work_id, provider.id(), &decision).await?;

    match decision.action {
        OperationsActionKind::DelegateDeploymentRecovery => {
            ledger::append_work_event(
                database,
                work_id,
                "DELEGATED",
                "operations-router",
                "Evidence supports the existing deployment recovery subsystem",
                &json!({}),
            )
            .await?;
            ledger::set_work_status(database, work_id, "DELEGATED").await?;
            database
                .enqueue(
                    JobType::Diagnose,
                    &format!("incident:{incident_id}:deployment-diagnose-after-operations"),
                    Some(incident_id),
                    &json!({
                        "incidentId": incident_id,
                        "pipeline": "deployment-recovery"
                    }),
                    3,
                )
                .await?;
        }
        OperationsActionKind::Escalate => {
            ledger::set_work_status(database, work_id, "ESCALATED").await?;
            database
                .escalate_incident(incident_id, &decision.escalation_reason)
                .await?;
        }
        OperationsActionKind::ObserveOnly => {
            ledger::set_work_status(database, work_id, "OBSERVING").await?;
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
            ledger::set_work_status(database, work_id, "ESCALATED").await?;
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

async fn persist_decision(
    database: &Database,
    work_id: Uuid,
    provider_id: &str,
    decision: &OperationsDecision,
) -> anyhow::Result<()> {
    let decision_json = serde_json::to_value(decision)?;
    ledger::append_work_event(
        database,
        work_id,
        "OPERATIONS_DECISION",
        &format!("provider:{provider_id}"),
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
    Ok(())
}

async fn plan_mutating_action(
    database: &Database,
    work: &nopager_db::IncidentWork,
    work_id: Uuid,
    decision: &OperationsDecision,
    restart_enabled: bool,
) -> anyhow::Result<()> {
    let target = config::configured_docker_target();
    let cooldown_clear = match target.as_deref() {
        Some(target) => ledger::restart_cooldown_clear(database, work.project_id, target).await?,
        None => false,
    };
    let policy = decide_operations(
        ActionRisk::Low,
        true,
        OperationsPolicyContext {
            mode: config::parse_safety_mode(&work.safety_mode)?,
            kill_switch_active: work.protection_paused,
            target_configured: target.as_deref() == Some(decision.target_id.as_str()),
            action_enabled: restart_enabled,
            verification_configured: decision.verification_signals.iter().any(|signal| {
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
    ledger::append_work_event(
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
        PolicyDecision::Allow => allow_execution(database, work, work_id, decision).await?,
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
            ledger::set_work_status(database, work_id, "WAITING_APPROVAL").await?;
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
                ledger::set_work_status(database, work_id, "PAUSED").await?;
            } else {
                ledger::set_work_status(database, work_id, "ESCALATED").await?;
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

async fn allow_execution(
    database: &Database,
    work: &nopager_db::IncidentWork,
    work_id: Uuid,
    decision: &OperationsDecision,
) -> anyhow::Result<()> {
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
    ledger::set_work_status(database, work_id, "EXECUTING").await?;
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
    Ok(())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
