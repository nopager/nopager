# GitHub App setup

NoPager uses a GitHub App instead of a long-lived personal access token. Install the App only on repositories that NoPager is allowed to inspect and repair.

## Required repository permissions

Configure these repository permissions on the GitHub App:

- **Contents: Read and write** — read commits/source and create the narrow repair commit/branch.
- **Pull requests: Read and write** — open or reuse the repair PR.
- **Actions: Read-only** — receive and inspect failed `workflow_run` events used as incident evidence.

Leave unrelated permissions at **No access**. NoPager does not need repository administration, organization administration, secrets, environments, packages, or members permissions for the Alpha repair loop.

## Webhook

Set the GitHub App webhook URL to the public NoPager **web** origin, not the internal Rust API port:

```text
https://YOUR_NOPAGER_HOST/api/webhooks/github
```

Generate a strong webhook secret (at least 16 random characters) and enter the same value in the NoPager setup wizard. NoPager verifies the signature against the raw request body before accepting a delivery.

Subscribe to the **Workflow run** event. The production health monitor and Vercel polling path do not depend on GitHub webhooks, but workflow failures provide a fast, useful incident signal.

## Install the App

1. Install the GitHub App on the account or organization that owns the protected repository.
2. Prefer **Only select repositories** and select only the app you intend to protect.
3. Copy the App ID and installation ID into NoPager setup.
4. Generate/download a GitHub App private key and paste the complete PEM block into the setup wizard.
5. Enter the repository owner and repository name. NoPager discovers the repository numeric ID and default branch automatically.
6. Use **Test & continue**. Setup fails closed if the installation token cannot access that exact repository.

## Private key handling

The private key is encrypted locally before persistence. Do not put the PEM in Git, `.env`, screenshots, issue reports, or shell history. If a key is exposed, revoke it in the GitHub App settings and generate a new one.

## Production exposure

The default Docker Compose configuration binds the Rust API to `127.0.0.1`. Keep it private. Expose the Next.js web service through your HTTPS reverse proxy; the web service forwards only the signed webhook headers and raw body required for verification.

## Troubleshooting

If setup returns `github_connection_failed`, verify the App ID, installation ID, and PEM. If it returns `github_repository_not_accessible`, verify that the installation includes the exact owner/repository and that the repository permissions above are enabled.
