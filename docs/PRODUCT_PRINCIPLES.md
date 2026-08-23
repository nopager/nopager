# NoPager product principles

This document separates the durable product thesis from the deliberately narrow v0.1 Alpha scope.

## What NoPager is

NoPager is an **open-source, incident-triggered AI operations engineer**.

**Production breaks. NoPager wakes up — not you.**

NoPager is built for production software teams where founders and developers still carry operational responsibility because a dedicated 24/7 operations/SRE function is too expensive or unjustified.

The product outcome is not more alerts and not an AI chat window that waits for an operator to ask for help. The goal is to reduce human operational load by keeping lightweight monitoring active continuously and waking the AI decision/execution layer only when production evidence indicates that something needs attention.

The intended operating loop is:

```text
24/7 lightweight monitoring / provider events
        → incident trigger
        → wake AI operations reasoning
        → gather cross-system evidence
        → classify the cause
        → choose the safest available action
        → execute through existing provider APIs and tools
        → verify recovery
        → return to monitoring
```

NoPager should behave more like an on-call operations engineer than a coding assistant: it is **event-driven by production conditions**, not primarily driven by a human prompt.

## What NoPager is not

NoPager is not defined by GitHub or Vercel.

GitHub and Vercel are the first Alpha connectors because they provide a narrow, testable path for proving safe diagnosis, repair, verification, approval, rollback, and durable recovery. They are implementation surfaces for the first proof, not the product category.

NoPager is also not intended to become a heavy AI process installed on every protected server. The durable direction is a low-overhead control plane that uses provider events, health signals, scoped APIs, and existing infrastructure primitives wherever practical. Expensive model reasoning should be invoked when an incident requires it rather than running continuously without need.

## What the Alpha proves

The first proof is intentionally small:

> Can a stranger connect one real GitHub + Vercel app, let NoPager detect a supported incident, receive a tested repair and healthy Preview, approve the production action, and see the service recover without unsafe behavior?

Until that is repeatable, broader infrastructure automation is a distraction.

The Alpha therefore remains self-hosted, single-admin, single-app, GitHub + Vercel, BYOK, with Safe Mode as the default.

This Alpha scope must never be confused with the long-term product definition.

## Long-term category: Autonomous Production Operations

The long-term category is **Autonomous Production Operations**: a vendor-neutral AI operations control plane that can reason across reliability, security, capacity, cost, deployment, traffic, data, and recovery.

The durable promise is simple:

> Keep production safe and available without requiring a human to be the first responder for every incident.

NoPager should not rebuild mature infrastructure. It should orchestrate the best available primitives.

Potential execution and evidence surfaces include:

- Linux/cloud compute for process health, restart, capacity, failover, and infrastructure state;
- Cloudflare for edge security, WAF, rate limiting, bot/DDoS controls, cache, and traffic policy;
- databases for health, connection controls, backup, restore, replication, and failover where safe APIs exist;
- observability products for metrics, logs, traces, alerts, and evidence;
- deployment providers for deploy, rollback, promotion, and runtime state;
- GitHub and other source systems for code evidence, review, repair delivery, and durable source recovery;
- DNS, queues, storage, CDNs, and other production services through scoped provider APIs when the action can be made safe, reversible, and verifiable.

The durable NoPager layer is the cross-system production graph, causal context, decision engine, policy, safe execution, verification, rollback, and incident memory.

**We orchestrate. We don't reinvent.**

## Passive trigger, active response

The system should be quiet while production is healthy.

Continuous protection should rely on cheap signals and deterministic machinery where possible: provider events, webhooks, health checks, metrics, bounded polling, and rule-based thresholds. These mechanisms can run continuously without keeping a large model hot.

When a meaningful incident is detected, NoPager should wake the AI layer immediately, assemble the minimum necessary evidence, and begin incident reasoning and response.

Fast detection and fast response are product goals, but recovery time is bounded by the underlying infrastructure. A provider event may arrive in milliseconds or seconds, while a deployment, database failover, rollback, or health-verification window can take longer. NoPager should optimize every stage without making false claims that every incident can be resolved in milliseconds.

## Decision before action

NoPager should not mechanically map a metric to a mutation.

A traffic spike might be legitimate growth, abuse, a bot wave, a recent regression, a cache failure, a database bottleneck, or a third-party outage. The system should gather evidence and classify the situation before deciding whether to scale capacity, change edge controls, roll back code, repair software, fail over, restart a service, or simply observe.

The same principle applies to security and reliability signals: high CPU is not automatically a reason to add compute; elevated 5xx is not automatically a reason to roll back; unusual traffic is not automatically an attack.

This is why NoPager is more than an AI code fixer. It coordinates production actions using shared context and verifies whether the selected action actually restored the intended state.

## Operations work NoPager should absorb

The long-term product should take over as much routine and incident-driven operations work as can be made safe and auditable, including:

- continuous service and health monitoring;
- incident detection and deduplication;
- evidence collection and root-cause analysis;
- process/service restart and controlled recovery;
- deployment rollback and repair workflows;
- capacity and traffic actions;
- edge security controls such as WAF/rate-limit changes;
- cache and routing actions;
- database recovery/failover actions when supported by safe provider primitives;
- code repair when the root cause is software;
- post-action verification and rollback when the attempted remedy makes things worse;
- durable incident history so the same failure becomes easier to handle next time.

Human operators remain the safety authority for actions that are high-risk, irreversible, insufficiently understood, or outside configured policy.

## Privacy by architecture

Customer trust cannot depend on saying "the model won't steal your code."

The current principle is:

**The model doesn't need your repository. It needs the evidence.**

The full repository stays in the trusted self-hosted repair workspace in the current Alpha. External model calls receive bounded incident evidence after deterministic secret redaction. Relevant code diffs can still leave the host through the customer's selected BYOK model provider, so NoPager must describe that boundary precisely.

As NoPager expands beyond code repair, the same minimum-necessary-evidence principle applies to infrastructure data: do not send entire logs, account inventories, or unrelated production state to a model when a bounded incident context is sufficient.

Future high-assurance modes can add confidential inference/remote attestation and fully local or air-gapped inference, but those capabilities must not be advertised before they exist.

See [`PRIVACY.md`](PRIVACY.md).

## Affordable does not mean cheap-only

AI should reduce the marginal cost of production operations and make 24/7 maintenance available to developers and small teams that previously could not justify a dedicated operations/SRE function.

That does **not** mean NoPager should price every customer near model API cost. The paid product is responsibility, reliability, coordination, policy, auditability, recovery, and reduced human interruption.

Useful framing:

**24/7 production operations for teams that cannot justify a full-time operations/SRE team.**

Longer-term category language:

**Production operations for everyone.**

Commercial pricing should follow protected production value and complexity rather than raw token usage.

## Cost and integration architecture

Prefer customer-owned accounts and mature providers whenever practical:

- BYOK model APIs;
- customer cloud, CDN, source, deployment, database, and observability accounts;
- scoped credentials and least privilege;
- provider APIs, webhooks, and event streams rather than rebuilding commodity infrastructure;
- lightweight always-on monitoring, with model reasoning activated by meaningful incidents.

This lets NoPager expand capability without inheriting every customer's compute, CDN, database, and model bill, and reduces the need to run heavyweight AI processes inside the protected application servers.

## Trust before autonomy

Production autonomy is earned incrementally.

The default progression is:

1. monitor and gather evidence;
2. diagnose and recommend;
3. prepare/test/verify actions automatically;
4. require approval for production mutations;
5. allow only predefined low-risk, reversible, verifiable actions automatically;
6. expand autonomous operations only after real incident evidence demonstrates safety.

The product should be aggressive about reducing human toil and conservative about irreversible production risk.
