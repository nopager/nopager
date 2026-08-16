#!/usr/bin/env sh
set -eu
umask 077

fail() {
  printf 'NoPager quickstart: %s\n' "$1" >&2
  exit 1
}

cleanup_tmp() {
  rm -f ".env.nopager.$$"
}
trap cleanup_tmp EXIT HUP INT TERM

command -v docker >/dev/null 2>&1 || fail "Docker is required."
docker compose version >/dev/null 2>&1 || fail "Docker Compose v2 is required."
docker info >/dev/null 2>&1 || fail "Docker daemon is not reachable."

docker_server_version=$(docker version --format '{{.Server.Version}}' 2>/dev/null || true)
[ -n "$docker_server_version" ] || fail "Docker Engine version could not be detected."
docker_server_major=${docker_server_version%%.*}
case "$docker_server_major" in
  ''|*[!0-9]*) fail "Docker Engine returned an unsupported version string: $docker_server_version" ;;
esac
[ "$docker_server_major" -ge 26 ] || fail "Docker Engine 26+ is required for repair-workspace volume subpath isolation (found $docker_server_version)."
printf 'Docker Engine %s detected.\n' "$docker_server_version"

if command -v curl >/dev/null 2>&1; then
  http_get() {
    curl --fail --show-error --silent "$1" >/dev/null 2>&1
  }
elif command -v wget >/dev/null 2>&1; then
  http_get() {
    wget --quiet --output-document=/dev/null "$1" >/dev/null 2>&1
  }
else
  fail "curl or wget is required so quickstart can verify API and console readiness."
fi

[ -f .env.example ] || fail ".env.example is missing; run quickstart from the NoPager repository root."

if [ ! -f .env ]; then
  cp .env.example .env
  printf 'Created .env from .env.example\n'
fi
chmod 600 .env

set_env() {
  key=$1
  value=$2
  tmp=".env.nopager.$$"
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

generate_secret() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -base64 32 | tr -d '\n'
  elif command -v python3 >/dev/null 2>&1; then
    python3 -c 'import base64, os; print(base64.b64encode(os.urandom(32)).decode())'
  else
    fail "OpenSSL or Python 3 is required once to generate NoPager secrets."
  fi
}

generate_uri_secret() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 32 | tr -d '\n'
  elif command -v python3 >/dev/null 2>&1; then
    python3 -c 'import secrets; print(secrets.token_hex(32))'
  else
    fail "OpenSSL or Python 3 is required once to generate the PostgreSQL password."
  fi
}

current_postgres_password=$(read_env POSTGRES_PASSWORD)
if [ -z "$current_postgres_password" ]; then
  legacy_database_url=$(read_env DATABASE_URL)
  case "$legacy_database_url" in
    postgresql://nopager:nopager@postgres:*)
      set_env POSTGRES_PASSWORD nopager
      printf 'Preserved legacy Alpha PostgreSQL password for the existing Docker volume.\n' >&2
      ;;
    *)
      set_env POSTGRES_PASSWORD "$(generate_uri_secret)"
      printf 'Generated POSTGRES_PASSWORD\n'
      ;;
  esac
fi

current_master_key=$(read_env NOPAGER_MASTER_KEY)
if [ -z "$current_master_key" ]; then
  set_env NOPAGER_MASTER_KEY "$(generate_secret)"
  printf 'Generated NOPAGER_MASTER_KEY\n'
fi

current_admin_token=$(read_env NOPAGER_ADMIN_TOKEN)
if [ -z "$current_admin_token" ]; then
  set_env NOPAGER_ADMIN_TOKEN "$(generate_secret)"
  printf 'Generated NOPAGER_ADMIN_TOKEN for local CLI/operator access\n'
fi

docker_gid=$(docker run --rm -v /var/run/docker.sock:/sock alpine:3.22 stat -c '%g' /sock 2>/dev/null || true)
if [ -n "$docker_gid" ]; then
  set_env DOCKER_GID "$docker_gid"
  printf 'Detected Docker socket group: %s\n' "$docker_gid"
else
  printf 'Warning: could not detect Docker socket group; keeping DOCKER_GID from .env.\n' >&2
fi

web_port=$(read_env NOPAGER_WEB_PORT)
web_port=${web_port:-3000}
api_port=$(read_env NOPAGER_API_PORT)
api_port=${api_port:-8080}

docker compose config --quiet || fail "Docker Compose configuration is invalid."

printf 'Building and starting NoPager...\n'
if ! docker compose up -d --build; then
  docker compose ps >&2 || true
  docker compose logs --no-color --tail=200 server worker web postgres >&2 || true
  fail "Docker Compose failed to start NoPager."
fi

printf 'Waiting for API and web console readiness...\n'
ready=0
attempt=1
while [ "$attempt" -le 60 ]; do
  if http_get "http://127.0.0.1:${api_port}/healthz" \
    && http_get "http://127.0.0.1:${web_port}/api/nopager/setup/status"; then
    ready=1
    break
  fi
  sleep 2
  attempt=$((attempt + 1))
done
if [ "$ready" -ne 1 ]; then
  docker compose ps >&2 || true
  docker compose logs --no-color --tail=200 server worker web postgres >&2 || true
  fail "services did not become ready within 120 seconds."
fi

printf '\nNoPager is ready.\n'
printf 'Console: http://localhost:%s/setup\n' "$web_port"
printf 'Local API health: http://127.0.0.1:%s/healthz\n' "$api_port"
printf '\nNext: open the console and complete GitHub, Vercel, AI provider, and health-check setup.\n'
printf 'CLI/operator commands read NOPAGER_ADMIN_TOKEN from .env automatically.\n'
printf 'Back up .env together with the PostgreSQL volume; losing NOPAGER_MASTER_KEY makes encrypted integration credentials unrecoverable.\n'
printf 'Logs: docker compose logs -f server worker web\n'
