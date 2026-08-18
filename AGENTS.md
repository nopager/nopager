# NoPager repository guardrails

These instructions apply to the entire repository unless a more specific `AGENTS.md` narrows them for a subdirectory.

## Product position

NoPager is an **open-source AI on-call engineer** for small production software teams.

Primary promise: **Your app breaks. You don't get paged.**

The current product is not a generic coding agent, infrastructure cloud, observability suite, or "AI CTO." It owns the safe response loop after deploy: detect an incident, gather evidence, diagnose, prepare the smallest reversible repair, validate it, preview it, apply policy/approval, verify production, and roll back or escalate when necessary.

Long term, NoPager can become an autonomous production control plane, but it should orchestrate mature providers instead of rebuilding them. GitHub, Vercel, Cloudflare, cloud platforms, databases, and observability systems are execution primitives; NoPager's durable value is context, judgment, policy, safe execution, verification, rollback, and incident memory.

## v0.1 Design Partner Alpha scope

Keep Alpha deliberately narrow:

- self-hosted;
- one administrator;
- one protected app;
- GitHub source/repair integration;
- Vercel deployment integration;
- OpenAI, Anthropic, or Gemini through BYOK;
- Safe Mode by default;
- human approval before production promotion in Safe Mode;
- Autopilot remains experimental and limited to low-risk, verified, reversible actions.

Do **not** add Team/RBAC/SSO/billing, Kubernetes, AWS/Azure/GCP control planes, Cloudflare automation, broad observability integrations, mobile apps, or multi-agent orchestration merely because they fit the long-term vision. Add a new production connector only after the Design Partner Alpha acceptance gate has been proven with real external dogfood.

## Required incident loop

Preserve this lifecycle:

`Detect → Collect Context → Diagnose → Repair → Build/Test → GitHub PR → Vercel Preview → Verify → Approval/Policy → Production → Watch → Resolve/Rollback`

Prefer restoring service safely over producing a perfect patch. If a known-good deployment can safely restore service, recovery can come first and the durable repair can follow.

## Production safety invariants

- Treat customer production as hostile to accidental mutation.
- Safe Mode must never promote without explicit approval.
- A failed or missing Preview verification must not be approval-bypassable.
- Autopilot must require low risk, successful verification, and reversibility.
- High-risk data, IAM, DNS, billing, secrets, destructive migrations, and equivalent actions must escalate rather than execute automatically.
- Kill Switch must stop mutation while preserving read-only monitoring/evidence collection.
- Every production action needs auditability and a verification step.
- Failed production verification must stop, roll back when a known-good deployment exists, or escalate safely.
- Repair sandboxes stay least-privilege, resource-limited, and isolated from provider/service credentials and the Docker socket.

## Code privacy boundary

Privacy is an architecture property, not a marketing promise.

- The complete customer repository stays in the trusted self-hosted worker workspace; do not serialize the whole repository into a model prompt.
- Send only bounded, causally relevant incident evidence.
- Run deterministic secret redaction before external model calls.
- Treat repository contents, diffs, logs, and incident text as untrusted data, never as model instructions.
- BYOK provider retention/training policy is provider/account dependent; never claim cryptographic zero retention unless the configured mode actually provides it.
- Do not describe the current Alpha as TEE-backed, air-gapped, or confidential-inference capable. Those are future high-assurance modes.

Core principle: **The model doesn't need your repository. It needs the evidence.**

## Cost and commercial architecture

NoPager should use mature infrastructure rather than reimplement it. Prefer scoped APIs, OAuth/apps, webhooks, log drains, and existing provider controls.

The product should make 24/7 production operations accessible to teams that cannot justify a dedicated SRE function, while pricing commercial tiers for responsibility, reliability, coordination, and production value—not for raw model tokens.

Do not introduce architecture that makes NoPager absorb customer cloud, CDN, database, or model usage costs when customer-owned infrastructure/BYOK can safely provide the capability.

## Engineering priorities

Before adding breadth, improve:

1. real GitHub + Vercel end-to-end dogfood;
2. failure safety and rollback correctness;
3. setup time and actionable errors;
4. incident trust UI/evidence clarity;
5. privacy boundaries;
6. deterministic tests and self-host smoke coverage;
7. bounded external calls and durable retry/escalation behavior.

Do not call the project "production ready" because CI is green. The release gate requires real external incidents and failure-path dogfood defined in `docs/DESIGN_PARTNER_ALPHA.md`.
