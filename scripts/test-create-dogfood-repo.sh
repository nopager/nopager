#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
TARGET_SCRIPT="$REPO_ROOT/scripts/create-dogfood-repo.sh"

TEST_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/nopager-dogfood-test.XXXXXX")
cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT HUP INT TERM

FAKE_BIN="$TEST_ROOT/bin"
mkdir -p "$FAKE_BIN"

cat >"$FAKE_BIN/gh" <<'EOF'
#!/bin/sh
set -eu

case "${1:-} ${2:-}" in
  "auth status")
    exit 0
    ;;
  "repo view")
    if [ "${FAKE_GH_REPO_EXISTS:-0}" = "1" ]; then
      exit 0
    fi
    exit 1
    ;;
  "repo create")
    printf '%s\n' "$*" >"$FAKE_GH_CAPTURE"
    exit 0
    ;;
  *)
    echo "unexpected fake gh invocation: $*" >&2
    exit 97
    ;;
esac
EOF
chmod +x "$FAKE_BIN/gh"

CAPTURE="$TEST_ROOT/gh-create.txt"
PATH="$FAKE_BIN:$PATH" \
FAKE_GH_CAPTURE="$CAPTURE" \
  sh "$TARGET_SCRIPT" test-owner/nopager-dogfood >"$TEST_ROOT/create.out"

grep -Fq 'repo create test-owner/nopager-dogfood --private --source=. --remote=origin --push' "$CAPTURE"
grep -Fq 'Created disposable dogfood repository: https://github.com/test-owner/nopager-dogfood' "$TEST_ROOT/create.out"

PUBLIC_CAPTURE="$TEST_ROOT/gh-create-public.txt"
PATH="$FAKE_BIN:$PATH" \
FAKE_GH_CAPTURE="$PUBLIC_CAPTURE" \
NOPAGER_DOGFOOD_VISIBILITY=public \
  sh "$TARGET_SCRIPT" test-owner/nopager-dogfood-public >/dev/null

grep -Fq 'repo create test-owner/nopager-dogfood-public --public --source=. --remote=origin --push' "$PUBLIC_CAPTURE"

EXISTING_CAPTURE="$TEST_ROOT/gh-existing.txt"
if PATH="$FAKE_BIN:$PATH" \
  FAKE_GH_CAPTURE="$EXISTING_CAPTURE" \
  FAKE_GH_REPO_EXISTS=1 \
  sh "$TARGET_SCRIPT" test-owner/existing >"$TEST_ROOT/existing.out" 2>"$TEST_ROOT/existing.err"; then
  echo "error: bootstrap unexpectedly accepted an existing repository" >&2
  exit 1
fi

test ! -e "$EXISTING_CAPTURE"
grep -Fq 'refusing to overwrite an existing repository' "$TEST_ROOT/existing.err"

if PATH="$FAKE_BIN:$PATH" \
  FAKE_GH_CAPTURE="$TEST_ROOT/invalid.txt" \
  sh "$TARGET_SCRIPT" invalid-target >"$TEST_ROOT/invalid.out" 2>"$TEST_ROOT/invalid.err"; then
  echo "error: bootstrap unexpectedly accepted a target without OWNER/REPO" >&2
  exit 1
fi

grep -Fq 'target must be OWNER/REPO' "$TEST_ROOT/invalid.err"

echo "dogfood bootstrap safety checks passed"
