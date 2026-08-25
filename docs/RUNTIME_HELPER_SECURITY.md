# Runtime-helper security architecture

## Outcome

Compromising an AI response, web process, or ordinary NoPager worker does not by itself expose the Docker API. Docker authority is confined to a small Linux host process with a closed, typed protocol.

```text
external HTTPS monitor / provider event
                 |
                 v
 web/API -> PostgreSQL <- unprivileged worker (uid 10001)
                              |
                   Unix socket + token + peer UID
                              |
                              v
              nopager-runtime-helper (trusted TCB)
                 fixed inspect / fixed restart
                              |
                              v
                       Docker Unix socket
                              |
                              v
                 one immutable enrolled target
```

The worker mounts only `/run/nopager-runtime` read-only and joins only the dedicated IPC group. It does not mount `/var/run/docker.sock`, join the Docker group, or contain the Docker CLI. The helper service account joins the Docker socket group. That privilege is intentionally not shared with the Compose stack.

## Closed protocol

Protocol v1 accepts one bounded JSON request (maximum 16 KiB) per Unix connection:

- `inspect { targetId }`
- `restart_container { targetId }`

`targetId` must be the enrolled 64-hex immutable container ID. Requests also contain a UUID request ID, protocol version, and installation credential. Unknown fields are rejected. The peer's Linux UID is obtained from `SO_PEERCRED`; the helper does not trust a caller-supplied UID.

There is intentionally no representation for `exec`, create/remove, pull/build, mounts, shell text, networking changes, Docker URLs, Docker method/path/body, or generic commands. The Docker backend constructs fixed argument arrays in trusted code and never includes caller-controlled arguments except the already-validated exact container ID.

## Independent policy checks

Before inspection or mutation, the helper checks protocol version, peer UID, credential and target ID; exact current container ID and enrolled name; configured Compose identity; the control-plane label; the explicit denylist; and separation from the NoPager Compose project. Missing, renamed/replaced, or identity-drifted targets fail closed. Bad enrollment prevents helper startup.

## Mutation state and failures

The worker persists one operations action and passes that action UUID as the helper request ID. The helper atomically creates and syncs a `started` journal record before invoking restart, then records the completed response durably.

| Condition                                      | Helper/worker behavior                     |
| ---------------------------------------------- | ------------------------------------------ |
| Helper unavailable before send                 | `FAILED_CLOSED`; escalate; no retry        |
| Response timeout/lost after send               | `AMBIGUOUS`; escalate; never replay        |
| Docker unavailable during read-only inspect    | fail closed; no mutation                   |
| Docker unavailable/restart error after claim   | ambiguous; marker remains; never replay    |
| Target disappeared or was replaced             | reject; escalate; no retargeting           |
| Duplicate completed request                    | return cached result; do not restart again |
| Duplicate started/unknown request              | ambiguous; never restart again             |
| Restart returned but post-inspect is uncertain | ambiguous; never retry                     |
| Container running, HTTP unhealthy              | verification failure; escalate             |
| External HTTP never recovers                   | verification failure; escalate             |

An operator must investigate ambiguous state directly on the host. Deleting the journal marker to force another attempt violates the safety model.

## Preserved controls and limitations

Safe Mode approval, Kill Switch, persisted action ownership, one execution attempt, cooldown, and independent external HTTP plus container-state verification remain in the database/worker. The helper adds an independent least-privilege gate. Model output remains untrusted data.

The helper, its files, Docker daemon, Linux kernel, and root administrators are trusted. Helper compromise can imply Docker-daemon and host compromise because Docker group access is typically root-equivalent. A malicious target may attack Docker/kernel vulnerabilities or falsify application behavior; external HTTP verification cannot prove the target is non-malicious.

This release does not expose shell, SSH, Kubernetes, Cloudflare, database, scaling, image-management, or general server operations.
