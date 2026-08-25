# Design Partner Acceptance Test

Run this only on the disposable/staging VPS described in [the release guide](DESIGN_PARTNER_RELEASE.md). Preserve terminal output, exact version/commit, incident JSON, helper journal, worker/helper logs, Docker/Linux versions, and operator name/time as the acceptance record.

## Complete-loop test

1. Install with `sh scripts/operations-quickstart.sh`, selecting Safe Mode and a purpose-built target whose public HTTPS endpoint can be made unhealthy and whose restart makes it healthy.
2. Export the exact immutable target ID and, after reviewing the incident, the explicit approval phrase:

   ```sh
   export NOPAGER_DOCKER_TARGET="$(docker inspect --format '{{.Id}}' acceptance-target)"
   export NOPAGER_ACCEPTANCE_APPROVE='I reviewed and approve one restart'
   sh scripts/design-partner-acceptance.sh | tee acceptance-record.txt
   ```

3. When prompted, cause three consecutive external health-check failures. The script waits for detection, bounded evidence and AI reasoning to produce a typed `restart_container` plan in `WAITING_APPROVAL`.
4. The script first enables the Kill Switch, proves approval is rejected and the container start timestamp remains unchanged, then resumes protection.
5. After the explicit human phrase, it approves exactly one persisted action, waits for helper execution, requires one changed start timestamp and exactly one Docker `restart` event, and accepts only independent helper state plus external HTTP verification ending in `RESOLVED`/`VERIFIED`.

The script refuses a dirty working tree so the record names an exact tested commit and `0.2.0-design-partner.1` version. It also runs the deterministic protocol/helper failure suite and proves the live worker has no Docker socket, Docker CLI, or Docker socket group.

## Required failure and ambiguity cases

The following are release gates, not optional exploratory tests:

| Case                                        | How exercised                                                                                                                 | Required result                                                                             |
| ------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Unknown/arbitrary helper operation (`exec`) | CI sends raw invalid Unix-socket JSON; protocol unit tests deserialize exec/create/generic requests                           | `invalid_request`, `not_started`; no Docker call                                            |
| Wrong peer UID, credential, target          | `exact_enrollment_and_peer_and_credential_are_independent_gates`                                                              | independent rejection before mutation                                                       |
| Duplicate completed request                 | `duplicate_completed_restart_replays_result_without_second_mutation`                                                          | cached response; restart count remains one                                                  |
| Restart response/error ambiguous            | `ambiguous_restart_is_never_retried`                                                                                          | durable started marker; duplicate is ambiguous; restart count remains one                   |
| Control plane/current worker/same Compose   | `control_plane_and_same_compose_project_are_blocked` plus live target enrollment                                              | reject before mutation                                                                      |
| Target disappeared/replaced                 | `disappeared_replaced_and_daemon_unavailable_fail_closed`                                                                     | typed disappeared/replaced failure; no retarget                                             |
| Docker daemon unavailable                   | same deterministic backend test; additionally stop Docker only on a disposable host and confirm helper/worker preflight fails | fail closed; no action replay after daemon returns                                          |
| Helper unavailable                          | stop `nopager-runtime-helper` after an incident reaches approval, approve once                                                | operation becomes `FAILED_CLOSED`, incident escalates; restarting helper does not replay it |
| Helper timeout/lost reply                   | deterministic ambiguous test (do not manufacture this on a real target)                                                       | `AMBIGUOUS`, escalated, no second attempt                                                   |
| HTTP recovery failure                       | enroll a fixture that stays HTTP 503 across restart                                                                           | exactly one restart, `VERIFICATION_FAILED`, incident escalates                              |
| Container running but HTTP unhealthy        | same fixture; confirm helper state is running in verification JSON                                                            | not resolved; explicit unhealthy-HTTP escalation                                            |
| Kill Switch                                 | complete-loop script pauses before approval                                                                                   | HTTP 409; zero restarts while paused                                                        |
| Cooldown                                    | trigger a second otherwise identical incident inside five minutes                                                             | policy blocks the second restart and escalates                                              |

Run the deterministic subset directly when reviewing a build:

```sh
cargo test --locked -p nopager-runtime-protocol \
  -p nopager-runtime-helper -p nopager-connectors
```

## Manual helper-unavailable test

On the disposable host, create a fresh health incident and wait for Safe Mode approval. Record the action UUID, then:

```sh
sudo systemctl stop nopager-runtime-helper
curl -X POST -H "authorization: Bearer $NOPAGER_ADMIN_TOKEN" \
  "http://127.0.0.1:8080/api/v1/incidents/$INCIDENT_ID/approve"
sudo systemctl start nopager-runtime-helper
```

The old action must remain failed-closed/escalated and target restart count must not change. Never retry by deleting helper journal entries.

## Release decision

Accept only if the complete loop passes, every failure row has preserved evidence, no target restarts more than once per action UUID, and no worker Docker authority is observable. Any ambiguous or unexplained outcome is a failed release gate, even if the service eventually becomes healthy.
