# Design Partner Alpha

This document defines the release boundary for the first NoPager design partners.

The goal is not feature completeness. The goal is to prove one thing safely:

> A small production web team can connect one app, experience a real incident, and let NoPager diagnose, repair, validate, preview, and safely promote or stop the repair without requiring an on-call engineer to drive every step.

## Scope freeze

The Alpha supports exactly:

- one self-hosted administrator;
- one protected production web app;
- GitHub as the source repository;
- Vercel as preview and production deployment provider;
- one BYOK model provider: OpenAI, Anthropic, or Gemini;
- a public HTTPS health check;
- Safe Mode by default;
- experimental Autopilot for low-risk, verified, reversible changes only.

Do not add Team, multi-service coordination, RBAC, SSO, billing, AWS, Azure, GCP, Kubernetes, Cloudflare, Datadog, Sentry, infrastructure provisioning, or database/IAM/DNS automation before this Alpha is proven with real design partners.

## Alpha acceptance gate

A release candidate is ready for a design partner only when the following end-to-end path works against a real GitHub repository and real Vercel project:

1. Start NoPager with `docker compose up -d --build`.
2. Open `/setup` and create the local administrator.
3. Connect GitHub.
4. Connect Vercel.
5. Configure a BYOK model provider.
6. Configure the production URL and health URL.
7. Keep Safe Mode enabled and click **Protect App**.
8. Inject one of the supported production failures below.
9. NoPager opens exactly one deduplicated incident.
10. NoPager collects the recent commit/deployment context.
11. The model produces a root-cause diagnosis and narrowly scoped repair.
12. The repair is applied only inside the isolated workspace.
13. Build/test validation passes before a repair branch is trusted.
14. NoPager opens a repair PR.
15. NoPager creates a Vercel Preview from the repair commit.
16. Preview deployment reaches `READY` and the Preview health URL passes.
17. Safe Mode enters `WAITING_APPROVAL` instead of mutating production.
18. Incident Detail clearly shows the outcome, evidence, patch, validation, Preview, policy, and approval state.
19. Administrator approval promotes the verified deployment.
20. Production verification passes before the incident becomes `RESOLVED`.
21. Audit events and the complete incident timeline remain available afterwards.

If any mandatory safety gate fails, the flow must stop or roll back. It must never "best effort" its way into production.

## Three supported incident scenarios

### Scenario A — Vercel build/deployment failure

Trigger a deterministic build failure in `examples/demo-next-app` and push it through the connected repository.

Pass criteria:

- the failed deployment opens or enriches an incident;
- recent deployment and commit evidence are attached;
- NoPager produces a repair that restores a successful build;
- the repair branch/PR is created only after sandbox validation;
- the Vercel Preview is healthy before production approval is possible.

### Scenario B — recent commit causes runtime 500

Deploy the deterministic runtime regression from `examples/demo-next-app`.

Pass criteria:

- production health degradation is detected;
- the incident points at the recent regression rather than unrelated repository files;
- the repair removes the runtime failure;
- the Preview health check proves the repair before the approval state.

### Scenario C — production health check regression

Deploy the demo health-check regression without relying on a Vercel build error.

Pass criteria:

- health monitoring requires the configured failure threshold before opening the incident;
- the repair follows the same sandbox → PR → Preview → verification path;
- a recovered health endpoint does not create duplicate incidents;
- the incident resolves only after production verification.

## Failure-safety matrix

| Failure | Required behavior |
| --- | --- |
| Model proposes a high-risk action | Escalate. Do not patch or deploy. |
| Model patch cannot be applied cleanly | Fail the attempt. Do not create a trusted Preview. |
| Declared changed-file set differs from applied patch | Fail the attempt. |
| Dependency manifest changes | Escalate for human review in Alpha. |
| Build/test fails | Save validation evidence and retry with the failure context, up to the configured attempt limit. |
| Three repair attempts fail | Escalate. Stop automatic repair. |
| Preview deployment fails/cancels | Stop. Never permit production promotion. |
| Preview health check fails | Hard block production promotion. |
| Kill Switch is active | Block mutations while keeping read-only monitoring/evidence collection. |
| No known rollback target | Never Autopilot. Require explicit approval. |
| Production verification fails | Roll back to the latest known-good deployment and verify rollback. |
| Rollback verification fails | Escalate immediately and keep mutations stopped. |

## Setup usability gate

The product target is a first protected app within ten minutes for a technically competent SaaS founder who already has the required GitHub App/Vercel credentials.

Measure it with a fresh machine/profile. Do not mark the gate complete by developer familiarity alone.

Current Alpha caveat: GitHub App ID, Installation ID, repository ID, private key, webhook secret, Vercel project/team IDs, token, and webhook secret are still entered manually. This is acceptable for the first design partners, but it is the largest setup-friction item and should be replaced by guided OAuth/App installation after the core demand is proven.

## Design partner operating rule

Use Safe Mode for every external design partner until NoPager has repeatedly passed the three scenarios above and multiple real incidents without an unsafe promotion.

Autopilot is for controlled dogfood only during the first Alpha.

## Evidence to capture from every design partner

For every incident, record:

- trigger type;
- time to detect;
- time to diagnosis;
- repair attempts;
- whether root cause was correct;
- whether the generated patch was accepted unchanged;
- sandbox/build/test result;
- Preview result;
- whether production approval was granted;
- production verification result;
- rollback result when applicable;
- time to recovery;
- whether the owner had to intervene before the approval step;
- whether the owner would trust the same class of repair again.

The primary product metric is **Resolved without owner intervention before the production approval boundary**. The commercial learning metric is whether a real team would keep NoPager connected to production after the trial.
