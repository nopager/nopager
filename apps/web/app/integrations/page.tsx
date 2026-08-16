import { Card, PageHeader, SectionTitle } from "@/components/ui";
import { api, type AppSettings } from "@/lib/api";

export default async function IntegrationsPage() {
  const settings = await api<AppSettings>("settings");
  const github = settings?.integrations.find((item) => item.type === "github");
  const vercel = settings?.integrations.find((item) => item.type === "vercel");
  return (
    <div className="page">
      <PageHeader
        eyebrow="Connections"
        title="Integrations"
        description="The production context NoPager can observe and act on safely."
      />
      <div className="integration-grid">
        <Integration
          name="GitHub"
          logo="GH"
          status={github?.status}
          rows={[
            ["Repository", github?.externalProjectId],
            ["Installation", github?.externalAccountId],
          ]}
        />
        <Integration
          name="Vercel"
          logo="▲"
          status={vercel?.status}
          rows={[
            ["Project", vercel?.externalProjectId],
            ["Production", settings?.project.productionUrl],
          ]}
        />
      </div>
      <Card>
        <SectionTitle
          title="Health checks"
          detail={`${settings?.healthChecks.length ?? 0} configured`}
        />
        <div className="health-list">
          {settings?.healthChecks.map((check) => (
            <div key={check.id}>
              <span className="green-dot mini-dot" />
              <div>
                <strong>{check.status}</strong>
                <code>{check.url}</code>
              </div>
              <span>
                {check.lastCheckedAt
                  ? new Date(check.lastCheckedAt).toLocaleString()
                  : "Not checked yet"}
              </span>
            </div>
          )) ?? <p>No health checks configured.</p>}
        </div>
      </Card>
    </div>
  );
}

function Integration({
  name,
  logo,
  status,
  rows,
}: {
  name: string;
  logo: string;
  status?: string;
  rows: Array<[string, string | null | undefined]>;
}) {
  return (
    <Card>
      <div className="integration-head">
        <span className="integration-logo">{logo}</span>
        <div>
          <h2>{name}</h2>
          <p>
            {name === "GitHub"
              ? "Source, commits, pull requests"
              : "Deployments, previews, rollback"}
          </p>
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
