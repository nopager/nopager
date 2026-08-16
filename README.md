# NoPager

**Your app breaks. You don't get paged.**

NoPager is an open-source, agentless, BYOK AI on-call engineer for small production web teams. It detects incidents, gathers GitHub and Vercel evidence, proposes a narrowly scoped repair, validates it in an isolated Docker sandbox, creates a PR and Preview, and applies the configured production safety policy.

The v0.1 Alpha supports one self-hosted administrator, one protected web app, GitHub, Vercel, and one OpenAI, Anthropic, or Gemini API key.

## Quick start

Requirements: Docker Engine 26 or newer with Compose v2 and a public HTTPS production health URL. Engine 26 introduced the volume-subpath mount used to expose only one incident workspace to a repair container.

```bash
git clone https://github.com/nopager/nopager.git
cd nopager
cp .env.example .env
```

Generate a 32-byte base64 master key and place it in `.env` as `NOPAGER_MASTER_KEY`. The Rust CLI can do this without overwriting an existing file:

```bash
cargo run -p nopager-cli -- init
```

Then start the stack:

```bash
docker compose up -d --build
```

On Linux, set `DOCKER_GID` in `.env` to the group ID that owns `/var/run/docker.sock` so the non-root Worker can reach the daemon.

Open [http://localhost:3000/setup](http://localhost:3000/setup). The wizard creates the local administrator and connects GitHub, Vercel, the model provider, the production health URL, and Safe Mode. Credentials entered in setup are encrypted with XChaCha20-Poly1305 before PostgreSQL persistence.

When the console is served behind HTTPS, set `NOPAGER_COOKIE_SECURE=true`. Terminate TLS at a trusted reverse proxy and expose only the web console plus the two webhook routes required by your integrations.

The API is available at `http://localhost:8080`; `/healthz` reports process health and `/readyz` verifies PostgreSQL readiness.

## Safety model

Safe Mode is the default. NoPager may diagnose, repair, build, test, open a PR, deploy a Preview, and verify it automatically; a production promotion waits for explicit administrator approval.

Autopilot is experimental and only permits low-risk, verified, reversible promotion. High-risk changes—including dependency manifests, database schema, IAM, DNS, billing, and secrets—are escalated. The Kill Switch pauses mutations while retaining read-only health monitoring and evidence collection.

Repair execution uses a non-root, resource-limited, capability-dropped Docker container with a read-only root filesystem. Network access is disabled for build and test and is enabled only for recognized dependency-fetch commands. The trusted worker needs access to the Docker daemon; repair containers never receive the daemon socket or service credentials.

## Incident lifecycle

```text
Detect → Collect Context → Diagnose → Repair → Build/Test
       → GitHub PR → Vercel Preview → Verify → Approval/Policy
       → Production → Watch → Resolve or Rollback
```

Jobs and incident transitions are durable and idempotency-keyed in PostgreSQL. A failed validation is supplied to a fresh repair attempt; after three failed patches NoPager escalates instead of looping.

## Architecture

- `apps/server`: Rust HTTP API, webhook verification, local authentication, and setup.
- `apps/worker`: Rust durable job processor and the end-to-end incident workflow.
- `apps/cli`: Rust self-hosting CLI (`init`, `doctor`, `status`, `protect`, `incidents`, `logs`, `pause`, `resume`).
- `apps/web`: Next.js operations console implementing Overview, Incidents, Incident Detail, Integrations, AI Provider, and Safety & Policy.
- `crates/*`: Rust domain, database, provider, connector, monitoring, policy, cryptography, webhook, and sandbox modules.
- PostgreSQL: durable configuration, incidents, audit events, deployments, attempts, and jobs.

See [the Rust-first architecture decision](docs/architecture/0001-rust-first.md).

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

Copy `.env.example` to `.env`; never commit the populated file. `nopager doctor` checks local dependencies, configuration, and API reachability.

## Webhooks

Configure the GitHub App webhook to `/api/v1/integrations/github/webhook` and the Vercel webhook to `/api/v1/integrations/vercel/webhook` on the externally reachable API origin. Both endpoints authenticate the raw request body before persisting a deduplicated delivery.

## Dogfood demo

[`examples/demo-next-app`](examples/demo-next-app) is a deliberately breakable Next.js target for the public Alpha demonstration. It provides a healthy endpoint, deterministic runtime 500, health-check and recent-regression modes, and a deterministic deployment build failure. Its README documents how to inject and restore each scenario before exercising the GitHub → Vercel repair loop.

## Alpha limitations

- GitHub and Vercel are the only production connectors.
- One administrator and one protected app per OSS installation.
- Preview verification uses HTTP health checks; browser verification is planned after Alpha.
- No Kubernetes, general observability backend, infrastructure provisioning, or automatic high-risk database/IAM/DNS actions.

## Security and contributions

Read [SECURITY.md](SECURITY.md) before reporting a vulnerability and [CONTRIBUTING.md](CONTRIBUTING.md) before opening a change. Never include production credentials, full logs containing secrets, or customer data in issues.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE). Formal commercial use should include counsel review as called out in the product baseline.
