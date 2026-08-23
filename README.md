# NoPager

**Production breaks. NoPager wakes up — not you.**

NoPager is an open-source, incident-triggered **AI operations engineer** for small production teams.

It is designed to keep lightweight monitoring active 24/7 and wake the AI reasoning/execution layer only when production evidence says something needs attention. The long-term goal is to absorb the routine and incident-driven work normally handled by an on-call operations/SRE engineer: detect, investigate, decide, act, verify, roll back when necessary, and keep watching.

NoPager is **not a GitHub or Vercel product**. GitHub + Vercel are the deliberately narrow first Alpha execution path used to prove one safe recovery loop with real provider behavior.

The intended operating model is:

```text
24/7 lightweight monitoring / provider events
        → incident trigger
        → wake AI operations reasoning
        → gather cross-system evidence
        → classify the cause
        → choose the safest available action
        → execute through existing provider APIs/tools
        → verify recovery
        → return to monitoring
```

The system should be quiet while production is healthy. Expensive model reasoning should not run continuously without need.

> **Design Partner Alpha:** v0.1 is intentionally scope-frozen around proving one safe GitHub → Vercel repair/recovery loop. This is the first validation surface, not the final product boundary. See [the Alpha acceptance plan](docs/DESIGN_PARTNER_ALPHA.md).

## What NoPager is trying to replace

Small teams often reach a point where the product has real users but there is still no dedicated 24/7 operations/SRE function. The founder or developer becomes the person who must notice outages, inspect logs and recent changes, decide whether to restart, roll back, patch, scale, block traffic, or fail over, then verify that production is actually healthy again.

NoPager's product thesis is that much of that operational work can be automated safely and far more cheaply than staffing a full-time on-call function.

The target outcome is:

- 24/7 production protection without a human watching dashboards all day;
- fast incident triggering from provider events, health checks, metrics, and other cheap signals;
- AI reasoning only when the situation requires it;
- safe use of existing infrastructure APIs instead of rebuilding every underlying system;
- automated verification after every action;
- human approval or escalation when the action is high-risk, irreversible, or insufficiently understood.

Fast detection and response are product goals. Recovery time still depends on the underlying infrastructure: an event may trigger quickly, while a deployment, failover, rollback, or verification window can take longer. NoPager should optimize each stage without claiming every incident can be resolved in milliseconds.

## Operations surfaces

NoPager should orchestrate mature infrastructure rather than reinvent it.

The long-term execution/evidence graph can include:

- **Linux / cloud compute** — service health, restart, capacity, failover, instance state;
- **Cloudflare** — WAF, rate limits, bot/DDoS controls, cache, traffic policy, edge security;
- **databases** — health, connections, backups, restore, replication, failover where safe APIs exist;
- **observability** — metrics, logs, traces, alerts, incident evidence;
- **deployment providers** — deploy, rollback, promotion, runtime state;
- **GitHub and other source systems** — code evidence, review, repair delivery, durable source recovery;
- **DNS, queues, storage, CDN and other production services** — through scoped provider APIs when an action can be made safe, reversible, and verifiable.

A traffic spike, for example, should not mechanically trigger scaling. It could be legitimate growth, abuse, a bot wave, a recent regression, a cache failure, a database bottleneck, or a third-party outage. NoPager should gather evidence first, then decide whether to scale, change edge controls, roll back code, repair software, fail over, restart, or simply observe.

**We orchestrate. We don't reinvent.**

See [Product principles](docs/PRODUCT_PRINCIPLES.md).

## Current v0.1 Alpha

The current Alpha is intentionally much narrower than the product vision. It supports:

- one self-hosted administrator;
- one protected web app;
- GitHub as the source/review surface;
- Vercel as Preview and Production deployment provider;
- one BYOK model provider: OpenAI, Anthropic, or Gemini;
- a public HTTPS health check;
- Safe Mode by default;
- experimental Autopilot only for low-risk, verified, reversible actions.

Today the concrete loop is:

```text
Detect → Collect Context → Diagnose → Repair → Build/Test
       → GitHub PR → Vercel Preview → Verify → Approval/Policy
       → Durable GitHub landing → Git-driven Production → Verify
       → Resolve
          or
       → Traffic rollback → Escalate while source remains unsafe
                         → Human-reviewed source revert
                         → Re-verify GitHub + Vercel + health → Resolve
```

This Alpha exists to prove the safety model before adding Cloudflare, cloud/server control, databases, broader observability, security actions, and other production execution surfaces.

## Why the AI is incident-triggered

NoPager is not intended to be a heavyweight AI agent burning model tokens 24/7 or consuming significant resources on every protected application server.

Continuous protection should use cheap deterministic mechanisms where possible: provider webhooks/events, health checks, metrics, bounded polling, and rule-based thresholds. When a meaningful incident is detected, NoPager wakes the AI layer, assembles the minimum necessary evidence, and starts incident reasoning.

The current Alpha is self-hosted and has its own API/worker/web/PostgreSQL stack, but the protected application does not need a large model process running inside it. The long-term architecture should increasingly rely on scoped external provider APIs and event surfaces so operators can connect existing infrastructure rather than install invasive AI runtimes across production hosts.

## Quick start

Requirements: Docker Engine 26 or newer with Compose v2 and a public HTTPS production health URL. Engine 26 introduced the volume-subpath mount used to expose only one incident workspace to a repair container.

```bash
git clone https://github.com/nopager/nopager.git
cd nopager
sh scripts/quickstart.sh
```

The bootstrap script creates `.env` when needed, generates a random PostgreSQL password plus random 32-byte `NOPAGER_MASTER_KEY` and `NOPAGER_ADMIN_TOKEN` secrets, detects the Docker socket group on Linux, starts PostgreSQL/API/Worker/Web, and waits for the API plus web console to become ready. On a clean Git checkout it first tries to pull prebuilt amd64/arm64 application images for that exact Git commit from GitHub Container Registry; if those images are unavailable or the local source tree has been modified, it falls back to building locally.

For a local evaluation, open:

```text
http://localhost:3000/setup
```

For a real externally reachable installation, put the trusted HTTPS reverse proxy in place first and open the setup wizard through the **final public console origin** before starting automatic GitHub App setup.

The setup wizard creates the local administrator and validates GitHub, Vercel, the model provider, the production health URL, and the selected safety mode before it stores the protected app.

For setup and operations, see:

- [Self-hosting and upgrade runbook](docs/SELF_HOSTING.md)
- [GitHub App setup](docs/GITHUB_APP_SETUP.md)
- [Vercel setup](docs/VERCEL_SETUP.md)
- [First design partner owner checklist](docs/ALPHA_OWNER_CHECKLIST.md)

The default Compose configuration binds both the web console and Rust API to `127.0.0.1`. For remote use, terminate TLS at a trusted reverse proxy and expose the web console deliberately; keep port 8080 private.

The CLI automatically reads `.env`:

```bash
cargo run -p nopager-cli -- doctor
cargo run -p nopager-cli -- status
cargo run -p nopager-cli -- incidents
cargo run -p nopager-cli -- pause
cargo run -p nopager-cli -- resume
```

`pause` is the Kill Switch: it blocks mutation actions while monitoring remains active.

## Safety model

Safe Mode is the default. In the current Alpha, NoPager may diagnose, repair, build, test, open a PR, deploy a Preview, and verify it automatically; a production promotion waits for explicit administrator approval.

Autopilot is experimental and only permits low-risk, verified, reversible promotion. A missing or failed Preview verification is a hard production block. High-risk changes—including dependency manifests, database schema, IAM, DNS, billing, and secrets—are escalated.

The same safety principle applies to future infrastructure operations: an AI action is not trusted because the model suggested it. It must be allowed by policy, narrowly scoped, reversible where practical, and followed by verification.

A repair is not considered durably resolved merely because traffic becomes healthy. In the current GitHub/Vercel Alpha, NoPager verifies source and authoritative Production convergence before `RESOLVED`.

Automatic rollback refuses to overwrite an unrelated external Production deployment that took over after the incident began.

## Code and infrastructure privacy

NoPager's privacy boundary is **minimum necessary incident context**.

The current principle is:

**The model doesn't need your repository. It needs the evidence.**

The complete repository stays in the trusted self-hosted worker workspace for repair and validation. NoPager does not serialize the whole repository as a model prompt. External model calls receive bounded incident evidence such as verified recent source diff context, stack traces, deployment metadata, and health evidence.

Before structured incident input is sent to the selected BYOK model provider, the provider boundary deterministically redacts secret-bearing JSON fields, private-key blocks, common credential assignments/token formats, and credentials embedded in URLs.

As NoPager expands into infrastructure operations, the same rule applies to logs, metrics, cloud state, and security evidence: send the minimum context necessary to reason about the incident, not the entire account or environment.

See [Code privacy and model boundary](docs/PRIVACY.md).

## Architecture

- `apps/server`: Rust HTTP API, webhook verification, local authentication, and setup.
- `apps/worker`: Rust durable job processor and the end-to-end incident workflow.
- `apps/cli`: Rust self-hosting CLI.
- `apps/web`: Next.js operations console and public signed-webhook proxy.
- `crates/*`: Rust domain, database, provider, connector, monitoring, policy, cryptography, webhook, and sandbox modules.
- PostgreSQL: durable configuration, incidents, audit events, deployments, attempts, and jobs.

See [the Rust-first architecture decision](docs/architecture/0001-rust-first.md).

## Current monitoring and triggers

Production deployment failures can arrive through Vercel webhooks when available. The current Alpha also polls the selected Vercel project approximately every 30 seconds and uses deployment IDs for incident deduplication.

This is an Alpha implementation detail, not the latency target for the mature product. Future provider integrations should prefer event-driven triggers wherever reliable provider events exist, with bounded polling as a fallback.

## Dogfood demo

[`examples/demo-next-app`](examples/demo-next-app) is a deliberately breakable Next.js target for the public Alpha demonstration. It provides a healthy endpoint, deterministic runtime 500, health-check and recent-regression modes, and a deterministic deployment build failure.

For design-partner validation, use:

- [Design Partner Alpha acceptance plan](docs/DESIGN_PARTNER_ALPHA.md)
- [Alpha release gate](docs/ALPHA_RELEASE_GATE.md)
- [First design partner owner checklist](docs/ALPHA_OWNER_CHECKLIST.md)
- [60–90 second demo runbook](docs/DEMO_RUNBOOK.md)
- [Real-provider dogfood checklist](https://github.com/nopager/nopager/issues/55)

The real-provider dogfood checklist is the remaining Alpha release gate. Do not treat repository CI or a local demo as proof that external provider behavior has been proven.

## Alpha limitations

- GitHub and Vercel are the only current production connectors.
- One administrator and one protected app per OSS installation.
- Preview verification uses HTTP health checks.
- External design-partner readiness still requires the real dogfood scenarios and durable rollback/source-recovery proof in the acceptance plan.
- External design partners should remain in Safe Mode for the Alpha.
- Cloudflare, general Linux/cloud operations, Kubernetes, broad observability backends, database operations, Team/RBAC/billing, and automatic high-risk IAM/DNS actions are not current Alpha features.

These limitations describe v0.1. They do not define NoPager's long-term category.

## Security and contributions

Read [SECURITY.md](SECURITY.md) and [Code privacy](docs/PRIVACY.md) before reporting a vulnerability, and [CONTRIBUTING.md](CONTRIBUTING.md) before opening a change. Never include production credentials, full logs containing secrets, or customer data in issues.

Back up `.env` together with PostgreSQL. Losing `NOPAGER_MASTER_KEY` makes encrypted integration credentials unrecoverable.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
