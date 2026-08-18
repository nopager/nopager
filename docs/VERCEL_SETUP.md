# Vercel setup

NoPager uses the Vercel REST API for deployment discovery, Preview creation, promotion, rollback, production-deployment polling, and Preview environment metadata checks.

## Access token

Create a Vercel access token with access to the account that owns the protected project. Keep it private and paste it only into the NoPager setup wizard.

For a **personal-account** project, leave **Team ID** empty.

For a **team-owned** project, enter the Team ID that owns the project and ensure the access token has access to that team.

In **Project ID or project name**, enter either value. NoPager resolves the project through Vercel and stores the canonical project ID and name automatically.

The token must be able to read the project's environment-variable metadata. NoPager does not request decrypted environment-variable values for the Preview safety check.

## Deployment detection

NoPager always runs a low-frequency REST API polling fallback for the protected Vercel project. The Alpha schedules one idempotent poll approximately every 30 seconds and only considers production deployments created after the integration was connected.

A new failed/canceled production deployment opens an incident using the deployment ID as the deduplication key. Successful production deployments are also recorded as deployment context.

This means a Vercel account webhook is **optional**. Polling is the default compatibility path for personal/Hobby-style setups and is also retained when a webhook is configured.

## Optional webhook

If your Vercel plan/account supports account webhooks, configure the public NoPager web URL:

```text
https://YOUR_NOPAGER_HOST/api/webhooks/vercel
```

Generate a webhook secret and enter the same value in the setup wizard. Webhooks reduce detection latency; polling remains enabled as a fallback, and the common deployment ID prevents duplicate incidents.

Do not point Vercel at the internal Rust API port. The public Next.js route forwards the raw signed request to the private API for verification.

## Preview environment safety

A NoPager repair Preview runs **AI-authored code**. Vercel Preview environment variables can therefore become visible to that code during build/runtime. Preview must never inherit production-privileged credentials merely because it is a non-production deployment.

Before creating a repair Preview, NoPager reads only Vercel environment-variable metadata and checks the variables effective for the generated repair branch. By default it refuses Preview creation when it detects obviously sensitive keys such as passwords, secrets, tokens, private keys, database/Redis URLs, connection strings, signing/encryption keys, credentials, or variables Vercel marks sensitive.

NoPager never uses this check as proof that a credential is harmless: variable names are only a conservative signal. If your application legitimately needs server-side Preview credentials, configure **separate non-production, least-privilege values** first. Examples include a disposable Preview database, test payment credentials, and service tokens that cannot mutate Production.

After reviewing the Preview environment, explicitly acknowledge the boundary in the self-host `.env` file:

```dotenv
NOPAGER_ALLOW_PREVIEW_SECRETS=true
```

Then restart the Worker:

```bash
docker compose up -d worker
```

This acknowledgement does not make Preview secrets safe. It records the operator decision that the effective Preview credentials have been reviewed and are acceptable for untrusted repair code. Do not set it merely to bypass an error.

NoPager explicitly requests Vercel's `preview` deployment target rather than relying on an omitted/default target.

## Protected Preview deployments

Vercel Deployment Protection can require authentication for Preview deployment URLs. NoPager must still be able to verify the exact Preview before a production promotion is allowed.

Vercel provides **Protection Bypass for Automation** for this use case. If your project has protected Preview deployments:

1. In Vercel, enable Protection Bypass for Automation for the protected project and generate a secret.
2. Put that secret in the self-host `.env` file:

```dotenv
VERCEL_AUTOMATION_BYPASS_SECRET=your-generated-vercel-secret
```

3. Restart the Worker:

```bash
docker compose up -d worker
```

NoPager sends Vercel's `x-vercel-protection-bypass` header only when the health-check target is a `*.vercel.app` deployment hostname and only from the Worker process. The secret is not injected into Server or Web by the default Compose topology, is never returned by the NoPager API, and is not sent to custom production domains.

The production health URL configured during setup must still pass publicly without this bypass. This keeps the ordinary production monitor independent from Vercel Preview authentication while allowing the repair Preview gate to work with protected deployments.

Treat this bypass secret like a deployment credential. Keep `.env` private, rotate the secret in Vercel if it is exposed, and restart the Worker after rotation.

## Production deployment requirement

During setup, NoPager verifies that the selected project is accessible and has a **READY production deployment**. The newest READY production deployment in the returned deployment history becomes the initial known-good rollback point; failed or canceled production attempts are never accepted as that baseline. If setup reports `vercel_production_deployment_not_found`, create a healthy production deployment first and retry.

After setup, later externally-created READY production deployments become eligible as the rollback target only after NoPager observes a successful production health check and no incident is active. This prevents a newly created deployment from becoming known-good based on provider state alone.

## Health URL

The production health check is configured separately from the Vercel generated deployment URL. It must be a public HTTPS endpoint that returns the expected healthy response without authentication. NoPager preserves its path/query when checking repair Preview deployments.

## Troubleshooting

- `vercel_project_not_accessible`: verify the token, Team ID scope, and project ID/name.
- `vercel_connection_failed`: verify token validity and account/team access.
- `vercel_production_deployment_not_found`: deploy the project to Production and wait until it is READY, then retry setup.
- Preview creation reports sensitive-looking environment variables: replace Production-grade Preview credentials with non-production/least-privilege values, review the effective Preview environment, then set `NOPAGER_ALLOW_PREVIEW_SECRETS=true` and restart the Worker.
- Preview environment metadata cannot be read: update the Vercel token/account permissions before relying on NoPager repair Preview.
- Preview health returns Vercel authentication/protection instead of your app: configure `VERCEL_AUTOMATION_BYPASS_SECRET` as described above and restart the Worker.
- Polling errors: inspect `docker compose logs -f worker` and confirm the access token still has access to the project.
