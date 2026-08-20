# Alpha release gate

Do not invite an external design partner to connect a production app until every mandatory gate below has real evidence.

Repository CI is necessary, but it is not proof that GitHub, Vercel, the selected model provider, and production traffic behave correctly together.

## Repository gate

- CI passes on the exact release commit.
- The full self-host smoke path passes on the exact release commit.
- Policy tests prove failed or missing Preview verification cannot be promoted.
- High-risk and irreversible repair classes remain approval-gated or blocked.

## Fresh setup gate

- Start from a fresh machine/profile and final public HTTPS NoPager console origin.
- Complete recommended GitHub App Manifest registration without manually copying an App private key.
- Verify live signed GitHub webhook delivery for the installed repository.
- Verify exact GitHub repository ID and default branch.
- Verify exact Vercel project, GitHub source identity, and explicit Production Branch.
- Verify one real BYOK model/account and the selected model capability probe.
- Verify the configured public HTTPS health check.
- A fresh technical user can protect the first app within the documented setup target without undocumented developer intervention.

## Real GitHub → Vercel incident gate

Run all supported scenarios against a disposable real GitHub repository and real Vercel project:

- Vercel build/deployment failure.
- Recent commit causing a runtime 500.
- Production health regression without relying on a build failure.

For every scenario prove:

- exactly one deduplicated incident is opened;
- bounded verified source/deployment evidence is collected;
- the repair is grounded in verified source evidence;
- sandbox build/test validation passes before a repair PR is trusted;
- exactly one repair PR and Vercel Preview are created;
- Preview reaches `READY` and the Preview health check passes;
- Safe Mode stops at explicit production approval;
- the verified repair is durably landed in protected GitHub source;
- the corresponding Git-driven Vercel deployment becomes the authoritative current Production target;
- production health verification passes before `RESOLVED`.

## Durable rollback and source-recovery gate

Force a production-verification failure after the repair has already been durably merged.

Prove all of the following:

- rollback restores the recorded pre-incident known-good deployment, never the failed repair itself;
- automatic rollback is blocked if an unrelated external Production deployment has taken over;
- traffic recovery does not mark the incident resolved while failed source remains on the protected branch;
- NoPager creates or surfaces the exact draft source-revert review action without blindly retrying an uncertain source mutation;
- the source-revert PR remains human-reviewed and is never auto-merged;
- after manual merge, NoPager verifies the exact PR identity and merge commit;
- the protected GitHub default-branch head still equals the reviewed recovery commit at closure time;
- the corresponding Vercel deployment is the authoritative current Production target;
- production passes the full health-verification window;
- final closure is recorded as human-assisted (`autonomous_resolution=false`).

If source identity, source head, Production identity, health, or provider response is ambiguous, the incident must remain escalated.

## Kill Switch gate

- Activate the Kill Switch during a controlled incident.
- Read-only monitoring and evidence collection continue.
- Every production/source mutation remains blocked.

## Customer-facing evidence gate

- Record setup duration from a fresh user/profile.
- Preserve incident timelines and deployment/commit identities for the three supported scenarios.
- Preserve rollback and source-recovery evidence.
- Record the first real 60–90 second demo from an actual successful run; editing pauses is acceptable, fabricating a successful result is not.
- Keep the external Design Partner Alpha positioning explicit. Do not call the release broadly production-ready until the real provider gate has repeatedly passed.

The executable dogfood checklist lives in GitHub issue #55. `docs/DESIGN_PARTNER_ALPHA.md` remains the canonical product-scope and acceptance definition.
