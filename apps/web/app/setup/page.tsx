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
  providerModel: "gpt-5.4-mini",
  productionUrl: "",
  healthCheckUrl: "",
  safetyMode: "safe",
};

const labels = ["Admin", "GitHub", "Vercel", "AI", "Production", "Safety"];

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
        if (appProtected) setComplete(true);
        else setStep(1);
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
        {fields(step, data, update)}
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
              onClick={() => setStep(step - 1)}
            >
              Back
            </button>
          )}
          <button className="primary-button" disabled={busy}>
            {busy
              ? "Checking…"
              : step === labels.length - 1
                ? "Protect App"
                : step === 0
                  ? "Save & continue"
                  : "Test & continue"}
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
        <div className="setup-progress">
          {labels.map((label, index) => (
            <span key={label} className={index < step ? "complete" : ""} />
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
    "Your local account controls production approvals.",
    "Add the GitHub App and repository NoPager may repair. Repository ID and default branch are discovered automatically.",
    "Select the Vercel project used for previews and production. Team ID is optional for a personal account.",
    "Your API key is encrypted locally and never shown again.",
    "NoPager will require a passing public HTTPS health check.",
    "Safe Mode requires approval before production changes.",
  ][step];
}

function Input({
  label,
  value,
  onChange,
  type = "text",
  full = false,
  required = true,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  full?: boolean;
  required?: boolean;
}) {
  return (
    <label className={full ? "full" : ""}>
      {label}
      <input
        required={required}
        type={type}
        value={value}
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

function fields(
  step: number,
  data: SetupData,
  update: <K extends keyof SetupData>(key: K, value: SetupData[K]) => void,
) {
  if (step === 0)
    return (
      <>
        <Input
          label="Username"
          value={data.username}
          onChange={(v) => update("username", v)}
        />
        <Input
          label="Password (12+ characters)"
          type="password"
          value={data.password}
          onChange={(v) => update("password", v)}
        />
      </>
    );
  if (step === 1)
    return (
      <>
        <Input
          label="App name"
          value={data.name}
          onChange={(v) => update("name", v)}
        />
        <Input
          label="Repository owner"
          value={data.repoOwner}
          onChange={(v) => update("repoOwner", v)}
        />
        <Input
          label="Repository name"
          value={data.repoName}
          onChange={(v) => update("repoName", v)}
        />
        <Input
          label="GitHub App ID"
          value={data.githubAppId}
          onChange={(v) => update("githubAppId", v)}
        />
        <Input
          label="Installation ID"
          value={data.githubInstallationId}
          onChange={(v) => update("githubInstallationId", v)}
        />
        <Input
          label="Webhook secret"
          type="password"
          value={data.githubWebhookSecret}
          onChange={(v) => update("githubWebhookSecret", v)}
        />
        <TextArea
          label="GitHub App private key (PEM)"
          value={data.githubPrivateKey}
          onChange={(v) => update("githubPrivateKey", v)}
        />
      </>
    );
  if (step === 2)
    return (
      <>
        <Input
          label="Team ID (optional for personal account)"
          value={data.vercelTeamId}
          onChange={(v) => update("vercelTeamId", v)}
          required={false}
        />
        <Input
          label="Project ID or project name"
          value={data.vercelProjectId}
          onChange={(v) => update("vercelProjectId", v)}
        />
        <Input
          label="Access token"
          type="password"
          value={data.vercelToken}
          onChange={(v) => update("vercelToken", v)}
        />
        <Input
          label="Webhook secret"
          type="password"
          value={data.vercelWebhookSecret}
          onChange={(v) => update("vercelWebhookSecret", v)}
        />
      </>
    );
  if (step === 3)
    return (
      <>
        <label>
          Provider
          <select
            value={data.provider}
            onChange={(event) =>
              update("provider", event.target.value as SetupData["provider"])
            }
          >
            <option value="openai">OpenAI</option>
            <option value="anthropic">Anthropic</option>
            <option value="gemini">Gemini</option>
          </select>
        </label>
        <Input
          label="Default model"
          value={data.providerModel}
          onChange={(v) => update("providerModel", v)}
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
  if (step === 4)
    return (
      <>
        <Input
          full
          label="Production URL"
          type="url"
          value={data.productionUrl}
          onChange={(v) => update("productionUrl", v)}
        />
        <Input
          full
          label="Health check URL"
          type="url"
          value={data.healthCheckUrl}
          onChange={(v) => update("healthCheckUrl", v)}
        />
      </>
    );
  return (
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
        High-risk actions are always blocked. Safe Mode waits for your approval
        before production.
      </small>
    </label>
  );
}

async function expectOk(response: Response) {
  if (response.ok) return;
  const body = (await response.json().catch(() => ({}))) as { error?: string };
  throw new Error(
    body.error?.replaceAll("_", " ") ?? `Request failed (${response.status})`,
  );
}

function message(reason: unknown) {
  return reason instanceof Error ? reason.message : "Something went wrong.";
}
