import Link from "next/link";
import { Card, IncidentLink, PageHeader, StatusBadge } from "@/components/ui";
import { api, type IncidentSummary } from "@/lib/api";
import { projectIncidentState } from "@/lib/model";

export default async function IncidentsPage() {
  const result = await api<{ incidents: IncidentSummary[] }>("incidents");
  if (!result) {
    return (
      <div className="page">
        <PageHeader
          eyebrow="Incident history"
          title="Incident history unavailable"
          description="Sign in or complete setup before viewing production incident records."
          action={
            <Link className="primary-button link-button" href="/setup">
              Open setup
            </Link>
          }
        />
      </div>
    );
  }

  const incidents = result.incidents;
  return (
    <div className="page">
      <PageHeader
        eyebrow="Incident history"
        title="Incidents"
        description="Every production event, action, and verification in one auditable record."
      />
      <Card className="table-card">
        <div className="incident-table-head">
          <span>Incident</span>
          <span>Source / impact</span>
          <span>Started</span>
          <span>Status</span>
        </div>
        {incidents.length === 0 ? (
          <p className="empty-state">No incidents recorded.</p>
        ) : (
          incidents.map((incident) => (
            <div className="incident-table-row" key={incident.id}>
              <div>
                <strong>{incident.title}</strong>
                <p>{incident.rootCauseSummary ?? incident.id}</p>
                <IncidentLink id={incident.id}>View incident</IncidentLink>
              </div>
              <span>
                {incident.triggerType?.replaceAll("_", " ") ?? "Unknown"} ·{" "}
                {incident.severity}
              </span>
              <time>
                {new Date(incident.openedAt).toLocaleString()}
                <br />
                <small>TTR {duration(incident.timeToRecoverySeconds)}</small>
              </time>
              <div>
                <StatusBadge state={projectIncidentState(incident.status)} />
                <p>
                  {incident.actionRequired
                    ? "Human action required"
                    : incident.autonomousResolution
                      ? "Resolved autonomously"
                      : "No action needed"}
                </p>
              </div>
            </div>
          ))
        )}
      </Card>
    </div>
  );
}

function duration(seconds?: number) {
  if (seconds === undefined) return "—";
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}
