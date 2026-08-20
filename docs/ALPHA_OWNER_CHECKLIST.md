# First design partner owner checklist

Use this checklist before connecting an external design partner app. The first customer run is a controlled production trial, not permission to widen NoPager's scope.

## Before setup

- Use a non-critical or staging-equivalent GitHub + Vercel app for the first run whenever possible.
- Keep Safe Mode enabled.
- Do not enable Autopilot for the first external run.
- Use the final public HTTPS NoPager console origin before starting automatic GitHub App registration.
- Back up `.env` and PostgreSQL before the trial. Losing `NOPAGER_MASTER_KEY` makes encrypted integration credentials unrecoverable.
- Confirm the public production health URL represents business health, not only process liveness.

## Provider identity

- Confirm the GitHub App is installed only on the intended repository.
- Confirm NoPager reports the expected GitHub repository ID and protected default branch.
- Confirm the Vercel project is linked to that same GitHub repository.
- Confirm Vercel has an explicit Production Branch and it matches the protected GitHub default branch.
- Confirm the currently recorded known-good deployment is the real current healthy Production deployment.
- Scope the Vercel token and BYOK model credential to the intended trial account/project where the provider supports that boundary.

## Safety controls

- Exercise the Kill Switch before the first injected incident.
- Confirm the owner can reach the Incident Detail approval controls from the public console.
- Keep the owner present for the first production approval and for any source-revert review.
- Never approve a repair whose Preview health, patch, validation evidence, or source identity is unclear.
- Treat `ESCALATED` as an unresolved safety state even if production traffic has already recovered.

## First incident operating rule

The expected Safe Mode path is:

`Detect → Diagnose → Repair → Test → PR → Preview → Verify → Human approval → Durable GitHub landing → Git-driven Production → Verify → Resolve`

If production verification fails after the repair has already landed in protected source, traffic rollback is only the first recovery step. The incident must remain human-visible until the failed source is reversed through a reviewed source-revert and GitHub source, authoritative Vercel Production, and health converge again.

## Before expanding the trial

- Complete the real dogfood evidence in GitHub issue #55.
- Review incident timelines with the design partner after the first controlled run.
- Record any manual intervention before the intended Safe Mode approval/review boundaries.
- Do not move a customer to Autopilot because one demo succeeded. Keep external design partners in Safe Mode until the documented Alpha gate has repeatedly passed.

## Trial offboarding

A design partner must be able to remove NoPager without depending on a central NoPager service.

1. Activate the Kill Switch before changing credentials or integrations.
2. In GitHub, uninstall the NoPager GitHub App from the repository/account or remove the repository from the App installation.
3. Revoke the Vercel access token used for the trial.
4. Revoke or rotate the BYOK model-provider API key used for the trial.
5. Remove any optional Vercel webhook and Protection Bypass for Automation secret created specifically for NoPager.
6. Decide whether to retain the self-hosted incident/audit history for evaluation. If it must be destroyed, remove the NoPager PostgreSQL data and encrypted backups according to the design partner's retention policy.
7. Retain or securely destroy `.env` together with the database backup; keeping one without the matching `NOPAGER_MASTER_KEY` does not preserve usable encrypted integration credentials.

Because the Alpha is self-hosted and BYOK, provider credentials and stored incident data remain under the operator's infrastructure/account control. NoPager should never claim that revoking one provider credential automatically deletes data retained by another provider; model-provider retention is governed by that provider/account policy.
