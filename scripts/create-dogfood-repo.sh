#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
Usage: sh scripts/create-dogfood-repo.sh OWNER/REPO

Creates a new disposable GitHub repository from examples/demo-next-app.
The target repository must not already exist.

Environment:
  NOPAGER_DOGFOOD_VISIBILITY=private|public  (default: private)
EOF
}

if [ "$#" -ne 1 ]; then
  usage >&2
  exit 2
fi

TARGET_REPO=$1
VISIBILITY=${NOPAGER_DOGFOOD_VISIBILITY:-private}

case "$TARGET_REPO" in
  */*) ;;
  *)
    echo "error: target must be OWNER/REPO" >&2
    exit 2
    ;;
esac

case "$TARGET_REPO" in
  *' '*|*\\*|*/|/*|*//* )
    echo "error: invalid target repository: $TARGET_REPO" >&2
    exit 2
    ;;
esac

case "$VISIBILITY" in
  private|public) ;;
  *)
    echo "error: NOPAGER_DOGFOOD_VISIBILITY must be private or public" >&2
    exit 2
    ;;
esac

for command_name in git gh mktemp; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "error: required command not found: $command_name" >&2
    exit 1
  fi
done

if ! gh auth status >/dev/null 2>&1; then
  echo "error: GitHub CLI is not authenticated; run 'gh auth login' first" >&2
  exit 1
fi

if gh repo view "$TARGET_REPO" >/dev/null 2>&1; then
  echo "error: target repository already exists: $TARGET_REPO" >&2
  echo "refusing to overwrite an existing repository" >&2
  exit 1
fi

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
SOURCE_DIR="$REPO_ROOT/examples/demo-next-app"

if [ ! -f "$SOURCE_DIR/package.json" ]; then
  echo "error: demo app not found at $SOURCE_DIR" >&2
  exit 1
fi

WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/nopager-dogfood.XXXXXX")
cleanup() {
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT HUP INT TERM

cp -R "$SOURCE_DIR/." "$WORK_DIR/"

cat >"$WORK_DIR/.gitignore" <<'EOF'
.next/
node_modules/
.vercel/
.env
.env.*
!.env.example
*.log
EOF

cat >"$WORK_DIR/DOGFOOD.md" <<'EOF'
# NoPager disposable dogfood target

This repository was generated from `examples/demo-next-app` in NoPager.
It is intentionally breakable and must not serve real user traffic.

Use the `DEMO_SCENARIO` Vercel environment variable to exercise the supported Alpha scenarios:

- unset / `healthy`: healthy baseline;
- `build-failure`: deterministic build failure;
- `health-failure`: `/api/health` returns 503;
- `recent-regression`: health regression correlated with the new deployment.

Keep the first NoPager run in Safe Mode. Follow the release evidence checklist in
https://github.com/nopager/nopager/issues/55.
EOF

cd "$WORK_DIR"
git init -b main >/dev/null
git add .
git \
  -c user.name="NoPager Dogfood Bootstrap" \
  -c user.email="dogfood@localhost" \
  commit -m "Create disposable NoPager dogfood app" >/dev/null

gh repo create "$TARGET_REPO" "--$VISIBILITY" --source=. --remote=origin --push

printf '\nCreated disposable dogfood repository: https://github.com/%s\n' "$TARGET_REPO"
printf 'Next: import this repository into a disposable Vercel project with Production Branch main.\n'
printf 'Do not point real user traffic at this app.\n'
