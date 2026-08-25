#!/usr/bin/env sh
set -eu

fail() { printf 'Design Partner Acceptance: %s\n' "$1" >&2; exit 1; }
note() { printf '%s\n' "$1"; }

[ -f DESIGN_PARTNER_VERSION ] || fail "run from the repository root."
version=$(tr -d '\r\n' < DESIGN_PARTNER_VERSION)
[ "$version" = "0.2.0-design-partner.1" ] || fail "unexpected release version: $version"
commit=$(git rev-parse HEAD 2>/dev/null) || fail "an exact Git commit is required."
[ -z "$(git status --porcelain)" ] || fail "working tree is not clean; acceptance must name immutable tested source."
command -v docker >/dev/null 2>&1 || fail "Docker is required."
command -v curl >/dev/null 2>&1 || fail "curl is required."
command -v python3 >/dev/null 2>&1 || fail "Python 3 is required."
command -v cargo >/dev/null 2>&1 || fail "Rust/Cargo 1.92 is required."

cargo test --locked -p nopager-runtime-protocol -p nopager-runtime-helper -p nopager-connectors \
  || fail "protocol/helper failure and ambiguity tests failed."

api_base=${NOPAGER_ACCEPTANCE_API_BASE:-http://127.0.0.1:8080/api/v1}
token=${NOPAGER_ADMIN_TOKEN:-}
if [ -z "$token" ] && [ -f .env ]; then
  token=$(awk -F= '$1 == "NOPAGER_ADMIN_TOKEN" { sub(/^[^=]*=/, ""); print; exit }' .env)
fi
[ -n "$token" ] || fail "NOPAGER_ADMIN_TOKEN is required."
target_id=${NOPAGER_DOCKER_TARGET:-}
if [ -z "$target_id" ] && [ -f .env ]; then
  target_id=$(awk -F= '$1 == "NOPAGER_DOCKER_TARGET" { sub(/^[^=]*=/, ""); print; exit }' .env)
fi
printf '%s' "$target_id" | grep -Eq '^[0-9a-f]{64}$' || fail "exact immutable NOPAGER_DOCKER_TARGET is required."

auth="authorization: Bearer $token"
worker=$(docker compose ps -q worker)
[ -n "$worker" ] || fail "worker is not running."
docker_gid=$(stat -c '%g' /var/run/docker.sock)
! docker inspect --format '{{range .Mounts}}{{println .Source}}{{end}}' "$worker" | grep -q '/var/run/docker.sock' \
  || fail "worker mounts the Docker socket."
docker compose exec -T worker sh -c 'test ! -S /var/run/docker.sock && ! command -v docker >/dev/null 2>&1' \
  || fail "worker has direct Docker access."
! docker compose exec -T worker sh -c "id -G | tr ' ' '\n' | grep -qx '$docker_gid'" \
  || fail "worker joined the Docker socket group."

note "Acceptance version=$version commit=$commit"
note "Boundary smoke passed: worker has no Docker socket, CLI, or Docker group."
note "Cause three consecutive failures at the configured public HTTPS health URL now."

timeout_seconds=${NOPAGER_ACCEPTANCE_TIMEOUT_SECONDS:-600}
elapsed=0
incident_id=
while [ "$elapsed" -lt "$timeout_seconds" ]; do
  incident_id=$(curl --fail --silent --show-error -H "$auth" "$api_base/incidents" | python3 -c '
import json,sys
items=json.load(sys.stdin).get("incidents", [])
print(next((x["id"] for x in items if x.get("status")=="WAITING_APPROVAL"), ""))
')
  [ -n "$incident_id" ] && break
  sleep 5
  elapsed=$((elapsed + 5))
done
[ -n "$incident_id" ] || fail "no Safe Mode WAITING_APPROVAL incident appeared."

detail=$(curl --fail --silent --show-error -H "$auth" "$api_base/incidents/$incident_id")
DETAIL_JSON=$detail python3 -c '
import json,os,sys
d=json.loads(os.environ["DETAIL_JSON"])
op=d.get("currentOperation") or {}
assert d.get("safetyMode") == "safe", d
assert op.get("actionKind") == "restart_container", op
assert op.get("targetId") == sys.argv[1], op
assert op.get("status") == "APPROVAL_REQUIRED", op
' "$target_id" || exit 1

before=$(docker inspect --format '{{.State.StartedAt}}' "$target_id")
event_log="/tmp/nopager-acceptance-restart-events.$$"
event_pid=
cleanup_events() {
  [ -z "$event_pid" ] || kill "$event_pid" >/dev/null 2>&1 || true
  [ -z "$event_pid" ] || wait "$event_pid" 2>/dev/null || true
  rm -f "$event_log"
}
trap cleanup_events EXIT HUP INT TERM
docker events --filter type=container --filter container="$target_id" --filter event=restart \
  --format '{{.Action}}' > "$event_log" &
event_pid=$!
sleep 1
curl --fail --silent --show-error -X POST -H "$auth" "$api_base/protection/pause" >/dev/null
status=$(curl --silent --output /tmp/nopager-acceptance-approve.json --write-out '%{http_code}' -X POST -H "$auth" "$api_base/incidents/$incident_id/approve")
[ "$status" = "409" ] || fail "Kill Switch did not block approval (HTTP $status)."
after_block=$(docker inspect --format '{{.State.StartedAt}}' "$target_id")
[ "$after_block" = "$before" ] || fail "target restarted while Kill Switch was active."
curl --fail --silent --show-error -X POST -H "$auth" "$api_base/protection/resume" >/dev/null

note "Kill Switch proof passed. Review incident $incident_id in the UI."
if [ "${NOPAGER_ACCEPTANCE_APPROVE:-}" != "I reviewed and approve one restart" ]; then
  fail "set NOPAGER_ACCEPTANCE_APPROVE='I reviewed and approve one restart' only after human review, then rerun while the incident is waiting."
fi
curl --fail --silent --show-error -X POST -H "$auth" "$api_base/incidents/$incident_id/approve" >/dev/null

elapsed=0
final_status=
while [ "$elapsed" -lt "$timeout_seconds" ]; do
  detail=$(curl --fail --silent --show-error -H "$auth" "$api_base/incidents/$incident_id")
  final_status=$(printf '%s' "$detail" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("status", ""))')
  case "$final_status" in RESOLVED|ESCALATED|FAILED) break;; esac
  sleep 5
  elapsed=$((elapsed + 5))
done
after=$(docker inspect --format '{{.State.StartedAt}}' "$target_id")
[ "$after" != "$before" ] || fail "target did not expose a restart transition."
sleep 1
kill "$event_pid" >/dev/null 2>&1 || true
wait "$event_pid" 2>/dev/null || true
event_pid=
restart_events=$(grep -c '^restart$' "$event_log" || true)
[ "$restart_events" -eq 1 ] || fail "expected exactly one Docker restart event; observed $restart_events"
DETAIL_JSON=$detail python3 -c '
import json,os
d=json.loads(os.environ["DETAIL_JSON"])
op=d.get("currentOperation") or {}
assert op.get("status") == "VERIFIED", op
assert op.get("execution") is not None, op
assert op.get("verification") is not None, op
' || exit 1

note "Complete loop reached $final_status with exactly one observed Docker restart event."
[ "$final_status" = "RESOLVED" ] || fail "independent verification escalated/failed; preserve evidence and investigate."
note "PASS version=$version commit=$commit incident=$incident_id"
