# Self-hosting NoPager

This guide is for the single-admin, single-app Design Partner Alpha. NoPager's default Docker Compose topology keeps the Rust API private on loopback and exposes the web console on loopback unless you deliberately change `NOPAGER_WEB_BIND`.

## First boot

```bash
git clone https://github.com/nopager/nopager.git
cd nopager
sh scripts/quickstart.sh
```

Quickstart creates `.env` if needed and generates three local secrets:

- `POSTGRES_PASSWORD`: URI-safe random PostgreSQL credential for new installs;
- `NOPAGER_MASTER_KEY`: 32-byte standard-base64 encryption key for stored integration credentials;
- `NOPAGER_ADMIN_TOKEN`: local operator/CLI bearer token.

It then detects the Docker socket group, starts the stack, and waits for both the API and web console to become ready.

Open `http://localhost:3000/setup` and complete the GitHub, Vercel, AI-provider, health-check, and safety-mode checks.

## Network boundary

The intended Alpha boundary is:

```text
Internet
   |
TLS reverse proxy
   |
NoPager web :3000
   |
private Docker network
   +-- NoPager API :8080
   +-- PostgreSQL :5432
   +-- Worker
```

Keep the Rust API private. The default host mapping for port 8080 is `127.0.0.1`; do not publish it to the Internet. Provider webhooks should target the web origin, which forwards only the required signed request data to the private API.

For remote console access:

1. put a trusted TLS reverse proxy in front of the web service;
2. set `NOPAGER_WEB_BIND=0.0.0.0` only if the proxy cannot reach a loopback bind;
3. set `NOPAGER_COOKIE_SECURE=true` when the browser uses HTTPS;
4. preserve the public `Host` header so same-origin mutation protection can compare it with the browser `Origin`;
5. rate-limit the login and first-admin POST paths at the reverse proxy;
6. keep port 8080 private even when port 3000 is reachable by the proxy.

Relevant browser mutation paths include `/api/nopager/auth/login`, `/api/nopager/setup/admin`, production approvals, protection pause/resume, and safety-mode changes. Console API responses are explicitly marked private/no-store.

## Secrets and backups

Treat `.env` as part of the encrypted data backup, not as disposable configuration. In particular:

- losing `NOPAGER_MASTER_KEY` makes encrypted GitHub, Vercel, and model-provider credentials unrecoverable;
- changing `POSTGRES_PASSWORD` without changing the existing PostgreSQL role password will prevent NoPager from connecting;
- rotating `NOPAGER_ADMIN_TOKEN` invalidates only CLI/operator bearer access after the services restart; browser sessions use separate random session tokens.

Recommended minimum backup set:

```text
.env
PostgreSQL Docker volume
```

Store backups encrypted and outside the Docker host. Do not copy live provider secrets into issue reports or logs.

## Upgrading an Alpha install

Before upgrading:

```bash
docker compose ps
docker compose logs --tail=100 server worker web
```

Back up `.env` and PostgreSQL, pull the new revision, then rerun:

```bash
sh scripts/quickstart.sh
```

Quickstart preserves existing non-empty secrets. It also recognizes the earlier Alpha Compose `DATABASE_URL=postgresql://nopager:nopager@postgres:...` form and keeps the legacy `nopager` database password instead of silently generating an incompatible password for an existing volume. New installations receive a random database password.

After an upgrade:

```bash
cargo run -p nopager-cli -- doctor
cargo run -p nopager-cli -- status
```

## Day-to-day operator commands

The CLI reads `.env` automatically:

```bash
cargo run -p nopager-cli -- doctor
cargo run -p nopager-cli -- status
cargo run -p nopager-cli -- incidents
cargo run -p nopager-cli -- pause
cargo run -p nopager-cli -- resume
cargo run -p nopager-cli -- logs
```

`pause` is the Kill Switch. It blocks mutating repair/deployment work while read-only monitoring continues. Resuming does not blindly continue stale mutation state; paused incidents are re-entered through fresh context collection.

## Recovery checklist

If NoPager itself is unhealthy:

1. `docker compose ps`;
2. `docker compose logs --tail=200 server worker web postgres`;
3. confirm `.env` still contains `POSTGRES_PASSWORD`, `NOPAGER_MASTER_KEY`, and `NOPAGER_ADMIN_TOKEN`;
4. verify `http://127.0.0.1:8080/healthz` and `/readyz` from the host;
5. run `cargo run -p nopager-cli -- doctor`;
6. if production risk is unclear, use `cargo run -p nopager-cli -- pause` before troubleshooting the repair pipeline.

Do not delete the PostgreSQL volume or replace `NOPAGER_MASTER_KEY` as a troubleshooting shortcut.

## Security assumptions

The worker is trusted and needs Docker daemon access to create isolated repair containers. Repair containers do not receive the Docker socket or provider credentials, run non-root, drop Linux capabilities, use a read-only root filesystem, and receive resource limits. Compromise of the trusted worker or Docker daemon remains a host-level security event; isolate the NoPager host accordingly.

Health checks accept public HTTPS targets only and reject local/private/reserved address resolution to reduce SSRF risk. High-risk repository and infrastructure paths are deterministically blocked from automatic AI repair.

## Alpha scope

This runbook does not turn the Design Partner Alpha into a claim of broad production readiness. Before relying on autonomous remediation, complete the real GitHub + Vercel scenarios in [`DESIGN_PARTNER_ALPHA.md`](DESIGN_PARTNER_ALPHA.md), including Preview verification, production approval, failed-production rollback, and Kill Switch behavior.