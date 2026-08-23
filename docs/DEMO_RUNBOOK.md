# NoPager 60–90 Second Demo Runbook

Use this demo for design-partner outreach. The purpose is to show the **incident-triggered AI operations** product promise and the current Alpha safety boundary, not to make GitHub or Vercel look like the product itself.

The audience should leave with one idea:

> **Production breaks. NoPager wakes up — not you.**

The current Alpha happens to prove that idea through one GitHub → Vercel recovery path. Treat those providers as execution surfaces inside the story, not the headline.

Only record the customer-facing live demo after the same flow has succeeded against a disposable real GitHub repository and real Vercel project. Repository CI alone is not a substitute for that run.

## Before recording

- Run NoPager in Safe Mode.
- Connect one disposable GitHub repository to one disposable Vercel project.
- Use `examples/demo-next-app` as the target or mirror it into the demo repository.
- Confirm GitHub repository/default-branch identity matches the Vercel Git source and Production Branch.
- Confirm the current Production deployment is healthy and recorded as the known-good baseline.
- Keep the NoPager Overview and Incident Detail pages ready in browser tabs.
- Never record real API keys, GitHub private keys, webhook secrets, customer data, or production credentials.

## Positioning rule

Do **not** introduce the demo as "an AI that fixes Vercel deployments."

Introduce it as an incident-triggered AI operations engineer:

> NoPager keeps lightweight production monitoring active. When an incident is detected, it wakes the AI operations layer, gathers evidence, chooses a safe recovery path, executes through connected infrastructure, verifies the result, and then returns to monitoring.

For the current Alpha, GitHub + Vercel are simply the first connected tools available to execute and verify that response.

Do not claim that Cloudflare, general server/cloud control, databases, or broader security/capacity automation are already implemented. Those are product-direction execution surfaces described in `PRODUCT_PRINCIPLES.md`, not current Alpha features.

## Recommended story

### 0–8s — Quiet while healthy

Show the app working, then the NoPager Overview.

Narration/message:

> Production is healthy. NoPager is watching it 24/7. The AI does not need to sit there reasoning while nothing is wrong.

### 8–18s — A real production signal fails

Show a small commit that activates the deterministic runtime 500 or health-check regression and push it.

Do not make the commit or deployment the hero of the scene. The point is simply that production has become unhealthy.

Narration/message:

> Something in production just broke.

### 18–30s — Incident trigger wakes the response loop

Refresh or switch to NoPager.

Show:

- incident opened;
- production unhealthy;
- current state moving through context collection/diagnosis.

Narration/message:

> NoPager detected the incident and woke the response loop before the owner had to notice it manually.

### 30–48s — Operations reasoning

Open Incident Detail.

Show the outcome-first summary, root cause, repair attempt, and a small part of the evidence/patch.

Narration/message:

> It gathered the available production evidence, correlated the failure with the recent change, chose a narrow recovery action, and tested that action in isolation.

The viewer should understand that **code repair is one possible operations action**, not the definition of NoPager.

### 48–62s — Execute through connected tools

Show:

- repair PR link;
- tests/validation passing;
- Vercel Preview ready;
- Preview health check passing.

Narration/message:

> In this Alpha, GitHub and Vercel are the connected execution tools. The repair is not trusted because an AI wrote it; it has to pass validation and a real Preview health check.

### 62–75s — Safe Mode production boundary

Show `WAITING_APPROVAL` and the approval bar.

Narration/message:

> Safe Mode stops at the production boundary. The system has already investigated and prepared the verified response; the owner only approves the production action.

Click **Approve**.

### 75–90s — Verify durable recovery

Do not cut directly from approval to a green badge. Show enough of the final timeline to prove that NoPager:

- durably lands the verified repair in the protected GitHub source path;
- observes the corresponding Git-driven Vercel deployment become the authoritative current Production target;
- verifies production health before `RESOLVED`.

Final message:

> Production broke. NoPager woke up, investigated, prepared and verified the recovery, waited at the configured safety boundary, and proved production was healthy before going quiet again. You didn't get paged.

## Future demo vocabulary

As new execution surfaces become real and tested, future demos can show different responses to different incident classes, for example:

- abusive traffic → Cloudflare WAF/rate-limit action;
- process failure → controlled service restart;
- capacity incident → scale action;
- bad deployment → rollback;
- software regression → tested code repair;
- database incident → safe failover/recovery primitive;
- cache/routing failure → traffic or cache action.

Those examples should only move from roadmap language into live demos after the corresponding connectors and safety gates actually exist.

## Safety proof to record separately

The 60–90 second happy-path demo is not enough for the Alpha gate. Keep a second, internal or longer-form recording of the durable rollback/source-recovery path:

1. Force production verification to fail after the repair has already landed in protected source.
2. Show NoPager restore the pre-incident known-good Vercel deployment.
3. Show that traffic recovery does **not** mark the incident resolved while failed source remains.
4. Show the draft source-revert action and manual human review/merge.
5. Show NoPager verify the exact revert identity, protected GitHub default-branch head, authoritative Vercel Production deployment, and health before final human-assisted closure.

This proof matters because traffic rollback and durable source recovery are different safety states.

## What not to show or claim

Do not turn the first demo into an observability product tour. Avoid long raw logs, trace IDs, token usage, model-selection details, Docker internals, or architecture diagrams.

Do not market NoPager as a GitHub/Vercel deployment assistant. Those are current Alpha connectors.

Do not claim unrestricted autonomous production access. Safe Mode is the default and the demo should visibly prove the production safety boundary.

Do not claim broad Cloudflare/cloud/server/Kubernetes/database remediation before those execution surfaces are implemented and verified.

Do not claim "millisecond incident resolution." Detection and trigger latency can become extremely fast with provider event paths, but actual recovery is constrained by the underlying operation and verification time.

Do not call the Alpha broadly production-ready until the real-provider acceptance gate in `docs/DESIGN_PARTNER_ALPHA.md` and GitHub issue #55 has passed.

## Backup demo

If a live model/API call is unreliable during recording, record the complete real flow once, then edit the pauses out. Do not fabricate a successful production result that the system did not actually achieve.

If the repair fails, keep the recording: a useful secondary demo is NoPager refusing to promote a failed Preview or refusing to overwrite an unrelated Production deployment. Those outcomes demonstrate the safety model better than a fabricated perfect happy path.
