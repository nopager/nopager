# Security Policy

NoPager performs production-adjacent automation. Please do not report vulnerabilities in public issues.

Email `security@nopager.dev` with a minimal reproduction, affected version, and impact. Do not include production credentials or customer data. We will acknowledge a report within three business days.

## Security boundaries

- Safe Mode is enabled by default.
- High-risk database, IAM, DNS, billing, and unverified production changes are blocked.
- Secrets must be encrypted at rest, masked in the UI, and redacted from logs.
- Repair sandboxes must be unprivileged, resource-limited, and isolated from host secrets.
