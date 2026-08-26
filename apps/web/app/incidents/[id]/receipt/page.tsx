import Link from "next/link";
import { PageHeader, StatusBadge } from "@/components/ui";
import { api, type IncidentDetail } from "@/lib/api";
import { projectIncidentState } from "@/lib/model";
import {
  apiDate,
  approvalState,
  humanize,
  planEvidence,
  planString,
  safeTechnicalEvidence,
  verificationSummary,
} from "@/lib/presentation";

export default async function AuthorityReceiptPage({
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
        <Link className="back-link" href={`/incidents/${id}`}>
          ← Back to incident
        </Link>
        <PageHeader
          eyebrow="Authority receipt"
          title="Receipt unavailable"
          description="The incident record could not be loaded."
        />
      </div>
    );
  }

  const operation = incident.currentOperation;
  if (!operation) {
    return (
      <div className="page receipt-page">
        <Link className="back-link" href={`/incidents/${id}`}>
          ← Back to incident
        </Link>
        <PageHeader
          eyebrow="Authority receipt"
          title="No bounded operation recorded"
          description="This incident does not contain a runtime-helper authority grant. Review the incident record for its deployment-recovery path."
          action={<StatusBadge state={projectIncidentState(incident.status)} />}
        />
      </div>
    );
  }

  const evidence = planEvidence(operation.plan);
  const conclusion =
    planString(operation.plan, "summary") ??
    incident.rootCauseSummary ??
    "No conclusion recorded";
  const rows = [
    {
      stage: "Trigger",
      value: humanize(incident.triggerType),
      detail: incident.title,
      state: "recorded",
    },
    {
      stage: "Evidence",
      value: evidence.length
        ? `${evidence.length} bounded findings`
        : "Not recorded",
      detail:
        evidence.map((item) => item.source).join(" · ") || "No evidence list",
      state: evidence.length ? "recorded" : "pending",
    },
    {
      stage: "AI conclusion",
      value: conclusion,
      detail: "Bounded conclusion only; no chain-of-thought is stored here",
      state: "recorded",
    },
    {
      stage: "Policy",
      value: humanize(operation.policyDecision),
      detail: `${humanize(incident.safetyMode)} mode`,
      state: operation.policyDecision === "block" ? "stopped" : "recorded",
    },
    {
      stage: "Allowed action",
      value: humanize(operation.actionKind),
      detail: "Typed capability only; no generic Docker or shell request",
      state: "recorded",
    },
    {
      stage: "Target",
      value: operation.targetId ?? "No target recorded",
      detail: "Exact enrolled target",
      state: operation.targetId ? "recorded" : "pending",
      machine: true,
    },
    {
      stage: "Attempt budget",
      value: "1 execution attempt",
      detail: "Ambiguous outcomes are not automatically replayed",
      state: "recorded",
    },
    {
      stage: "Approval",
      value: approvalState(operation),
      detail: `Operation status: ${humanize(operation.status)}`,
      state:
        operation.status === "APPROVAL_REQUIRED"
          ? "pending"
          : operation.status === "REJECTED"
            ? "stopped"
            : "recorded",
    },
    {
      stage: "Execution",
      value: operation.execution ? "Result recorded" : "Not executed",
      detail: operation.startedAt
        ? `Started ${formatTimestamp(operation.startedAt)}`
        : "No execution start time",
      state: operation.execution ? "recorded" : "pending",
    },
    {
      stage: "Verification",
      value: operation.verification ? "Result recorded" : "Not completed",
      detail: verificationSummary(operation.plan),
      state: operation.verification ? "recorded" : "pending",
    },
    {
      stage: "Outcome",
      value: humanize(incident.status),
      detail: incident.resolvedAt
        ? `Closed ${formatTimestamp(incident.resolvedAt)}`
        : "Incident remains open",
      state:
        incident.status === "RESOLVED"
          ? "resolved"
          : ["FAILED", "ESCALATED", "CANCELLED"].includes(incident.status)
            ? "stopped"
            : "pending",
    },
  ] as const;

  return (
    <div className="page receipt-page">
      <Link className="back-link" href={`/incidents/${id}`}>
        ← Back to incident
      </Link>
      <PageHeader
        eyebrow="Authority receipt"
        title="Production action record"
        description="A human-readable ledger of what was observed, what policy allowed, and what actually happened."
        action={<StatusBadge state={projectIncidentState(incident.status)} />}
      />

      <section className="receipt-shell">
        <header className="receipt-header">
          <div>
            <span>Incident</span>
            <strong>{incident.title}</strong>
          </div>
          <dl>
            <div>
              <dt>Incident ID</dt>
              <dd>
                <code>{incident.id}</code>
              </dd>
            </div>
            <div>
              <dt>Action ID</dt>
              <dd>
                <code>{operation.id}</code>
              </dd>
            </div>
            <div>
              <dt>Created</dt>
              <dd>{formatTimestamp(operation.createdAt)}</dd>
            </div>
          </dl>
        </header>

        <ol className="receipt-ledger">
          {rows.map((row, index) => (
            <li key={row.stage} className={`receipt-${row.state}`}>
              <span className="receipt-index">
                {String(index + 1).padStart(2, "0")}
              </span>
              <div className="receipt-stage">{row.stage}</div>
              <div className="receipt-value">
                {"machine" in row && row.machine ? (
                  <code>{row.value}</code>
                ) : (
                  <strong>{row.value}</strong>
                )}
                <span>{row.detail}</span>
              </div>
              <span className="receipt-state">{row.state}</span>
            </li>
          ))}
        </ol>

        <footer className="receipt-footer">
          <p>
            This is an auditable database-backed record. NoPager does not claim
            this view is cryptographically immutable.
          </p>
          <details>
            <summary>Display-redacted machine record</summary>
            <pre>
              {JSON.stringify(
                safeTechnicalEvidence({
                  operation,
                  events: incident.events,
                }),
                null,
                2,
              )}
            </pre>
          </details>
        </footer>
      </section>
    </div>
  );
}

function formatTimestamp(value: unknown) {
  const date = apiDate(value);
  if (!date) return "Time unavailable";
  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "medium",
  }).format(date);
}
