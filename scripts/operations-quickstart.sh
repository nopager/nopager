#!/usr/bin/env sh
set -eu
umask 077

fail() {
  printf 'NoPager operations quickstart: %s\n' "$1" >&2
  exit 1
}

note() {
  printf '%s\n' "$1"
}

cleanup_tmp() {
  rm -f ".env.nopager-ops.$$" "/tmp/nopager-ops-response.$$" "/tmp/nopager-ops-status.$$"
  if [ "${admin_echo_disabled:-0}" = "1" ]; then
    stty echo 2>/dev/null || true
  fi
}
trap cleanup_tmp EXIT HUP INT TERM

command -v docker >/dev/null 2>&1 || fail "Docker is required."
docker compose version >/dev/null 2>&1 || fail "Docker Compose v2 is required."
command -v curl >/dev/null 2>&1 || fail "curl is required for setup preflight checks."
command -v python3 >/dev/null 2>&1 || fail "Python 3 is required for safe local JSON handling during setup."
[ -f .env.example ] || fail ".env.example is missing; run this from the NoPager repository root."

if [ ! -f .env ]; then
  cp .env.example .env
  note "Created .env from .env.example"
fi
chmod 600 .env

set_env() {
  key=$1
  value=$2
  tmp=".env.nopager-ops.$$"
  awk -v key="$key" -v value="$value" '
    BEGIN { found = 0 }
    $0 ~ ("^" key "=") { print key "=" value; found = 1; next }
    { print }
    END { if (!found) print key "=" value }
  ' .env > "$tmp"
  mv "$tmp" .env
  chmod 600 .env
}

read_env() {
  key=$1
  awk -v key="$key" '
    index($0, key "=") == 1 { sub(/^[^=]*=/, ""); print; exit }
  ' .env
}

resolve_value() {
  explicit=$1
  key=$2
  if [ -n "$explicit" ]; then
    printf '%s' "$explicit"
  else
    read_env "$key"
  fi
}

prompt_required() {
  variable_name=$1
  prompt=$2
  current=$3
  if [ -n "$current" ]; then
    printf '%s' "$current"
    return
  fi
  [ -t 0 ] || fail "$variable_name is required for non-interactive setup."
  printf '%s: ' "$prompt" >&2
  IFS= read -r entered
  [ -n "$entered" ] || fail "$variable_name cannot be empty."
  printf '%s' "$entered"
}

app_name=$(prompt_required NOPAGER_APP_NAME "Protected app name" "${NOPAGER_APP_NAME:-}")
production_url=$(prompt_required NOPAGER_PRODUCTION_URL "Public production URL (HTTPS)" "${NOPAGER_PRODUCTION_URL:-}")
health_url=${NOPAGER_HEALTH_URL:-}
if [ -z "$health_url" ]; then
  if [ -t 0 ]; then
    printf 'Public health URL (HTTPS, Enter to use production URL): ' >&2
    IFS= read -r health_url || true
  fi
  health_url=${health_url:-$production_url}
fi

docker_target=$(resolve_value "${NOPAGER_DOCKER_TARGET:-}" NOPAGER_DOCKER_TARGET)
docker_target=$(prompt_required NOPAGER_DOCKER_TARGET "Docker container name or ID NoPager may restart" "$docker_target")
provider=${NOPAGER_AI_PROVIDER:-openai}
model=$(prompt_required NOPAGER_AI_MODEL "Exact AI model ID" "${NOPAGER_AI_MODEL:-}")
safety_input=${NOPAGER_SAFETY_MODE:-safe}
admin_username=${NOPAGER_ADMIN_USERNAME:-admin}

case "$provider" in
  openai)
    provider_key=${OPENAI_API_KEY:-}
    provider_key_name=OPENAI_API_KEY
    ;;
  anthropic)
    provider_key=${ANTHROPIC_API_KEY:-}
    provider_key_name=ANTHROPIC_API_KEY
    ;;
  gemini)
    provider_key=${GEMINI_API_KEY:-}
    provider_key_name=GEMINI_API_KEY
    ;;
  *) fail "NOPAGER_AI_PROVIDER must be openai, anthropic, or gemini." ;;
esac
provider_key=$(prompt_required "$provider_key_name" "$provider provider API key" "$provider_key")

case "$safety_input" in
  safe) safety_mode=safe ;;
  autopilot|autopilot_experimental) safety_mode=autopilot_experimental ;;
  *) fail "NOPAGER_SAFETY_MODE must be safe or autopilot." ;;
esac

printf '%s' "$app_name" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._ -]{0,99}$' \
  || fail "NOPAGER_APP_NAME must be 1-100 simple printable characters."
printf '%s' "$docker_target" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$' \
  || fail "NOPAGER_DOCKER_TARGET must be a Docker name/ID without shell metacharacters."
printf '%s' "$admin_username" | grep -Eq '^[A-Za-z0-9_-]{3,64}$' \
  || fail "NOPAGER_ADMIN_USERNAME must be 3-64 letters, numbers, underscores, or hyphens."

note "Starting NoPager control plane..."
NOPAGER_QUICKSTART_MODE=operations sh scripts/quickstart.sh

api_port=$(read_env NOPAGER_API_PORT)
api_port=${api_port:-8080}
web_port=$(read_env NOPAGER_WEB_PORT)
web_port=${web_port:-3000}
admin_token=$(read_env NOPAGER_ADMIN_TOKEN)
[ -n "$admin_token" ] || fail "NOPAGER_ADMIN_TOKEN is missing after quickstart."
api_base="http://127.0.0.1:${api_port}/api/v1"

status_json=$(curl --fail --show-error --silent "$api_base/setup/status")
admin_exists=$(printf '%s' "$status_json" | python3 -c 'import json,sys; print("true" if json.load(sys.stdin).get("adminCreated") else "false")')
app_exists=$(printf '%s' "$status_json" | python3 -c 'import json,sys; print("true" if json.load(sys.stdin).get("appProtected") else "false")')

if [ "$admin_exists" != "true" ]; then
  admin_password=${NOPAGER_ADMIN_PASSWORD:-}
  if [ -z "$admin_password" ]; then
    [ -t 0 ] || fail "NOPAGER_ADMIN_PASSWORD is required for non-interactive first setup."
    printf 'Local admin password (12+ characters; not stored in .env): ' >&2
    stty -echo
    admin_echo_disabled=1
    IFS= read -r admin_password
    stty echo
    admin_echo_disabled=0
    printf '\n' >&2
  fi
  [ "${#admin_password}" -ge 12 ] || fail "Local admin password must be at least 12 characters."
  admin_payload=$(python3 -c 'import json,sys; print(json.dumps({"username":sys.argv[1],"password":sys.argv[2]}))' "$admin_username" "$admin_password")
  create_status=$(curl --show-error --silent \
    --output "/tmp/nopager-ops-response.$$" \
    --write-out '%{http_code}' \
    -H 'content-type: application/json' \
    -X POST \
    --data "$admin_payload" \
    "$api_base/setup/admin")
  case "$create_status" in
    2??) note "Created local administrator: $admin_username" ;;
    *) cat "/tmp/nopager-ops-response.$$" >&2 || true; fail "local administrator creation failed (HTTP $create_status)." ;;
  esac
fi

post_admin_json() {
  endpoint=$1
  payload=$2
  label=$3
  http_status=$(curl --show-error --silent \
    --output "/tmp/nopager-ops-response.$$" \
    --write-out '%{http_code}' \
    -H "authorization: Bearer $admin_token" \
    -H 'content-type: application/json' \
    -X POST \
    --data "$payload" \
    "$api_base/$endpoint")
  case "$http_status" in
    2??) return 0 ;;
    *)
      printf '%s failed (HTTP %s): ' "$label" "$http_status" >&2
      cat "/tmp/nopager-ops-response.$$" >&2 || true
      printf '\n' >&2
      return 1
      ;;
  esac
}

provider_payload=$(python3 -c 'import json,sys; print(json.dumps({"provider":sys.argv[1],"apiKey":sys.argv[2],"model":sys.argv[3]}))' "$provider" "$provider_key" "$model")
post_admin_json setup/test/provider "$provider_payload" "AI provider preflight" \
  || fail "AI provider/model preflight failed. No production protection was configured."
note "AI provider/model preflight passed."

production_payload=$(python3 -c 'import json,sys; print(json.dumps({"productionUrl":sys.argv[1]}))' "$production_url")
post_admin_json setup/discover/health "$production_payload" "production URL safety preflight" \
  || fail "Production URL was rejected. No production protection was configured."

health_payload=$(python3 -c 'import json,sys; print(json.dumps({"url":sys.argv[1]}))' "$health_url")
post_admin_json setup/test/health "$health_payload" "health check preflight" \
  || fail "Health URL must be public HTTPS and return HTTP 200 before protection starts."
note "External production health preflight passed."

worker_id=$(docker compose ps -q worker)
[ -n "$worker_id" ] || fail "NoPager worker container is not running."

if ! docker container inspect "$docker_target" >/dev/null 2>&1; then
  fail "Docker target '$docker_target' does not exist on this Docker daemon."
fi

target_control=$(docker inspect --format '{{index .Config.Labels "com.nopager.control-plane"}}' "$docker_target" 2>/dev/null || true)
[ "$target_control" != "true" ] || fail "refusing to protect a container marked com.nopager.control-plane=true."

target_id=$(docker inspect --format '{{.Id}}' "$docker_target")
[ "$target_id" != "$worker_id" ] || fail "refusing to target the NoPager worker itself."
worker_project=$(docker inspect --format '{{index .Config.Labels "com.docker.compose.project"}}' "$worker_id" 2>/dev/null || true)
target_project=$(docker inspect --format '{{index .Config.Labels "com.docker.compose.project"}}' "$docker_target" 2>/dev/null || true)
if [ -n "$worker_project" ] && [ "$worker_project" != "<no value>" ] \
  && [ "$target_project" = "$worker_project" ]; then
  fail "refusing a Docker target in the same Compose project as the NoPager control plane."
fi

if ! docker compose exec -T worker docker container inspect --format '{{.State.Status}}' -- "$docker_target" >/dev/null 2>&1; then
  fail "NoPager worker cannot inspect the Docker target through its runtime path. Check Docker socket access/DOCKER_GID."
fi
note "Docker target preflight passed: $docker_target"

if [ "$app_exists" = "true" ]; then
  settings_json=$(curl --fail --show-error --silent \
    -H "authorization: Bearer $admin_token" \
    "$api_base/settings") \
    || fail "existing protected-app settings could not be loaded."
  printf '%s' "$settings_json" > "/tmp/nopager-ops-status.$$"
  if ! python3 - "$production_url" "$health_url" "$docker_target" "$provider" "$model" "/tmp/nopager-ops-status.$$" <<'PY'
import json
from pathlib import Path
import sys

expected_production, expected_health, expected_target, expected_provider, expected_model, path = sys.argv[1:]
data = json.loads(Path(path).read_text())
project = data.get("project") or {}
integrations = data.get("integrations") or []
health_checks = data.get("healthChecks") or []

def integration(kind):
    return next((item for item in integrations if item.get("type") == kind), None)

docker = integration("docker_ops")
provider = integration("model_provider")
errors = []
if project.get("repoOwner") or project.get("repoName"):
    errors.append("existing app is configured for the GitHub/Vercel deployment-recovery subsystem")
if project.get("productionUrl") != expected_production:
    errors.append("production URL differs from the existing operations-only app")
if not health_checks or health_checks[0].get("url") != expected_health:
    errors.append("health URL differs from the existing operations-only app")
if not docker or (docker.get("metadata") or {}).get("targetId") != expected_target:
    errors.append("Docker target differs from the existing operations-only app")
metadata = (provider or {}).get("metadata") or {}
if not provider or metadata.get("provider") != expected_provider or metadata.get("model") != expected_model:
    errors.append("AI provider/model differs from the existing operations-only app")
if errors:
    for error in errors:
        print(f"NoPager operations quickstart: {error}.", file=sys.stderr)
    sys.exit(1)
PY
  then
    fail "refusing to silently change an existing protected app. Use a separate installation or an explicit reconfiguration path."
  fi
  note "Existing operations-only configuration matches the requested target."
else
  setup_payload=$(python3 -c 'import json,sys; print(json.dumps({"name":sys.argv[1],"provider":sys.argv[2],"providerApiKey":sys.argv[3],"providerModel":sys.argv[4],"productionUrl":sys.argv[5],"healthCheckUrl":sys.argv[6],"dockerTarget":sys.argv[7],"safetyMode":sys.argv[8]}))' \
    "$app_name" "$provider" "$provider_key" "$model" "$production_url" "$health_url" "$docker_target" "$safety_input")
  post_admin_json setup/operations-app "$setup_payload" "operations-only protected-app setup" \
    || fail "operations-only protected app was not persisted."
  note "Operations-only protected app persisted with encrypted BYOK credentials."
fi

# The model provider key is persisted encrypted in PostgreSQL by the setup API.
# Do not duplicate that secret into worker environment variables. The Docker
# target and capability flag remain explicit local hard gates for the worker.
set_env NOPAGER_AI_PROVIDER ""
set_env NOPAGER_AI_MODEL ""
set_env OPENAI_API_KEY ""
set_env ANTHROPIC_API_KEY ""
set_env GEMINI_API_KEY ""
set_env NOPAGER_DOCKER_TARGET "$docker_target"
set_env NOPAGER_ALLOW_CONTAINER_RESTART true

docker compose up -d --no-deps --force-recreate worker >/dev/null

if ! docker compose exec -T worker docker container inspect --format '{{.State.Status}}' -- "$docker_target" >/dev/null 2>&1; then
  fail "worker lost access to the configured Docker target after final configuration."
fi

note ""
note "NoPager production operations is configured."
note "Sign in: http://localhost:${web_port}/setup"
note "Login username: $admin_username"
note "Mode: $safety_mode"
note "Protected Docker target: $docker_target"
note "Health signal: $health_url"
note ""
if [ "$safety_mode" = "safe" ]; then
  note "Safe Mode behavior: NoPager monitors continuously, wakes AI after a real health incident, prepares a bounded restart plan, and waits for your approval before the production restart."
else
  note "Experimental Autopilot is enabled: policy may execute only the currently supported low-risk bounded restart after all deterministic gates pass. Safe Mode is recommended for first design-partner incidents."
fi
note "Kill Switch: use the console or 'cargo run -p nopager-cli -- pause' to block mutations while monitoring continues."
note "For the first design-partner incident, keep Safe Mode enabled and verify the incident timeline before approving the restart."
