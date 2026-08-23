# NoPager first server/production-operations MVP

This milestone is the first implementation proof of the actual NoPager product category. It is intentionally separate from the existing GitHub + Vercel deployment-recovery Alpha.

See [`PRODUCT_POSITIONING.md`](PRODUCT_POSITIONING.md) for the product definition and issue #61 for the acceptance checklist.

## Goal

Prove that NoPager can protect a running production service when the incident is **not caused by a new Git commit or deployment**.

The first successful run should look like:

```text
production healthy
    → operational failure occurs
    → lightweight monitoring detects it
    → incident trigger wakes AI
    → bounded server/cloud/edge evidence is collected
    → AI classifies the cause
    → one policy-allowed provider-native action executes
    → health is independently verified
    → incident resolves or escalates
    → AI returns to sleep
```

## Non-goals

This MVP is not trying to support every cloud or every operations task.

It is not:

- a Kubernetes platform;
- a full observability backend;
- a general-purpose remote shell agent;
- a replacement for provider metrics/log storage;
- a reason to run an LLM continuously on the protected host;
- another GitHub/Vercel demo.

## Architecture: keep the protected server light

The protected server should not host a resident AI model.

Prefer a control-plane design:

```text
                       ┌────────────────────────────┐
                       │          NoPager           │
                       │                            │
                       │ Trigger / incident state   │
                       │ Evidence graph             │
                       │ AI decision engine         │
                       │ Policy / audit             │
                       │ Action orchestration       │
                       │ Verification               │
                       └─────────────┬──────────────┘
                                     │
                  scoped API / event │ access
                                     │
       ┌─────────────────────────────┼─────────────────────────────┐
       │                             │                             │
       ▼                             ▼                             ▼
 cloud/VM provider              Cloudflare                  observability
 metrics + actions              edge/security              logs/metrics
       │                             │                             │
       └───────────────┬─────────────┴───────────────┬─────────────┘
                       │                             │
                       ▼                             ▼
                 protected service             external probes
```

When a provider API cannot restart a specific process, an optional on-demand remote executor may use narrowly scoped SSH or another explicit remote-control surface. It must not become an unrestricted always-on shell agent.

## Core abstractions to build

### 1. Operational signal

A normalized signal entering the incident engine.

Examples:

- HTTP health failed;
- TCP endpoint failed;
- instance health degraded;
- CPU sustained above threshold;
- memory pressure sustained;
- disk nearly full;
- abnormal request rate;
- Cloudflare security event;
- provider outage/dependency event.

Signals should carry provider identity, protected-resource identity, timestamp, evidence references, and deduplication keys.

### 2. Operational incident context

A bounded snapshot assembled after trigger.

Possible evidence:

- current external health;
- recent health history;
- cloud instance state;
- CPU/memory/disk/network metrics;
- selected recent logs/errors;
- current traffic shape;
- Cloudflare security/analytics evidence;
- current deployment identity only as one possible cause signal;
- recent infrastructure actions;
- known-good baseline.

The context builder should request evidence on demand. Do not dump an entire account or unbounded logs into the model.

### 3. Operations action

Provider-neutral action types should describe intent before connector-specific translation.

Initial candidates:

- `ObserveOnly`;
- `RestartService`;
- `RestartInstance`;
- `ScaleCapacity`;
- `Failover`;
- `ApplyRateLimit`;
- `ApplyWafRule`;
- `BlockSource`;
- `PurgeCache`;
- `ChangeTrafficPolicy`;
- `RollbackDeployment`;
- `RepairCode`;
- `Escalate`.

The MVP only needs a small safe subset implemented end-to-end.

### 4. Action policy

Policy evaluates the proposed action before execution.

It should consider:

- protected resource;
- action type;
- blast radius;
- reversibility;
- evidence confidence;
- action-specific limits;
- cooldown/repetition history;
- Safe Mode / Autopilot mode;
- Kill Switch state.

### 5. Verification plan

Every mutating action must define how success is proven independently.

Verification can include:

- external HTTP/TCP health;
- provider instance health;
- CPU/memory pressure returning to acceptable range;
- request error rate recovering;
- Cloudflare traffic/security signal changing as expected;
- service/process status where available;
- repeated healthy checks over a configured window.

Do not resolve an incident just because the provider API returned `200 OK` for the mutation request.

## First implementation slice

Build one narrow server-operations path before broad connector expansion.

Recommended slice:

### External health failure + cloud/VM recovery

1. Monitor one external HTTP health endpoint continuously using a cheap deterministic probe.
2. Trigger only after a configured failure threshold to avoid one-sample noise.
3. Confirm there was no new deployment/source change that obviously explains the incident.
4. Pull bounded instance/provider evidence.
5. Wake the AI decision layer.
6. Allow AI to choose between `ObserveOnly`, one narrowly scoped restart/recovery action, or `Escalate`.
7. Execute through a scoped provider API or explicit on-demand remote executor.
8. Verify external health repeatedly.
9. Prevent blind repeated restarts with cooldown/idempotency protection.
10. Resolve only after the verification window passes.

This proves true production operations without requiring Cloudflare or database automation in the first run.

## Second slice: Cloudflare edge operations

After the server-health path is proven, add Cloudflare as an edge/security execution surface.

Initial evidence:

- request volume/time series;
- response status distribution where available;
- security/bot events;
- top source patterns where allowed;
- cache/edge indicators;
- application health correlation.

Initial actions should be deliberately narrow and reversible, for example:

- temporary rate-limit policy;
- narrowly scoped WAF rule;
- temporary source/IP block when evidence is strong;
- cache purge when evidence supports stale/broken cache;
- observe/escalate when classification is uncertain.

Every temporary action needs expiry/revert semantics and post-action verification.

## Third slice: capacity operations

Use provider metrics and actions to distinguish sustained resource pressure from incidents that should not be solved by scaling.

The AI should consider at least:

- sustained vs transient CPU/memory pressure;
- error/latency correlation;
- traffic growth vs abuse;
- cache behavior;
- dependency failures;
- recent deployment/infrastructure changes.

A valid outcome may be `ObserveOnly` or `Escalate`; NoPager should not mutate production merely to look autonomous.

## Trigger latency vs recovery latency

The mature product should prefer event-driven detection whenever providers expose reliable events.

Target behavior:

- trigger path: as fast as the available signal permits;
- incident creation: immediate after threshold/event validation;
- AI wake: immediate after incident creation;
- recovery: as fast as the selected infrastructure action and safe verification allow.

Do not collapse these into one misleading "millisecond recovery" metric.

Track separately:

- signal-to-trigger latency;
- trigger-to-decision latency;
- decision-to-action latency;
- action-to-traffic-recovery latency;
- action-to-verified-recovery latency.

## Resource footprint requirement

For the protected application host:

- no resident LLM;
- no requirement for a heavyweight NoPager agent;
- external probes/provider APIs first;
- on-demand remote execution only if necessary;
- no continuous repository scanning;
- no continuous model inference;
- bounded evidence collection only after trigger.

NoPager itself still needs a control plane somewhere. The claim is low impact on the protected production workload, not zero compute anywhere in the system.

## Safety gates

Before calling the first server-operations MVP real:

- real provider account;
- disposable/non-critical server or production-like service;
- least-privilege credential documented;
- deterministic incident injection;
- one deduplicated incident;
- no manual prompting before AI wake;
- real provider/server action;
- independent post-action verification;
- failed action escalates instead of looping;
- Kill Switch blocks mutation;
- complete audit trail;
- recorded demo from the real run.

## Public demo rule

The first true NoPager demo should be understandable without mentioning GitHub or Vercel:

> A running service breaks. NoPager notices first, wakes AI, investigates the production state, performs the safest allowed operations action, proves the service recovered, and goes quiet again.

If code/deployment is later determined to be the cause, GitHub/Vercel may appear as tools used by the response. They are not the product story.
