"use client";

import Link from "next/link";
import { useEffect, useRef, useState } from "react";
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

type GitHubAppOwner = "personal" | "organization";

type GitHubManifestExchange = {
  appId: number;
  slug: string;
  privateKey: string;
  webhookSecret: string;
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

export default function SetupPage() {
  const [step, setStep] = useState(0);
  const [data, setData] = useState(initial);
  const [adminExists, setAdminExists] = useState(false);
  const [appProtected, setAppProtected] = useState(false);
  const [complete, setComplete] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [githubAppOwner, setGithubAppOwner] =
    useState<GitHubAppOwner>("personal");
  const [githubManifestState, setGithubManifestState] = useState("");
  const [githubAppSlug, setGithubAppSlug] = useState("");
  const [githubAutoBusy, setGithubAutoBusy] = useState(false);
  const manifestPopup = useRef<Window | null>(null);
  const installPopup = useRef<Window | null>(null);

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

  useEffect(() => {
    function receiveGitHubSetup(event: MessageEvent) {
      if (event.origin !== window.location.origin || !event.data) return;
      const payload = event.data as {
        type?: unknown;
        code?: unknown;
        state?: unknown;
        installationId?: unknown;
      };

      if (payload.type === "nopager-github-manifest") {
        if (manifestPopup.current && event.source !== manifestPopup.current) return;
        if (
          typeof payload.code !== "string" ||
          typeof payload.state !== "string" ||
          payload.state !== githubManifestState
        ) {
          setError("GitHub App registration state did not match. Try again.");
          return;
        }
        setGithubAutoBusy(true);
        setError("");
        void fetch("/api/github-manifest/convert", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ code: payload.code }),
        })
          .then(async (response) => {
            await expectOk(response);
            return response.json() as Promise<GitHubManifestExchange>;
          })
          .then((app) => {
            setData((current) => ({
              ...current,
              githubAppId: String(app.appId),
              githubPrivateKey: app.privateKey,
              githubWebhookSecret: app.webhookSecret,
              githubInstallationId: "",
            }));
            setGithubAppSlug(app.slug);
          })
          .catch((reason: unknown) => setError(message(reason)))
          .finally(() => setGithubAutoBusy(false));
        return;
      }

      if (payload.type === "nopager-github-install") {
        if (installPopup.current && event.source !== installPopup.current) return;
        if (
          typeof payload.installationId !== "string" ||
          !/^\d+$/.test(payload.installationId)
        ) {
          setError("GitHub returned an invalid installation ID. Try the install step again.");
          return;
        }
        setData((current) => ({
          ...current,
          githubInstallationId: payload.installationId as string,
        }));
        setError("");
      }
    }

    window.addEventListener("message", receiveGitHubSetup);
    return () => window.removeEventListener("message", receiveGitHubSetup);
  }, [githubManifestState]);

  function update<K extends keyof SetupData>(key: K, value: SetupData[K]) {
    setData((current) => ({ ...current, [key]: value }));
  }

  function startGithubManifest() {
    if (!data.name.trim() || !data.repoOwner.trim() || !data.repoName.trim()) {
      setError("Enter the app name and GitHub repository first.");
      return;
    }
    if (
      window.location.protocol !== "https:" &&
      !["localhost", "127.0.0.1", "::1"].includes(window.location.hostname)
    ) {
      setError("Use HTTPS for remote NoPager setup before creating the GitHub App.");
      return;
    }

    const popup = window.open(
      "",
      "nopager-github-manifest",
      "popup,width=760,height=760",
    );
    if (!popup) {
      setError("Allow popups for this NoPager console and try again.");
      return;
    }
    manifestPopup.current = popup;

    const state = crypto.randomUUID();
    setGithubManifestState(state);
    setGithubAppSlug("");
    setData((current) => ({
      ...current,
      githubAppId: "",
      githubInstallationId: "",
      githubPrivateKey: "",
      githubWebhookSecret: "",
    }));
    setError("");

    const origin = window.location.origin;
    const manifest = {
      name: `NoPager ${data.repoName} ${state.slice(0, 8)}`,
      url: origin,
      description: "Self-hosted NoPager AI on-call integration",
      hook_attributes: {
        url: `${origin}/api/webhooks/github`,
        active: true,
      },
      redirect_url: `${origin}/setup/github-manifest`,
      setup_url: `${origin}/setup/github-install`,
      setup_on_update: true,
      public: false,
      default_permissions: {
        actions: "read",
        contents: "write",
        pull_requests: "write",
      },
      default_events: ["workflow_run"],
    };

    const form = document.createElement("form");
    form.method = "POST";
    form.target = "nopager-github-manifest";
    form.action =
      githubAppOwner === "organization"
        ? `https://github.com/organizations/${encodeURIComponent(data.repoOwner.trim())}/settings/apps/new?state=${encodeURIComponent(state)}`
        : `https://github.com/settings/apps/new?state=${encodeURIComponent(state)}`;
    const input = document.createElement("input");
    input.type = "hidden";
    input.name = "manifest";
    input.value = JSON.stringify(manifest);
    form.appendChild(input);
    document.body.appendChild(form);
    form.submit();
    form.remove();
  }

  function startGithubInstall() {
    if (!githubAppSlug) {
      setError("Create the GitHub App first.");
      return;
    }
    const popup = window.open(
      `https://github.com/apps/${encodeURIComponent(githubAppSlug)}/installations/new`,
      "nopager-github-install",
      "popup,width=760,height=760",
    );
    if (!popup) {
      setError("Allow popups for this NoPager console and try again.");
      return;
    }
    installPopup.current = popup;
    setError("");
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

  const githubReady =
    data.githubAppId.length > 0 &&
    data.githubInstallationId.length > 0 &&
    data.githubPrivateKey.length > 0 &&
    data.githubWebhookSecret.length >= 16;

  return (
    <SetupShell step={step}>
      <p className="eyebrow">
        STEP {step + 1} OF {labels.length} · {labels[step]}
      </p>
      <h1>{title(step, adminExists)}</h1>
      <p>{description(step)}</p>
      <form className="form-grid setup-form" onSubmit={submit}>
        {step === 1 ? (
          <GitHubSetupFields
            data={data}
            update={update}
            owner={githubAppOwner}
            setOwner={setGithubAppOwner}
            appSlug={githubAppSlug}
            autoBusy={githubAutoBusy}
            onCreate={startGithubManifest}
            onInstall={startGithubInstall}
          />
        ) : (
          fields(step, data, update)
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
              onClick={() => setStep(step - 1)}
            >
              Back
            </button>
          )}
          <button
            className="primary-button"
            disabled={busy || githubAutoBusy || (step === 1 && !githubReady)}
          >
            {busy
              ? "Checking…"
              : step === labels.length - 1
                ? "Protect App"
                : step === 0
                  ? "Save & continue"
                  : step === 1 && !githubReady
                    ? "Complete GitHub setup above"
                    : "Test & continue"}
          </button>
        </div>
      </form>
    </SetupShell>
  );
}

function GitHubSetupFields({
  data,
  update,
  owner,
  setOwner,
  appSlug,
  autoBusy,
  onCreate,
  onInstall,
}: {
  data: SetupData;
  update: <K extends keyof SetupData>(key: K, value: SetupData[K]) => void;
  owner: GitHubAppOwner;
  setOwner: (value: GitHubAppOwner) => void;
  appSlug: string;
  autoBusy: boolean;
  onCreate: () => void;
  onInstall: () => void;
}) {
  const created =
    data.githubAppId.length > 0 &&
    data.githubPrivateKey.length > 0 &&
    data.githubWebhookSecret.length >= 16;
  const installed = created && data.githubInstallationId.length > 0;

  return (
    <>
      <Input
        label="App name"
        value={data.name}
        onChange={(value) => update("name", value)}
      />
      <Input
        label="Repository owner"
        value={data.repoOwner}
        onChange={(value) => update("repoOwner", value)}
      />
      <Input
        full
        label="Repository name"
        value={data.repoName}
        onChange={(value) => update("repoName", value)}
      />
      <label className="full">
        Where should GitHub own this App?
        <select
          value={owner}
          onChange={(event) => setOwner(event.target.value as GitHubAppOwner)}
        >
          <option value="personal">My personal GitHub account</option>
          <option value="organization">Repository owner organization</option>
        </select>
        <small>
          For organization repositories, choose the organization option and make
          sure you can create GitHub Apps there.
        </small>
      </label>
      <div className="full form-actions">
        <button
          type="button"
          className="secondary-button"
          disabled={autoBusy}
          onClick={onCreate}
        >
          {autoBusy
            ? "Finishing GitHub registration…"
            : created
              ? "Re-create GitHub App"
              : "Create GitHub App automatically"}
        </button>
        {created && !installed && appSlug && (
          <button type="button" className="primary-button" onClick={onInstall}>
            Install on repository
          </button>
        )}
      </div>
      {created && !installed && (
        <p className="full">
          GitHub App created. Install it and select only the repository NoPager
          should protect.
        </p>
      )}
      {installed && (
        <p className="full">
          ✓ GitHub App registered and installed. NoPager will verify access when
          you continue.
        </p>
      )}
      <details className="full">
        <summary>Advanced: use an existing GitHub App</summary>
        <div className="form-grid">
          <Input
            label="GitHub App ID"
            type="number"
            required={false}
            value={data.githubAppId}
            onChange={(value) => update("githubAppId", value)}
          />
          <Input
            label="Installation ID"
            type="number"
            required={false}
            value={data.githubInstallationId}
            onChange={(value) => update("githubInstallationId", value)}
          />
          <Input
            full
            label="Webhook secret"
            type="password"
            required={false}
            value={data.githubWebhookSecret}
            onChange={(value) => update("githubWebhookSecret", value)}
          />
          <TextArea
            label="GitHub App private key (PEM)"
            required={false}
            value={data.githubPrivateKey}
            onChange={(value) => update("githubPrivateKey", value)}
          />
        </div>
      </details>
    </>
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
    "Choose the repository, then let NoPager create a least-privilege GitHub App with the required permissions. Existing GitHub Apps remain supported under Advanced.",
    "Select the Vercel project used for previews and production. Team ID and webhook secret are optional; polling remains active without a Vercel webhook.",
    "Your API key is encrypted locally and never shown again. The full repository stays in the self-hosted worker; NoPager sends only bounded incident evidence to your BYOK model provider after local secret redaction. Relevant code diffs may still leave this host. Enter the exact model ID supported by your provider.",
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
  minLength,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  full?: boolean;
  required?: boolean;
  minLength?: number;
}) {
  return (
    <label className={full ? "full" : ""}>
      {label}
      <input
        required={required}
        minLength={minLength}
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
  required = true,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  required?: boolean;
}) {
  return (
    <label className="full">
      {label}
      <textarea
        required={required}
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
          onChange={(value) => update("username", value)}
        />
        <Input
          label="Password (12+ characters)"
          type="password"
          minLength={12}
          value={data.password}
          onChange={(value) => update("password", value)}
        />
      </>
    );
  if (step === 2)
    return (
      <>
        <Input
          label="Team ID (optional for personal account)"
          value={data.vercelTeamId}
          onChange={(value) => update("vercelTeamId", value)}
          required={false}
        />
        <Input
          label="Project ID or project name"
          value={data.vercelProjectId}
          onChange={(value) => update("vercelProjectId", value)}
        />
        <Input
          label="Access token"
          type="password"
          value={data.vercelToken}
          onChange={(value) => update("vercelToken", value)}
        />
        <Input
          label="Webhook secret (optional)"
          type="password"
          value={data.vercelWebhookSecret}
          onChange={(value) => update("vercelWebhookSecret", value)}
          required={false}
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
          onChange={(value) => update("providerModel", value)}
        />
        <Input
          full
          label="API key"
          type="password"
          value={data.providerApiKey}
          onChange={(value) => update("providerApiKey", value)}
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
          onChange={(value) => update("productionUrl", value)}
        />
        <Input
          full
          label="Health check URL"
          type="url"
          value={data.healthCheckUrl}
          onChange={(value) => update("healthCheckUrl", value)}
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
