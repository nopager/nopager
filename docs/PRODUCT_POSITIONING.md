# NoPager canonical product positioning

This document is the product-definition source of truth for public copy, demos, roadmap decisions, issues, and architecture discussions.

If another document, post, demo, or issue conflicts with this file, fix the conflicting material instead of changing the product into a deployment tool.

## One-sentence definition

**NoPager is an incident-triggered AI operations system that keeps production under low-overhead 24/7 protection, wakes AI only when something meaningful goes wrong, uses the connected infrastructure to recover safely, verifies the result, and then returns to monitoring.**

Short public version:

> **Production breaks. NoPager wakes up — not you.**

## What job NoPager replaces

NoPager targets the operational work normally carried by an on-call operations/SRE/DevOps person after a product is already running in production.

That work includes, where safe automation and provider capabilities allow it:

- continuous production health monitoring;
- incident detection and deduplication;
- gathering metrics, logs, health, traffic, deployment, host and dependency evidence;
- root-cause classification;
- restarting or recovering unhealthy services;
- cloud/VM capacity and instance actions;
- failover and traffic recovery;
- Cloudflare WAF, rate-limit, bot/DDoS, cache and traffic controls;
- deployment rollback when deployment is actually the cause;
- code repair when software is actually the cause;
- database recovery/failover actions when safe provider primitives exist;
- post-action verification;
- rollback/revert when an attempted action makes production worse;
- escalation when evidence is insufficient or an action is too risky;
- durable incident memory and audit history.

The product goal is to remove as much routine 24/7 operational burden as can be made safe, auditable, bounded and reversible.

## Core operating model

```text
cheap 24/7 monitoring / provider events
        ↓
meaningful production incident
        ↓
trigger immediately
        ↓
wake AI operations reasoning
        ↓
collect bounded evidence
        ↓
classify the cause
        ↓
choose an allowed action
        ↓
execute through connected infrastructure APIs/tools
        ↓
verify production recovery
        ↓
rollback / escalate if necessary
        ↓
return to monitoring
```

The AI is **passively triggered by production conditions**. The normal product flow does not require a human to notice the problem first and open a chat window.

## Low-overhead architecture principle

NoPager should not require a heavyweight resident AI model or large agent on every protected production server.

Prefer, in this order:

1. provider webhooks and event streams;
2. external health checks and synthetic probes;
3. provider metrics/log/security APIs;
4. cloud/CDN/database/deployment control APIs;
5. bounded polling as a fallback;
6. narrowly scoped on-demand remote execution only when provider APIs cannot perform the necessary operation.

The always-on path should remain deterministic and inexpensive. Model reasoning should activate after a meaningful incident trigger.

"Agentless" means no heavyweight permanent AI runtime is required inside the protected application host. It does not mean NoPager may never execute a bounded remote command when an operator explicitly grants that capability.

## What NoPager is NOT

NoPager is **not**:

- a GitHub product;
- a Vercel product;
- an AI deployment assistant;
- primarily a coding agent;
- a chatbot that waits for a human prompt;
- an observability dashboard whose main job is to send more alerts;
- a continuously running LLM process on every production server.

## Deployment recovery is one operations module

GitHub + Vercel represents the current deployment-recovery implementation path.

That path can prove reusable mechanisms such as:

- incident state;
- evidence collection;
- AI reasoning boundaries;
- policy;
- idempotency;
- safe execution;
- verification;
- rollback discipline;
- auditability.

It **cannot** by itself prove that NoPager performs general server operations.

A GitHub PR, Vercel Preview, deployment promotion, or deployment rollback is only relevant when source/deployment is actually part of the incident.

A production service can fail with no new commit and no new deployment. NoPager must eventually handle that class of incident directly through server/cloud/edge/data operations.

## Execution surfaces, not product identity

NoPager's product is the decision-and-recovery control plane. External systems are execution/evidence surfaces.

Examples:

- Linux / VM / cloud compute;
- Cloudflare and other edge/CDN providers;
- cloud load balancers and traffic managers;
- databases;
- observability/log/metrics providers;
- container platforms;
- DNS;
- queues and storage;
- deployment providers;
- GitHub and other source-control systems.

Use mature provider capabilities whenever possible.

**We orchestrate. We don't reinvent.**

## Decision before action

NoPager must reason about causes before mutating production.

Examples:

- high CPU does not automatically mean scale up;
- high traffic does not automatically mean DDoS;
- 5xx does not automatically mean roll back;
- a failed health check does not automatically mean restart;
- a deployment near the incident does not automatically mean the deployment caused it.

The AI layer should correlate bounded evidence and choose between actions such as observe, restart, scale, block, rate-limit, change WAF policy, fail over, clear cache, roll back, repair code, or escalate.

## Speed claim

NoPager should optimize for very fast detection and reaction.

Provider events and lightweight triggers may arrive in milliseconds or seconds. The system should begin incident processing immediately after a valid trigger.

Do **not** claim that every incident is fully solved in milliseconds. Recovery time can be bounded by infrastructure operations such as reboot, failover, deployment, database recovery, DNS propagation, or verification windows.

Preferred claim:

> **Detect fast. Respond immediately. Recover as fast as the infrastructure safely allows.**

## Trust and autonomy

Autonomy is earned per action, not granted to the entire system at once.

A production action should be allowed automatically only when it is:

- within explicit operator policy;
- narrowly scoped;
- supported by sufficient evidence;
- idempotent or protected from duplicate execution;
- reversible where practical;
- followed by independent verification.

Ambiguous, irreversible, high-impact, credential, billing, IAM, destructive database, or otherwise high-risk operations should require approval or escalation.

The Kill Switch must stop mutations while allowing monitoring/evidence collection to continue.

## Current implementation truth

The repository currently contains a narrow GitHub + Vercel deployment-recovery Alpha. That is an implemented subsystem and safety proof path, not the complete product.

General Linux/cloud operations, Cloudflare operations, database operations, broad capacity automation, and other server/production execution surfaces must not be advertised as shipped until they have real implementation and real-provider evidence.

Track the first true server/production-operations proof in issue #61.

## Customer language

For founders and small teams:

> **You build the product. NoPager handles the 24/7 production incidents you cannot afford a full operations team to watch.**

For technical audiences:

> **NoPager is an incident-triggered AI operations control plane that monitors production cheaply, wakes AI on real incidents, orchestrates provider-native recovery actions, and verifies the result.**

Avoid making GitHub, Vercel, PRs, or deployments the first sentence unless the specific discussion is about the deployment-recovery module.
