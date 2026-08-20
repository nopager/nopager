# Design Partner Alpha changelog

This Alpha deliberately narrows NoPager around the first design-partner proof instead of adding breadth.

## Product behavior

- Preview verification is a hard production gate. A failed or missing Preview cannot be overridden by approval.
- Irreversible low-risk repairs can never Autopilot; they require explicit approval.
- Incident Detail starts with a result-language summary so the operator immediately sees whether NoPager is working, waiting for approval, verifying production, rolling back, recovered-but-escalated, resolved, or escalated.
- The recommended GitHub onboarding path uses GitHub App Manifest registration, then verifies the returned installation against the exact repository before storing the protected app. Manual App credentials remain a fallback for organizations that disallow manifest registration.
- Setup verifies that the selected Vercel project is linked to the same GitHub repository and that its explicit Production Branch matches the protected GitHub default branch.
- Provider setup can discover the models available to the supplied OpenAI, Anthropic, or Gemini account and validates the selected model with a bounded structured-output capability probe.
- Production setup can safely probe common public HTTPS health endpoints before requiring a manual health URL.
- Model-bound incident context is locally redacted and constrained to bounded, causally relevant evidence; repair targets are constrained to verified source evidence before the model may propose a patch.
- Clean self-host installs first try exact-commit prebuilt application images and fall back to a local Docker build when those images are unavailable.
- Durable repair completion now requires the verified repair to land in protected GitHub source and the corresponding Git-driven Vercel deployment to become authoritative current Production before `RESOLVED`.
- Vercel current Production identity is derived from the authoritative Production target rather than assuming that any `target=production` deployment is live.
- NoPager refuses automatic rollback if an unrelated external Production deployment has taken over, avoiding overwriting human or third-party changes.
- The pre-incident known-good rollback baseline is preserved until a durable repair has fully passed Production verification.
- If Production fails after the repair has already been merged, NoPager can restore traffic to the previous known-good deployment but keeps the incident escalated while failed source remains.
- Source recovery creates or surfaces a draft source-revert for human review. NoPager does not automatically merge that revert.
- After the reviewed revert is merged, NoPager closes source recovery only after it re-verifies the exact PR identity, protected default-branch head, authoritative Vercel Production deployment, and production health. That closure is recorded as human-assisted rather than autonomous.

## Validation and sales enablement

- The repository-standard CI covers formatting, lint/Clippy, TypeScript, tests, production builds, quickstart syntax, and a full self-host smoke path.
- The self-host smoke verifies generated secrets, container hardening, network isolation, Rust setup API readiness, the web/API path, operator bearer auth, cross-origin mutation rejection, and first-admin bootstrap.
- The Design Partner Alpha acceptance gate covers one protected app, GitHub, Vercel, BYOK, Safe Mode, three supported incident scenarios, durable rollback/source recovery, Kill Switch behavior, and failure-safety behavior.
- The setup acceptance path reflects automatic GitHub App registration, provider model discovery/capability validation, safe health-endpoint discovery, and GitHub↔Vercel source compatibility validation.
- The 60–90 second demo runbook now requires durable GitHub + Git-driven Production closure instead of cutting directly from approval to a green status.
- The first-design-partner owner checklist explicitly separates traffic recovery from durable source recovery and keeps external customers in Safe Mode.
- GitHub issue #55 is the executable real-provider dogfood checklist. It is the remaining release gate; CI green alone is not treated as proof of safe production behavior.
- README states that this is a scope-frozen Design Partner Alpha rather than broad production readiness.

## Intentionally not included

No Team, multi-service coordination, RBAC, SSO, billing, additional cloud connectors, Kubernetes, or high-risk production mutation support is added before the Alpha is proven with real design partners.

## Remaining blockers before an external production trial

- Run the full GitHub → Vercel loop against a disposable real project for all three supported incident scenarios in GitHub issue #55.
- Force a failure after durable source merge and prove the complete traffic rollback → reviewed source-revert → GitHub/Vercel/health convergence path against real provider accounts.
- Confirm automatic rollback refuses to overwrite an unrelated newer Production deployment on real Vercel.
- Run automatic GitHub App registration from the final public HTTPS NoPager console origin and verify the live signed webhook delivers to that installation.
- Time setup with a fresh technical user. The remaining credential friction is primarily the BYOK Vercel access token and model-provider API key, not a pre-created GitHub App.
- Exercise at least one real supported model/account per provider family so account model discovery and the capability probe are proven against current provider APIs, not only fixtures.
- Record the first real 60–90 second demo only after the loop succeeds without manual intervention before the intended Safe Mode approval/review boundaries.

Do not call the Alpha broadly production-ready until these real-provider gates have passed repeatedly.
