# Vercel setup

NoPager uses the Vercel REST API for deployment discovery, Preview creation, promotion, rollback, and production-deployment polling.

## Access token

Create a Vercel access token with access to the account that owns the protected project. Keep it private and paste it only into the NoPager setup wizard.

For a **personal-account** project, leave **Team ID** empty.

For a **team-owned** project, enter the Team ID that owns the project and ensure the access token has access to that team.

In **Project ID or project name**, enter either value. NoPager resolves the project through Vercel and stores the canonical project ID and name automatically.

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

## Production deployment requirement

During setup, NoPager verifies that the selected project is accessible and has a **READY production deployment**. The newest READY production deployment in the returned deployment history becomes the initial known-good rollback point; failed or canceled production attempts are never accepted as that baseline. If setup reports `vercel_production_deployment_not_found`, create a healthy production deployment first and retry.

## Health URL

The production health check is configured separately from the Vercel generated deployment URL. It must be a public HTTPS endpoint that returns the expected healthy response without authentication. NoPager preserves its path/query when checking repair Preview deployments.

## Troubleshooting

- `vercel_project_not_accessible`: verify the token, Team ID scope, and project ID/name.
- `vercel_connection_failed`: verify token validity and account/team access.
- `vercel_production_deployment_not_found`: deploy the project to Production and wait until it is READY, then retry setup.
- Polling errors: inspect `docker compose logs -f worker` and confirm the access token still has access to the project.
