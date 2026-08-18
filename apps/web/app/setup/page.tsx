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

type GitHubMode = "automatic" | "manual";
type ManifestStage = "idle" | "created" | "installed";

type ManifestCredentials = {
  appId: number;
  privateKey: string;
  webhookSecret: string;
  appUrl: string;
  slug: string | null;
};

type ManifestDraft = {
  name: string;
  repoOwner: string;
  repoName: string;
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
const manifestStateKey = "nopager.githubManifest.state";
const manifestCredentialsKey = "nopager.githubManifest.credentials";
const manifestDraftKey = "nopager.githubManifest.draft";

const setupErrors: Record<string, string> = {
  github_connection_failed:
    "GitHub could not authenticate this App. Check the App ID, Installation ID, and private-key PEM, then try again.",
  github_repository_not_accessible:
    "GitHub authentication worked, but this App cannot access the selected repository. Install the App on that repository and verify Contents and Pull requests permissions.",
  github_manifest_exchange_failed:
    "GitHub could not complete the App registration handshake. The temporary code may have expired or already been used. Start automatic GitHub setup again.",
  github_manifest_exchange_unavailable:
    "NoPager could not reach GitHub to finish App registration. Check network access from this self-hosted instance and retry.",
  github_manifest_exchange_invalid_response:
    "GitHub returned an incomplete App registration response. Start automatic GitHub setup again.",
  invalid_github_manifest_code:
    "The GitHub App registration callback was invalid. Start automatic GitHub setup again.",
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
  unauthorized:
    "Your local admin session expired. Sign in again and continue setup.",
};

export default function SetupPage() {
  const [step, setStep] = useState(0);
  const [data, setData] = useState(initial);
  const [adminExists, setAdminExists] = useState(false);
  const [appProtected, setAppProtected] = useState(false);
  const [complete, setComplete] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [githubMode, setGitHubMode] = useState<GitHubMode>("automatic");
  const [manifestStage, setManifestStage] = useState<ManifestStage>("idle");
  const [manifestAppUrl, setManifestAppUrl] = useState("");

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
    let cancelled = false;

    async function resumeManifestFlow() {
      const params = new URLSearchParams(window.location.search);
      const code = params.get("code");
      const returnedState = params.get("state");
      const installationId = params.get("installation_id");
      const storedCredentials = readSessionJson<ManifestCredentials>(
        manifestCredentialsKey,
      );
      const draft = readSessionJson<ManifestDraft>(manifestDraftKey);

      if (code) {
        const expectedState = sessionStorage.getItem(manifestStateKey);
        if (
          !expectedState ||
          !returnedState ||
          expectedState !== returnedState
        ) {
          if (!cancelled) {
            setError(
              "GitHub App registration state did not match this setup session. Start automatic GitHub setup again.",
            );
            setStep(1);
          }
          clearManifestQuery();
          return;
        }

        sessionStorage.removeItem(manifestStateKey);
        if (!cancelled) {
          setBusy(true);
          setError("");
          setStep(1);
        }
        try {
          const response = await fetch("/api/github-manifest", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ code }),
          });
          const credentials = await expectJson<ManifestCredentials>(response);
          sessionStorage.setItem(
            manifestCredentialsKey,
            JSON.stringify(credentials),
          );
          if (!cancelled) {
            applyManifestCredentials(credentials, draft, null);
            setManifestAppUrl(credentials.appUrl);
            setManifestStage("created");
            setGitHubMode("automatic");
          }
        } catch (reason) {
          if (!cancelled) setError(message(reason));
        } finally {
          if (!cancelled) setBusy(false);
          clearManifestQuery();
        }
        return;
      }

      if (installationId && storedCredentials) {
        if (!/^\d+$/.test(installationId)) {
          if (!cancelled)
            setError("GitHub returned an invalid installation ID.");
          clearManifestQuery();
          return;
        }
        if (!cancelled) {
          applyManifestCredentials(storedCredentials, draft, installationId);
          setManifestAppUrl(storedCredentials.appUrl);
          setManifestStage("installed");
          setGitHubMode("automatic");
          setStep(1);
        }
        clearManifestQuery();
        return;
      }

      if (storedCredentials && !cancelled) {
        applyManifestCredentials(storedCredentials, draft, null);
        setManifestAppUrl(storedCredentials.appUrl);
        setManifestStage("created");
        setGitHubMode("automatic");
      }
    }

    function applyManifestCredentials(
      credentials: ManifestCredentials,
      draft: ManifestDraft | null,
      installationId: string | null,
    ) {
      setData((current) => ({
        ...current,
        name: draft?.name || current.name,
        repoOwner: draft?.repoOwner || current.repoOwner,
        repoName: draft?.repoName || current.repoName,
        githubAppId: String(credentials.appId),
        githubPrivateKey: credentials.privateKey,
        githubWebhookSecret: credentials.webhookSecret,
        githubInstallationId: installationId ?? current.githubInstallationId,
      }));
    }

    void resumeManifestFlow();
    return () => {
      cancelled = true;
    };
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

  function startGitHubManifest() {
    if (!data.repoOwner.trim() || !data.repoName.trim()) {
      setError("Enter the repository owner and repository name first.");
      return;
    }
    setError("");
    const state = randomHex(24);
    const draft: ManifestDraft = {
      name: data.name || data.repoName,
      repoOwner: data.repoOwner,
      repoName: data.repoName,
    };
    sessionStorage.setItem(manifestStateKey, state);
    sessionStorage.setItem(manifestDraftKey, JSON.stringify(draft));
    sessionStorage.removeItem(manifestCredentialsKey);

    const origin = window.location.origin;
    const manifest = {
      name: `NoPager-${randomHex(4)}`,
      url: "https://github.com/nopager/nopager",
      description: `Self-hosted NoPager access for ${data.repoOwner}/${data.repoName}`,
      redirect_url: `${origin}/setup`,
      setup_url: `${origin}/setup`,
      setup_on_update: true,
      public: false,
      hook_attributes: {
        url: `${origin}/api/webhooks/github`,
        active: publicHttpsOrigin(origin),
      },
      default_permissions: {
        contents: "write",
        pull_requests: "write",
        actions: "read",
      },
      default_events: ["workflow_run"],
    };

    const form = document.createElement("form");
    form.method = "post";
    form.action = `https://github.com/settings/apps/new?state=${encodeURIComponent(state)}`;
    const input = document.createElement("input");
    input.type = "hidden";
    input.name = "manifest";
    input.value = JSON.stringify(manifest);
    form.appendChild(input);
    document.body.appendChild(form);
    form.submit();
  }

  function useManualGitHubSetup() {
    clearManifestSession();
    setGitHubMode("manual");
    setManifestStage("idle");
    setManifestAppUrl("");
    setData((current) => ({
      ...current,
      githubAppId: "",
      githubInstallationId: "",
      githubPrivateKey: "",
      githubWebhookSecret: generateWebhookSecret(),
    }));
    setError("");
  }

  function useAutomaticGitHubSetup() {
    clearManifestSession();
    setGitHubMode("automatic");
    setManifestStage("idle");
    setManifestAppUrl("");
    setData((current) => ({
      ...current,
      githubAppId: "",
      githubInstallationId: "",
      githubPrivateKey: "",
      githubWebhookSecret: "",
    }));
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
        if (appProtected) {
          setComplete(true);
        } else {
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
        if (step === 1) clearManifestSession();
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
        clearManifestSession();
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

  const waitingForAutomaticGitHub =
    step === 1 && githubMode === "automatic" && manifestStage !== "installed";

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
          githubMode,
          manifestStage,
          manifestAppUrl,
          startGitHubManifest,
          useManualGitHubSetup,
          useAutomaticGitHubSetup,
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
          <button
            className="primary-button"
            disabled={busy || waitingForAutomaticGitHub}
          >
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
    "Give NoPager access only to the repository it may inspect and repair. The recommended path creates the least-privilege GitHub App for you.",
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
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  }

  return (
    <label className="full">
      GitHub webhook secret
      <input readOnly value={value} autoComplete="off" spellCheck={false} />
      <small>
        Generated locally in your browser. Copy this exact value into the GitHub
        App webhook secret field.{" "}
        <button type="button" className="text-link" onClick={copy}>
          {copied ? "Copied" : "Copy secret"}
        </button>{" "}
        ·{" "}
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
  githubMode: GitHubMode,
  manifestStage: ManifestStage,
  manifestAppUrl: string,
  startGitHubManifest: () => void,
  useManualGitHubSetup: () => void,
  useAutomaticGitHubSetup: () => void,
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
    const commonFields = (
      <>
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
      </>
    );

    if (githubMode === "automatic") {
      return (
        <>
          <SetupNote title="Recommended: let GitHub create the App">
            NoPager sends GitHub a manifest containing only the required
            Contents/Pull requests permissions, Actions read access, the setup
            callback, and the workflow-run webhook configuration. GitHub creates
            the App and generates the private key and webhook secret; NoPager
            never needs a central service for this flow.
          </SetupNote>
          {commonFields}
          {manifestStage === "idle" && (
            <div className="full">
              <button
                type="button"
                className="primary-button"
                onClick={startGitHubManifest}
              >
                Create GitHub App automatically
              </button>
            </div>
          )}
          {manifestStage === "created" && (
            <SetupNote title="GitHub App created">
              The generated credentials are held only for this browser setup
              session.{" "}
              <a className="text-link" href={manifestAppUrl} target="_self">
                Open the GitHub App and install it on this repository →
              </a>
            </SetupNote>
          )}
          {manifestStage === "installed" && (
            <SetupNote title="GitHub App installed">
              GitHub returned installation {data.githubInstallationId}. NoPager
              will now exchange an App JWT for a repository-scoped installation
              token and verify that it can access exactly {data.repoOwner}/
              {data.repoName}. The callback ID is not trusted on its own.
            </SetupNote>
          )}
          <div className="full">
            <button
              type="button"
              className="text-link"
              onClick={useManualGitHubSetup}
            >
              Use manual GitHub App setup instead
            </button>
          </div>
        </>
      );
    }

    return (
      <>
        <SetupNote title="Manual GitHub App setup">
          Use this fallback when your GitHub policy does not allow manifest
          registration. Create a GitHub App with Contents and Pull requests
          read/write plus Actions read-only, install it only on the repository
          NoPager may protect, then paste the values below.{" "}
          <a
            className="text-link"
            href="https://github.com/nopager/nopager/blob/main/docs/GITHUB_APP_SETUP.md"
            target="_blank"
            rel="noreferrer"
          >
            Open the exact setup guide ↗
          </a>
        </SetupNote>
        {commonFields}
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
        <div className="full">
          <button
            type="button"
            className="text-link"
            onClick={useAutomaticGitHubSetup}
          >
            Return to automatic GitHub setup
          </button>
        </div>
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

async function expectJson<T>(response: Response): Promise<T> {
  if (!response.ok) {
    await expectOk(response);
    throw new Error("Request failed");
  }
  return (await response.json()) as T;
}

function randomHex(bytesLength: number) {
  const bytes = new Uint8Array(bytesLength);
  window.crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function generateWebhookSecret() {
  return randomHex(32);
}

function publicHttpsOrigin(origin: string) {
  try {
    const url = new URL(origin);
    const hostname = url.hostname.toLowerCase();
    return (
      url.protocol === "https:" &&
      hostname !== "localhost" &&
      hostname !== "127.0.0.1" &&
      hostname !== "::1"
    );
  } catch {
    return false;
  }
}

function repositoryParts(value: string) {
  const trimmed = value
    .trim()
    .replace(/^https?:\/\/github\.com\//, "")
    .replace(/\.git$/, "")
    .replace(/^\/+|\/+$/g, "");
  const parts = trimmed.split("/");
  if (parts.length !== 2 || parts.some((part) => part.trim() === ""))
    return null;
  return { owner: parts[0], name: parts[1] };
}

function readSessionJson<T>(key: string): T | null {
  try {
    const value = sessionStorage.getItem(key);
    return value ? (JSON.parse(value) as T) : null;
  } catch {
    return null;
  }
}

function clearManifestSession() {
  sessionStorage.removeItem(manifestStateKey);
  sessionStorage.removeItem(manifestCredentialsKey);
  sessionStorage.removeItem(manifestDraftKey);
}

function clearManifestQuery() {
  const url = new URL(window.location.href);
  for (const key of ["code", "state", "installation_id", "setup_action"]) {
    url.searchParams.delete(key);
  }
  window.history.replaceState(
    {},
    "",
    `${url.pathname}${url.search}${url.hash}`,
  );
}

function message(reason: unknown) {
  return reason instanceof Error ? reason.message : "Something went wrong.";
}
