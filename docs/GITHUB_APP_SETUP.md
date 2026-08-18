# GitHub App setup

NoPager uses a GitHub App instead of a long-lived personal access token. Install the App only on repositories that NoPager is allowed to inspect and repair.

## Recommended: automatic setup

The self-hosted setup wizard can register the GitHub App for you using GitHub's App Manifest flow.

1. Open `/setup` and sign in as the local administrator.
2. Enter the protected app name, repository owner, and repository name.
3. Choose whether the GitHub App should be owned by your personal account or by the repository owner organization.
4. Click **Create GitHub App automatically**. GitHub opens in a popup with NoPager's permission and webhook configuration already filled in. You only need to confirm the App registration and its name.
5. Back in NoPager, click **Install on repository** and select only the repository NoPager should protect.
6. Click **Test & continue**. NoPager requests an installation token scoped to that repository and fails closed unless the exact repository is accessible.

The manifest exchange runs through the self-hosted web console and requires an authenticated local NoPager session. GitHub's one-time manifest code is exchanged server-side. The generated App ID, private key, and webhook secret are returned only to the still-open setup page, held in memory for the setup flow, and then encrypted by the existing Rust setup API before persistence. They are not written to browser local storage or placed in callback URLs.

For remote NoPager consoles, use HTTPS before running automatic setup. `http://localhost` remains suitable for local setup redirects, but GitHub cannot deliver webhooks to a loopback-only console.

## Required repository permissions

The automatic manifest requests only these repository permissions:

- **Contents: Read and write** — read commits/source and create the narrow repair commit/branch.
- **Pull requests: Read and write** — open or reuse the repair PR.
- **Actions: Read-only** — receive failed `workflow_run` events used as incident evidence.

Leave unrelated permissions at **No access**. NoPager does not need repository administration, organization administration, secrets, environments, packages, or members permissions for the Alpha repair loop.

## Webhook

Automatic setup configures the GitHub App webhook URL to the NoPager **web** origin:

```text
https://YOUR_NOPAGER_HOST/api/webhooks/github
```

GitHub generates the webhook secret during the Manifest flow. NoPager verifies the signature against the raw request body before accepting a delivery.

The App subscribes to the **Workflow run** event. The production health monitor and Vercel polling path do not depend on GitHub webhooks, but workflow failures provide a fast, useful incident signal.

If NoPager is running only on localhost, the GitHub API/repair integration still works after installation, but GitHub cannot reach the local webhook URL. Expose the web console deliberately through HTTPS when you need real-time GitHub webhook delivery; keep the Rust API port private.

## Advanced: existing GitHub App

The setup wizard keeps a manual fallback under **Advanced: use an existing GitHub App**.

If you use it:

1. Configure the three repository permissions above and subscribe to **Workflow run**.
2. Set the webhook URL to `https://YOUR_NOPAGER_HOST/api/webhooks/github` and generate a strong webhook secret of at least 16 random characters.
3. Install the GitHub App on the account or organization that owns the protected repository. Prefer **Only select repositories** and select only the app you intend to protect.
4. Enter the App ID, installation ID, webhook secret, and complete PEM private key in NoPager.
5. Use **Test & continue**. Setup fails closed if the installation token cannot access the exact repository.

## Private key handling

The private key is encrypted locally before persistence. Do not put the PEM in Git, `.env`, screenshots, issue reports, shell history, or browser storage. If a key is exposed, revoke it in the GitHub App settings and generate a new one.

## Production exposure

The default Docker Compose configuration binds the Rust API to `127.0.0.1`. Keep it private. Expose the Next.js web service through your HTTPS reverse proxy; the web service forwards only the signed webhook headers and raw body required for verification.

## Troubleshooting

If automatic registration fails, confirm that popups are allowed for the NoPager console and that your GitHub account has permission to create a GitHub App at the selected owner. Organization-owned Apps require the relevant organization permissions.

If setup returns `github_connection_failed`, retry the install step or verify the App ID, installation ID, and PEM in the Advanced fallback. If it returns `github_repository_not_accessible`, verify that the installation includes the exact owner/repository and that the repository permissions above are enabled.
