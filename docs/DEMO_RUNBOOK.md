# NoPager 60–90 Second Demo Runbook

Use this demo for design-partner outreach. The purpose is to show the product promise and safety boundary, not every internal implementation detail.

Only record the customer-facing demo after the same flow has succeeded against a disposable real GitHub repository and real Vercel project. Repository CI alone is not a substitute for that run.

## Before recording

- Run NoPager in Safe Mode.
- Connect one disposable GitHub repository to one disposable Vercel project.
- Use `examples/demo-next-app` as the target or mirror it into the demo repository.
- Confirm GitHub repository/default-branch identity matches the Vercel Git source and Production Branch.
- Confirm the current Production deployment is healthy and recorded as the known-good baseline.
- Keep the NoPager Overview and Incident Detail pages ready in browser tabs.
- Never record real API keys, GitHub private keys, webhook secrets, customer data, or production credentials.

## Recommended story

### 0–8s — Healthy production

Show the app working, then the NoPager Overview.

Narration/message:

> Production is healthy. NoPager is watching it 24/7. No action needed.

### 8–18s — Introduce a real regression

Show a small commit that activates the deterministic runtime 500 or health-check regression and push it.

Do not spend time explaining code. The point is that a normal deployment introduced a production failure.

### 18–30s — Detection

Refresh or switch to NoPager.

Show:

- incident opened;
- production unhealthy;
- current state moving through context collection/diagnosis.

Narration/message:

> The app broke. NoPager detected it and started investigating without paging the owner.

### 30–48s — Diagnosis and repair

Open Incident Detail.

Show the outcome-first summary, root cause, repair attempt, and a small part of the patch.

Narration/message:

> NoPager correlated the failure with the recent change, produced the smallest repair, and tested it in an isolated sandbox.

### 48–62s — PR and Preview

Show:

- repair PR link;
- tests/validation passing;
- Vercel Preview ready;
- Preview health check passing.

Narration/message:

> The repair is not trusted just because the model wrote code. It has to pass tests and a real Preview health check first.

### 62–75s — Safe Mode approval

Show `WAITING_APPROVAL` and the approval bar.

Narration/message:

> Safe Mode stops at the production boundary. The owner sees exactly what changed and approves the verified repair.

Click **Approve**.

### 75–90s — Durable recovery

Do not cut directly from approval to a green badge. Show enough of the final timeline to prove that NoPager:

- durably lands the verified repair in the protected GitHub source path;
- observes the corresponding Git-driven Vercel deployment become the authoritative current Production target;
- verifies production health before `RESOLVED`.

Final message:

> Production broke. NoPager diagnosed the regression, prepared and verified the repair, waited for approval, then proved source and production converged before closing the incident. You didn't get paged.

## Safety proof to record separately

The 60–90 second happy-path demo is not enough for the Alpha gate. Keep a second, internal or longer-form recording of the durable rollback/source-recovery path:

1. Force production verification to fail after the repair has already landed in protected source.
2. Show NoPager restore the pre-incident known-good Vercel deployment.
3. Show that traffic recovery does **not** mark the incident resolved while failed source remains.
4. Show the draft source-revert action and manual human review/merge.
5. Show NoPager verify the exact revert identity, protected GitHub default-branch head, authoritative Vercel Production deployment, and health before final human-assisted closure.

This proof matters because traffic rollback and durable source recovery are different safety states.

## What not to show

Do not turn the first demo into an observability product tour. Avoid long raw logs, trace IDs, token usage, model-selection details, Docker internals, or architecture diagrams.

Do not claim unrestricted autonomous production access. Safe Mode is the default and the demo should visibly prove the production safety boundary.

Do not claim broad cloud/Kubernetes/database remediation. The Alpha is GitHub + Vercel + one protected app.

Do not call the Alpha broadly production-ready until the real provider acceptance gate in `docs/DESIGN_PARTNER_ALPHA.md` and GitHub issue #55 has passed.

## Backup demo

If a live model/API call is unreliable during recording, record the complete real flow once, then edit the pauses out. Do not fabricate a successful production result that the system did not actually achieve.

If the repair fails, keep the recording: a useful secondary demo is NoPager refusing to promote a failed Preview or refusing to overwrite an unrelated Production deployment. Those outcomes demonstrate the safety model better than a fabricated perfect happy path.
