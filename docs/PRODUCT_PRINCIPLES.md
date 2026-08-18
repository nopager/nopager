# NoPager product principles

This document separates the durable product thesis from the deliberately narrow v0.1 Alpha scope.

## What NoPager is

NoPager is an **open-source AI on-call engineer**.

**Your app breaks. You don't get paged.**

It is built for production software teams where founders and developers still carry operational responsibility because a dedicated 24/7 SRE function is too expensive or unjustified.

The customer outcome is not "cheaper tokens" or "more alerts." It is lower human on-call burden with a trustworthy recovery loop.

## What the Alpha proves

The first proof is intentionally small:

> Can a stranger connect one real GitHub + Vercel app, let NoPager detect a supported incident, receive a tested repair and healthy Preview, approve the production action, and see the service recover without unsafe behavior?

Until that is repeatable, broader infrastructure automation is a distraction.

The Alpha therefore remains self-hosted, single-admin, single-app, GitHub + Vercel, BYOK, with Safe Mode as the default.

## Long-term direction

The long-term category is **Autonomous Production Operations**: a vendor-neutral AI production control plane that can reason across reliability, security, capacity, cost, and recovery.

NoPager should not rebuild mature infrastructure. It should orchestrate the best available primitives.

Examples of future execution surfaces include:

- GitHub for source, review, and repair delivery;
- Vercel/cloud compute for deployments and capacity;
- Cloudflare for edge security, WAF, rate limiting, bot/DDoS controls, and traffic policy;
- databases for backup/failover/connection controls;
- observability products for telemetry and evidence.

The durable NoPager layer is the cross-system production graph, causal context, decision engine, policy, safe execution, verification, rollback, and incident memory.

**We orchestrate. We don't reinvent.**

## Decision before action

NoPager should not mechanically map a metric to a mutation.

A traffic spike might be legitimate growth, abuse, a bot wave, a recent regression, a cache failure, a database bottleneck, or a third-party outage. The system should gather evidence and classify the situation before deciding whether to scale capacity, change edge controls, roll back code, repair software, or simply observe.

This is the reason the long-term product is more than an AI code fixer: it coordinates production actions using shared context.

## Privacy by architecture

Customer trust cannot depend on saying "the model won't steal your code."

The current principle is:

**The model doesn't need your repository. It needs the evidence.**

The full repository stays in the trusted self-hosted repair workspace. External model calls receive bounded incident evidence after deterministic secret redaction. Relevant code diffs can still leave the host through the customer's selected BYOK model provider, so NoPager must describe that boundary precisely.

Future high-assurance modes can add confidential inference/remote attestation and fully local or air-gapped inference, but those capabilities must not be advertised before they exist.

See [`PRIVACY.md`](PRIVACY.md).

## Affordable does not mean cheap-only

AI should reduce the marginal cost of production operations and make 24/7 maintenance available to developers and small teams that previously could not justify a dedicated SRE function.

That does **not** mean NoPager should price every customer near model API cost. The paid product is responsibility, reliability, coordination, policy, auditability, recovery, and reduced human interruption.

A useful framing is:

**Production maintenance for teams that can't afford an SRE team.**

Longer-term category language can become:

**Production operations for everyone.**

Commercial pricing should follow protected production value and complexity rather than raw token usage.

## Cost architecture

Prefer customer-owned accounts and mature providers whenever practical:

- BYOK model APIs;
- customer GitHub/Vercel/cloud accounts;
- scoped credentials and least privilege;
- provider APIs rather than rebuilding commodity infrastructure.

This lets NoPager expand capability without inheriting every customer's compute, CDN, database, and model bill.

## Trust before autonomy

Production autonomy is earned incrementally.

The default progression is:

1. observe and diagnose;
2. prepare/test/preview automatically;
3. require approval for production;
4. allow only predefined low-risk, reversible, verifiable actions automatically;
5. expand autonomy only after real incident evidence demonstrates safety.

The product should be bold for the user and conservative toward production.
