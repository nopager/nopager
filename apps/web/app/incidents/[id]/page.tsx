import Link from "next/link";
import { ApproveButton, RejectButton } from "@/components/approve-button";
import { Card, PageHeader, SectionTitle, StatusBadge } from "@/components/ui";
import { api, type IncidentDetail } from "@/lib/api";
import {
  projectIncidentState,
  sourceRecoveryAction,
  type SourceRecoveryAction,
} from "@/lib/model";

export default async function IncidentDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const incident = await api<IncidentDetail>(
    `incidents/${encodeURIComponent(id)}`,
  );
  if (!incident)
    return (
      <div className="page">
        <Link href="/incidents" className="back-link">
          ← Back to incidents
        </Link>
        <PageHeader
          eyebrow={id}
          title="Incident not found"
          description="The incident may have been removed or the API is unavailable."
        />
      </div>
    );
  const attempt = incident.currentAttempt;
  const diagnosis = attempt?.diagnosis;
  const diagnosisSummary =
    stringValue(
      diagnosis,
      "root_cause_summary",
      "rootCauseSummary",
      "suspectedRootCause",
    ) ??
    incident.rootCauseSummary ??
    "No root-cause summary is available yet.";
  const sourceRecovery =
    incident.status === "ESCALATED"
      ? sourceRecoveryAction(incident.events)
      : null;
  const sourceRecoveryCopy = sourceRecovery
    ? sourceRecoveryNotice(sourceRecovery)
    : null;
  const outcome = incidentOutcome(incident, sourceRecoveryCopy);

  return (
    <div className="page">
      <Link href="/incidents" className="back-link">
        ← Back to incidents
      </Link>
      <PageHeader
        eyebrow={incident.id}
        title={incident.title}
        description={outcome.headline}
        action={<StatusBadge state={projectIncidentState(incident.status)} />}
      />
      {incident.status === "WAITING_APPROVAL" && (
        <div className="notice amber">
          <span>!</span>
          <div>
            <strong>Your approval is required</strong>
            <p>
              The preview passed policy checks. Safe Mode prevents an unapproved
              production change.
            </p>
          </div>
          <ApproveButton incidentId={incident.id} />
        </div>
      )}
      {sourceRecovery && sourceRecoveryCopy && (
        <div className="notice amber">
          <span>!</span>
          <div>
            <strong>{sourceRecoveryCopy.title}</strong>
            <p>{sourceRecoveryCopy.message}</p>
            {sourceRecovery.pullRequestUrl && sourceRecoveryCopy.linkLabel && (
              <a
                className="text-link"
                href={sourceRecovery.pullRequestUrl}
                target="_blank"
                rel="noreferrer"
              >
                {sourceRecoveryCopy.linkLabel}
                {sourceRecovery.pullRequestNumber
                  ? ` #${sourceRecovery.pullRequestNumber}`
                  : ""}{" "}
                →
              </a>
            )}
          </div>
        </div>
      )}
      <Card>
        <SectionTitle title="Current outcome" detail={outcome.label} />
        <p className="result-copy">{outcome.message}</p>
        <p className="muted">{outcome.nextStep}</p>
      </Card>
      <div className="detail-grid">
        <div className="detail-main">
          <Card>
            <SectionTitle
              title="Timeline"
              detail={`${incident.events.length} events`}
            />
            <div className="timeline">
              {incident.events.map((event) => (
                <div className="timeline-item" key={event.id}>
                  <span className="timeline-dot green-dot" />
                  <time>{new Date(event.createdAt).toLocaleTimeString()}</time>
                  <div>
                    <strong>{event.message}</strong>
                    <p>
                      {event.actor} · {event.type}
                    </p>
                  </div>
                </div>
              ))}
            </div>
          </Card>
          <Card>
            <SectionTitle
              title="Evidence & root cause"
              detail={
                stringValue(diagnosis, "confidence")
                  ? `Confidence ${stringValue(diagnosis, "confidence")}`
                  : undefined
              }
            />
            <p className="result-copy">{diagnosisSummary}</p>
            {diagnosis && (
              <details>
                <summary>Technical evidence</summary>
                <pre>{JSON.stringify(diagnosis, null, 2)}</pre>
              </details>
            )}
          </Card>
          <Card>
            <SectionTitle
              title="Patch / repair"
              detail={
                attempt ? `Attempt ${attempt.attemptNumber}` : "Not started"
              }
            />
            {attempt?.patchDiff ? (
              <pre className="code-block">{attempt.patchDiff}</pre>
            ) : (
              <p className="muted">No patch has been generated.</p>
            )}
            {attempt?.pullRequestUrl && (
              <a
                className="text-link"
                href={attempt.pullRequestUrl}
                target="_blank"
                rel="noreferrer"
              >
                Open repair pull request →
              </a>
            )}
          </Card>
        </div>
        <aside className="detail-side">
          <Card>
            <SectionTitle title="Sandbox & preview" />
            <ul className="check-list">
              <li>Sandbox: {attempt?.sandboxStatus ?? "PENDING"}</li>
              <li>Tests: {attempt?.testStatus ?? "PENDING"}</li>
              <li>Validation checks: {attempt?.validation.length ?? 0}</li>
              <li>Preview: {attempt?.previewUrl ? "Ready" : "Pending"}</li>
            </ul>
            {attempt?.previewUrl && (
              <a
                className="text-link"
                href={attempt.previewUrl}
                target="_blank"
                rel="noreferrer"
              >
                Open preview →
              </a>
            )}
          </Card>
          <Card>
            <SectionTitle title="Production policy" />
            <p className="muted">
              {incident.safetyMode.replaceAll("_", " ")} ·{" "}
              {attempt?.riskLevel ?? "Risk pending"}
            </p>
            {incident.status === "WAITING_APPROVAL" && (
              <>
                <ApproveButton incidentId={incident.id} />
                <RejectButton incidentId={incident.id} />
              </>
            )}
          </Card>
        </aside>
      </div>
    </div>
  );
}

type SourceRecoveryCopy = {
  title: string;
  message: string;
  nextStep: string;
  linkLabel: string | null;
};

function sourceRecoveryNotice(action: SourceRecoveryAction): SourceRecoveryCopy {
  switch (action.kind) {
    case "review_source_revert":
      return {
        title: "Review the draft source-revert PR",
        message:
          "Production is back on the known-good deployment. NoPager created a draft PR to reverse the failed merged repair.",
        nextStep:
          "Review the draft and repository checks in GitHub. NoPager will not merge the source revert automatically.",
        linkLabel: "Open draft source-revert PR",
      };
    case "verify_existing_source_revert":
      return {
        title: "Verify the existing source-revert candidate",
        message:
          "Production is recovered. NoPager found a PR carrying its recovery marker, but the marker alone is not trusted as proof of origin.",
        nextStep:
          "Open the candidate and verify that it reverts the failed repair before merging it.",
        linkLabel: "Open source-revert candidate",
      };
    case "create_or_verify_source_revert":
      return {
        title: "Source recovery needs manual verification",
        message:
          "Production is recovered, but NoPager could not safely create or prove a source-revert PR.",
        nextStep:
          "Inspect the merged repair in GitHub and create or verify a revert before allowing the next production deployment.",
        linkLabel: null,
      };
    case "revert_merged_repair":
      return {
        title: "Revert the merged repair",
        message:
          "Production is recovered, but the failed repair is still present on the protected source branch.",
        nextStep:
          "Use the repair PR below to prepare a source revert, then review repository checks before merging it.",
        linkLabel: null,
      };
  }
}

function incidentOutcome(
  incident: IncidentDetail,
  sourceRecovery: SourceRecoveryCopy | null,
) {
  if (incident.status === "ESCALATED" && sourceRecovery) {
    return {
      label: "Source recovery needs review",
      headline: "Production recovered, but source still needs attention.",
      message: sourceRecovery.message,
      nextStep: sourceRecovery.nextStep,
    };
  }

  switch (incident.status) {
    case "RESOLVED":
      return {
        label: incident.autonomousResolution
          ? "Resolved autonomously"
          : "Resolved",
        headline: "Production is healthy again.",
        message:
          incident.rootCauseSummary ??
          "NoPager completed the repair and production verification.",
        nextStep: "No action needed. The incident remains available for audit.",
      };
    case "ROLLED_BACK":
      return {
        label: "Rolled back safely",
        headline: "The previous known-good deployment was restored.",
        message:
          incident.rootCauseSummary ??
          "NoPager restored the previous known-good production deployment.",
        nextStep: "No action needed. The incident remains available for audit.",
      };
    case "WAITING_APPROVAL":
      return {
        label: "Ready for production approval",
        headline: "The repair is verified and waiting for you.",
        message:
          "NoPager diagnosed the incident, prepared a repair, passed sandbox validation, and verified the Vercel Preview.",
        nextStep:
          "Review the root cause, patch, tests, and Preview below, then approve or reject the production promotion.",
      };
    case "VERIFYING_PREVIEW":
      return {
        label: "Preview verification",
        headline: "The repair is being verified before production can change.",
        message:
          "NoPager is checking the Vercel Preview and its health endpoint. Production remains untouched until this gate passes.",
        nextStep: "No action needed while Preview verification is running.",
      };
    case "PRODUCTION_DEPLOYING":
    case "VERIFYING_PRODUCTION":
      return {
        label: "Production verification",
        headline: "The verified repair is being checked in production.",
        message:
          "NoPager is watching the production health signal before declaring the incident resolved.",
        nextStep: "No action needed unless production verification fails.",
      };
    case "ROLLING_BACK":
      return {
        label: "Rollback in progress",
        headline: "Production verification failed. NoPager is rolling back.",
        message:
          "The repair did not satisfy the production safety gate, so the previous known-good deployment is being restored.",
        nextStep:
          "No new repair will be trusted until rollback verification completes.",
      };
    case "PAUSED":
      return {
        label: "Protection paused",
        headline: "Production mutations are paused.",
        message:
          "NoPager will keep read-only monitoring and evidence collection active while mutation actions remain blocked.",
        nextStep:
          "Resume protection only after the production risk is understood.",
      };
    case "FAILED":
    case "ESCALATED":
      return {
        label: "Human action required",
        headline: "NoPager stopped before making an unsafe change.",
        message:
          incident.rootCauseSummary ??
          "The automated repair path could not be verified safely.",
        nextStep:
          "Review the evidence and failed attempt below. Production mutations remain stopped until the incident is handled.",
      };
    case "CANCELLED":
      return {
        label: "Repair rejected",
        headline: "The proposed production change was not applied.",
        message:
          "The incident was closed without promoting the repair to production.",
        nextStep: "Production remains on the previously approved deployment.",
      };
    case "IGNORED":
      return {
        label: "No production action required",
        headline: "This incident was intentionally ignored.",
        message: "No production mutation is scheduled from this incident.",
        nextStep: "No action needed unless the incident should be reopened.",
      };
    case "DUPLICATE":
      return {
        label: "Duplicate incident",
        headline: "This signal belongs to an existing incident.",
        message:
          "NoPager linked the signal to the existing incident instead of starting duplicate remediation.",
        nextStep: "Review the active incident for the current repair status.",
      };
    default:
      return {
        label: "NoPager is working",
        headline: "NoPager is working through the incident lifecycle.",
        message:
          incident.rootCauseSummary ??
          "NoPager is collecting evidence, diagnosing the issue, and preparing the smallest reversible repair.",
        nextStep:
          "No action needed unless the incident is escalated or requests approval.",
      };
  }
}

function stringValue(
  value: Record<string, unknown> | null | undefined,
  ...keys: string[]
) {
  for (const key of keys) {
    const item = value?.[key];
    if (typeof item === "string" || typeof item === "number")
      return String(item);
  }
  return null;
}
