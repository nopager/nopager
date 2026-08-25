# Production-operations quickstart

This is the first user-facing path for NoPager's **non-deployment** production-operations proof.

It does **not** require GitHub or Vercel. It protects one public web application whose runtime is one Docker container enrolled into a small host-side runtime helper. The worker has no Docker daemon authority.

The current bounded production action is deliberately narrow:

```text
public HTTPS health signal
  → three consecutive failures
  → one deduplicated incident
  → AI operations triage
  → optionally propose restart_container for one configured target
  → Safe Mode approval (default)
  → one Docker restart
  → independent external HTTP + container-status verification
  → resolve or escalate without blind retry
```

This is enough for first design-partner use on a disposable or non-critical workload. It is **not** a claim that NoPager already replaces every SRE/operations task.

## Who should use this build

Use it if all of these are true:

- you run a real web app with a public HTTPS health endpoint;
- the app runtime to protect is one Docker container on the same Docker daemon that the NoPager host can access;
- you can run NoPager on a separate control-plane Compose project;
- you can supply an OpenAI, Anthropic, or Gemini API key and exact model ID;
- you are willing to start in **Safe Mode** and review the first production restart before approval;
- you accept that this is an early design-partner build, not a general server/cloud/Cloudflare/database operations product yet.

Do not use the first design-partner build as the only recovery mechanism for a safety-critical, regulated, or high-consequence system.

## Prerequisites

- Dedicated disposable/staging Linux host with systemd, Docker Engine 26+, Docker Compose v2, Python 3, and Rust/Cargo 1.92.0.
- The protected app container is **not** part of NoPager's own Compose project.
- A public production URL such as `https://app.example.com`.
- A public HTTPS health URL returning HTTP 200 while healthy, such as `https://app.example.com/health`.
- One BYOK provider key and an exact supported model ID.
- A local administrator password of at least 12 characters.

NoPager's control plane is self-hosted. The protected app does not run a model process or heavyweight NoPager agent.

## One-path setup

Clone the repository:

```bash
git clone https://github.com/nopager/nopager.git
cd nopager
git checkout '<tested-design-partner-commit-or-tag>'
test "$(cat DESIGN_PARTNER_VERSION)" = "0.2.0-design-partner.1"
```

Then run the operations quickstart. For an interactive setup, only export the provider/model values you do not want to type repeatedly:

```bash
export NOPAGER_AI_PROVIDER=openai
export NOPAGER_AI_MODEL='<exact-model-id>'
export OPENAI_API_KEY='<provider-key>'
sh scripts/operations-quickstart.sh
```

The script will ask for:

- protected app name;
- public production URL;
- public health URL;
- exact Docker container name/ID;
- local admin password.

For a non-interactive installation, provide all required values:

```bash
NOPAGER_APP_NAME='my-app' \
NOPAGER_PRODUCTION_URL='https://app.example.com' \
NOPAGER_HEALTH_URL='https://app.example.com/health' \
NOPAGER_DOCKER_TARGET='my-app-web-1' \
NOPAGER_AI_PROVIDER='openai' \
NOPAGER_AI_MODEL='<exact-model-id>' \
OPENAI_API_KEY='<provider-key>' \
NOPAGER_ADMIN_USERNAME='admin' \
NOPAGER_ADMIN_PASSWORD='<12+-character-password>' \
sh scripts/operations-quickstart.sh
```

For Anthropic, use `ANTHROPIC_API_KEY`; for Gemini, use `GEMINI_API_KEY`.

The BYOK key is sent only to the local setup API during preflight/persistence, encrypted with `NOPAGER_MASTER_KEY`, and stored in PostgreSQL as the `model_provider` integration. After setup, the operations quickstart clears provider-key environment entries from NoPager's `.env` before recreating the worker. The admin password is used only to create the local account and is **not** written to `.env`.

The setup resolves the name to an immutable 64-hex container ID, builds and installs the host helper through `sudo`, and stores that exact ID plus `NOPAGER_ALLOW_CONTAINER_RESTART=true` as worker-side hard gates. The helper holds a separate enrollment file and independently checks it. Changing values is not a supported silent retarget: rerunning setup fails closed if stored app identity differs. Re-enrollment requires an explicit host install and worker reconfiguration.

## What the setup proves before enabling protection

The operations quickstart fails closed unless all of these checks pass:

1. Docker and Compose are available.
2. NoPager's normal self-host quickstart becomes ready.
3. The BYOK provider authenticates and the selected model passes NoPager's structured-output capability probe.
4. The production URL passes the same public-HTTPS/SSRF safety validation used by the API.
5. The health URL is public HTTPS and currently returns HTTP 200.
6. The configured Docker target exists and resolves to an immutable ID.
7. The target is not marked `com.nopager.control-plane=true`.
8. The target is not any explicitly denied NoPager resource or the current worker.
9. The target is not in NoPager's own Compose project.
10. The helper service account alone can reach Docker; its startup independently revalidates enrollment.
11. The worker has no Docker socket/CLI/group and can inspect only through authenticated typed IPC.
12. Only then is the operations-only protected app persisted and monitoring enabled.

GitHub and Vercel are not configured or required on this path.

## First real incident

Keep **Safe Mode** enabled.

A real health incident requires three consecutive failed health checks. Once the incident opens, NoPager wakes the model and presents only the bounded operations actions actually configured. An operations-only project does not expose GitHub/Vercel deployment recovery. The model can select a typed action but cannot produce an executable command. The worker sends only inspect/restart requests for the immutable ID; the helper reconstructs a fixed Docker command after its own policy checks.

When the AI proposes the restart, the incident enters `WAITING_APPROVAL`. In the console, review:

- incident evidence;
- AI root-cause summary;
- action kind;
- exact target ID/name;
- policy decision;
- required verification signals.

Approve only if the target and evidence are correct.

After approval, NoPager rechecks the Kill Switch and cooldown, sends one persisted action UUID to the helper at most once, then requires both:

- the configured Docker container to be running; and
- the independent public HTTPS health check to recover for the required consecutive successes.

If the helper is unavailable before send, execution fails closed. If a request may have reached the helper but the outcome is unknown, it is marked ambiguous. The durable helper journal prevents the same action UUID from causing a second restart. HTTP failure, a running-but-unhealthy container, replacement/disappearance, Docker outage, or timeout escalates without blind retry.

## Kill Switch

The Kill Switch blocks production mutations while monitoring continues.

From the console, use **Pause protection**, or from a local checkout:

```bash
cargo run -p nopager-cli -- pause
```

Resume only after you understand why production was paused:

```bash
cargo run -p nopager-cli -- resume
```

## Current boundary

The first production-operations user path currently proves one incident class: external service health failure with one bounded Docker-container restart.

It does **not** yet implement general:

- CPU/memory/disk/capacity remediation;
- Cloudflare WAF/rate-limit/bot actions;
- cloud instance scaling/failover;
- database failover/recovery;
- Kubernetes operations;
- arbitrary SSH/shell execution;
- broad network/security mutation.

Those are separate operations surfaces to add only after they have equivalent least-privilege policy, idempotency, verification, rollback/escalation and audit boundaries.

## Design-partner acceptance checklist

Before treating an installation as ready for a first real user, verify:

- [ ] operations quickstart completes without GitHub/Vercel values;
- [ ] console login works;
- [ ] overview shows the protected app and active health check;
- [ ] Safe Mode is enabled;
- [ ] Kill Switch pauses mutations and can be resumed;
- [ ] configured Docker target is a separate application container;
- [ ] worker has no Docker socket, Docker CLI, or Docker socket group;
- [ ] helper rejects unknown operations and unenrolled targets;
- [ ] a controlled health failure opens exactly one incident after the failure threshold;
- [ ] the incident shows a bounded AI operations plan;
- [ ] Safe Mode waits for approval rather than restarting automatically;
- [ ] approval restarts exactly the configured target once;
- [ ] healthy recovery is independently verified;
- [ ] failed verification escalates without a second blind restart;
- [ ] incident/audit history contains the policy, execution and verification record.

Use the formal [release guide](DESIGN_PARTNER_RELEASE.md), [security architecture](RUNTIME_HELPER_SECURITY.md), and [complete acceptance suite](DESIGN_PARTNER_ACCEPTANCE.md); this checklist alone is not a release sign-off.

The public demo for this path should tell the operations story directly:

> production healthy → real runtime failure → automatic trigger → AI investigation → bounded production action → verified recovery → return to monitoring

GitHub/Vercel should appear only when a deployment regression is genuinely the root cause.
