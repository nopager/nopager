# Alpha test matrix

Run this matrix before handing NoPager to an external design partner.

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
- **A12 — Autopilot + low risk + verified + reversible:** Production promotion may proceed.
- **A13 — Autopilot + medium risk:** Requires approval.
- **A14 — Autopilot + no rollback target:** Requires approval; never auto-promotes.
- **A15 — Kill Switch active:** All mutations block; monitoring/evidence collection continue.
- **A16 — Production verification succeeds:** Incident resolves after watch period.
- **A17 — Production verification fails:** Rollback to latest known-good deployment starts.
- **A18 — Rollback verification succeeds:** Incident records rollback outcome and stops unsafe repair.
- **A19 — Rollback verification fails:** Incident escalates; no further production mutation.
- **A20 — External design-partner run:** No owner intervention before the Safe Mode approval boundary except initial setup.

For A2–A4 use `examples/demo-next-app`. For A5–A19 use controlled dogfood fixtures or integration tests; do not use a real customer production system for destructive testing.
