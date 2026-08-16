import Link from "next/link";
import { ApproveButton, RejectButton } from "@/components/approve-button";
import { Card, PageHeader, SectionTitle, StatusBadge } from "@/components/ui";
import { api, type IncidentDetail } from "@/lib/api";
import { projectIncidentState } from "@/lib/model";

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
  return (
    <div className="page">
      <Link href="/incidents" className="back-link">
        ← Back to incidents
      </Link>
      <PageHeader
        eyebrow={incident.id}
        title={incident.title}
        description={
          incident.rootCauseSummary ??
          "NoPager is working through the incident lifecycle."
        }
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
