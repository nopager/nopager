import Link from "next/link";
import { Card, PageHeader, SectionTitle } from "@/components/ui";
import { api, type AppSettings } from "@/lib/api";

export default async function IntegrationsPage() {
  const settings = await api<AppSettings>("settings");
  if (!settings) {
    return (
      <div className="page">
        <PageHeader
          eyebrow="Connections"
          title="Integrations unavailable"
          description="Sign in or complete setup before managing production connections."
          action={
            <Link className="primary-button link-button" href="/setup">
              Open setup
            </Link>
          }
        />
      </div>
    );
  }

  const docker = settings.integrations.find(
    (item) => item.type === "docker_ops",
  );
  const github = settings.integrations.find((item) => item.type === "github");
  const vercel = settings.integrations.find((item) => item.type === "vercel");
  const dockerTarget = stringValue(docker?.metadata.targetId);

  return (
    <div className="page">
      <PageHeader
        eyebrow="Connections"
        title="Integrations"
        description="The production context NoPager can observe and act on safely."
      />
      <div className="integration-grid">
        {docker && (
          <Integration
            name="Docker operations"
            logo="DO"
            status={docker.status}
            description="Bounded runtime inspection and container recovery"
            rows={[
              ["Target", dockerTarget ?? docker.externalProjectId],
              ["Control plane", docker.externalAccountId],
              ["Production", settings.project.productionUrl],
            ]}
          />
        )}
        {github && (
          <Integration
            name="GitHub"
            logo="GH"
            status={github.status}
            description="Optional deployment-recovery source and review surface"
            rows={[
              ["Repository", github.externalProjectId],
              ["Installation", github.externalAccountId],
            ]}
          />
        )}
        {vercel && (
          <Integration
            name="Vercel"
            logo="▲"
            status={vercel.status}
            description="Optional deployment, Preview and rollback subsystem"
            rows={[
              ["Project", vercel.externalProjectId],
              ["Production", settings.project.productionUrl],
            ]}
          />
        )}
      </div>
      {!docker && !github && !vercel && (
        <Card>
          <p>No production execution connector is configured.</p>
        </Card>
      )}
      <Card>
        <SectionTitle
          title="Health checks"
          detail={`${settings.healthChecks.length} configured`}
        />
        <div className="health-list">
          {settings.healthChecks.length ? (
            settings.healthChecks.map((check) => (
              <div key={check.id}>
                <span
                  className={
                    check.status === "HEALTHY"
                      ? "connected"
                      : "status-badge waiting"
                  }
                >
                  {check.status}
                </span>
                <div>
                  <strong>{healthMeaning(check.status)}</strong>
                  <code>{check.url}</code>
                </div>
                <span>
                  {check.lastCheckedAt
                    ? new Date(check.lastCheckedAt).toLocaleString()
                    : "Not checked yet"}
                </span>
              </div>
            ))
          ) : (
            <p>No health checks configured.</p>
          )}
        </div>
      </Card>
    </div>
  );
}

function Integration({
  name,
  logo,
  status,
  description,
  rows,
}: {
  name: string;
  logo: string;
  status?: string;
  description: string;
  rows: Array<[string, string | null | undefined]>;
}) {
  return (
    <Card>
      <div className="integration-head">
        <span className="integration-logo">{logo}</span>
        <div>
          <h2>{name}</h2>
          <p>{description}</p>
        </div>
        <span className={status === "CONNECTED" ? "connected" : "status-badge"}>
          {status ?? "Not configured"}
        </span>
      </div>
      <dl className="summary-list">
        {rows.map(([label, value]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>{value ?? "—"}</dd>
          </div>
        ))}
      </dl>
    </Card>
  );
}

function stringValue(value: unknown) {
  return typeof value === "string" ? value : null;
}

function healthMeaning(status: string) {
  if (status === "HEALTHY") return "Production responding normally";
  if (status === "FAILING") return "Failure threshold not reached yet";
  if (status === "DOWN") return "Incident threshold reached";
  return "Health state pending";
}
