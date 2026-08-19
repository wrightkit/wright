#!/usr/bin/env bash
# Functional tests for install.sh against a mock release server (#108).
#
# Serves fake release archives and checksums over a local HTTP server and
# exercises install.sh end to end: platform mapping, checksum verification,
# archive layout, version resolution, and failure modes. Runs on Linux and
# macOS runners; WRIGHT_INSTALL_OS/ARCH overrides cover the other
# architectures.
#
# Usage: scripts/test-install.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALLER="$ROOT/install.sh"
VERSION="9.9.9"
WORK="$(mktemp -d)"
PORT=18765
PASS=0
FAIL=0
INSTALL_OUTPUT="$WORK/wright-install.out"

cleanup() {
  [[ -n "${SERVER_PID:-}" ]] && kill "$SERVER_PID" 2>/dev/null || true
  [[ -n "${BROKEN_PID:-}" ]] && kill "$BROKEN_PID" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

# --- mock release tree -------------------------------------------------------

make_fake_binary() {
  cat > "$1" <<EOF
#!/bin/sh
echo "fake $2 $VERSION"
EOF
  chmod +x "$1"
}

make_archive() {
  local root="$1" triple="$2"
  local release="$root/v$VERSION"
  local dir="$release/wright-$VERSION-$triple"
  local archive="$release/wright-$VERSION-$triple.tar.gz"
  mkdir -p "$dir"
  make_fake_binary "$dir/wright" wright
  make_fake_binary "$dir/wright-lsp" wright-lsp
  printf '{"version":"%s"}\n' "$VERSION" > "$dir/version.json"
  tar -czf "$archive" -C "$release" "wright-$VERSION-$triple"
  (cd "$release" && shasum -a 256 "wright-$VERSION-$triple.tar.gz" > "wright-$VERSION-$triple.tar.gz.sha256")
  rm -rf "$dir"
}

wait_for_server() {
  local base_url="$1"
  for _ in $(seq 1 50); do
    if curl -fsS "$base_url/" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.1
  done
  echo "mock release server did not become ready: $base_url" >&2
  return 1
}

for triple in x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-apple-darwin; do
  make_archive "$WORK/mock" "$triple"
done

mkdir -p "$WORK/mock/repos/wrightkit/wright/releases"
cat > "$WORK/mock/repos/wrightkit/wright/releases/latest" <<EOF
{"tag_name": "v$VERSION", "draft": false, "prerelease": false}
EOF

python3 -m http.server "$PORT" --directory "$WORK/mock" >/dev/null 2>&1 &
SERVER_PID=$!
BASE_URL="http://127.0.0.1:$PORT"
API_URL="$BASE_URL/repos/wrightkit/wright/releases/latest"
wait_for_server "$BASE_URL"

# --- helpers -----------------------------------------------------------------

report() {
  local name="$1" ok="$2"
  if [[ "$ok" == "ok" ]]; then
    PASS=$((PASS + 1))
    echo "PASS: $name"
  else
    FAIL=$((FAIL + 1))
    echo "FAIL: $name"
  fi
}

run_install() {
  WRIGHT_INSTALL_BASE_URL="$BASE_URL" \
  WRIGHT_API_URL="$API_URL" \
  "$INSTALLER" "$@"
}

expect_success() {
  local name="$1" dir="$2"; shift 2
  if run_install --dir "$dir" "$@" >/dev/null 2>&1 &&
     [[ -x "$dir/wright" && -x "$dir/wright-lsp" ]] &&
     "$dir/wright" --version 2>/dev/null | grep -Fq "$VERSION" &&
     "$dir/wright-lsp" --version 2>/dev/null | grep -Fq "$VERSION"; then
    report "$name" ok
  else
    report "$name" fail
  fi
}

expect_failure() {
  local name="$1" pattern="$2" dir="$3"; shift 3
  if run_install --dir "$dir" "$@" >"$INSTALL_OUTPUT" 2>&1; then
    report "$name" fail
  elif grep -Fq "$pattern" "$INSTALL_OUTPUT"; then
    report "$name" ok
  else
    report "$name" fail
    sed 's/^/    /' "$INSTALL_OUTPUT" >&2
  fi
}

# --- tests -------------------------------------------------------------------

expect_success "pinned install (native platform)" "$WORK/d1" --version "$VERSION"

expect_success "latest-release resolution" "$WORK/d2"

printf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  %s\n' \
  "wright-$VERSION-x86_64-unknown-linux-gnu.tar.gz" \
  > "$WORK/mock/v$VERSION/wright-$VERSION-x86_64-unknown-linux-gnu.tar.gz.sha256"
if WRIGHT_INSTALL_OS=linux WRIGHT_INSTALL_ARCH=x86_64 \
   run_install --dir "$WORK/d3" --version "$VERSION" >"$INSTALL_OUTPUT" 2>&1; then
  report "checksum mismatch is rejected before install" fail
else
  grep -Fq "checksum verification failed" "$INSTALL_OUTPUT" \
    && report "checksum mismatch is rejected before install" ok \
    || report "checksum mismatch is rejected before install" fail
fi
test ! -e "$WORK/d3/wright" \
  && report "nothing installed after checksum failure" ok \
  || report "nothing installed after checksum failure" fail

printf 'not-a-hash  %s\n' "wright-$VERSION-x86_64-unknown-linux-gnu.tar.gz" \
  > "$WORK/mock/v$VERSION/wright-$VERSION-x86_64-unknown-linux-gnu.tar.gz.sha256"
if WRIGHT_INSTALL_OS=linux WRIGHT_INSTALL_ARCH=x86_64 \
   run_install --dir "$WORK/d3b" --version "$VERSION" >"$INSTALL_OUTPUT" 2>&1; then
  report "malformed checksum file is rejected" fail
else
  grep -Fq "invalid checksum" "$INSTALL_OUTPUT" \
    && report "malformed checksum file is rejected" ok \
    || report "malformed checksum file is rejected" fail
fi

(cd "$WORK/mock/v$VERSION" && shasum -a 256 "wright-$VERSION-x86_64-unknown-linux-gnu.tar.gz" \
  > "wright-$VERSION-x86_64-unknown-linux-gnu.tar.gz.sha256")

expect_failure "unknown version fails with an actionable error" "does release" "$WORK/d4" \
  --version 0.0.0

if WRIGHT_INSTALL_OS=windows run_install --dir "$WORK/d5" --version "$VERSION" \
   >"$INSTALL_OUTPUT" 2>&1; then
  report "unsupported OS fails explicitly" fail
else
  grep -Fq "unsupported" "$INSTALL_OUTPUT" \
    && report "unsupported OS fails explicitly" ok \
    || report "unsupported OS fails explicitly" fail
fi

if WRIGHT_INSTALL_OS=linux WRIGHT_INSTALL_ARCH=arm64 \
   run_install --dir "$WORK/d6" --version "$VERSION" >"$INSTALL_OUTPUT" 2>&1; then
  report "linux/arm64 fails explicitly" fail
else
  grep -Fq "unsupported" "$INSTALL_OUTPUT" \
    && report "linux/arm64 fails explicitly" ok \
    || report "linux/arm64 fails explicitly" fail
fi

mkdir -p "$WORK/readonly"
chmod 555 "$WORK/readonly"
expect_failure "unwritable install directory fails with guidance" "not writable" "$WORK/readonly/bin" \
  --version "$VERSION"
chmod 755 "$WORK/readonly"

if WRIGHT_INSTALL_OS=darwin WRIGHT_INSTALL_ARCH=aarch64 \
   run_install --dir "$WORK/d7" --version "$VERSION" >/dev/null 2>&1 &&
   test -x "$WORK/d7/wright"; then
  report "macOS arm64 mapping" ok
else
  report "macOS arm64 mapping" fail
fi

if WRIGHT_INSTALL_OS=darwin WRIGHT_INSTALL_ARCH=x86_64 \
   run_install --dir "$WORK/d8" --version "$VERSION" >/dev/null 2>&1 &&
   test -x "$WORK/d8/wright"; then
  report "macOS x86_64 mapping" ok
else
  report "macOS x86_64 mapping" fail
fi

mkdir -p "$WORK/home"
if HOME="$WORK/home" WRIGHT_INSTALL_BASE_URL="$BASE_URL" WRIGHT_API_URL="$API_URL" \
   "$INSTALLER" --version "$VERSION" >/dev/null 2>&1 &&
   test -x "$WORK/home/.local/bin/wright"; then
  report "default install directory (\$HOME/.local/bin)" ok
else
  report "default install directory (\$HOME/.local/bin)" fail
fi

# Archive-layout regression: an archive missing wright-lsp must fail cleanly.
make_archive "$WORK/mock-broken" "x86_64-unknown-linux-gnu"
release="$WORK/mock-broken/v$VERSION"
rm -f "$release/wright-$VERSION-x86_64-unknown-linux-gnu.tar.gz" \
      "$release/wright-$VERSION-x86_64-unknown-linux-gnu.tar.gz.sha256"
dir="$release/wright-$VERSION-x86_64-unknown-linux-gnu"
mkdir -p "$dir"
make_fake_binary "$dir/wright" wright
tar -czf "$release/wright-$VERSION-x86_64-unknown-linux-gnu.tar.gz" -C "$release" \
  "wright-$VERSION-x86_64-unknown-linux-gnu"
(cd "$release" && shasum -a 256 "wright-$VERSION-x86_64-unknown-linux-gnu.tar.gz" \
  > "wright-$VERSION-x86_64-unknown-linux-gnu.tar.gz.sha256")
rm -rf "$dir"
python3 -m http.server $((PORT + 1)) --directory "$WORK/mock-broken" >/dev/null 2>&1 &
BROKEN_PID=$!
BROKEN_BASE_URL="http://127.0.0.1:$((PORT + 1))"
wait_for_server "$BROKEN_BASE_URL"
if WRIGHT_INSTALL_OS=linux WRIGHT_INSTALL_ARCH=x86_64 \
   WRIGHT_INSTALL_BASE_URL="$BROKEN_BASE_URL" WRIGHT_API_URL="$API_URL" \
   "$INSTALLER" --dir "$WORK/d9" --version "$VERSION" >"$INSTALL_OUTPUT" 2>&1; then
  report "archive missing wright-lsp fails cleanly" fail
else
  grep -Fq "archive layout" "$INSTALL_OUTPUT" \
    && report "archive missing wright-lsp fails cleanly" ok \
    || report "archive missing wright-lsp fails cleanly" fail
fi

if run_install --dir "$WORK/d10" --version "$VERSION" >"$INSTALL_OUTPUT" 2>&1; then
  if grep -Fq "installing shell completion" "$INSTALL_OUTPUT"; then
    report "install.sh runs completion installation" ok
  else
    report "install.sh runs completion installation" fail
  fi
else
  report "install.sh runs completion installation" fail
fi

echo
echo "installer tests: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
