import Link from "next/link";
import {
  Card,
  IncidentLink,
  PageHeader,
  SectionTitle,
  StatusBadge,
} from "@/components/ui";
import { api, type Overview } from "@/lib/api";
import { projectIncidentState } from "@/lib/model";

export default async function OverviewPage() {
  const overview = await api<Overview>("overview");
  if (!overview?.configured)
    return (
      <div className="page">
        <PageHeader
          eyebrow="Production overview"
          title="Protect your first app"
          description="Complete setup to start 24/7 production protection."
          action={
            <Link className="primary-button link-button" href="/setup">
              Open setup
            </Link>
          }
        />
        <Card>
          <p>The NoPager API is waiting for a protected app.</p>
        </Card>
      </div>
    );
  const healthy =
    overview.systemStatus === "HEALTHY" && !overview.actionRequired;
  return (
    <div className="page">
      <PageHeader
        eyebrow="Production overview"
        title={`System Status: ${display(overview.systemStatus)}`}
        description={
          healthy
            ? `No action needed. ${checked(overview.lastCheckedAt)}`
            : "Review the current production state."
        }
        action={
          <span className={healthy ? "healthy-pill" : "status-badge waiting"}>
            <span />
            {healthy ? "All systems operational" : "Attention needed"}
          </span>
        }
      />
      <Card className="hero-status">
        <div className="status-orb">{healthy ? "✓" : "!"}</div>
        <div>
          <p className="eyebrow">{overview.project?.name} PRODUCTION</p>
          <h2>
            {overview.protectionPaused
              ? "Protection is paused."
              : "Your app is protected."}
          </h2>
          <p>
            NoPager monitors production, deployments, and health checks around
            the clock.
          </p>
        </div>
        <div className="status-metric">
          <strong>{display(overview.protectionMode ?? "safe")}</strong>
          <span>Protection mode</span>
        </div>
      </Card>
      <div className="metric-grid">
        <Card>
          <p className="metric-label">Protection</p>
          <strong className="metric-value green">
            {overview.protectionPaused ? "Paused" : "Active"}
          </strong>
          <p>
            {display(overview.protectionMode ?? "safe")} ·{" "}
            {overview.healthCheckCount ?? 0} health checks
          </p>
        </Card>
        <Card>
          <p className="metric-label">Last deployment</p>
          <strong className="metric-value">
            {overview.latestDeployment
              ? display(overview.latestDeployment.status)
              : "None"}
          </strong>
          <p>
            {overview.latestDeployment
              ? new Date(overview.latestDeployment.createdAt).toLocaleString()
              : "No deployment recorded"}
          </p>
        </Card>
        <Card>
          <p className="metric-label">Incidents this month</p>
          <strong className="metric-value">
            {overview.incidentsThisMonth ?? 0}
          </strong>
          <p>{overview.autonomousThisMonth ?? 0} resolved autonomously</p>
        </Card>
      </div>
      <Card>
        <SectionTitle
          title="Latest incident"
          detail={
            overview.latestIncident
              ? "Most recent production event"
              : "Quiet is good."
          }
        />
        {overview.latestIncident ? (
          <div className="activity-row">
            <span className="timeline-dot blue-dot" />
            <div>
              <strong>{overview.latestIncident.title}</strong>
              <p>{overview.latestIncident.severity} severity</p>
              <IncidentLink id={overview.latestIncident.id}>
                Review incident
              </IncidentLink>
            </div>
            <StatusBadge
              state={projectIncidentState(overview.latestIncident.status)}
            />
          </div>
        ) : (
          <p className="muted">No incidents recorded.</p>
        )}
      </Card>
    </div>
  );
}

function display(value: string) {
  return value
    .toLowerCase()
    .replaceAll("_", " ")
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function checked(value?: string | null) {
  return value
    ? `Last checked ${new Date(value).toLocaleString()}.`
    : "First health check pending.";
}
