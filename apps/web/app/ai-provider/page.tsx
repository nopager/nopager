import { Card, PageHeader, SectionTitle } from "@/components/ui";
import { api, type AppSettings } from "@/lib/api";
import { maskSecret } from "@/lib/model";

export default async function AiProviderPage() {
  const settings = await api<AppSettings>("settings");
  const provider = settings?.integrations.find(
    (item) => item.type === "model_provider",
  );
  const name =
    string(provider?.metadata.provider) ??
    provider?.externalAccountId ??
    "Not configured";
  const model = string(provider?.metadata.model) ?? "—";
  const suffix = string(provider?.metadata.keySuffix) ?? "••••";
  return (
    <div className="page">
      <PageHeader
        eyebrow="Bring your own key"
        title="AI Provider"
        description="Your key is encrypted at rest and never returned by the API."
      />
      <Card className="provider-card">
        <div className="provider-logo">AI</div>
        <div>
          <h2>{name}</h2>
          <p>
            {provider?.status === "CONNECTED"
              ? "Connected · Used for diagnosis and repair"
              : "Complete setup to connect a provider"}
          </p>
        </div>
        <span className="connected">
          {provider?.status ?? "Not configured"}
        </span>
      </Card>
      <Card>
        <SectionTitle title="Provider settings" />
        <dl className="summary-list">
          <div>
            <dt>Provider</dt>
            <dd>{name}</dd>
          </div>
          <div>
            <dt>Model</dt>
            <dd>{model}</dd>
          </div>
          <div>
            <dt>API key</dt>
            <dd>
              <code>{maskSecret(suffix)}</code>
            </dd>
          </div>
        </dl>
        <small>
          Secrets are never included in logs, incident evidence, or model
          context.
        </small>
      </Card>
      <div className="notice blue">
        <span>i</span>
        <div>
          <strong>BYOK keeps model usage under your control</strong>
          <p>
            NoPager does not mark up token costs or use tokens as a billing
            unit.
          </p>
        </div>
      </div>
    </div>
  );
}

function string(value: unknown) {
  return typeof value === "string" ? value : null;
}
