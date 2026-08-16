# Design Partner Alpha changelog

This branch narrows NoPager around the first design-partner proof instead of adding breadth.

## Product behavior

- Preview verification is now a hard production gate. A failed or missing Preview cannot be overridden by approval.
- Irreversible low-risk repairs can never Autopilot; they require explicit approval.
- Incident Detail now starts with a result-language summary so the operator immediately sees whether NoPager is working, waiting for approval, verifying production, rolling back, resolved, or escalated.

## Validation and sales enablement

- Added an explicit Design Partner Alpha acceptance gate covering one protected app, GitHub, Vercel, BYOK, Safe Mode, three supported incident scenarios, and failure-safety behavior.
- Added a 60–90 second demo runbook for customer outreach.
- README now states that this is a scope-frozen Design Partner Alpha rather than broad production readiness.

## Intentionally not included

No Team, multi-service coordination, RBAC, SSO, billing, additional cloud connectors, Kubernetes, or high-risk production mutation support is added in this branch.

## Remaining blockers before an external design partner

- Run the full GitHub → Vercel loop against a disposable real project for all three scenarios.
- Confirm production-verification failure rolls back and rollback verification is visible end to end.
- Time setup with a fresh user; manual GitHub/Vercel credential entry is still the largest onboarding friction.
- Record the first real 60–90 second demo only after the loop succeeds without manual intervention before the Safe Mode approval boundary.
