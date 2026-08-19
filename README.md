# NoPager

**Your app breaks. You don't get paged.**

NoPager is an open-source, agentless, BYOK AI on-call engineer for small production web teams. It detects incidents, gathers GitHub and Vercel evidence, proposes a narrowly scoped repair, validates it in an isolated Docker sandbox, creates a PR and Preview, and applies the configured production safety policy.

The v0.1 Alpha supports one self-hosted administrator, one protected web app, GitHub, Vercel, and one OpenAI, Anthropic, or Gemini API key.

> **Design Partner Alpha:** the project is intentionally scope-frozen around proving one safe GitHub → Vercel repair loop with real production-like incidents. See [the Alpha acceptance plan](docs/DESIGN_PARTNER_ALPHA.md). This is not yet a claim of broad production readiness.

## Who NoPager is for

NoPager is for production software teams where founders and developers still carry on-call responsibility because a dedicated 24/7 SRE function is too expensive or unjustified.

The goal is to make trustworthy production maintenance available before a team can justify hiring an SRE team—not to sell the cheapest possible model tokens. The open-source Alpha is self-hosted and BYOK so operators keep their existing GitHub, Vercel, and model-provider accounts while NoPager focuses on the response loop: context, repair, safety policy, verification, and rollback.

Long term, NoPager can become an autonomous production control plane, but it should orchestrate mature infrastructure rather than rebuild it. See [Product principles](docs/PRODUCT_PRINCIPLES.md).

## Quick start

Requirements: Docker Engine 26 or newer with Compose v2 and a public HTTPS production health URL. Engine 26 introduced the volume-subpath mount used to expose only one incident workspace to a repair container.

```bash
git clone https://github.com/nopager/nopager.git
cd nopager
sh scripts/quickstart.sh
```

The bootstrap script creates `.env` when needed, generates a random PostgreSQL password plus random 32-byte `NOPAGER_MASTER_KEY` and `NOPAGER_ADMIN_TOKEN` secrets, detects the Docker socket group on Linux, starts PostgreSQL/API/Worker/Web, and waits for the API plus web console to become ready. On a clean Git checkout it first tries to pull prebuilt amd64/arm64 application images for that exact Git commit from GitHub Container Registry; if those images are unavailable or the local source tree has been modified, it falls back to building locally. Existing Alpha installs keep their existing non-empty secrets, and the bootstrap recognizes the earlier fixed PostgreSQL credential so an existing volume is not silently made unbootable during upgrade.

Then open:

```text
http://localhost:3000/setup
```

The setup wizard creates the local administrator and validates GitHub, Vercel, the model provider, the production health URL, and the selected safety mode before it stores the protected app. GitHub repository ID/default branch and the canonical Vercel project metadata are discovered automatically.

For setup and operations, see:

- [Self-hosting and upgrade runbook](docs/SELF_HOSTING.md)
- [GitHub App setup](docs/GITHUB_APP_SETUP.md)
- [Vercel setup](docs/VERCEL_SETUP.md)

The default Compose configuration binds **both** the web console and Rust API to `127.0.0.1`, so the first-admin bootstrap is not exposed to the network by default. For remote use, terminate TLS at a trusted reverse proxy and expose the web console deliberately; keep port 8080 private. Set `NOPAGER_WEB_BIND=0.0.0.0` only when your reverse-proxy/network topology requires a non-loopback host bind, and set `NOPAGER_COOKIE_SECURE=true` whenever the console is served through HTTPS.

Console API responses are marked `private, no-store`; cross-origin browser mutations are rejected. The self-hosting runbook also calls out reverse-proxy rate limiting for login/bootstrap endpoints and the requirement to preserve the public `Host` header.

Local process checks remain available on the host at `http://127.0.0.1:8080/healthz` and `/readyz`.

The CLI automatically reads `.env`, so operator commands work without manually exporting the generated token:

```bash
cargo run -p nopager-cli -- doctor
cargo run -p nopager-cli -- status
cargo run -p nopager-cli -- incidents
cargo run -p nopager-cli -- pause
cargo run -p nopager-cli -- resume
```

`pause` is the Kill Switch: it blocks mutation actions while monitoring remains active.

## Safety model

Safe Mode is the default. NoPager may diagnose, repair, build, test, open a PR, deploy a Preview, and verify it automatically; a production promotion waits for explicit administrator approval.

Autopilot is experimental and only permits low-risk, verified, reversible promotion. Enabling it in the console requires an explicit confirmation. A missing or failed Preview verification is a hard production block, not something that human approval can bypass. High-risk changes—including dependency manifests, database schema, IAM, DNS, billing, and secrets—are escalated. The Kill Switch pauses mutations while retaining read-only monitoring and evidence collection; resuming protection restarts paused incidents from fresh context instead of continuing stale mutation state.

If production verification fails after a repair promotion, NoPager explicitly restores the previously recorded READY known-good Vercel deployment. It does not infer current traffic from Vercel's `target=production` field.

Repair execution uses a non-root, resource-limited, capability-dropped Docker container with a read-only root filesystem. Network access is disabled for build and test and is enabled only for recognized dependency-fetch commands. The trusted worker needs access to the Docker daemon; repair containers never receive the daemon socket or service credentials.

## Code privacy

NoPager's privacy boundary is **minimum necessary incident context**, not "upload the repository and trust the model provider."

The complete repository stays in the trusted self-hosted worker workspace for repair and validation. NoPager does **not** serialize the whole repository as a model prompt. External model calls receive bounded incident evidence such as verified recent GitHub diff context, stack traces, deployment metadata, and health evidence.

Before any structured incident input is sent to the selected BYOK model provider, the provider boundary deterministically redacts secret-bearing JSON fields, private-key blocks, common credential assignments/token formats, and credentials embedded in URLs. Verified GitHub diff evidence is redacted before it is preserved for the repair stage.

This is deliberately not marketed as "no source code ever leaves the machine": relevant code diffs can still be sent to the operator's selected OpenAI, Anthropic, or Gemini account. Provider retention/training terms are provider/account policy, not a cryptographic NoPager guarantee. Confidential inference/TEE and fully local or air-gapped inference are future high-assurance modes, not Alpha features.

**The model doesn't need your repository. It needs the evidence.**

See [Code privacy and model boundary](docs/PRIVACY.md) for the exact current guarantee.

## Incident lifecycle

```text
Detect → Collect Context → Diagnose → Repair → Build/Test
       → GitHub PR → Vercel Preview → Verify → Approval/Policy
       → Production → Watch → Resolve or Rollback
```

Jobs and incident transitions are durable and idempotency-keyed in PostgreSQL. A failed validation is supplied to a fresh repair attempt; after the configured repair-attempt limit NoPager escalates instead of looping.

Production deployment failures can arrive through Vercel webhooks when available, but NoPager also polls the selected Vercel project approximately every 30 seconds. The polling path makes webhook support optional and uses deployment IDs for incident deduplication.

## Architecture

- `apps/server`: Rust HTTP API, webhook verification, local authentication, and setup.
- `apps/worker`: Rust durable job processor and the end-to-end incident workflow.
- `apps/cli`: Rust self-hosting CLI (`init`, `doctor`, `status`, `protect`, `incidents`, `logs`, `pause`, `resume`).
- `apps/web`: Next.js operations console and the public signed-webhook proxy.
- `crates/*`: Rust domain, database, provider, connector, monitoring, policy, cryptography, webhook, and sandbox modules.
- PostgreSQL: durable configuration, incidents, audit events, deployments, attempts, and jobs.

See [the Rust-first architecture decision](docs/architecture/0001-rust-first.md).

## Webhooks

The public webhook URLs are on the same origin as the web console:

```text
https://YOUR_NOPAGER_HOST/api/webhooks/github
https://YOUR_NOPAGER_HOST/api/webhooks/vercel
```

The Next.js routes forward only the provider headers needed for signature validation plus the raw request body to the private Rust API. GitHub webhook verification is part of the standard Alpha setup. The Vercel webhook is optional because REST polling remains active as a fallback.

## Local development

Requirements: Rust 1.92, Node.js 22, pnpm 10, PostgreSQL 17, and Docker for sandbox execution.

```bash
pnpm install --frozen-lockfile
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

Run services individually with:

```bash
cargo run -p nopager-server
cargo run -p nopager-worker
pnpm --filter @nopager/web dev
```

Copy `.env.example` to `.env`; never commit the populated file. `nopager doctor` checks Docker/Compose, local secret configuration, and API/PostgreSQL readiness.

## Dogfood demo

[`examples/demo-next-app`](examples/demo-next-app) is a deliberately breakable Next.js target for the public Alpha demonstration. It provides a healthy endpoint, deterministic runtime 500, health-check and recent-regression modes, and a deterministic deployment build failure. Its README documents how to inject and restore each scenario before exercising the GitHub → Vercel repair loop.

For design-partner validation, use:

- [Design Partner Alpha acceptance plan](docs/DESIGN_PARTNER_ALPHA.md)
- [60–90 second demo runbook](docs/DEMO_RUNBOOK.md)

## Alpha limitations

- GitHub and Vercel are the only production connectors.
- One administrator and one protected app per OSS installation.
- Preview verification uses HTTP health checks; browser verification is planned after Alpha.
- Initial GitHub App/Vercel credentials are still entered manually, but repository/project metadata is discovered and tested by the wizard.
- External design-partner readiness still requires the real dogfood scenarios in the acceptance plan; CI alone is not treated as proof of safe production behavior.
- No Kubernetes, general observability backend, infrastructure provisioning, Team/RBAC/billing, or automatic high-risk database/IAM/DNS actions.

## Security and contributions

Read [SECURITY.md](SECURITY.md) and [Code privacy](docs/PRIVACY.md) before reporting a vulnerability, and [CONTRIBUTING.md](CONTRIBUTING.md) before opening a change. Never include production credentials, full logs containing secrets, or customer data in issues.

Back up `.env` together with PostgreSQL. Losing `NOPAGER_MASTER_KEY` makes encrypted integration credentials unrecoverable; do not rotate it casually as a troubleshooting step.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE). Formal commercial use should include counsel review as called out in the product baseline.
