import Link from "next/link";
import { Card, IncidentLink, PageHeader, StatusBadge } from "@/components/ui";
import { api, type IncidentSummary, type Overview } from "@/lib/api";
import { projectIncidentState } from "@/lib/model";
import { apiDate, humanize } from "@/lib/presentation";

export default async function OverviewPage() {
  const [overview, incidentResult] = await Promise.all([
    api<Overview>("overview"),
    api<{ incidents: IncidentSummary[] }>("incidents"),
  ]);
  if (!overview?.configured) {
    return (
      <div className="page overview-page">
        <PageHeader
          eyebrow="Overview"
          title="Protect your first app"
          description="Complete setup to begin bounded production monitoring in Safe Mode."
          action={
            <Link className="primary-button link-button" href="/setup">
              Open setup
            </Link>
          }
        />
        <Card className="empty-brief">
          <span className="empty-brief-mark">N</span>
          <div>
            <h2>No protected service yet</h2>
            <p>The NoPager API is waiting for one protected application.</p>
          </div>
        </Card>
      </div>
    );
  }

  const incidents = incidentResult?.incidents.slice(0, 4) ?? [];
  const unresolvedAttention = incidents.some(
    (incident) =>
      incident.actionRequired ||
      incident.status === "WAITING_APPROVAL" ||
      incident.status === "ESCALATED" ||
      incident.status === "FAILED",
  );
  const healthy =
    overview.systemStatus === "HEALTHY" &&
    !overview.actionRequired &&
    !unresolvedAttention;

  return (
    <div className="page overview-page">
      <PageHeader
        eyebrow="Overview"
        title="Production protection"
        description="A concise operating brief. NoPager stays quiet until a decision or recovery needs your attention."
        action={
          <span className={healthy ? "health-state" : "health-state attention"}>
            <span />
            {healthy ? "Protected" : "Attention required"}
          </span>
        }
      />

      <section className={`executive-brief${healthy ? " healthy" : ""}`}>
        <div className="brief-mark" aria-hidden="true">
          {healthy ? "✓" : "!"}
        </div>
        <div className="brief-copy">
          <p className="eyebrow">Current posture</p>
          <h2>
            {healthy
              ? "All systems protected. No action required."
              : overview.protectionPaused
                ? "Protection actions are paused."
                : "A production incident needs attention."}
          </h2>
          <p>
            {healthy
              ? "NoPager is on watch."
              : "Review the latest incident before production changes."}
          </p>
        </div>
        <div className="brief-check">
          <span>Last checked</span>
          <strong>{formatChecked(overview.lastCheckedAt)}</strong>
        </div>
      </section>

      <div className="overview-columns">
        <section className="overview-section">
          <div className="overview-section-head">
            <div>
              <p className="eyebrow">Protected services</p>
              <h2>Coverage</h2>
            </div>
            <span>1 service</span>
          </div>
          <div className="service-row">
            <span className={`service-status${healthy ? " healthy" : ""}`} />
            <div className="service-name">
              <strong>
                {overview.project?.name ?? "Protected application"}
              </strong>
              <span>Production</span>
            </div>
            <dl className="service-facts">
              <div>
                <dt>Protection</dt>
                <dd>{overview.protectionPaused ? "Paused" : "Active"}</dd>
              </div>
              <div>
                <dt>Mode</dt>
                <dd>{humanize(overview.protectionMode ?? "safe")}</dd>
              </div>
              <div>
                <dt>Signals</dt>
                <dd>{overview.healthCheckCount ?? 0} health checks</dd>
              </div>
            </dl>
          </div>
        </section>

        <section className="overview-section recent-section">
          <div className="overview-section-head">
            <div>
              <p className="eyebrow">Recent incidents</p>
              <h2>Decision history</h2>
            </div>
            <Link className="text-link" href="/incidents">
              View all →
            </Link>
          </div>
          {incidents.length ? (
            <div className="recent-list">
              {incidents.map((incident) => (
                <article className="recent-row" key={incident.id}>
                  <div>
                    <strong>{incident.title}</strong>
                    <span>
                      {humanize(incident.severity)} ·{" "}
                      {formatDate(incident.openedAt)}
                    </span>
                  </div>
                  <StatusBadge state={projectIncidentState(incident.status)} />
                  <IncidentLink id={incident.id}>Review</IncidentLink>
                </article>
              ))}
            </div>
          ) : (
            <div className="quiet-history">
              <span aria-hidden="true">✓</span>
              <p>No incidents recorded. Quiet is the expected state.</p>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function formatChecked(value?: unknown) {
  if (!value) return "First check pending";
  const date = apiDate(value);
  if (!date) return "Check time unavailable";
  return new Intl.DateTimeFormat("en", {
    hour: "numeric",
    minute: "2-digit",
    month: "short",
    day: "numeric",
  }).format(date);
}

function formatDate(value: unknown) {
  const date = apiDate(value);
  if (!date) return "Time unavailable";
  return new Intl.DateTimeFormat("en", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}
