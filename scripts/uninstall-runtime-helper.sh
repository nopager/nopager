#!/usr/bin/env sh
set -eu

fail() {
  printf 'NoPager runtime-helper uninstall: %s\n' "$1" >&2
  exit 1
}

[ "$(id -u)" -eq 0 ] || fail "run with sudo/root."

if command -v systemctl >/dev/null 2>&1; then
  systemctl disable --now nopager-runtime-helper.service >/dev/null 2>&1 || true
  rm -f /etc/systemd/system/nopager-runtime-helper.service
  systemctl daemon-reload
fi

rm -f /run/nopager-runtime/runtime-helper.sock
rm -f /etc/nopager/runtime-helper.token /etc/nopager/runtime-helper.json
rm -f /usr/local/libexec/nopager-runtime-helper
if id nopager-runtime-helper >/dev/null 2>&1; then
  userdel nopager-runtime-helper
fi
if getent group nopager-runtime >/dev/null 2>&1; then
  groupdel nopager-runtime
fi

# Preserve the request journal by default as mutation audit evidence. Operators
# may remove /var/lib/nopager-runtime only after retaining any required records.
printf 'Runtime-helper authority revoked. The worker can no longer inspect or restart Docker targets.\n'
printf 'Preserved replay/audit journal: /var/lib/nopager-runtime/requests\n'
printf 'Also disable NOPAGER_ALLOW_CONTAINER_RESTART and clear NOPAGER_RUNTIME_HELPER_TOKEN in .env before restarting the worker.\n'
