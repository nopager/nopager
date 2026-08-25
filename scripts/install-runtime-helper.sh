#!/usr/bin/env sh
set -eu
umask 077

fail() {
  printf 'NoPager runtime-helper install: %s\n' "$1" >&2
  exit 1
}

[ "$(id -u)" -eq 0 ] || fail "run this installer with sudo/root after building the helper as the unprivileged repository user."
[ "$#" -eq 2 ] || fail "usage: sudo sh scripts/install-runtime-helper.sh TARGET_NAME_OR_ID /absolute/path/to/nopager-runtime-helper"

target_selector=$1
helper_binary=$2
case "$helper_binary" in
  /*) ;;
  *) fail "helper binary path must be absolute." ;;
esac
[ -f "$helper_binary" ] && [ -x "$helper_binary" ] || fail "runtime-helper binary is missing or not executable: $helper_binary"

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
env_file="$repo_root/.env"
[ -f "$env_file" ] || fail ".env is missing; run scripts/quickstart.sh first so the IPC credential exists."
command -v docker >/dev/null 2>&1 || fail "Docker CLI is required on the host."
command -v systemctl >/dev/null 2>&1 || fail "systemd is required for the Design Partner helper service."
command -v python3 >/dev/null 2>&1 || fail "Python 3 is required to render strict enrollment JSON."

read_env() {
  key=$1
  awk -v key="$key" 'index($0, key "=") == 1 { sub(/^[^=]*=/, ""); print; exit }' "$env_file"
}

credential=$(read_env NOPAGER_RUNTIME_HELPER_TOKEN)
[ "${#credential}" -ge 32 ] || fail "NOPAGER_RUNTIME_HELPER_TOKEN is missing or too short."

docker info >/dev/null 2>&1 || fail "Docker daemon is unavailable."
target_id=$(docker inspect --format '{{.Id}}' -- "$target_selector" 2>/dev/null) \
  || fail "target does not exist: $target_selector"
target_name=$(docker inspect --format '{{.Name}}' -- "$target_id" | sed 's#^/##')
target_project=$(docker inspect --format '{{index .Config.Labels "com.docker.compose.project"}}' -- "$target_id" 2>/dev/null || true)
[ "$target_project" != "<no value>" ] || target_project=
target_control=$(docker inspect --format '{{index .Config.Labels "com.nopager.control-plane"}}' -- "$target_id" 2>/dev/null || true)
[ "$target_control" != "true" ] || fail "target is labeled as a NoPager control-plane resource."

compose_json=$(docker compose --project-directory "$repo_root" -f "$repo_root/docker-compose.yml" config --format json)
control_project=$(printf '%s' "$compose_json" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("name", ""))')
[ -n "$control_project" ] || fail "NoPager control-plane Compose project could not be determined."
[ "$target_project" != "$control_project" ] || fail "target belongs to the NoPager control-plane Compose project."

docker_socket=/var/run/docker.sock
[ -S "$docker_socket" ] || fail "Docker Unix socket is unavailable at $docker_socket."
docker_gid=$(stat -c '%g' "$docker_socket")
docker_group=$(getent group "$docker_gid" | cut -d: -f1)
[ -n "$docker_group" ] || fail "Docker socket group could not be resolved from gid $docker_gid."

getent group nopager-runtime >/dev/null 2>&1 || groupadd --system nopager-runtime
if ! id nopager-runtime-helper >/dev/null 2>&1; then
  useradd --system --no-create-home --shell /usr/sbin/nologin --gid nopager-runtime nopager-runtime-helper
fi
usermod -a -G "$docker_group" nopager-runtime-helper
runtime_gid=$(getent group nopager-runtime | cut -d: -f3)
[ "$runtime_gid" != "$docker_gid" ] || fail "dedicated runtime IPC group must not be the Docker socket group."

install -d -m 0750 -o root -g nopager-runtime /etc/nopager
install -d -m 0750 -o nopager-runtime-helper -g nopager-runtime /run/nopager-runtime
install -d -m 0700 -o nopager-runtime-helper -g nopager-runtime /var/lib/nopager-runtime/requests
install -d -m 0755 -o root -g root /usr/local/libexec
install -m 0755 -o root -g root "$helper_binary" /usr/local/libexec/nopager-runtime-helper
printf '%s\n' "$credential" > /etc/nopager/runtime-helper.token
chown root:nopager-runtime /etc/nopager/runtime-helper.token
chmod 0640 /etc/nopager/runtime-helper.token

denied_ids=$(docker ps -aq --filter label=com.nopager.control-plane=true | python3 -c 'import json,sys; print(json.dumps([line.strip() for line in sys.stdin if line.strip()]))')
python3 - "$target_id" "$target_name" "$target_project" "$control_project" "$denied_ids" > /etc/nopager/runtime-helper.json <<'PY'
import json
import sys

target_id, target_name, target_project, control_project, denied_json = sys.argv[1:]
denied = [value for value in json.loads(denied_json) if value != target_id]
print(json.dumps({
    "protocolVersion": 1,
    "allowedPeerUid": 10001,
    "controlPlaneComposeProject": control_project,
    "enrolledTarget": {
        "id": target_id,
        "name": target_name,
        "composeProject": target_project or None,
    },
    "deniedContainerIds": denied,
}, indent=2))
PY
chown root:root /etc/nopager/runtime-helper.json
chmod 0644 /etc/nopager/runtime-helper.json

cat > /etc/systemd/system/nopager-runtime-helper.service <<'UNIT'
[Unit]
Description=NoPager deterministic Docker runtime helper
Documentation=https://github.com/nopager/nopager/blob/main/docs/RUNTIME_HELPER_SECURITY.md
After=docker.service
Requires=docker.service

[Service]
Type=simple
User=nopager-runtime-helper
Group=nopager-runtime
SupplementaryGroups=docker
ExecStart=/usr/local/libexec/nopager-runtime-helper
Restart=on-failure
RestartSec=5s
UMask=0007
NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
ProtectSystem=strict
ProtectHome=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectKernelLogs=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_UNIX
RestrictNamespaces=true
LockPersonality=true
MemoryDenyWriteExecute=true
ReadWritePaths=/run/nopager-runtime /var/lib/nopager-runtime

[Install]
WantedBy=multi-user.target
UNIT

# The Docker group name may not literally be "docker". Keep the hardened unit
# declarative while substituting only the host-resolved group identifier.
sed -i "s/^SupplementaryGroups=docker$/SupplementaryGroups=$docker_group/" /etc/systemd/system/nopager-runtime-helper.service

systemctl daemon-reload
systemctl enable --now nopager-runtime-helper.service >/dev/null
attempt=0
while [ "$attempt" -lt 20 ]; do
  [ -S /run/nopager-runtime/runtime-helper.sock ] && break
  sleep 1
  attempt=$((attempt + 1))
done
systemctl is-active --quiet nopager-runtime-helper.service \
  || { journalctl -u nopager-runtime-helper.service --no-pager -n 50 >&2 || true; fail "helper service did not become active."; }
[ -S /run/nopager-runtime/runtime-helper.sock ] || fail "helper Unix socket was not created."

printf 'NOPAGER_RUNTIME_GID=%s\n' "$runtime_gid"
printf 'NOPAGER_RUNTIME_SOCKET_DIR=/run/nopager-runtime\n'
printf 'NOPAGER_DOCKER_TARGET=%s\n' "$target_id"
printf 'Runtime helper enrolled %s (%s); worker receives no Docker socket authority.\n' "$target_name" "$target_id"
