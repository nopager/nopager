# Alpha test matrix

Run this matrix before handing NoPager to an external design partner.

## Incident and repair safety

- **A1 — Healthy app:** Overview shows Healthy / No action needed.
- **A2 — Three consecutive health failures:** One incident opens; duplicate health checks do not open duplicates.
- **A3 — Runtime 500 from recent commit:** Diagnosis uses recent commit context and prepares a bounded repair.
- **A4 — Deterministic Vercel build failure:** Repair restores build in sandbox before PR/Preview.
- **A5 — Patch applies to undeclared file:** Attempt fails before trusted PR/Preview.
- **A6 — Dependency manifest changed:** Incident escalates; no automatic production mutation.
- **A7 — Sandbox build/test failure:** Failure is stored and supplied to a fresh repair attempt.
- **A8 — Three repair attempts fail:** Incident escalates and automatic repair stops.
- **A9 — Preview deployment ERROR/CANCELED:** Production promotion is blocked.
- **A10 — Preview HTTP verification fails:** Policy returns Block; approval cannot bypass it.
- **A11 — Safe Mode + verified Preview:** Incident enters `WAITING_APPROVAL`.
- **A12 — Autopilot + low risk + verified + reversible:** Production promotion may proceed only in controlled dogfood.
- **A13 — Autopilot + medium risk:** Requires approval.
- **A14 — Autopilot + no rollback target:** Requires approval; never auto-promotes.
- **A15 — Kill Switch active:** All mutations block; monitoring/evidence collection continue.

## Durable production closure

- **A16 — Promoted Preview is healthy but source is not durably landed:** Incident does not become `RESOLVED` merely because the temporary Production deployment is healthy.
- **A17 — Durable repair lands in protected GitHub source:** The worker records the exact merged repair identity and waits for the corresponding Git-driven Vercel Production deployment.
- **A18 — Corresponding Git-driven Production is current and healthy:** Only the authoritative current Production deployment for the durable repair may become the new known-good baseline and close the incident after the full watch period.
- **A19 — Production verification fails before durable source landing:** Rollback to the recorded pre-incident known-good deployment may proceed; successful rollback records the outcome without inventing durable source work that never happened.
- **A20 — Production verification fails after durable source merge:** Traffic rolls back to the pre-incident known-good deployment, but the incident remains `ESCALATED` because failed source is still present on the protected branch.
- **A21 — Temporary promoted repair must not replace rollback baseline:** Before durable Production verification completes, the pre-incident known-good deployment remains the rollback target.
- **A22 — Rollback verification fails:** Incident escalates; no further automatic production mutation.
- **A23 — External Production drift before rollback:** If authoritative current Vercel Production is an unrelated third deployment, automatic rollback is blocked rather than overwriting it.

## Reviewed source recovery

- **A24 — Source-revert creation:** After traffic recovery from a failed durable repair, NoPager creates a draft source-revert or surfaces an existing candidate without auto-merging it.
- **A25 — Uncertain source mutation:** If GitHub's source-revert mutation result is ambiguous, NoPager does not blindly retry and risk duplicate revert PRs; the incident stays escalated for human verification.
- **A26 — Source-revert identity drift:** If the tracked revert PR number/node/head/base identity changes, automatic closure stops and the incident remains escalated.
- **A27 — Human merges exact source-revert:** NoPager observes the provider-attested merge commit using read-only repository-scoped credentials; it never performs the merge itself.
- **A28 — Source-recovery Production verification:** The Git-driven deployment for the reviewed revert must become authoritative current Vercel Production and pass the full production health window.
- **A29 — Protected source advances again before closure:** If the GitHub default-branch head no longer equals the reviewed revert merge commit, NoPager refuses to report source/runtime alignment even if the revert deployment is healthy.
- **A30 — Final reviewed recovery:** Only when exact revert identity, protected GitHub head, authoritative Vercel Production, and health all agree may the escalated incident close; record `autonomous_resolution=false`.

## Setup and provider compatibility

- **A31 — Automatic GitHub App onboarding:** From the final public HTTPS NoPager console origin, the Manifest flow creates the private least-privilege App, the operator installs it only on the intended repository, the callback returns an installation ID, and **Verify GitHub** proves access to that exact repository. The workflow-run webhook is active and a real signed event reaches NoPager.
- **A32 — Manual GitHub fallback:** An organization that disallows Manifest registration can enter an App ID, installation ID, private-key PEM, and webhook secret manually and pass the same exact-repository verification.
- **A33 — Vercel Git source compatibility:** Setup proves the selected Vercel project is linked to the same protected GitHub repository and has an explicit Production Branch matching the GitHub default branch; mismatch or unverifiable identity fails closed.
- **A34 — Authoritative initial Production baseline:** Setup records Vercel's authoritative current READY Production deployment as initial known-good; it does not choose an arbitrary historical `target=production` deployment.
- **A35 — Provider model discovery:** For each supported provider family, a real BYOK account can load its available models; selecting an unavailable exact model fails closed with an actionable error.
- **A36 — Provider capability probe:** A selected supported model passes NoPager's bounded structured-output capability probe; a model/account combination that cannot satisfy the required API shape fails setup without protecting the app.
- **A37 — Health endpoint discovery:** For a public HTTPS production origin, safe discovery selects a standard endpoint only when it returns HTTP 200; unsafe/private destinations remain rejected and manual health-URL entry still works.

## External design-partner readiness

- **A38 — Real GitHub → Vercel scenarios:** Build failure, runtime 500, and health regression all pass end to end against a disposable real GitHub repository and real Vercel project.
- **A39 — Durable rollback/source-recovery dogfood:** The complete A20–A30 path is proven against real provider accounts before relying on it with a customer production app.
- **A40 — Fresh-user setup:** A technical user completes setup from a fresh profile within the documented target without undocumented developer intervention beyond the stated credential prerequisites.
- **A41 — First external run:** No owner intervention is required before the intended Safe Mode production approval boundary except initial setup; source-revert review remains explicitly human-assisted when that safety path is exercised.
- **A42 — Trial offboarding:** Kill Switch is activated, GitHub App access can be removed, Vercel/model credentials can be revoked, optional webhook/bypass credentials can be removed, and local retained data can be preserved or destroyed according to the operator's policy without depending on a central NoPager service.

For A2–A4 use `examples/demo-next-app`. For A5–A30 use controlled dogfood fixtures or integration tests; do not use a real customer production system for destructive testing. Run A31 and A33–A42 against disposable but real external accounts/projects before the first design partner so current provider behavior is proven rather than inferred from mocks alone.
