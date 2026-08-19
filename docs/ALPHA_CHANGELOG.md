# Design Partner Alpha changelog

This branch narrows NoPager around the first design-partner proof instead of adding breadth.

## Product behavior

- Preview verification is a hard production gate. A failed or missing Preview cannot be overridden by approval.
- Irreversible low-risk repairs can never Autopilot; they require explicit approval.
- Incident Detail starts with a result-language summary so the operator immediately sees whether NoPager is working, waiting for approval, verifying production, rolling back, resolved, or escalated.
- The recommended GitHub onboarding path now uses GitHub App Manifest registration, then verifies the returned installation against the exact repository before storing the protected app. Manual App credentials remain a fallback for organizations that disallow manifest registration.
- Provider setup can discover the models available to the supplied OpenAI, Anthropic, or Gemini account and validates the selected model with a bounded structured-output capability probe.
- Production setup can safely probe common public HTTPS health endpoints before requiring a manual health URL.
- Model-bound incident context is locally redacted and constrained to bounded, causally relevant evidence; repair targets are constrained to verified source evidence before the model may propose a patch.
- Clean self-host installs first try exact-commit prebuilt application images and fall back to a local Docker build when those images are unavailable.

## Validation and sales enablement

- The repository-standard CI covers formatting, lint/Clippy, TypeScript, tests, production builds, quickstart syntax, and a full self-host smoke path.
- The self-host smoke verifies generated secrets, container hardening, network isolation, Rust setup API readiness, the web/API path, operator bearer auth, cross-origin mutation rejection, and first-admin bootstrap.
- The Design Partner Alpha acceptance gate covers one protected app, GitHub, Vercel, BYOK, Safe Mode, three supported incident scenarios, rollback, Kill Switch behavior, and failure-safety behavior.
- The setup acceptance path now reflects automatic GitHub App registration, provider model discovery/capability validation, and safe health-endpoint discovery instead of the earlier all-manual setup assumptions.
- Added a 60–90 second demo runbook for customer outreach.
- README states that this is a scope-frozen Design Partner Alpha rather than broad production readiness.

## Intentionally not included

No Team, multi-service coordination, RBAC, SSO, billing, additional cloud connectors, Kubernetes, or high-risk production mutation support is added in this branch.

## Remaining blockers before an external design partner

- Run the full GitHub → Vercel loop against a disposable real project for all three supported incident scenarios.
- Confirm production-verification failure rolls back and rollback verification is visible end to end against a real Vercel project.
- Run automatic GitHub App registration from the final public HTTPS NoPager console origin and verify the live workflow-run webhook delivers to that installation.
- Time setup with a fresh technical user. The remaining credential friction is primarily the BYOK Vercel access token and model-provider API key, not a pre-created GitHub App.
- Exercise at least one real supported model/account per provider family so account model discovery and the capability probe are proven against current provider APIs, not only fixtures.
- Record the first real 60–90 second demo only after the loop succeeds without manual intervention before the Safe Mode approval boundary.
