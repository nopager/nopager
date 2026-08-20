# Vercel setup

NoPager uses the Vercel REST API for project/source verification, deployment discovery, Preview creation, promotion, rollback, production-deployment polling, and Preview environment metadata checks.

## Access token

Create a Vercel access token with access to the account that owns the protected project. Keep it private and paste it only into the NoPager setup wizard.

For a **personal-account** project, leave **Team ID** empty.

For a **team-owned** project, enter the Team ID that owns the project and ensure the access token has access to that team.

In **Project ID or project name**, enter either value. NoPager resolves the project through Vercel and stores the canonical project ID and name automatically.

The token must be able to read the project's environment-variable metadata. NoPager does not request decrypted environment-variable values for the Preview safety check.

## Git source compatibility

The selected Vercel project must be durably connected to the same GitHub repository that NoPager protects. Setup fails closed when NoPager cannot prove that source identity.

NoPager verifies:

- the Vercel project uses a supported GitHub/GitHub Limited link;
- the linked GitHub owner matches the protected repository owner;
- repository name and/or repository ID prove the same repository identity;
- the Vercel project has an explicit Production Branch;
- that Production Branch matches the protected GitHub default branch.

The setup wizard performs this check during the Vercel connection step for early feedback and repeats it with fresh provider data immediately before **Protect App**. Browser-supplied identity is never the final security boundary.

Common source-compatibility errors:

- `vercel_github_link_required`: connect the Vercel project to the protected GitHub repository using Vercel's Git integration.
- `vercel_github_repository_unverifiable`: Vercel did not expose enough repository identity to prove the link safely.
- `vercel_github_repository_mismatch`: the Vercel project is linked to a different GitHub repository.
- `vercel_production_branch_missing`: configure an explicit Production Branch in Vercel.
- `vercel_production_branch_mismatch`: make the Vercel Production Branch match the protected GitHub default branch before protecting the app.

NoPager does not guess a missing Production Branch as `main`.

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

## Authoritative current Production and rollback baseline

NoPager does **not** treat an arbitrary deployment with `target=production` as proof that it is currently serving Production traffic. Historical, staged, or previously promoted Production deployments are not interchangeable with Vercel's authoritative current Production target.

During setup, NoPager reads the project's authoritative Production target and requires a concrete current deployment identity. That deployment must be Production and `READY`; when Vercel explicitly reports a non-current/staged substate such as staged or rolling, NoPager fails closed. A provider response that omits optional current-substate metadata is accepted only when the project target itself proves current Production identity and the deployment is `READY`.

That exact current deployment becomes the initial known-good rollback baseline. NoPager does not choose the newest READY item from deployment history merely because it is labeled Production.

After setup, a new deployment is promoted to the known-good baseline only after the corresponding durable GitHub repair has become authoritative current Vercel Production and production health has passed the verification window. The temporary promoted repair does not overwrite the pre-incident rollback baseline before durable closure.

If production verification fails after source has already been merged, NoPager may restore traffic to the pre-incident known-good deployment, but the incident remains escalated until protected GitHub source is also recovered through a reviewed source-revert.

Before an automatic rollback mutation, NoPager re-reads authoritative current Production. If a third-party or human deployment has taken over, it refuses the rollback rather than overwriting that external deployment.

## Health URL

The production health check is configured separately from the Vercel generated deployment URL. It must be a public HTTPS endpoint that returns the expected healthy response without authentication. NoPager preserves its path/query when checking repair Preview deployments.

## Troubleshooting

- `vercel_project_not_accessible`: verify the token, Team ID scope, and project ID/name.
- `vercel_connection_failed`: verify token validity and account/team access.
- `vercel_github_link_required`: connect the project to GitHub through Vercel's supported Git integration.
- `vercel_github_repository_unverifiable`: inspect the Vercel Git link; NoPager could not prove the linked repository identity.
- `vercel_github_repository_mismatch`: select the Vercel project linked to the protected GitHub repository.
- `vercel_production_branch_missing`: configure an explicit Vercel Production Branch.
- `vercel_production_branch_mismatch`: align the Vercel Production Branch with the protected GitHub default branch.
- `vercel_production_deployment_not_found`: create or promote a healthy current Production deployment and retry setup.
- Preview creation reports sensitive-looking environment variables: replace Production-grade Preview credentials with non-production/least-privilege values, review the effective Preview environment, then set `NOPAGER_ALLOW_PREVIEW_SECRETS=true` and restart the Worker.
- Preview environment metadata cannot be read: update the Vercel token/account permissions before relying on NoPager repair Preview.
- Preview health returns Vercel authentication/protection instead of your app: configure `VERCEL_AUTOMATION_BYPASS_SECRET` as described above and restart the Worker.
- Polling errors: inspect `docker compose logs -f worker` and confirm the access token still has access to the project.

If Vercel provider state is ambiguous, do not bypass the setup or rollback gate. Fix the project/source configuration first.