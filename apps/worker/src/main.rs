use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};

use nopager_connectors::github::{GitHubAppAuth, PullRequestInput, RepairFile, repair_branch};
use nopager_connectors::vercel::{GitSource, VercelClient};
use nopager_core::IncidentState;
use nopager_crypto::SecretCipher;
use nopager_db::{Database, IncidentTransition, IncidentTrigger, Job, JobType};
use nopager_monitor::check_http;
use nopager_policy::{ActionRisk, PolicyContext, PolicyDecision, SafetyMode, decide};
use nopager_providers::{
    AnthropicProvider, CommitContext, DiagnosisInput, DiagnosisResult, GeminiProvider,
    ModelProvider, OpenAiProvider, RepairInput, RepairProposal, RiskLevel,
};
use nopager_sandbox::{ControlledCommand, DockerSandbox, apply_unified_diff};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .json()
        .init();
    info!("NoPager worker ready");
    let database_url = std::env::var("DATABASE_URL")?;
    let database = Database::connect(&database_url).await?;
    database.migrate().await?;
    let worker_id = format!("worker-{}", std::process::id());

    let mut scheduler = tokio::time::interval(Duration::from_secs(5));
    scheduler.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = scheduler.tick() => {
                if let Err(error) = database.enqueue_due_health_checks(100).await {
                    error!(%error, "failed to schedule health checks");
                }
        if let Err(error) = database.enqueue_due_vercel_polls(100).await {
            error!(%error, "failed to schedule Vercel polling fallback");
        }
            }
            () = tokio::time::sleep(Duration::from_millis(750)) => {
                if let Some(job) = database.claim_next(&worker_id).await? {
                    match execute_job(&database, &job).await {
                        Ok(()) => database.complete_job(job.id, &worker_id).await?,
                        Err(error) => {
                            error!(job_id = %job.id, %error, "job failed");
                            database.fail_job(&job, &worker_id, &error.to_string()).await?;
                            if job.attempt >= job.max_attempts
                                && let Some(incident_id) = job.correlation_id
                            {
                                database.escalate_incident(incident_id, &error.to_string()).await?;
                            }
                        }
                    }
                }
            }
            result = tokio::signal::ctrl_c() => {
                result?;
                info!("worker shutting down");
                return Ok(());
            }
        }
    }
}

async fn execute_job(database: &Database, job: &Job) -> anyhow::Result<()> {
    info!(job_id = %job.id, job_type = %job.job_type, attempt = job.attempt, "executing job");
    match job.job_type.as_str() {
        "health-check" => process_health_check(database, &job.payload_json).await,
        "vercel-poll" => process_vercel_poll(database, &job.payload_json).await,
        "webhook-process" => process_webhook(database, &job.payload_json).await,
        "incident-context" => process_incident_context(database, &job.payload_json).await,
        "diagnose" => process_diagnosis(database, &job.payload_json).await,
        "repair" => process_repair(database, &job.payload_json).await,
        "build-test" => process_build_test(database, &job.payload_json).await,
        "preview-deploy" => process_preview_deploy(database, &job.payload_json).await,
        "verify" => process_verification(database, &job.payload_json).await,
        "production-action" => process_production_action(database, &job.payload_json).await,
        "post-deploy-watch" => process_post_deploy_watch(database, &job.payload_json).await,
        "cleanup" => process_cleanup(&job.payload_json),
        other => anyhow::bail!("unsupported job type: {other}"),
    }
}

async fn process_incident_context(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let work = database.incident_work(incident_id).await?;
    if work.state == IncidentState::Open {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::Open,
                next: IncidentState::CollectingContext,
                actor: "worker".into(),
                message: "Collecting recent deployment and failure context".into(),
                metadata: json!({}),
            })
            .await?;
    } else if work.state != IncidentState::CollectingContext {
        return Ok(());
    }
    database
        .enqueue(
            JobType::Diagnose,
            &format!("incident:{incident_id}:diagnose"),
            Some(incident_id),
            &json!({ "incidentId": incident_id }),
            3,
        )
        .await?;
    Ok(())
}

async fn process_diagnosis(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let mut work = database.incident_work(incident_id).await?;
    if work.state == IncidentState::CollectingContext {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::CollectingContext,
                next: IncidentState::Diagnosing,
                actor: "worker".into(),
                message: "Analyzing the incident evidence".into(),
                metadata: json!({}),
            })
            .await?;
        work.state = IncidentState::Diagnosing;
    }
    if work.state != IncidentState::Diagnosing {
        return Ok(());
    }
    let base_sha = find_string(
        &work.trigger_context,
        &["sha", "head_sha", "githubCommitSha"],
    )
    .or_else(|| {
        find_string(
            &work.deployment_context,
            &["commit_sha", "sha", "githubCommitSha"],
        )
    })
    .unwrap_or_else(|| "unknown".into());
    let recent_commits = if base_sha == "unknown" {
        Vec::new()
    } else {
        let github = github_client(database, &work).await?;
        let commit = github
            .get_commit(&work.repo_owner, &work.repo_name, &base_sha)
            .await?;
        vec![CommitContext {
            sha: commit.sha,
            message: commit.message,
            changed_files: commit.changed_files,
        }]
    };
    let provider = provider_for(database, work.project_id).await?;
    let input = DiagnosisInput {
        incident_summary: work.title.clone(),
        recent_commits,
        stack_trace: find_string(&work.trigger_context, &["stack", "stackTrace", "error"]),
        deployment: work.deployment_context.clone(),
        health_failure: work.trigger_context.clone(),
        relevant_files: Vec::new(),
    };
    let diagnosis = provider.diagnose(&input).await?;
    let diagnosis_json = serde_json::to_value(&diagnosis)?;
    let attempt_id = database
        .create_repair_attempt(incident_id, &base_sha, &diagnosis_json)
        .await?;
    database
        .record_audit_event(
            work.project_id,
            Some(incident_id),
            &format!("provider:{}", provider.id()),
            "model.diagnose",
            &attempt_id.to_string(),
            "success",
            &json!({ "riskLevel": diagnosis.risk_level }),
        )
        .await?;
    database
        .save_root_cause_summary(incident_id, &diagnosis.suspected_root_cause)
        .await?;
    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::Diagnosing,
            next: IncidentState::Planning,
            actor: format!("provider:{}", provider.id()),
            message: "Root cause analysis completed".into(),
            metadata: diagnosis_json.clone(),
        })
        .await?;
    database.enqueue(JobType::Repair, &format!("incident:{incident_id}:attempt:{attempt_id}:repair"), Some(incident_id), &json!({ "incidentId": incident_id, "attemptId": attempt_id, "diagnosis": diagnosis_json }), 3).await?;
    Ok(())
}

async fn process_repair(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let attempt_id: uuid::Uuid = required_string(payload, "attemptId")?.parse()?;
    let mut work = database.incident_work(incident_id).await?;
    if work.state == IncidentState::Planning {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::Planning,
                next: IncidentState::Repairing,
                actor: "worker".into(),
                message: "Preparing the smallest reversible repair".into(),
                metadata: json!({}),
            })
            .await?;
        work.state = IncidentState::Repairing;
    }
    if work.state != IncidentState::Repairing {
        return Ok(());
    }
    let diagnosis: DiagnosisResult = serde_json::from_value(
        payload
            .get("diagnosis")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("diagnosis is missing"))?,
    )?;
    if diagnosis.risk_level == RiskLevel::High {
        database
            .escalate_incident(incident_id, "diagnosis classified the repair as high risk")
            .await?;
        return Ok(());
    }
    let provider = provider_for(database, work.project_id).await?;
    let proposal = provider
        .propose_patch(&RepairInput {
            diagnosis,
            repository_rules: vec![
                "Do not add secrets, destructive migrations, or production-side commands".into(),
            ],
            previous_failures: payload
                .get("previousFailures")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        })
        .await?;
    database
        .record_audit_event(
            work.project_id,
            Some(incident_id),
            &format!("provider:{}", provider.id()),
            "model.repair",
            &attempt_id.to_string(),
            "success",
            &json!({ "changedFiles": proposal.changed_files }),
        )
        .await?;
    let fingerprint = hex::encode(Sha256::digest(proposal.unified_diff.as_bytes()));
    let proposal_json = serde_json::to_value(&proposal)?;
    database
        .save_repair_proposal(
            attempt_id,
            &proposal_json,
            &proposal.unified_diff,
            &fingerprint,
        )
        .await?;
    database
        .enqueue(
            JobType::BuildTest,
            &format!("incident:{incident_id}:attempt:{attempt_id}:build"),
            Some(incident_id),
            &json!({ "incidentId": incident_id, "attemptId": attempt_id }),
            3,
        )
        .await?;
    Ok(())
}

async fn process_build_test(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let attempt_id = required_string(payload, "attemptId")?.parse()?;
    let mut work = database.incident_work(incident_id).await?;
    let attempt = database.repair_attempt(attempt_id).await?;
    if attempt.incident_id != incident_id {
        anyhow::bail!("repair attempt does not belong to the incident");
    }
    if work.protection_paused {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: work.state,
                next: IncidentState::Paused,
                actor: "worker".into(),
                message: "Protection paused before sandbox validation or repository mutation"
                    .into(),
                metadata: json!({}),
            })
            .await?;
        return Ok(());
    }
    if attempt.repair_branch.is_some() {
        database
            .enqueue(
                JobType::PreviewDeploy,
                &format!("incident:{incident_id}:attempt:{attempt_id}:preview"),
                Some(incident_id),
                &json!({ "incidentId": incident_id, "attemptId": attempt_id }),
                10,
            )
            .await?;
        return Ok(());
    }
    if work.state == IncidentState::Repairing {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::Repairing,
                next: IncidentState::Testing,
                actor: "worker".into(),
                message: "Running the repair in an isolated sandbox".into(),
                metadata: json!({}),
            })
            .await?;
        work.state = IncidentState::Testing;
    }
    if work.state != IncidentState::Testing {
        return Ok(());
    }
    if attempt.base_commit_sha == "unknown" {
        anyhow::bail!("no concrete base commit SHA is available for this incident");
    }
    let proposal: RepairProposal = serde_json::from_value(attempt.proposal.clone())?;
    if proposal
        .changed_files
        .iter()
        .any(|path| is_dependency_manifest(path))
    {
        database
            .escalate_incident(
                incident_id,
                "dependency manifest changes require human review",
            )
            .await?;
        return Ok(());
    }

    let (workspace, relative_workspace) = prepare_workspace(incident_id, attempt_id)?;
    let github = github_client(database, &work).await?;
    github
        .download_archive(
            &work.repo_owner,
            &work.repo_name,
            &attempt.base_commit_sha,
            &workspace,
        )
        .await?;
    let patched = apply_unified_diff(&workspace, &attempt.patch_diff)?;
    let expected: BTreeSet<PathBuf> = proposal.changed_files.iter().map(PathBuf::from).collect();
    let actual: BTreeSet<PathBuf> = patched.into_iter().collect();
    if actual != expected {
        anyhow::bail!("patch file set differs from the provider's declared changed files");
    }

    let mut validation = Vec::new();
    for specification in detected_validation_commands(&workspace)? {
        let network = is_install_command(&specification);
        let image = if specification.program == "cargo" {
            "rust:1.92-bookworm"
        } else {
            "node:22-bookworm"
        };
        let sandbox = sandbox_for(
            workspace.clone(),
            relative_workspace.clone(),
            image,
            network,
        )?;
        let output = sandbox.run(&specification).await?;
        let passed = output.exit_code == Some(0);
        validation.push(json!({ "command": specification, "output": output, "passed": passed }));
        if !passed {
            let failure = validation
                .last()
                .map(ToString::to_string)
                .unwrap_or_else(|| "sandbox validation failed without output".into());
            database
                .save_validation(attempt_id, &Value::Array(validation), false)
                .await?;
            schedule_followup_repair(database, &work, &attempt, &failure).await?;
            return Ok(());
        }
    }
    database
        .save_validation(attempt_id, &Value::Array(validation.clone()), true)
        .await?;

    work = database.incident_work(incident_id).await?;
    if work.protection_paused {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::Testing,
                next: IncidentState::Paused,
                actor: "worker".into(),
                message: "Protection paused before creating the repair pull request".into(),
                metadata: json!({}),
            })
            .await?;
        return Ok(());
    }

    let files = proposal
        .changed_files
        .iter()
        .map(|path| {
            let contents = std::fs::read(workspace.join(path))?;
            Ok(RepairFile {
                path: path.clone(),
                contents,
            })
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    let diagnosis: DiagnosisResult = serde_json::from_value(attempt.diagnosis.clone())?;
    let branch = repair_branch(&incident_id.to_string(), &work.title);
    let pull_request = github
        .open_repair_pr(&PullRequestInput {
            owner: work.repo_owner.clone(),
            repository: work.repo_name.clone(),
            base_branch: work
                .github_metadata
                .get("baseBranch")
                .and_then(Value::as_str)
                .unwrap_or("main")
                .to_owned(),
            base_sha: attempt.base_commit_sha,
            incident_id: incident_id.to_string(),
            title: format!("fix: {}", work.title),
            diagnosis: diagnosis.suspected_root_cause,
            verification: validation_summary(&validation),
            rollback: diagnosis.rollback_plan,
            files,
        })
        .await?;
    database
        .save_pull_request(
            attempt_id,
            &branch,
            &pull_request.head.sha,
            pull_request.number,
            &pull_request.html_url,
        )
        .await?;
    database
        .record_audit_event(
            work.project_id,
            Some(incident_id),
            "worker",
            "github.repair_pr.create",
            &pull_request.html_url,
            "success",
            &json!({ "number": pull_request.number, "branch": branch }),
        )
        .await?;
    database
        .enqueue(
            JobType::PreviewDeploy,
            &format!("incident:{incident_id}:attempt:{attempt_id}:preview"),
            Some(incident_id),
            &json!({ "incidentId": incident_id, "attemptId": attempt_id }),
            10,
        )
        .await?;
    Ok(())
}

async fn schedule_followup_repair(
    database: &Database,
    work: &nopager_db::IncidentWork,
    attempt: &nopager_db::RepairAttemptWork,
    failure: &str,
) -> anyhow::Result<()> {
    let max_repair_attempts = database.max_repair_attempts(work.project_id).await?;
    if attempt.attempt_number >= max_repair_attempts {
        database
            .escalate_incident(work.id, "three repair attempts failed sandbox validation")
            .await?;
        return Ok(());
    }
    let next_attempt = database
        .create_followup_repair_attempt(work.id, &attempt.base_commit_sha, &attempt.diagnosis)
        .await?;
    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id: work.id,
            expected: IncidentState::Testing,
            next: IncidentState::Repairing,
            actor: "worker".into(),
            message: format!(
                "Validation failed; starting repair attempt {}",
                attempt.attempt_number + 1
            ),
            metadata: json!({ "previousAttemptId": attempt.id, "failure": failure }),
        })
        .await?;
    database
        .enqueue(
            JobType::Repair,
            &format!("incident:{}:attempt:{next_attempt}:repair", work.id),
            Some(work.id),
            &json!({
                "incidentId": work.id,
                "attemptId": next_attempt,
                "diagnosis": attempt.diagnosis,
                "previousFailures": [failure]
            }),
            3,
        )
        .await?;
    Ok(())
}

async fn process_preview_deploy(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let attempt_id = required_string(payload, "attemptId")?.parse()?;
    let mut work = database.incident_work(incident_id).await?;
    let attempt = database.repair_attempt(attempt_id).await?;
    if work.protection_paused {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: work.state,
                next: IncidentState::Paused,
                actor: "worker".into(),
                message: "Protection was paused before preview deployment".into(),
                metadata: json!({}),
            })
            .await?;
        return Ok(());
    }
    if work.state == IncidentState::Testing {
        database
            .transition_incident(IncidentTransition {
                project_id: work.project_id,
                incident_id,
                expected: IncidentState::Testing,
                next: IncidentState::PreviewDeploying,
                actor: "worker".into(),
                message: "Creating an isolated Vercel preview".into(),
                metadata: json!({}),
            })
            .await?;
        work.state = IncidentState::PreviewDeploying;
    }
    if work.state != IncidentState::PreviewDeploying {
        return Ok(());
    }
    let (deployment_id, preview_url) =
        if let (Some(id), Some(url)) = (attempt.preview_deployment_id, attempt.preview_url) {
            (id, url)
        } else {
            let branch = attempt
                .repair_branch
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("repair branch is missing"))?;
            let commit_sha = attempt
                .repair_commit_sha
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("repair commit SHA is missing"))?;
            let repo_id = metadata_u64(&work.github_metadata, "repoId")?;
            let deployment = vercel_client(database, &work)
                .await?
                .create_preview(
                    vercel_project_name(&work)?,
                    &GitSource {
                        kind: "github".into(),
                        repo_id,
                        ref_name: branch.into(),
                        sha: commit_sha.into(),
                    },
                )
                .await?;
            let url = https_url(&deployment.url);
            database
                .save_preview(attempt_id, &deployment.id, &url)
                .await?;
            database
                .save_deployment(
                    work.project_id,
                    &deployment.id,
                    "preview",
                    commit_sha,
                    &url,
                    deployment.ready_state.as_deref().unwrap_or("QUEUED"),
                    false,
                )
                .await?;
            database
                .record_audit_event(
                    work.project_id,
                    Some(incident_id),
                    "worker",
                    "vercel.preview.create",
                    &deployment.id,
                    "success",
                    &json!({ "url": url }),
                )
                .await?;
            (deployment.id, url)
        };
    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::PreviewDeploying,
            next: IncidentState::VerifyingPreview,
            actor: "worker".into(),
            message: "Preview created; waiting for build and health verification".into(),
            metadata: json!({ "deploymentId": deployment_id, "previewUrl": preview_url }),
        })
        .await?;
    database
        .enqueue_after(
            JobType::Verify,
            &format!("incident:{incident_id}:attempt:{attempt_id}:verify-preview"),
            Some(incident_id),
            &json!({ "incidentId": incident_id, "attemptId": attempt_id, "phase": "preview" }),
            10,
            5,
        )
        .await?;
    Ok(())
}

async fn process_verification(database: &Database, payload: &Value) -> anyhow::Result<()> {
    match required_string(payload, "phase")? {
        "preview" => verify_preview(database, payload).await,
        "production" => verify_production(database, payload).await,
        "rollback" => verify_rollback(database, payload).await,
        phase => anyhow::bail!("unsupported verification phase: {phase}"),
    }
}

async fn verify_preview(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let attempt_id = required_string(payload, "attemptId")?.parse()?;
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::VerifyingPreview {
        return Ok(());
    }
    let attempt = database.repair_attempt(attempt_id).await?;
    let deployment_id = attempt
        .preview_deployment_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("preview deployment id is missing"))?;
    let deployment = vercel_client(database, &work)
        .await?
        .get_deployment(deployment_id)
        .await?;
    match deployment.ready_state.as_deref() {
        Some("READY") => {}
        Some("ERROR" | "CANCELED") => anyhow::bail!("Vercel preview deployment failed"),
        _ => anyhow::bail!("Vercel preview is not ready yet"),
    }
    let preview_url = attempt
        .preview_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("preview URL is missing"))?;
    let preview_check_url = preview_health_url(preview_url, &work.health_check_url)?;
    require_healthy(&preview_check_url).await?;
    database
        .save_deployment(
            work.project_id,
            deployment_id,
            "preview",
            attempt.repair_commit_sha.as_deref().unwrap_or("unknown"),
            preview_url,
            "READY",
            false,
        )
        .await?;
    let diagnosis: DiagnosisResult = serde_json::from_value(attempt.diagnosis)?;
    let risk = match diagnosis.risk_level {
        RiskLevel::Low => ActionRisk::Low,
        RiskLevel::Medium => ActionRisk::Medium,
        RiskLevel::High => ActionRisk::High,
    };
    let reversible = database
        .latest_known_good_deployment(work.project_id)
        .await?
        .is_some();
    let decision = decide(
        risk,
        PolicyContext {
            mode: parse_safety_mode(&work.safety_mode)?,
            kill_switch_active: work.protection_paused,
            preview_verified: true,
            reversible,
        },
    );
    let next = match decision {
        PolicyDecision::Allow => IncidentState::ProductionDeploying,
        PolicyDecision::RequireApproval => IncidentState::WaitingApproval,
        PolicyDecision::Block => IncidentState::Paused,
    };
    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::VerifyingPreview,
            next,
            actor: "policy-engine".into(),
            message: match next {
                IncidentState::WaitingApproval => {
                    "Preview verified; production approval is required"
                }
                IncidentState::ProductionDeploying => {
                    "Preview verified; low-risk Autopilot promotion allowed"
                }
                _ => "Preview verified, but mutations are paused",
            }
            .into(),
            metadata: json!({ "risk": format!("{risk:?}"), "reversible": reversible }),
        })
        .await?;
    if next == IncidentState::ProductionDeploying {
        database
            .enqueue(
                JobType::ProductionAction,
                &format!("incident:{incident_id}:production"),
                Some(incident_id),
                &json!({ "incidentId": incident_id, "attemptId": attempt_id, "action": "promote" }),
                5,
            )
            .await?;
    }
    Ok(())
}

async fn process_production_action(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let attempt_id = required_string(payload, "attemptId")?.parse()?;
    let action = required_string(payload, "action")?;
    let work = database.incident_work(incident_id).await?;
    if work.protection_paused {
        if work.state != IncidentState::Paused {
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id,
                    expected: work.state,
                    next: IncidentState::Paused,
                    actor: "worker".into(),
                    message: "Protection paused before production mutation".into(),
                    metadata: json!({}),
                })
                .await?;
        }
        return Ok(());
    }
    let attempt = database.repair_attempt(attempt_id).await?;
    let vercel_project_id = work
        .vercel_project_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Vercel project id is missing"))?;
    let vercel = vercel_client(database, &work).await?;
    match action {
        "promote" => {
            if work.state != IncidentState::ProductionDeploying {
                return Ok(());
            }
            let deployment_id = attempt
                .preview_deployment_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("preview deployment id is missing"))?;
            let current = vercel.get_deployment(deployment_id).await?;
            if current.target.as_deref() != Some("production") {
                vercel.promote(vercel_project_id, deployment_id).await?;
            }
            database
                .save_deployment(
                    work.project_id,
                    deployment_id,
                    "production",
                    attempt.repair_commit_sha.as_deref().unwrap_or("unknown"),
                    &work.production_url,
                    "PROMOTED",
                    false,
                )
                .await?;
            database
                .record_audit_event(
                    work.project_id,
                    Some(incident_id),
                    "worker",
                    "vercel.production.promote",
                    deployment_id,
                    "success",
                    &json!({}),
                )
                .await?;
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id,
                    expected: IncidentState::ProductionDeploying,
                    next: IncidentState::VerifyingProduction,
                    actor: "worker".into(),
                    message: "Repair promoted; verifying production health".into(),
                    metadata: json!({ "deploymentId": deployment_id }),
                })
                .await?;
            database.enqueue_after(JobType::Verify, &format!("incident:{incident_id}:verify-production"), Some(incident_id), &json!({ "incidentId": incident_id, "attemptId": attempt_id, "phase": "production" }), 5, 5).await?;
        }
        "rollback" => {
            if work.state != IncidentState::RollingBack {
                return Ok(());
            }
            let (known_good_id, _) = database
                .latest_known_good_deployment(work.project_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("no known-good production deployment exists"))?;
            // The known-good record predates this repair promotion.
            // Vercel target describes an environment, not current traffic.
            // Always restore the recorded known-good deployment explicitly.
            vercel.rollback(vercel_project_id, &known_good_id).await?;
            database
                .record_audit_event(
                    work.project_id,
                    Some(incident_id),
                    "worker",
                    "vercel.production.rollback",
                    &known_good_id,
                    "success",
                    &json!({}),
                )
                .await?;
            database
                .transition_incident(IncidentTransition {
                    project_id: work.project_id,
                    incident_id,
                    expected: IncidentState::RollingBack,
                    next: IncidentState::RolledBack,
                    actor: "worker".into(),
                    message: "Production rolled back to the previous known-good deployment".into(),
                    metadata: json!({ "deploymentId": known_good_id }),
                })
                .await?;
            database.enqueue_after(JobType::Verify, &format!("incident:{incident_id}:verify-rollback"), Some(incident_id), &json!({ "incidentId": incident_id, "attemptId": attempt_id, "phase": "rollback" }), 5, 5).await?;
        }
        _ => anyhow::bail!("unsupported production action: {action}"),
    }
    Ok(())
}

async fn verify_production(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let attempt_id: uuid::Uuid = required_string(payload, "attemptId")?.parse()?;
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::VerifyingProduction {
        return Ok(());
    }
    if let Err(error) = require_healthy(&work.health_check_url).await {
        begin_rollback(database, &work, incident_id, attempt_id, &error.to_string()).await?;
        return Ok(());
    }
    database
        .enqueue_after(
            JobType::PostDeployWatch,
            &format!("incident:{incident_id}:watch:0"),
            Some(incident_id),
            &json!({ "incidentId": incident_id, "attemptId": attempt_id, "check": 0 }),
            3,
            10,
        )
        .await?;
    Ok(())
}

async fn process_post_deploy_watch(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let attempt_id = required_string(payload, "attemptId")?.parse()?;
    let check = payload
        .get("check")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("watch check is missing"))?;
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::VerifyingProduction {
        return Ok(());
    }
    if let Err(error) = require_healthy(&work.health_check_url).await {
        begin_rollback(database, &work, incident_id, attempt_id, &error.to_string()).await?;
        return Ok(());
    }
    if check < 2 {
        let next = check + 1;
        database
            .enqueue_after(
                JobType::PostDeployWatch,
                &format!("incident:{incident_id}:watch:{next}"),
                Some(incident_id),
                &json!({ "incidentId": incident_id, "attemptId": attempt_id, "check": next }),
                3,
                10,
            )
            .await?;
        return Ok(());
    }
    let attempt = database.repair_attempt(attempt_id).await?;
    let deployment_id = attempt
        .preview_deployment_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("production deployment id is missing"))?;
    database
        .mark_deployment_known_good(work.project_id, deployment_id)
        .await?;
    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::VerifyingProduction,
            next: IncidentState::Resolved,
            actor: "worker".into(),
            message: "Production remained healthy throughout the post-deploy watch".into(),
            metadata: json!({ "checks": 3 }),
        })
        .await?;
    enqueue_cleanup(database, incident_id, attempt_id).await?;
    Ok(())
}

async fn begin_rollback(
    database: &Database,
    work: &nopager_db::IncidentWork,
    incident_id: uuid::Uuid,
    attempt_id: uuid::Uuid,
    error: &str,
) -> anyhow::Result<()> {
    if database
        .latest_known_good_deployment(work.project_id)
        .await?
        .is_none()
    {
        database
            .escalate_incident(
                incident_id,
                "production verification failed and no known-good deployment exists",
            )
            .await?;
        return Ok(());
    }
    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::VerifyingProduction,
            next: IncidentState::RollingBack,
            actor: "worker".into(),
            message: "Production verification failed; starting rollback".into(),
            metadata: json!({ "error": error }),
        })
        .await?;
    database
        .enqueue(
            JobType::ProductionAction,
            &format!("incident:{incident_id}:rollback"),
            Some(incident_id),
            &json!({ "incidentId": incident_id, "attemptId": attempt_id, "action": "rollback" }),
            5,
        )
        .await?;
    Ok(())
}

async fn verify_rollback(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let attempt_id = required_string(payload, "attemptId")?.parse()?;
    let work = database.incident_work(incident_id).await?;
    if work.state != IncidentState::RolledBack {
        return Ok(());
    }
    require_healthy(&work.health_check_url).await?;
    database
        .transition_incident(IncidentTransition {
            project_id: work.project_id,
            incident_id,
            expected: IncidentState::RolledBack,
            next: IncidentState::Resolved,
            actor: "worker".into(),
            message: "Rollback restored production health".into(),
            metadata: json!({}),
        })
        .await?;
    enqueue_cleanup(database, incident_id, attempt_id).await?;
    Ok(())
}

async fn enqueue_cleanup(
    database: &Database,
    incident_id: uuid::Uuid,
    attempt_id: uuid::Uuid,
) -> anyhow::Result<()> {
    database
        .enqueue(
            JobType::Cleanup,
            &format!("incident:{incident_id}:attempt:{attempt_id}:cleanup"),
            Some(incident_id),
            &json!({ "incidentId": incident_id, "attemptId": attempt_id }),
            3,
        )
        .await?;
    Ok(())
}

fn process_cleanup(payload: &Value) -> anyhow::Result<()> {
    let incident_id = incident_id(payload)?;
    let attempt_id: uuid::Uuid = required_string(payload, "attemptId")?.parse()?;
    let root = work_root()?;
    let workspace = root
        .join(incident_id.to_string())
        .join(attempt_id.to_string());
    if workspace.exists() {
        let canonical = workspace.canonicalize()?;
        if !canonical.starts_with(&root) {
            anyhow::bail!("cleanup path escaped workspace root");
        }
        std::fs::remove_dir_all(canonical)?;
    }
    Ok(())
}

async fn vercel_client(
    database: &Database,
    work: &nopager_db::IncidentWork,
) -> anyhow::Result<VercelClient> {
    if let Ok(credentials) = integration_credentials(database, work.project_id, "vercel").await {
        let token = required_string(&credentials, "token")?;
        let team_id = work
            .vercel_metadata
            .get("teamId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        return Ok(VercelClient::new(
            SecretString::from(token.to_owned()),
            team_id,
        )?);
    }
    let team_id = work
        .vercel_metadata
        .get("teamId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| std::env::var("VERCEL_TEAM_ID").ok());
    Ok(VercelClient::new(secret_env("VERCEL_TOKEN")?, team_id)?)
}

fn vercel_project_name(work: &nopager_db::IncidentWork) -> anyhow::Result<&str> {
    work.vercel_metadata
        .get("projectName")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Vercel integration metadata projectName is required"))
}

fn metadata_u64(metadata: &Value, key: &str) -> anyhow::Result<u64> {
    metadata
        .get(key)
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
        .ok_or_else(|| anyhow::anyhow!("integration metadata {key} is required"))
}

fn https_url(value: &str) -> String {
    if value.starts_with("https://") {
        value.into()
    } else {
        format!("https://{value}")
    }
}

fn preview_health_url(preview_url: &str, production_health_url: &str) -> anyhow::Result<String> {
    let mut preview = url::Url::parse(preview_url)?;
    let production = url::Url::parse(production_health_url)?;
    preview.set_path(production.path());
    preview.set_query(production.query());
    Ok(preview.to_string())
}

async fn require_healthy(value: &str) -> anyhow::Result<()> {
    let url = url::Url::parse(value)?;
    let observation = check_http(&url, 200, Duration::from_secs(15)).await?;
    if !observation.success {
        anyhow::bail!(
            "health verification failed with status {:?}",
            observation.status_code
        );
    }
    Ok(())
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

fn work_root() -> anyhow::Result<PathBuf> {
    let root = std::env::var_os("NOPAGER_WORK_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("nopager-workspaces"));
    let root = if root.is_absolute() {
        root
    } else {
        std::env::current_dir()?.join(root)
    };
    std::fs::create_dir_all(&root)?;
    Ok(root.canonicalize()?)
}

async fn github_client(
    database: &Database,
    work: &nopager_db::IncidentWork,
) -> anyhow::Result<nopager_connectors::github::GitHubClient> {
    if let Ok(credentials) = integration_credentials(database, work.project_id, "github").await {
        let app_id = required_u64(&credentials, "appId")?;
        let installation_id = required_u64(&credentials, "installationId")?;
        let private_key = required_string(&credentials, "privateKey")?.replace("\\n", "\n");
        return Ok(
            GitHubAppAuth::new(app_id, installation_id, SecretString::from(private_key))?
                .installation_client(&work.repo_name)
                .await?,
        );
    }
    let app_id = std::env::var("GITHUB_APP_ID")?.parse()?;
    let installation_id = env_or_metadata_u64(
        "GITHUB_INSTALLATION_ID",
        &work.github_metadata,
        "installationId",
    )?;
    let private_key = std::env::var("GITHUB_APP_PRIVATE_KEY")?.replace("\\n", "\n");
    Ok(
        GitHubAppAuth::new(app_id, installation_id, SecretString::from(private_key))?
            .installation_client(&work.repo_name)
            .await?,
    )
}

fn env_or_metadata_u64(name: &str, metadata: &Value, key: &str) -> anyhow::Result<u64> {
    if let Ok(value) = std::env::var(name) {
        return Ok(value.parse()?);
    }
    metadata
        .get(key)
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
        .ok_or_else(|| anyhow::anyhow!("{name} or integration metadata {key} is required"))
}

fn prepare_workspace(
    incident_id: uuid::Uuid,
    attempt_id: uuid::Uuid,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    let root = std::env::var_os("NOPAGER_WORK_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("nopager-workspaces"));
    let root = if root.is_absolute() {
        root
    } else {
        std::env::current_dir()?.join(root)
    };
    std::fs::create_dir_all(&root)?;
    let canonical_root = root.canonicalize()?;
    let relative = PathBuf::from(incident_id.to_string()).join(attempt_id.to_string());
    let workspace = canonical_root.join(&relative);
    if workspace.exists() {
        let canonical = workspace.canonicalize()?;
        if !canonical.starts_with(&canonical_root) {
            anyhow::bail!("workspace path escaped its root");
        }
        std::fs::remove_dir_all(&canonical)?;
    }
    std::fs::create_dir_all(&workspace)?;
    Ok((workspace, relative))
}

fn sandbox_for(
    workspace: PathBuf,
    relative_workspace: PathBuf,
    image: &str,
    network: bool,
) -> anyhow::Result<DockerSandbox> {
    let timeout = Duration::from_secs(env_u64("SANDBOX_TIMEOUT_SECONDS", 900)?);
    let memory = u32::try_from(env_u64("SANDBOX_MAX_MEMORY_MB", 4096)?)?;
    let cpus = std::env::var("SANDBOX_MAX_CPU")
        .unwrap_or_else(|_| "2".into())
        .parse()?;
    let mut sandbox = DockerSandbox::new(
        PathBuf::from(std::env::var("DOCKER_BIN").unwrap_or_else(|_| "docker".into())),
        workspace,
        image.into(),
        timeout,
        memory,
        cpus,
        256,
        network,
    )?
    .with_container_user(
        std::env::var("SANDBOX_UID_GID").unwrap_or_else(|_| "65532:65532".into()),
    )?;
    if let Ok(volume) = std::env::var("NOPAGER_SANDBOX_VOLUME") {
        sandbox = sandbox.with_volume_mount(volume, relative_workspace)?;
    }
    Ok(sandbox)
}

fn env_u64(name: &str, fallback: u64) -> anyhow::Result<u64> {
    Ok(std::env::var(name).map_or(Ok(fallback), |value| value.parse())?)
}

fn detected_validation_commands(workspace: &Path) -> anyhow::Result<Vec<ControlledCommand>> {
    let mut commands = Vec::new();
    let cwd = PathBuf::new();
    if workspace.join("pnpm-lock.yaml").exists() {
        commands.push(command(
            "corepack",
            &["pnpm", "install", "--frozen-lockfile", "--ignore-scripts"],
            &cwd,
        ));
        add_package_scripts(workspace, "corepack", "pnpm", &cwd, &mut commands)?;
    } else if workspace.join("yarn.lock").exists() {
        commands.push(command(
            "corepack",
            &["yarn", "install", "--immutable", "--mode=skip-builds"],
            &cwd,
        ));
        add_package_scripts(workspace, "corepack", "yarn", &cwd, &mut commands)?;
    } else if workspace.join("package-lock.json").exists()
        || workspace.join("package.json").exists()
    {
        commands.push(command(
            "npm",
            &[
                if workspace.join("package-lock.json").exists() {
                    "ci"
                } else {
                    "install"
                },
                "--ignore-scripts",
            ],
            &cwd,
        ));
        add_package_scripts(workspace, "npm", "", &cwd, &mut commands)?;
    }
    if workspace.join("Cargo.toml").exists() {
        let locked = workspace.join("Cargo.lock").exists();
        for action in ["fetch", "build", "test"] {
            let arguments = if locked {
                vec![action, "--locked"]
            } else {
                vec![action]
            };
            commands.push(command("cargo", &arguments, &cwd));
        }
    }
    if commands.is_empty() {
        anyhow::bail!("repository has no supported Node or Rust project manifest");
    }
    Ok(commands)
}

fn add_package_scripts(
    workspace: &Path,
    program: &str,
    manager: &str,
    cwd: &Path,
    commands: &mut Vec<ControlledCommand>,
) -> anyhow::Result<()> {
    let package: Value = serde_json::from_slice(&std::fs::read(workspace.join("package.json"))?)?;
    for script in ["build", "test"] {
        if package
            .pointer(&format!("/scripts/{script}"))
            .and_then(Value::as_str)
            .is_some()
        {
            let args = if manager.is_empty() {
                vec!["run", script]
            } else {
                vec![manager, script]
            };
            commands.push(command(program, &args, cwd));
        }
    }
    Ok(())
}

fn command(program: &str, arguments: &[&str], working_directory: &Path) -> ControlledCommand {
    ControlledCommand {
        program: program.into(),
        arguments: arguments.iter().map(ToString::to_string).collect(),
        working_directory: working_directory.into(),
    }
}

fn is_install_command(command: &ControlledCommand) -> bool {
    matches!(command.program.as_str(), "npm" | "corepack")
        && command
            .arguments
            .iter()
            .any(|argument| matches!(argument.as_str(), "ci" | "install"))
        || command.program == "cargo"
            && command
                .arguments
                .first()
                .is_some_and(|argument| argument == "fetch")
}

fn is_dependency_manifest(path: &str) -> bool {
    matches!(
        Path::new(path).file_name().and_then(|name| name.to_str()),
        Some(
            "Cargo.toml"
                | "Cargo.lock"
                | "package.json"
                | "package-lock.json"
                | "pnpm-lock.yaml"
                | "yarn.lock"
        )
    )
}

fn validation_summary(validation: &[Value]) -> String {
    validation
        .iter()
        .filter_map(|item| item.pointer("/command/program").and_then(Value::as_str))
        .map(|program| format!("- `{program}` completed successfully"))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn provider_for(
    database: &Database,
    project_id: uuid::Uuid,
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

async fn integration_credentials(
    database: &Database,
    project_id: uuid::Uuid,
    kind: &str,
) -> anyhow::Result<Value> {
    let integration = database.integration_secret(project_id, kind).await?;
    decrypt_credentials(&integration.encrypted_credentials)
}

fn decrypt_credentials(encrypted: &str) -> anyhow::Result<Value> {
    let key = SecretString::from(std::env::var("NOPAGER_MASTER_KEY")?);
    let plaintext = SecretCipher::from_base64_key(&key)?.decrypt(encrypted)?;
    Ok(serde_json::from_str(plaintext.expose_secret())?)
}

fn required_u64(value: &Value, key: &str) -> anyhow::Result<u64> {
    value
        .get(key)
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
        .ok_or_else(|| anyhow::anyhow!("credential {key} is required"))
}

fn secret_env(name: &str) -> anyhow::Result<SecretString> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .map(SecretString::from)
        .ok_or_else(|| anyhow::anyhow!("{name} is required for the selected provider"))
}

fn incident_id(payload: &Value) -> anyhow::Result<uuid::Uuid> {
    required_string(payload, "incidentId")?
        .parse()
        .map_err(Into::into)
}

fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key).and_then(Value::as_str) {
                    return Some(value.to_owned());
                }
            }
            map.values().find_map(|value| find_string(value, keys))
        }
        Value::Array(values) => values.iter().find_map(|value| find_string(value, keys)),
        _ => None,
    }
}

async fn process_health_check(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let id = payload
        .get("healthCheckId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("healthCheckId is missing"))?
        .parse()?;
    let check = database.health_check(id).await?;
    let url = url::Url::parse(&check.url)?;
    let observation = check_http(
        &url,
        u16::try_from(check.expected_status)?,
        Duration::from_millis(u64::try_from(check.timeout_ms)?),
    )
    .await?;
    let metadata = json!({
        "url": check.url,
        "statusCode": observation.status_code,
        "latencyMs": observation.latency_ms,
        "errorClass": observation.error_class,
    });
    database
        .record_health_observation(&check, observation.success, &metadata)
        .await?;
    Ok(())
}

async fn process_vercel_poll(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let project_id: uuid::Uuid = required_string(payload, "projectId")?.parse()?;
    let vercel_project_id = required_string(payload, "vercelProjectId")?;
    let since_ms = payload
        .get("sinceMs")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow::anyhow!("sinceMs is missing"))?;
    let integration = database.integration_secret(project_id, "vercel").await?;
    let credentials = decrypt_credentials(&integration.encrypted_credentials)?;
    let token = required_string(&credentials, "token")?;
    let team_id = integration
        .metadata
        .get("teamId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let client = VercelClient::new(SecretString::from(token.to_owned()), team_id)?;
    let deployments = client
        .list_deployments_since(vercel_project_id, 20, Some(since_ms))
        .await?;

    for deployment in deployments {
        if deployment.target.as_deref() != Some("production") {
            continue;
        }
        let state = deployment
            .ready_state
            .as_deref()
            .or(deployment.state.as_deref())
            .unwrap_or("UNKNOWN")
            .to_owned();
        let url = https_url(&deployment.url);
        let sha = deployment
            .meta
            .get("githubCommitSha")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        database
            .save_deployment(
                project_id,
                &deployment.id,
                "production",
                &sha,
                &url,
                &state,
                false,
            )
            .await?;

        if matches!(state.as_str(), "ERROR" | "CANCELED" | "CANCELLED") {
            database
                .open_incident(&IncidentTrigger {
                    project_id,
                    deduplication_key: format!("vercel-deployment:{}", deployment.id),
                    trigger_type: "VERCEL_DEPLOYMENT_FAILED".into(),
                    severity: "high".into(),
                    title: "Vercel production deployment failed".into(),
                    metadata: json!({
                        "source": "vercel-poll",
                        "deployment": {
                            "id": deployment.id,
                            "url": url,
                            "target": deployment.target,
                            "state": state,
                            "created": deployment.created,
                            "meta": deployment.meta
                        }
                    }),
                })
                .await?;
        }
    }
    Ok(())
}

async fn process_webhook(database: &Database, payload: &Value) -> anyhow::Result<()> {
    let provider = required_string(payload, "provider")?;
    let delivery_id = required_string(payload, "deliveryId")?;
    let event_type = required_string(payload, "eventType")?;
    let metadata = database.webhook_metadata(provider, delivery_id).await?;
    let external = metadata
        .get("payload")
        .ok_or_else(|| anyhow::anyhow!("webhook payload is missing"))?;

    if provider == "vercel" && matches!(event_type, "deployment.succeeded" | "deployment.ready") {
        let external_project_id = pointer_string(external, "/payload/project/id")
            .or_else(|| pointer_string(external, "/project/id"))
            .ok_or_else(|| anyhow::anyhow!("Vercel project id is missing"))?;
        let Some(project_id) = database
            .project_for_integration("vercel", external_project_id)
            .await?
        else {
            return Ok(());
        };
        let deployment_id = pointer_string(external, "/payload/deployment/id")
            .or_else(|| pointer_string(external, "/deployment/id"))
            .ok_or_else(|| anyhow::anyhow!("Vercel deployment id is missing"))?;
        let url = pointer_string(external, "/payload/deployment/url")
            .or_else(|| pointer_string(external, "/deployment/url"))
            .map(https_url)
            .ok_or_else(|| anyhow::anyhow!("Vercel deployment URL is missing"))?;
        let target = pointer_string(external, "/payload/target")
            .or_else(|| pointer_string(external, "/target"))
            .unwrap_or("preview");
        let sha = pointer_string(external, "/payload/deployment/meta/githubCommitSha")
            .or_else(|| pointer_string(external, "/deployment/meta/githubCommitSha"))
            .unwrap_or("unknown");
        database
            .save_deployment(project_id, deployment_id, target, sha, &url, "READY", false)
            .await?;
        return Ok(());
    }

    let trigger = match provider {
        "vercel" if event_type == "deployment.error" => {
            let external_project_id = pointer_string(external, "/payload/project/id")
                .or_else(|| pointer_string(external, "/project/id"))
                .ok_or_else(|| anyhow::anyhow!("Vercel project id is missing"))?;
            let Some(project_id) = database
                .project_for_integration("vercel", external_project_id)
                .await?
            else {
                info!(%external_project_id, "ignoring webhook for an unprotected Vercel project");
                return Ok(());
            };
            let deployment_id = pointer_string(external, "/payload/deployment/id")
                .or_else(|| pointer_string(external, "/deployment/id"))
                .unwrap_or(delivery_id);
            Some(IncidentTrigger {
                project_id,
                deduplication_key: format!("vercel-deployment:{deployment_id}"),
                trigger_type: "VERCEL_DEPLOYMENT_FAILED".into(),
                severity: "high".into(),
                title: "Vercel deployment failed".into(),
                metadata: external.clone(),
            })
        }
        "github"
            if event_type == "workflow_run"
                && pointer_string(external, "/workflow_run/conclusion") == Some("failure") =>
        {
            let owner = pointer_string(external, "/repository/owner/login")
                .ok_or_else(|| anyhow::anyhow!("GitHub owner is missing"))?;
            let repository = pointer_string(external, "/repository/name")
                .ok_or_else(|| anyhow::anyhow!("GitHub repository is missing"))?;
            let Some(project_id) = database.project_for_repository(owner, repository).await? else {
                info!(%owner, %repository, "ignoring webhook for an unprotected GitHub repository");
                return Ok(());
            };
            let run_id = external
                .pointer("/workflow_run/id")
                .and_then(Value::as_u64)
                .map(|id| id.to_string())
                .unwrap_or_else(|| delivery_id.into());
            Some(IncidentTrigger {
                project_id,
                deduplication_key: format!("github-workflow:{run_id}"),
                trigger_type: "GITHUB_WORKFLOW_FAILED".into(),
                severity: "medium".into(),
                title: "GitHub workflow failed".into(),
                metadata: external.clone(),
            })
        }
        _ => None,
    };
    if let Some(trigger) = trigger {
        database.open_incident(&trigger).await?;
    }
    Ok(())
}

fn required_string<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{key} is missing"))
}

fn pointer_string<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_manifests_never_enter_automatic_repair() {
        assert!(is_dependency_manifest("package.json"));
        assert!(is_dependency_manifest("crates/api/Cargo.lock"));
        assert!(!is_dependency_manifest("src/main.rs"));
    }

    #[test]
    fn install_commands_are_the_only_commands_with_network() {
        let cwd = PathBuf::new();
        assert!(is_install_command(&command(
            "corepack",
            &["pnpm", "install", "--ignore-scripts"],
            &cwd
        )));
        assert!(is_install_command(&command("cargo", &["fetch"], &cwd)));
        assert!(!is_install_command(&command("cargo", &["test"], &cwd)));
        assert!(!is_install_command(&command(
            "corepack",
            &["pnpm", "build"],
            &cwd
        )));
    }

    #[test]
    fn detects_node_build_and_test_without_running_repo_code() {
        let root =
            std::env::temp_dir().join(format!("nopager-command-detection-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"scripts":{"build":"next build","test":"vitest run"}}"#,
        )
        .unwrap();
        let commands = detected_validation_commands(&root).unwrap();
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].arguments[1], "install");
        assert_eq!(commands[1].arguments[1], "build");
        assert_eq!(commands[2].arguments[1], "test");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn safety_modes_are_explicit() {
        assert_eq!(parse_safety_mode("safe").unwrap(), SafetyMode::Safe);
        assert_eq!(
            parse_safety_mode("autopilot_experimental").unwrap(),
            SafetyMode::AutopilotExperimental
        );
        assert!(parse_safety_mode("automatic").is_err());
    }

    #[test]
    fn preview_verification_preserves_the_configured_health_path() {
        assert_eq!(
            preview_health_url(
                "https://preview.example.vercel.app",
                "https://production.example.com/api/health?deep=true"
            )
            .unwrap(),
            "https://preview.example.vercel.app/api/health?deep=true"
        );
    }
}
