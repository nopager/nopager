#!/usr/bin/env sh
set -eu
umask 077

fail() {
  printf 'runtime boundary smoke: %s\n' "$1" >&2
  exit 1
}

command -v docker >/dev/null 2>&1 || fail "Docker is required"
command -v cargo >/dev/null 2>&1 || fail "Cargo is required"
command -v python3 >/dev/null 2>&1 || fail "Python 3 is required"
command -v timeout >/dev/null 2>&1 || fail "GNU timeout is required"
[ "$(uname -s)" = "Linux" ] || fail "this smoke test is Linux-only"

for container in nopager-ci-ops-target nopager-ci-unenrolled nopager-ci-same-project \
  nopager-ci-replaced nopager-ci-replaced-old; do
  docker rm -f "$container" >/dev/null 2>&1 || true
done

docker run -d --name nopager-ci-ops-target \
  --label com.docker.compose.project=customer-ci \
  alpine:3.22 sleep 600 >/dev/null
docker run -d --name nopager-ci-unenrolled \
  --label com.docker.compose.project=customer-ci \
  alpine:3.22 sleep 600 >/dev/null

target_id=$(docker inspect --format '{{.Id}}' nopager-ci-ops-target)
unenrolled_id=$(docker inspect --format '{{.Id}}' nopager-ci-unenrolled)
token=$(awk -F= '$1 == "NOPAGER_RUNTIME_HELPER_TOKEN" { sub(/^[^=]*=/, ""); print; exit }' .env)
[ "${#token}" -ge 32 ] || fail "runtime helper token is missing"
docker_gid=$(stat -c '%g' /var/run/docker.sock)
runtime_gid=49001
[ "$runtime_gid" != "$docker_gid" ] || runtime_gid=49002
control_project=$(docker compose config --format json | python3 -c 'import json,sys; print(json.load(sys.stdin)["name"])')
[ -n "$control_project" ] || fail "control-plane Compose project is missing"

mkdir -p .runtime/ci-ipc .runtime/ci-state .runtime/unsafe
sudo chgrp "$runtime_gid" .runtime/ci-ipc
sudo chmod 2777 .runtime/ci-ipc
cat > .runtime/ci-helper.json <<JSON
{"protocolVersion":1,"allowedPeerUid":10001,"controlPlaneComposeProject":"$control_project","enrolledTarget":{"id":"$target_id","name":"nopager-ci-ops-target","composeProject":"customer-ci"},"deniedContainerIds":[]}
JSON
printf '%s\n' "$token" > .runtime/ci-helper.token

cargo build --release --locked -p nopager-runtime-helper
nohup target/release/nopager-runtime-helper \
  --config "$PWD/.runtime/ci-helper.json" \
  --credential-file "$PWD/.runtime/ci-helper.token" \
  --socket "$PWD/.runtime/ci-ipc/runtime-helper.sock" \
  --state-directory "$PWD/.runtime/ci-state" \
  --docker-program /usr/bin/docker \
  > .runtime/ci-helper.log 2>&1 &
echo $! > .runtime/ci-helper.pid
attempt=0
while [ "$attempt" -lt 20 ]; do
  [ -S .runtime/ci-ipc/runtime-helper.sock ] && break
  sleep 1
  attempt=$((attempt + 1))
done
[ -S .runtime/ci-ipc/runtime-helper.sock ] \
  || { cat .runtime/ci-helper.log >&2; fail "helper socket was not created"; }

cat > .runtime/helper-client.py <<'PY'
import json
import os
import socket
import sys

socket_path, action, target_id, request_id = sys.argv[1:]
credential = os.environ['HELPER_TOKEN']
if action == 'raw_exec':
    payload = {'action': {'type': 'exec', 'command': 'id'}}
else:
    payload = {
        'protocolVersion': 1,
        'requestId': request_id,
        'credential': credential,
        'action': {'type': action, 'target_id': target_id},
    }
client = socket.socket(socket.AF_UNIX)
client.connect(socket_path)
client.sendall(json.dumps(payload, separators=(',', ':')).encode())
client.shutdown(socket.SHUT_WR)
chunks = []
while True:
    chunk = client.recv(16384)
    if not chunk:
        break
    chunks.append(chunk)
print(b''.join(chunks).decode())
PY
chmod 0444 .runtime/helper-client.py

client_request() {
  action=$1
  requested_target=$2
  request_id=$3
  docker run --rm \
    --user 10001 \
    --group-add "$runtime_gid" \
    --network none \
    -e HELPER_TOKEN="$token" \
    -v "$PWD/.runtime/ci-ipc:/ipc" \
    -v "$PWD/.runtime/helper-client.py:/client.py:ro" \
    python:3.13-alpine \
    python /client.py /ipc/runtime-helper.sock "$action" "$requested_target" "$request_id"
}

assert_rejection() {
  expected=$1
  python3 -c 'import json,sys; value=json.load(sys.stdin); assert value["result"]["status"] == "rejected", value; assert value["result"]["code"] == sys.argv[1], value' "$expected"
}

inspect_id=018f0000-0000-7000-8000-000000000001
inspect_response=$(client_request inspect "$target_id" "$inspect_id")
printf '%s' "$inspect_response" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert value["result"]["status"] == "success", value; assert value["result"]["operation"] == "inspect", value; assert value["result"]["state"]["id"] == sys.argv[1], value' "$target_id"

[ "$(id -u)" != "10001" ] || fail "host runner unexpectedly has the allowed worker UID"
wrong_uid_response=$(HELPER_TOKEN="$token" python3 .runtime/helper-client.py \
  "$PWD/.runtime/ci-ipc/runtime-helper.sock" inspect "$target_id" 018f0000-0000-7000-8000-000000000002)
printf '%s' "$wrong_uid_response" | assert_rejection peer_not_allowed

invalid_credential_response=$(docker run --rm --user 10001 --group-add "$runtime_gid" --network none \
  -e HELPER_TOKEN=wrong-credential-with-at-least-32-bytes \
  -v "$PWD/.runtime/ci-ipc:/ipc" \
  -v "$PWD/.runtime/helper-client.py:/client.py:ro" \
  python:3.13-alpine python /client.py /ipc/runtime-helper.sock inspect "$target_id" \
  018f0000-0000-7000-8000-000000000003)
printf '%s' "$invalid_credential_response" | assert_rejection authentication_failed

unenrolled_response=$(client_request inspect "$unenrolled_id" 018f0000-0000-7000-8000-000000000004)
printf '%s' "$unenrolled_response" | assert_rejection target_not_enrolled
raw_response=$(client_request raw_exec "$target_id" 018f0000-0000-7000-8000-000000000005)
printf '%s' "$raw_response" | assert_rejection invalid_request

restart_id=018f0000-0000-7000-8000-000000000006
restart_before=$(docker inspect --format '{{.State.StartedAt}}' "$target_id")
event_log="$PWD/.runtime/ci-restart-events.log"
docker events --filter type=container --filter container="$target_id" --filter event=restart \
  --format '{{.Action}}' > "$event_log" &
event_pid=$!
sleep 1
restart_response=$(client_request restart_container "$target_id" "$restart_id")
printf '%s' "$restart_response" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert value["result"]["status"] == "success", value; assert value["result"]["operation"] == "restart_container", value; assert value["duplicate"] is False, value'
restart_once=$(docker inspect --format '{{.State.StartedAt}}' "$target_id")
[ "$restart_once" != "$restart_before" ] || fail "target did not expose a restart transition"
duplicate_response=$(client_request restart_container "$target_id" "$restart_id")
printf '%s' "$duplicate_response" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert value["result"]["status"] == "success", value; assert value["duplicate"] is True, value'
restart_after=$(docker inspect --format '{{.State.StartedAt}}' "$target_id")
[ "$restart_after" = "$restart_once" ] || fail "duplicate request caused another restart transition"
sleep 1
kill "$event_pid" >/dev/null 2>&1 || true
wait "$event_pid" 2>/dev/null || true
restart_events=$(grep -c '^restart$' "$event_log" || true)
[ "$restart_events" -eq 1 ] || fail "expected exactly one Docker restart event, observed $restart_events"

ambiguous_id=018f0000-0000-7000-8000-000000000007
cat > ".runtime/ci-state/$ambiguous_id.json" <<JSON
{"requestId":"$ambiguous_id","targetId":"$target_id","status":"started","response":null}
JSON
ambiguous_before=$(docker inspect --format '{{.State.StartedAt}}' "$target_id")
ambiguous_response=$(client_request restart_container "$target_id" "$ambiguous_id")
printf '%s' "$ambiguous_response" | assert_rejection duplicate_ambiguous
ambiguous_after=$(docker inspect --format '{{.State.StartedAt}}' "$target_id")
[ "$ambiguous_after" = "$ambiguous_before" ] \
  || fail "ambiguous journal request was replayed"

python3 - "$target_id" "$token" "$runtime_gid" <<'PY'
from pathlib import Path
import sys

target, token, runtime_gid = sys.argv[1:]
path = Path('.env')
values = {
    'NOPAGER_DOCKER_TARGET': target,
    'NOPAGER_ALLOW_CONTAINER_RESTART': 'true',
    'NOPAGER_RUNTIME_HELPER_TOKEN': token,
    'NOPAGER_RUNTIME_SOCKET_DIR': str(Path('.runtime/ci-ipc').resolve()),
    'NOPAGER_RUNTIME_GID': runtime_gid,
}
lines = path.read_text().splitlines()
rendered = []
seen = set()
for line in lines:
    key = line.split('=', 1)[0] if '=' in line else None
    if key in values:
        rendered.append(f'{key}={values[key]}')
        seen.add(key)
    else:
        rendered.append(line)
rendered.extend(f'{key}={value}' for key, value in values.items() if key not in seen)
path.write_text('\n'.join(rendered) + '\n')
PY
docker compose up -d --no-deps --force-recreate worker >/dev/null
sleep 5
worker=$(docker compose ps -q worker)
[ "$(docker inspect --format '{{.State.Running}}' "$worker")" = "true" ] \
  || { docker compose logs --tail=100 worker >&2; fail "worker helper preflight failed"; }
! docker inspect --format '{{range .Mounts}}{{println .Source}}{{end}}' "$worker" \
  | grep -q '/var/run/docker.sock' || fail "worker mounts Docker socket"
docker compose exec -T worker sh -c 'test ! -S /var/run/docker.sock && test -z "${DOCKER_HOST:-}" && ! command -v docker >/dev/null 2>&1' \
  || fail "worker has a direct Docker client/socket"
! docker compose exec -T worker sh -c "id -G | tr ' ' '\n' | grep -qx '$docker_gid'" \
  || fail "worker joined Docker socket group"

expect_unsafe_enrollment() {
  label=$1
  expected=$2
  config_path=$3
  socket_path="$PWD/.runtime/unsafe/$label.sock"
  state_path="$PWD/.runtime/unsafe/$label-state"
  mkdir -p "$state_path"
  if timeout 5 target/release/nopager-runtime-helper \
    --config "$config_path" \
    --credential-file "$PWD/.runtime/ci-helper.token" \
    --socket "$socket_path" \
    --state-directory "$state_path" \
    --docker-program /usr/bin/docker \
    > ".runtime/unsafe/$label.log" 2>&1; then
    fail "unsafe helper enrollment unexpectedly started: $label"
  fi
  grep -q "$expected" ".runtime/unsafe/$label.log" \
    || { cat ".runtime/unsafe/$label.log" >&2; fail "unsafe enrollment failed for the wrong reason: $label"; }
}

worker_name=$(docker inspect --format '{{.Name}}' "$worker" | sed 's#^/##')
worker_project=$(docker inspect --format '{{index .Config.Labels "com.docker.compose.project"}}' "$worker")
cat > .runtime/unsafe/worker.json <<JSON
{"protocolVersion":1,"allowedPeerUid":10001,"controlPlaneComposeProject":"$control_project","enrolledTarget":{"id":"$worker","name":"$worker_name","composeProject":"$worker_project"},"deniedContainerIds":[]}
JSON
expect_unsafe_enrollment worker ControlPlaneTarget "$PWD/.runtime/unsafe/worker.json"

docker run -d --name nopager-ci-same-project \
  --label com.docker.compose.project="$control_project" \
  alpine:3.22 sleep 600 >/dev/null
same_id=$(docker inspect --format '{{.Id}}' nopager-ci-same-project)
cat > .runtime/unsafe/same-project.json <<JSON
{"protocolVersion":1,"allowedPeerUid":10001,"controlPlaneComposeProject":"$control_project","enrolledTarget":{"id":"$same_id","name":"nopager-ci-same-project","composeProject":"$control_project"},"deniedContainerIds":[]}
JSON
expect_unsafe_enrollment same-project SameControlPlaneProject "$PWD/.runtime/unsafe/same-project.json"

docker run -d --name nopager-ci-replaced \
  --label com.docker.compose.project=customer-ci \
  alpine:3.22 sleep 600 >/dev/null
replaced_id=$(docker inspect --format '{{.Id}}' nopager-ci-replaced)
docker rename nopager-ci-replaced nopager-ci-replaced-old
docker run -d --name nopager-ci-replaced \
  --label com.docker.compose.project=customer-ci \
  alpine:3.22 sleep 600 >/dev/null
cat > .runtime/unsafe/replaced.json <<JSON
{"protocolVersion":1,"allowedPeerUid":10001,"controlPlaneComposeProject":"$control_project","enrolledTarget":{"id":"$replaced_id","name":"nopager-ci-replaced","composeProject":"customer-ci"},"deniedContainerIds":[]}
JSON
expect_unsafe_enrollment replaced TargetReplaced "$PWD/.runtime/unsafe/replaced.json"

printf 'runtime boundary smoke passed\n'
