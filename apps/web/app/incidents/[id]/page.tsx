import Link from "next/link";
import { ApprovalActions } from "@/components/approve-button";
import { AuthorityCard } from "@/components/authority-card";
import { Card, PageHeader, SectionTitle, StatusBadge } from "@/components/ui";
import { api, type IncidentDetail } from "@/lib/api";
import { projectIncidentState, sourceRecoveryAction } from "@/lib/model";
import {
  apiDate,
  humanize,
  planEvidence,
  planNumber,
  planString,
  safeTechnicalEvidence,
} from "@/lib/presentation";

export default async function IncidentDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  const incident = await api<IncidentDetail>(
    `incidents/${encodeURIComponent(id)}`,
  );
  if (!incident) {
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
  }

  const operation = incident.currentOperation;
  const attempt = incident.currentAttempt;
  const diagnosis = attempt?.diagnosis;
  const plan = operation?.plan ?? diagnosis ?? attempt?.proposal;
  const assessment =
    planString(plan, "summary", "rootCauseSummary", "root_cause_summary") ??
    incident.rootCauseSummary ??
    "No AI assessment is available yet.";
  const confidence = planNumber(plan, "confidence");
  const evidence = planEvidence(plan);
  const outcome = incidentOutcome(incident);
  const sourceRecovery =
    incident.status === "ESCALATED" && !operation
      ? sourceRecoveryAction(incident.events)
      : null;
  const awaitingApproval = incident.status === "WAITING_APPROVAL";

  return (
    <div className="page incident-page">
      <div className="incident-topline">
        <Link href="/incidents" className="back-link">
          ← Incidents
        </Link>
        {operation ? (
          <Link
            className="receipt-link-inline"
            href={`/incidents/${id}/receipt`}
          >
            Authority receipt →
          </Link>
        ) : null}
      </div>
      <PageHeader
        eyebrow={`${humanize(incident.severity)} incident · ${shortId(incident.id)}`}
        title={incident.title}
        description={outcome.headline}
        action={<StatusBadge state={projectIncidentState(incident.status)} />}
      />

      {sourceRecovery ? (
        <div className="notice amber">
          <span>!</span>
          <div>
            <strong>Source recovery needs manual review</strong>
            <p>
              Production may be recovered, but NoPager has not proven that the
              protected source branch is safe.
            </p>
          </div>
          {sourceRecovery.pullRequestUrl ? (
            <a
              className="text-link"
              href={sourceRecovery.pullRequestUrl}
              target="_blank"
              rel="noreferrer"
            >
              Open candidate PR →
            </a>
          ) : null}
        </div>
      ) : null}

      <div
        className={`detail-grid executive-detail${awaitingApproval ? " approval-layout" : ""}`}
      >
        <div className="detail-main">
          <Card className="narrative-card">
            <SectionTitle title="What happened" detail={outcome.label} />
            <p className="incident-lede">{outcome.message}</p>
            <dl className="incident-facts">
              <div>
                <dt>Detected</dt>
                <dd>{formatTimestamp(incident.openedAt)}</dd>
              </div>
              <div>
                <dt>Signal</dt>
                <dd>{humanize(incident.triggerType)}</dd>
              </div>
              <div>
                <dt>Protection mode</dt>
                <dd>{humanize(incident.safetyMode)}</dd>
              </div>
            </dl>
          </Card>

          <Card>
            <SectionTitle
              title="Evidence"
              detail={
                evidence.length
                  ? `${evidence.length} bounded findings`
                  : "Collecting"
              }
            />
            {evidence.length ? (
              <ol className="evidence-list">
                {evidence.map((item, index) => (
                  <li key={`${item.source}-${index}`}>
                    <span>{index + 1}</span>
                    <div>
                      <code>{humanize(item.source)}</code>
                      <p>{item.finding}</p>
                    </div>
                  </li>
                ))}
              </ol>
            ) : (
              <p className="muted">
                No bounded evidence findings have been recorded yet.
              </p>
            )}
          </Card>

          <Card>
            <SectionTitle
              title="AI assessment"
              detail={
                confidence !== null
                  ? `${Math.round(confidence * 100)}% model confidence`
                  : "No confidence recorded"
              }
            />
            <p className="incident-lede">{assessment}</p>
            <p className="assessment-boundary">
              This is a bounded conclusion, not hidden reasoning. Deterministic
              policy decides whether the proposed action is allowed.
            </p>
          </Card>

          <Card className="timeline-card">
            <SectionTitle
              title="Incident record"
              detail={`${incident.events.length} events`}
            />
            <div className="timeline">
              {incident.events.map((event) => (
                <div className="timeline-item" key={event.id}>
                  <span className="timeline-dot" />
                  <time>{formatTime(event.createdAt)}</time>
                  <div>
                    <strong>{event.message}</strong>
                    <p>
                      {humanize(event.actor)} · {humanize(event.type)}
                    </p>
                  </div>
                </div>
              ))}
            </div>
          </Card>

          <Card className="raw-evidence-card" id="technical-evidence">
            <details>
              <summary>
                <span>Raw technical evidence</span>
                <small>Collapsed by default · display-redacted</small>
              </summary>
              <pre>
                {JSON.stringify(
                  safeTechnicalEvidence({
                    events: incident.events,
                    operation,
                    attempt,
                  }),
                  null,
                  2,
                )}
              </pre>
            </details>
          </Card>
        </div>

        <aside className="detail-side">
          {operation ? (
            <AuthorityCard incident={incident} />
          ) : (
            <Card className="production-gate-card">
              <p className="eyebrow">Production gate</p>
              <h2>
                {awaitingApproval
                  ? "Verified repair awaiting approval"
                  : humanize(incident.status)}
              </h2>
              <dl className="authority-fields">
                <div>
                  <dt>Mode</dt>
                  <dd>{humanize(incident.safetyMode)}</dd>
                </div>
                <div>
                  <dt>Risk</dt>
                  <dd>{attempt?.riskLevel ?? "Not recorded"}</dd>
                </div>
                <div>
                  <dt>Preview</dt>
                  <dd>
                    {attempt?.previewUrl
                      ? "Verified URL recorded"
                      : "Not ready"}
                  </dd>
                </div>
              </dl>
              {awaitingApproval ? (
                <ApprovalActions
                  incidentId={incident.id}
                  actionKind="production_deploy"
                />
              ) : null}
            </Card>
          )}
        </aside>
      </div>
    </div>
  );
}

function incidentOutcome(incident: IncidentDetail) {
  const operation = incident.currentOperation;
  if (operation) {
    switch (incident.status) {
      case "RESOLVED":
        return {
          label: "Recovery verified",
          headline: "Production is healthy again.",
          message:
            incident.rootCauseSummary ??
            "NoPager executed the bounded operation and independently verified recovery.",
        };
      case "WAITING_APPROVAL":
        return {
          label: "Decision required",
          headline: "A bounded recovery action is ready for review.",
          message: `NoPager proposes ${humanize(operation.actionKind)} on the exact enrolled target. No mutation has executed.`,
        };
      case "FAILED":
      case "ESCALATED":
        return {
          label: "Human action required",
          headline: "NoPager stopped rather than exceed its authority.",
          message:
            incident.rootCauseSummary ??
            "The bounded recovery path could not be proven safe or successful.",
        };
      case "PAUSED":
        return {
          label: "Kill Switch active",
          headline: "Production mutations are paused.",
          message:
            "Read-only monitoring continues while every mutating action is blocked.",
        };
      default:
        return {
          label: "Recovery in progress",
          headline: "NoPager is investigating within its configured authority.",
          message:
            incident.rootCauseSummary ??
            "Evidence is being collected before any production decision is made.",
        };
    }
  }

  if (incident.status === "RESOLVED" || incident.status === "ROLLED_BACK") {
    return {
      label: incident.status === "ROLLED_BACK" ? "Rolled back" : "Resolved",
      headline: "Production is healthy again.",
      message:
        incident.rootCauseSummary ??
        "The repair passed production verification and the incident is closed.",
    };
  }
  if (incident.status === "WAITING_APPROVAL") {
    return {
      label: "Decision required",
      headline: "A verified production repair is waiting for approval.",
      message:
        "Safe Mode keeps production unchanged until an administrator reviews the repair and Preview evidence.",
    };
  }
  return {
    label: humanize(incident.status),
    headline: "NoPager is working through the incident safely.",
    message:
      incident.rootCauseSummary ??
      "NoPager is collecting bounded evidence and preparing the smallest reversible response.",
  };
}

function shortId(value: string) {
  return value.length > 12 ? `${value.slice(0, 8)}…` : value;
}

function formatTimestamp(value: unknown) {
  const date = apiDate(value);
  if (!date) return "Time unavailable";
  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function formatTime(value: unknown) {
  const date = apiDate(value);
  if (!date) return "Time unavailable";
  return new Intl.DateTimeFormat("en", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(date);
}
