# Security policy and architecture

NoPager performs production-adjacent automation. The Design Partner release is deliberately limited to one real mutation: `restart_container` for one immutable, explicitly enrolled Docker container.

## Privilege and trust boundaries

The web/API control plane and ordinary worker run as unprivileged containers. The worker image contains no Docker CLI, does not mount `/var/run/docker.sock`, and is not a member of the Docker socket group. It can contact `nopager-runtime-helper` only through a local Unix socket using a dedicated IPC group and a random installation credential.

The Linux host-side helper is the small trusted execution boundary. It has Docker socket group membership, but exposes only two versioned JSON operations: inspect the enrolled immutable container ID and restart that same ID. It independently checks the peer UID, credential, immutable ID, container name, Compose identity, denylist, NoPager control-plane label, and control-plane Compose project. Unknown JSON fields and all other action types are rejected. There is no exec, shell, create, remove, image pull/build, mount, network mutation, arbitrary Docker request, or generic command passthrough.

The helper writes a durable request claim before restart. A completed duplicate receives the cached result. A request found in an in-progress/unknown state is reported as ambiguous and is never replayed. Compromise of the helper or Docker daemon remains a host-level security event; isolate and patch that host accordingly. See [runtime-helper security architecture](docs/RUNTIME_HELPER_SECURITY.md) and [threat model](docs/THREAT_MODEL.md).

## Approval and mutation authority

Safe Mode is the default. Detection, bounded evidence collection, and AI reasoning do not grant mutation authority. The model may select only typed actions supplied in its input. Policy, persisted action state, explicit administrator approval, Kill Switch state, cooldown, exact enrollment, and helper validation all have to permit execution. The worker makes at most one execution attempt. Ambiguous results fail closed and escalate; they are not blindly retried.

Recovery requires independent container-state inspection through the helper plus consecutive external HTTP successes. A running container with unhealthy HTTP is not considered recovered.

## Secrets and model-provider evidence

BYOK provider keys are accepted by the local setup API, encrypted with `NOPAGER_MASTER_KEY`, and stored in PostgreSQL. The helper IPC credential is separate from provider credentials and grants access only to the narrow helper protocol. Keep `.env`, the master key, database backups, helper token, and host administrator access secret; rotate the IPC token after suspected disclosure.

External model calls receive bounded incident evidence after deterministic redaction. No Docker credential/socket, helper credential, provider key, master key, database contents, arbitrary host files, or whole repository is sent. Some bounded health/deployment metadata, runtime state, stack traces, commit information, and relevant source diffs can leave the machine depending on the enabled subsystem. Exact fields, redaction limits, BYOK behavior, and request-level approximate usage records are documented in [model evidence boundary](docs/MODEL_EVIDENCE_BOUNDARY.md).

## Auditability

NoPager persists incident state, policy decisions, approvals, mutation request IDs, helper outcomes, verification evidence, escalation reasons, and one audit event per model operations request. The helper separately retains durable request-journal entries. Preserve both data stores for incident review. Never edit or delete an ambiguous request marker to force a replay; create a fresh incident only after a human establishes the real target state.

## Vulnerability reporting and updates

Do not report vulnerabilities in public issues. Email `security@nopager.dev` with the affected version, minimal reproduction, and impact; do not include production credentials or customer data. We aim to acknowledge reports within three business days, assess severity within seven days, and publish a fix/advisory as soon as practical. These are maintenance targets, not a paid support SLA.

Design Partner builds receive security fixes only on the documented release line. Operators must pin and record the tested version, subscribe to repository security advisories/releases, apply critical Docker/Linux/NoPager updates promptly, rotate affected credentials, and rerun the acceptance suite after every update. An unmaintained helper, Docker daemon, kernel, or host must not retain production mutation authority.
