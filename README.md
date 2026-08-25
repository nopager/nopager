# NoPager

**Production breaks. NoPager wakes up — not you.**

NoPager is an open-source, incident-triggered **AI operations engineer** for small production teams.

It keeps cheap monitoring active while production is healthy. When a meaningful incident crosses a deterministic threshold, NoPager wakes the AI operations layer, gives it bounded evidence, chooses from explicitly configured actions, applies safety policy, executes through trusted connectors, verifies recovery, and then goes quiet again.

```text
24/7 lightweight monitoring
        → meaningful incident trigger
        → wake AI operations reasoning
        → collect bounded evidence
        → classify the cause
        → choose from configured actions
        → policy / approval
        → execute through trusted connectors
        → independently verify recovery
        → resolve or escalate
        → return to monitoring
```

NoPager is **not a GitHub or Vercel product**. GitHub + Vercel is an optional **deployment-recovery subsystem** for code/deployment incidents. The product category is **Autonomous Production Operations**.

## Current first production-operations Alpha

The first non-deployment design-partner path is now deliberately small and real:

```text
public HTTPS health check
        → 3 consecutive failures
        → one deduplicated incident
        → AI operations triage
        → bounded restart_container proposal
        → Safe Mode approval by default
        → restart exactly one configured Docker container
        → external HTTP + container-state verification
        → RESOLVED or ESCALATED without blind retry
```

This path does **not require GitHub or Vercel**.

It currently proves one production-operations incident class: a web service becomes unhealthy while its configured Docker runtime is available for a controlled restart. It is appropriate for first design partners who can run NoPager separately from the protected application and begin in Safe Mode.

The current path includes:

- one self-hosted administrator and one protected app per OSS installation;
- public HTTPS health monitoring with failure/recovery debouncing;
- BYOK OpenAI, Anthropic, or Gemini model provider;
- incident-triggered model use rather than continuous model reasoning;
- one immutable enrolled Docker target with a fixed trusted restart operation;
- a small host-side helper that alone holds Docker socket group authority;
- an ordinary worker with no Docker socket mount, Docker group, or Docker CLI;
- no model-generated shell command surface;
- target checks that refuse NoPager control-plane containers and the NoPager Compose project;
- deterministic policy plus Kill Switch;
- Safe Mode approval for the production restart;
- experimental Autopilot only for the currently supported low-risk bounded action;
- one persisted mutation record per incident;
- refusal to blindly repeat an ambiguous mutation;
- independent public-health and container-state verification;
- incident timeline, execution/verification record, and audit trail.

See [Production-operations quickstart](docs/OPERATIONS_QUICKSTART.md), [runtime-helper security architecture](docs/RUNTIME_HELPER_SECURITY.md), and [Design Partner release](docs/DESIGN_PARTNER_RELEASE.md).

## Operations-first quick start

Requirements:

- dedicated disposable/staging Linux host with systemd, Docker Engine 26+, Docker Compose v2, Python 3, and Rust/Cargo 1.92;
- a public HTTPS production URL and health URL;
- one application Docker container on the same Docker daemon NoPager can access;
- NoPager running in a separate Compose project from the protected app;
- one BYOK model provider key and exact supported model ID.

```bash
git clone https://github.com/nopager/nopager.git
cd nopager

export NOPAGER_AI_PROVIDER=openai
export NOPAGER_AI_MODEL='<exact-model-id>'
export OPENAI_API_KEY='<provider-key>'

sh scripts/operations-quickstart.sh
```

The operations quickstart fails closed before protection is persisted unless it can verify:

1. the NoPager control plane starts successfully;
2. the BYOK provider and selected model pass the capability probe;
3. the production and health URLs pass the public-HTTPS/SSRF boundary;
4. the health endpoint currently returns HTTP 200;
5. the Docker target exists;
6. the target is not NoPager itself or part of NoPager's Compose project;
7. the host helper independently enrolls the immutable target;
8. the worker stays running after helper preflight without Docker socket/CLI/group authority.

The provider key is sent to the local NoPager setup API, encrypted with `NOPAGER_MASTER_KEY`, and stored in PostgreSQL. The operations quickstart does not keep a second provider-key copy in worker `.env`. The immutable target and restart capability remain worker hard gates; the helper keeps a separate enrollment and protocol credential.

After setup, sign in through:

```text
http://localhost:3000/setup
```

For remote use, terminate TLS at a trusted reverse proxy, expose only the web console deliberately, and keep the Rust API port private.

## Safety model

**Safe Mode is the default and the recommended mode for first design partners.**

For the current Docker operations path, Safe Mode can monitor, open an incident, gather evidence, ask the model for a bounded operations decision, and prepare the persisted action automatically. It does **not** execute the production restart until an administrator approves it.

Approval is not a generic “trust the AI” button. NoPager rechecks the Kill Switch and persisted action state, advances the incident transactionally, writes the audit record, and enqueues exactly one execution job. After execution, recovery still has to pass independent verification.

If the restart outcome is ambiguous, or if health verification fails, NoPager escalates instead of issuing another blind restart.

`pause` is the Kill Switch. It blocks mutations while monitoring continues:

```bash
cargo run -p nopager-cli -- pause
cargo run -p nopager-cli -- resume
```

Experimental Autopilot exists, but first production-operations design partners should start in Safe Mode.

## Why the AI is incident-triggered

NoPager is not intended to run a heavyweight model process on every protected server or burn model tokens 24/7.

The always-on layer should be cheap and deterministic: health checks, provider events, metrics, bounded polling, and thresholds. Expensive model reasoning starts only after evidence says an incident needs attention.

The current self-hosted control plane has its own API, worker, web console, and PostgreSQL. Its ordinary containers have no Docker daemon access. A small host-side helper holds the narrow inspect/restart boundary. The protected application does not need a model process or heavyweight agent.

## Deployment recovery is a subsystem

NoPager also contains a separate GitHub → Vercel deployment-recovery subsystem. That path can collect deployment/source evidence, diagnose a regression, create and validate a repair, open a GitHub PR, use Vercel Preview/Production, verify health, and follow its own approval/rollback/source-recovery rules.

That is **deployment operations**. It must not be confused with proof that NoPager can already perform every server, network, security, database, or capacity operation.

Deployment-recovery references:

- [Design Partner Alpha acceptance plan](docs/DESIGN_PARTNER_ALPHA.md)
- [Alpha release gate](docs/ALPHA_RELEASE_GATE.md)
- [GitHub App setup](docs/GITHUB_APP_SETUP.md)
- [Vercel setup](docs/VERCEL_SETUP.md)
- [Real-provider deployment dogfood](https://github.com/nopager/nopager/issues/55)

## Where NoPager is going

The long-term product should orchestrate mature infrastructure rather than reinvent it. Future evidence/action surfaces can include:

- Linux and cloud compute — service health, capacity, restart, failover, instance state;
- Cloudflare — traffic analytics, WAF, rate limits, bot/DDoS controls, cache and traffic policy;
- databases — health, connections, replication, backup/restore and failover where safe APIs exist;
- observability systems — metrics, logs, traces and alert evidence;
- deployment providers and source systems — deploy, rollback, repair delivery and durable source recovery;
- DNS, queues, storage and CDN services through scoped provider APIs.

A traffic spike should not mechanically trigger scaling or blocking. It could be legitimate growth, abuse, a cache failure, an application regression, a database bottleneck, or a third-party outage. NoPager's job is to gather evidence first and choose the safest available action only when the evidence justifies it.

**We orchestrate. We don't reinvent.**

See [Product principles](docs/PRODUCT_PRINCIPLES.md) and [the first real operations MVP](https://github.com/nopager/nopager/issues/61).

## Current monitoring and triggers

The production-operations Alpha uses public HTTPS health checks as its first cheap always-on signal. Three consecutive failures are required before an incident opens; recovery also requires consecutive successes. This avoids waking the AI for a single noisy sample.

The deployment-recovery subsystem separately receives Vercel events when available and can use bounded Vercel polling. That deployment polling cadence is not the latency benchmark for the broader operations product.

Future server/edge/cloud connectors should prefer provider-native events and metrics when they provide better signal and lower detection latency.

## Code and infrastructure privacy

NoPager's privacy boundary is **minimum necessary incident context**.

**The model doesn't need your account. It needs the evidence.**

External model calls receive bounded incident evidence. Before structured input crosses the provider boundary, NoPager deterministically redacts secret-bearing JSON fields, private-key blocks, common credential assignments/token formats, and credentials embedded in URLs.

For deployment repair, the complete repository remains in the trusted self-hosted worker workspace; NoPager does not serialize the whole repository into a model prompt. The same minimum-evidence rule applies as operations expands into logs, metrics, cloud state, and security evidence.

See [Code privacy and model boundary](docs/PRIVACY.md) and [exact provider evidence/accounting](docs/MODEL_EVIDENCE_BOUNDARY.md).

## Architecture

- `apps/server` — Rust API, local authentication, setup, webhook verification and production-control endpoints.
- `apps/worker` — durable jobs, health monitoring, operations reasoning/execution, deployment recovery and verification.
- `apps/runtime-helper` — Linux host TCB exposing only typed inspect/restart for one enrolled target.
- `apps/cli` — self-hosting/operator CLI.
- `apps/web` — operations console.
- `crates/nopager-connectors` — external connectors and the unprivileged runtime-helper IPC client.
- `crates/nopager-runtime-protocol` — closed, versioned helper request/response types.
- `crates/nopager-policy` — deterministic production-action policy.
- `crates/nopager-monitor` — health checks and signal safety.
- PostgreSQL — durable configuration, incidents, actions, audit events and jobs.

See [the Rust-first architecture decision](docs/architecture/0001-rust-first.md).

## Current limitations

The code that exists today is narrower than the long-term product:

- the first general production-operations action is one configured Docker-container restart;
- production-operations evidence is currently centered on public HTTP health plus Docker runtime state;
- one administrator and one protected app per OSS installation;
- first operations design partners need the protected container on the same daemon as the separately installed host helper;
- Cloudflare automation is not implemented yet;
- general CPU/memory/disk/capacity remediation is not implemented yet;
- general cloud instance scaling/failover is not implemented yet;
- database operations/failover are not implemented yet;
- Kubernetes and arbitrary SSH/shell execution are not implemented;
- Team/RBAC/billing and broad high-risk IAM/DNS mutations are not current features.

These limitations describe current implementation truth. They do not define NoPager's product category.

## Self-hosting, security, and contributions

- [Self-hosting and upgrade runbook](docs/SELF_HOSTING.md)
- [Production-operations quickstart](docs/OPERATIONS_QUICKSTART.md)
- [Security policy](SECURITY.md)
- [Privacy boundary](docs/PRIVACY.md)
- [Contributing](CONTRIBUTING.md)

Back up `.env` together with PostgreSQL. Losing `NOPAGER_MASTER_KEY` makes encrypted integration credentials unrecoverable.

Never include production credentials, full logs containing secrets, or customer data in public issues.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
