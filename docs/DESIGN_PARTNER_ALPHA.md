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

1. Start NoPager with `sh scripts/quickstart.sh` and confirm the script reports the stack ready.
2. Open `/setup` and create the local administrator. For a production-like run, use the final public HTTPS NoPager console origin before starting automatic GitHub App registration.
3. Connect GitHub. The recommended path must complete GitHub App Manifest creation/installation without requiring the operator to manually copy an App private key. NoPager must then verify the exact repository and discover its repository ID and default branch. The documented manual GitHub App path remains a fallback.
4. Connect Vercel. Team ID is optional for personal projects; the project lookup must succeed.
5. Configure a BYOK model provider, load the models available to that account, select an exact model ID, and pass NoPager's bounded structured-output capability probe. Manual exact model entry remains a fallback when discovery is unavailable.
6. Configure the public HTTPS production URL, use safe health-endpoint discovery or enter the health URL manually, and pass the live health test.
7. Keep Safe Mode enabled and click **Protect App**.
8. Run each of the supported production-failure scenarios below on the dogfood app.
9. NoPager opens exactly one deduplicated incident for each injected failure.
10. NoPager collects recent commit/deployment context, including bounded verified GitHub text-diff evidence when available.
11. The model produces a root-cause diagnosis and a narrowly scoped repair grounded in that verified source evidence.
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
22. Separately force a production-verification failure and prove NoPager restores the recorded known-good deployment and verifies rollback.
23. Activate the Kill Switch during a controlled incident and prove read-only monitoring continues while mutating work remains blocked.

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

- **Verified GitHub source/diff context is unavailable:** Refuse to invent a repair patch and escalate/retry according to the incident path.
- **Model proposes a high-risk action:** Escalate. Do not patch or deploy.
- **Model patch touches a deterministic sensitive path:** Block it even if the model labels the repair low-risk.
- **Model patch cannot be applied cleanly:** Fail the attempt. Do not create a trusted Preview.
- **Declared changed-file set differs from applied patch:** Fail the attempt.
- **Dependency manifest changes:** Escalate for human review in Alpha.
- **Build/test fails:** Save validation evidence and retry with the failure context, up to the configured attempt limit.
- **Three repair attempts fail:** Escalate. Stop automatic repair.
- **Preview deployment fails/cancels:** Stop. Never permit production promotion.
- **Preview health check fails:** Hard block production promotion.
- **Kill Switch is active:** Block mutations while keeping read-only monitoring/evidence collection.
- **No known rollback target:** Never Autopilot. Require explicit approval.
- **Production verification fails:** Roll back to the latest known-good deployment and verify rollback.
- **Rollback verification fails:** Escalate immediately and keep mutations stopped.

## Setup usability gate

The product target is a first protected app within ten minutes for a technically competent SaaS founder who already has access to the target GitHub repository, a Vercel access token for the target project, and a supported model-provider API key. A pre-created GitHub App should not be required on the recommended path.

Measure it with a fresh machine/profile. Do not mark the gate complete by developer familiarity alone.

Current Alpha setup behavior: the recommended GitHub path uses GitHub App Manifest registration to generate the least-privilege App credentials and then guides the operator through repository-scoped installation. Production-like automatic setup must be started from the final public HTTPS NoPager console origin so the generated workflow-run webhook is deliverable; manual App ID/installation ID/private-key entry remains a fallback for organizations that disallow manifest registration. Vercel project ID/name and access token are entered manually, with Team ID optional for personal projects and the Vercel webhook secret optional because polling remains active. The model-provider API key is BYOK/manual, while the wizard can discover the models available to that provider account and validates the selected model. Repository ID/default branch, canonical Vercel project metadata, and common health endpoints are discovered by NoPager rather than typed manually.

The remaining setup work after Alpha should focus on reducing Vercel credential friction and proving the ten-minute target with fresh external users, not reintroducing a central credential broker.

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
