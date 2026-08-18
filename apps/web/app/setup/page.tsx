"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import type { FormEvent, ReactNode } from "react";

type SetupData = {
  username: string;
  password: string;
  name: string;
  repoOwner: string;
  repoName: string;
  githubAppId: string;
  githubInstallationId: string;
  githubPrivateKey: string;
  githubWebhookSecret: string;
  vercelTeamId: string;
  vercelProjectId: string;
  vercelToken: string;
  vercelWebhookSecret: string;
  provider: "openai" | "anthropic" | "gemini";
  providerApiKey: string;
  providerModel: string;
  productionUrl: string;
  healthCheckUrl: string;
  safetyMode: "safe" | "autopilot";
};

const initial: SetupData = {
  username: "admin",
  password: "",
  name: "",
  repoOwner: "",
  repoName: "",
  githubAppId: "",
  githubInstallationId: "",
  githubPrivateKey: "",
  githubWebhookSecret: "",
  vercelTeamId: "",
  vercelProjectId: "",
  vercelToken: "",
  vercelWebhookSecret: "",
  provider: "openai",
  providerApiKey: "",
  providerModel: "",
  productionUrl: "",
  healthCheckUrl: "",
  safetyMode: "safe",
};

const labels = ["Admin", "GitHub", "Vercel", "AI", "Production", "Safety"];

const setupErrors: Record<string, string> = {
  github_connection_failed:
    "GitHub could not authenticate this App. Check the App ID, Installation ID, and private-key PEM, then try again.",
  github_repository_not_accessible:
    "GitHub authentication worked, but this App cannot access the selected repository. Install the App on that repository and verify Contents and Pull requests permissions.",
  vercel_connection_failed:
    "Vercel could not authenticate this token. Check the token and Team ID, if your project belongs to a team.",
  vercel_project_not_accessible:
    "Vercel authentication worked, but the selected project is not accessible. Check the project name/ID and account or Team ID.",
  vercel_production_deployment_not_found:
    "No READY production deployment was found for this Vercel project. Deploy the app to production once, then retry.",
  provider_connection_failed:
    "The model provider rejected this connection. Check the API key and exact model ID supported by your provider account.",
  production_health_failed:
    "The health URL did not return a successful HTTP 200 response. It must be publicly reachable from the NoPager host without browser login.",
  unsafe_health_check_url:
    "Use a public HTTPS health URL. Localhost, private-network, credential-bearing, and unsafe addresses are blocked.",
  unsafe_production_url:
    "Use a public HTTPS production URL. Localhost, private-network, credential-bearing, and unsafe addresses are blocked.",
  invalid_setup:
    "One or more required setup values are missing or invalid. Return to the earlier step and verify each connection.",
  app_already_protected:
    "This NoPager Alpha installation already protects an app. The current open-source Alpha supports one protected app per installation.",
  unauthorized: "Your local admin session expired. Sign in again and continue setup.",
};

export default function SetupPage() {
  const [step, setStep] = useState(0);
  const [data, setData] = useState(initial);
  const [adminExists, setAdminExists] = useState(false);
  const [appProtected, setAppProtected] = useState(false);
  const [complete, setComplete] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    fetch("/api/nopager/setup/status", { cache: "no-store" })
      .then(async (response) => {
        if (!response.ok) throw new Error("NoPager API is not ready.");
        return response.json() as Promise<{
          adminCreated: boolean;
          appProtected: boolean;
          authenticated: boolean;
        }>;
      })
      .then((status) => {
        setAdminExists(status.adminCreated);
        setAppProtected(status.appProtected);
        setComplete(status.appProtected && status.authenticated);
      })
      .catch((reason: unknown) => setError(message(reason)));
  }, []);

  function update<K extends keyof SetupData>(key: K, value: SetupData[K]) {
    setData((current) => ({ ...current, [key]: value }));
  }

  function updateRepositoryName(value: string) {
    setData((current) => ({
      ...current,
      repoName: value,
      name:
        current.name.trim() === "" || current.name === current.repoName
          ? value
          : current.name,
    }));
  }

  function updateRepositoryOwner(value: string) {
    const pasted = repositoryParts(value);
    if (!pasted) {
      update("repoOwner", value);
      return;
    }
    setData((current) => ({
      ...current,
      repoOwner: pasted.owner,
      repoName: pasted.name,
      name:
        current.name.trim() === "" || current.name === current.repoName
          ? pasted.name
          : current.name,
    }));
  }

  function updateProductionUrl(value: string) {
    setData((current) => ({
      ...current,
      productionUrl: value,
      healthCheckUrl:
        current.healthCheckUrl.trim() === "" ||
        current.healthCheckUrl === current.productionUrl
          ? value
          : current.healthCheckUrl,
    }));
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      if (step === 0) {
        const response = await fetch(
          adminExists ? "/api/nopager/auth/login" : "/api/nopager/setup/admin",
          {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              username: data.username,
              password: data.password,
            }),
          },
        );
        await expectOk(response);
        setAdminExists(true);
        if (appProtected) {
          setComplete(true);
        } else {
          setData((current) => ({
            ...current,
            githubWebhookSecret:
              current.githubWebhookSecret || generateWebhookSecret(),
          }));
          setStep(1);
        }
      } else if (step < labels.length - 1) {
        const tests: Record<number, [string, object]> = {
          1: [
            "/api/nopager/setup/test/github",
            {
              appId: Number(data.githubAppId),
              installationId: Number(data.githubInstallationId),
              privateKey: data.githubPrivateKey,
              repoOwner: data.repoOwner,
              repoName: data.repoName,
            },
          ],
          2: [
            "/api/nopager/setup/test/vercel",
            {
              teamId: data.vercelTeamId,
              projectId: data.vercelProjectId,
              token: data.vercelToken,
            },
          ],
          3: [
            "/api/nopager/setup/test/provider",
            {
              provider: data.provider,
              apiKey: data.providerApiKey,
              model: data.providerModel,
            },
          ],
          4: ["/api/nopager/setup/test/health", { url: data.healthCheckUrl }],
        };
        const test = tests[step];
        if (test) {
          const response = await fetch(test[0], {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify(test[1]),
          });
          await expectOk(response);
        }
        setStep((current) => current + 1);
      } else {
        const response = await fetch("/api/nopager/setup/app", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            ...data,
            githubAppId: Number(data.githubAppId),
            githubInstallationId: Number(data.githubInstallationId),
          }),
        });
        await expectOk(response);
        setComplete(true);
      }
    } catch (reason) {
      setError(message(reason));
    } finally {
      setBusy(false);
    }
  }

  if (complete) {
    return (
      <SetupShell step={labels.length}>
        <div className="success-mark">✓</div>
        <p className="eyebrow">SETUP COMPLETE</p>
        <h1>Protected 24/7</h1>
        <p>
          NoPager is monitoring your production app. Safe production controls
          remain active.
        </p>
        <div className="setup-summary">
          <Summary
            label="GitHub"
            value={
              data.repoOwner && data.repoName
                ? `${data.repoOwner}/${data.repoName}`
                : "Connected"
            }
          />
          <Summary label="Vercel" value={data.vercelProjectId || "Connected"} />
          <Summary
            label="Health check"
            value={data.healthCheckUrl || "Active"}
          />
          <Summary
            label="AI provider"
            value={data.providerApiKey ? `${data.provider} · Ready` : "Ready"}
          />
        </div>
        <Link href="/" className="primary-button link-button">
          Open production overview
        </Link>
      </SetupShell>
    );
  }

  return (
    <SetupShell step={step}>
      <p className="eyebrow">
        STEP {step + 1} OF {labels.length} · {labels[step]}
      </p>
      <h1>{title(step, adminExists)}</h1>
      <p>{description(step)}</p>
      <form className="form-grid setup-form" onSubmit={submit}>
        {fields(
          step,
          data,
          update,
          updateRepositoryOwner,
          updateRepositoryName,
          updateProductionUrl,
        )}
        {error && (
          <p className="form-error full" role="alert">
            {error}
          </p>
        )}
        <div className="form-actions full">
          {step > 0 && (
            <button
              type="button"
              className="secondary-button"
              onClick={() => {
                setError("");
                setStep(step - 1);
              }}
            >
              Back
            </button>
          )}
          <button className="primary-button" disabled={busy}>
            {busy ? "Checking…" : primaryAction(step, adminExists)}
          </button>
        </div>
      </form>
    </SetupShell>
  );
}

function SetupShell({ step, children }: { step: number; children: ReactNode }) {
  return (
    <div className="setup-page">
      <div className="setup-brand">
        <span className="brand-mark">N</span> NoPager
      </div>
      <div className="setup-card">
        <div className="setup-progress" aria-label="Setup progress">
          {labels.map((label, index) => (
            <span
              key={label}
              className={index < step ? "complete" : ""}
              title={label}
            />
          ))}
        </div>
        {children}
      </div>
    </div>
  );
}

function Summary({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function SetupNote({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <div className="notice blue full">
      <span aria-hidden="true">i</span>
      <div>
        <strong>{title}</strong>
        <p>{children}</p>
      </div>
    </div>
  );
}

function title(step: number, adminExists: boolean) {
  return [
    adminExists ? "Sign in" : "Create local admin",
    "Connect GitHub",
    "Connect Vercel",
    "Choose AI provider",
    "Verify production",
    "Choose a safety mode",
  ][step];
}

function description(step: number) {
  return [
    "This account exists only on your NoPager installation and controls production approvals.",
    "Give NoPager access only to the repository it may inspect and repair. Repository ID and default branch are discovered automatically.",
    "Connect the Vercel project NoPager will use for Preview verification, promotion, and rollback.",
    "Bring your own model API key. NoPager does not proxy or pay for model usage; your provider bills your account directly.",
    "NoPager needs a public HTTPS health URL that returns HTTP 200 when the app is healthy.",
    "Safe Mode is the recommended starting point and always requires approval before production promotion.",
  ][step];
}

function primaryAction(step: number, adminExists: boolean) {
  return [
    adminExists ? "Sign in & continue" : "Create admin & continue",
    "Verify GitHub",
    "Verify Vercel",
    "Verify AI provider",
    "Verify production health",
    "Protect App",
  ][step];
}

function Input({
  label,
  value,
  onChange,
  type = "text",
  full = false,
  required = true,
  minLength,
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  full?: boolean;
  required?: boolean;
  minLength?: number;
  placeholder?: string;
}) {
  return (
    <label className={full ? "full" : ""}>
      {label}
      <input
        required={required}
        minLength={minLength}
        type={type}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
        autoComplete="off"
      />
    </label>
  );
}

function TextArea({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="full">
      {label}
      <textarea
        required
        rows={9}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        autoComplete="off"
        spellCheck={false}
      />
      <small>Paste the complete PEM block, including BEGIN/END lines.</small>
    </label>
  );
}

function GeneratedSecret({
  value,
  onRegenerate,
}: {
  value: string;
  onRegenerate: () => void;
}) {
  const [copied, setCopied] = useState(false);

  async function copy() {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }

  return (
    <label className="full">
      GitHub webhook secret
      <input readOnly value={value} autoComplete="off" spellCheck={false} />
      <small>
        Generated locally in your browser. Copy this exact value into the GitHub
        App webhook secret field. {" "}
        <button type="button" className="text-link" onClick={copy}>
          {copied ? "Copied" : "Copy secret"}
        </button>{" "}
        · {" "}
        <button type="button" className="text-link" onClick={onRegenerate}>
          Regenerate
        </button>
      </small>
    </label>
  );
}

function fields(
  step: number,
  data: SetupData,
  update: <K extends keyof SetupData>(key: K, value: SetupData[K]) => void,
  updateRepositoryOwner: (value: string) => void,
  updateRepositoryName: (value: string) => void,
  updateProductionUrl: (value: string) => void,
) {
  if (step === 0) {
    return (
      <>
        <SetupNote title="Local by default">
          Your password and session stay on this self-hosted NoPager instance.
          There is no NoPager cloud account to create.
        </SetupNote>
        <Input
          label="Username"
          value={data.username}
          onChange={(v) => update("username", v)}
          placeholder="admin"
        />
        <Input
          label="Password (12+ characters)"
          type="password"
          minLength={12}
          value={data.password}
          onChange={(v) => update("password", v)}
        />
      </>
    );
  }

  if (step === 1) {
    return (
      <>
        <SetupNote title="One GitHub App, one repository">
          Create a GitHub App with Contents and Pull requests read/write plus
          Actions read-only, install it only on the repository you want NoPager
          to protect, then paste the App values below. {" "}
          <a
            className="text-link"
            href="https://github.com/nopager/nopager/blob/main/docs/GITHUB_APP_SETUP.md"
            target="_blank"
            rel="noreferrer"
          >
            Open the exact setup guide ↗
          </a>
        </SetupNote>
        <Input
          label="App name"
          value={data.name}
          onChange={(v) => update("name", v)}
          placeholder="my-app"
        />
        <Input
          label="Repository owner"
          value={data.repoOwner}
          onChange={updateRepositoryOwner}
          placeholder="owner (or paste owner/repository)"
        />
        <Input
          label="Repository name"
          value={data.repoName}
          onChange={updateRepositoryName}
          placeholder="repository"
        />
        <Input
          label="GitHub App ID"
          type="number"
          value={data.githubAppId}
          onChange={(v) => update("githubAppId", v)}
          placeholder="123456"
        />
        <Input
          label="Installation ID"
          type="number"
          value={data.githubInstallationId}
          onChange={(v) => update("githubInstallationId", v)}
          placeholder="12345678"
        />
        <GeneratedSecret
          value={data.githubWebhookSecret}
          onRegenerate={() =>
            update("githubWebhookSecret", generateWebhookSecret())
          }
        />
        <TextArea
          label="GitHub App private key (PEM)"
          value={data.githubPrivateKey}
          onChange={(v) => update("githubPrivateKey", v)}
        />
      </>
    );
  }

  if (step === 2) {
    return (
      <>
        <SetupNote title="Your Vercel account stays yours">
          Use a Vercel access token that can read this project and manage its
          deployments. Team ID is only needed for team-owned projects. The
          Vercel webhook is optional because NoPager also polls deployments.
        </SetupNote>
        <Input
          label="Project ID or project name"
          value={data.vercelProjectId}
          onChange={(v) => update("vercelProjectId", v)}
          placeholder="my-app"
        />
        <Input
          label="Access token"
          type="password"
          value={data.vercelToken}
          onChange={(v) => update("vercelToken", v)}
        />
        <Input
          label="Team ID (optional)"
          value={data.vercelTeamId}
          onChange={(v) => update("vercelTeamId", v)}
          required={false}
          placeholder="Leave blank for a personal account"
        />
        <Input
          label="Webhook secret (optional)"
          type="password"
          value={data.vercelWebhookSecret}
          onChange={(v) => update("vercelWebhookSecret", v)}
          required={false}
          placeholder="Polling works without this"
        />
      </>
    );
  }

  if (step === 3) {
    return (
      <>
        <SetupNote title="BYOK: NoPager never resells your tokens">
          Your API key is encrypted locally before database persistence. Model
          usage is billed directly by your selected provider. Only bounded,
          locally redacted incident evidence is sent to that provider; the full
          repository is not uploaded as a prompt.
        </SetupNote>
        <label>
          Provider
          <select
            value={data.provider}
            onChange={(event) => {
              update("provider", event.target.value as SetupData["provider"]);
              update("providerModel", "");
            }}
          >
            <option value="openai">OpenAI</option>
            <option value="anthropic">Anthropic</option>
            <option value="gemini">Gemini</option>
          </select>
        </label>
        <Input
          label="Model ID"
          value={data.providerModel}
          onChange={(v) => update("providerModel", v)}
          placeholder="Exact model ID from your provider"
        />
        <Input
          full
          label="API key"
          type="password"
          value={data.providerApiKey}
          onChange={(v) => update("providerApiKey", v)}
        />
      </>
    );
  }

  if (step === 4) {
    return (
      <>
        <SetupNote title="Start with the simplest health check">
          Enter your normal production URL first; NoPager pre-fills the same URL
          as the health check. If your app has a dedicated endpoint such as
          /health or /api/health, replace only the health URL. It must be public
          HTTPS and return HTTP 200 without interactive login.
        </SetupNote>
        <Input
          full
          label="Production URL"
          type="url"
          value={data.productionUrl}
          onChange={updateProductionUrl}
          placeholder="https://example.com"
        />
        <Input
          full
          label="Health check URL"
          type="url"
          value={data.healthCheckUrl}
          onChange={(v) => update("healthCheckUrl", v)}
          placeholder="https://example.com/health"
        />
      </>
    );
  }

  return (
    <>
      <SetupNote title="Safe Mode is the right first run">
        NoPager can diagnose, repair, test, create a PR, deploy a Preview, and
        verify it automatically. In Safe Mode, production still waits for your
        explicit approval. High-risk actions remain blocked regardless of mode.
      </SetupNote>
      <label className="full">
        Safety mode
        <select
          value={data.safetyMode}
          onChange={(event) =>
            update("safetyMode", event.target.value as SetupData["safetyMode"])
          }
        >
          <option value="safe">Safe Mode (recommended)</option>
          <option value="autopilot">Autopilot (Experimental)</option>
        </select>
        <small>
          Start with Safe Mode for the first real incident. Autopilot is limited
          to low-risk, verified, reversible production actions.
        </small>
      </label>
    </>
  );
}

async function expectOk(response: Response) {
  if (response.ok) return;
  const body = (await response.json().catch(() => ({}))) as { error?: string };
  const code = body.error;
  throw new Error(
    (code && setupErrors[code]) ||
      code?.replaceAll("_", " ") ||
      `Request failed (${response.status})`,
  );
}

function generateWebhookSecret() {
  const bytes = new Uint8Array(32);
  window.crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function repositoryParts(value: string) {
  const trimmed = value
    .trim()
    .replace(/^https?:\/\/github\.com\//, "")
    .replace(/\.git$/, "")
    .replace(/^\/+|\/+$/g, "");
  const parts = trimmed.split("/");
  if (
    parts.length !== 2 ||
    parts.some((part) => part.trim() === "")
  )
    return null;
  return { owner: parts[0], name: parts[1] };
}

function message(reason: unknown) {
  return reason instanceof Error ? reason.message : "Something went wrong.";
}
