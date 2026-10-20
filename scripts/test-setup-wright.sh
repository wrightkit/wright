#!/usr/bin/env bash
# Usage: scripts/test-setup-wright.sh
#
# Functional tests for .github/actions/setup-wright/setup-wright.sh (#717)
# against a mock release distribution that logs every request. A "cache" is a
# directory that survives between helper invocations, so cache hit/miss is
# observable as the presence or absence of distribution requests.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HELPER="$ROOT/.github/actions/setup-wright/setup-wright.sh"
WORK="$(mktemp -d)"
LOG="$WORK/requests.log"
PASS=0
FAIL=0

cleanup() {
  [[ -n "${SERVER_PID:-}" ]] && kill "$SERVER_PID" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

# Serve $2 on loopback port $1, appending each request path to $3. A plain
# threading TCPServer avoids the reverse-DNS stall of `python3 -m http.server`.
serve() {
  python3 - "$1" "$2" "$3" >/dev/null 2>&1 <<'PY' &
import functools, http.server, socketserver, sys

port, root, log = int(sys.argv[1]), sys.argv[2], sys.argv[3]

class Handler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, fmt, *args):
        with open(log, "a") as f:
            f.write(self.path + "\n")

socketserver.ThreadingTCPServer(
    ("127.0.0.1", port), functools.partial(Handler, directory=root)
).serve_forever()
PY
}

make_release() {
  local version="$1" triple="$2" release="$WORK/mock/wright/releases/$1"
  local dir="$release/wright-$version-$triple"
  mkdir -p "$dir"
  printf '#!/bin/sh\necho "wright %s"\n' "$version" > "$dir/wright"
  chmod +x "$dir/wright"
  tar -czf "$release/wright-$version-$triple.tar.gz" -C "$release" "wright-$version-$triple"
  (cd "$release" && shasum -a 256 "wright-$version-$triple.tar.gz" > "wright-$version-$triple.tar.gz.sha256")
  rm -rf "$dir"
}

report() {
  if [[ "$2" == ok ]]; then PASS=$((PASS + 1)); echo "PASS: $1"; else FAIL=$((FAIL + 1)); echo "FAIL: $1"; fi
}

check() { # name command...
  local name="$1"; shift
  if "$@" >/dev/null 2>&1; then report "$name" ok; else report "$name" fail; fi
}

port="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
for triple in x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-apple-darwin; do
  make_release 1.2.3 "$triple"
  make_release 1.3.0 "$triple"
done
mkdir -p "$WORK/mock/wright/latest"
printf '1.2.3\n' > "$WORK/mock/wright/latest/version"
: > "$LOG"
serve "$port" "$WORK/mock" "$LOG"
SERVER_PID=$!
for _ in $(seq 1 50); do curl -fsS "http://127.0.0.1:$port/" >/dev/null 2>&1 && break; sleep 0.1; done

: > "$LOG"
export WRIGHT_INSTALL_BASE_URL="http://127.0.0.1:$port/wright"
export RUNNER_TEMP="$WORK/runner-temp"
mkdir -p "$RUNNER_TEMP"

# resolve OS ARCH VERSION -> sets OUT file; succeeds or fails like the helper.
resolve() {
  OUT="$WORK/out.$RANDOM"
  : > "$OUT"
  GITHUB_OUTPUT="$OUT" WRIGHT_SETUP_OS="$1" WRIGHT_SETUP_ARCH="$2" WRIGHT_VERSION="$3" "$HELPER" resolve
}
out() { grep "^$1=" "$OUT" | cut -d= -f2-; }
requests() { wc -l < "$LOG" | tr -d ' '; }

# Version resolution
resolve Linux X64 1.2.3 >/dev/null
[[ "$(out version)" == 1.2.3 && "$(out target)" == x86_64-unknown-linux-gnu \
  && "$(out cache-key)" == setup-wright-v1-1.2.3-x86_64-unknown-linux-gnu ]] \
  && report "exact version resolves without a distribution request" ok || report "exact version resolves without a distribution request" fail
[[ "$(requests)" == 0 ]] && report "exact version makes no request" ok || report "exact version makes no request" fail
resolve Linux X64 v1.2.3 >/dev/null
[[ "$(out version)" == 1.2.3 ]] && report "leading v is stripped" ok || report "leading v is stripped" fail
resolve Linux X64 latest >/dev/null
[[ "$(out version)" == 1.2.3 && "$(requests)" == 1 ]] && grep -Fxq /wright/latest/version "$LOG" \
  && report "latest reads only the version pointer" ok || report "latest reads only the version pointer" fail
printf '1.3.0\n' > "$WORK/mock/wright/latest/version"
resolve Linux X64 latest >/dev/null
[[ "$(out version)" == 1.3.0 && "$(out cache-key)" != setup-wright-v1-1.2.3-* ]] \
  && report "new latest version changes the cache key" ok || report "new latest version changes the cache key" fail
printf '1.2.3\n' > "$WORK/mock/wright/latest/version"
check "invalid version is rejected" bash -c '! WRIGHT_VERSION="1.2" GITHUB_OUTPUT=/dev/null WRIGHT_SETUP_OS=Linux WRIGHT_SETUP_ARCH=X64 "$0" resolve' "$HELPER"
check "version with a path is rejected" bash -c '! WRIGHT_VERSION="../x" GITHUB_OUTPUT=/dev/null WRIGHT_SETUP_OS=Linux WRIGHT_SETUP_ARCH=X64 "$0" resolve' "$HELPER"

# Platform identity and unsupported targets
keys=""
for platform in "Linux X64" "macOS X64" "macOS ARM64" "Windows X64"; do
  resolve $platform 1.2.3 >/dev/null
  keys="$keys$(out cache-key)"$'\n'
done
[[ "$(printf %s "$keys" | sort -u | wc -l | tr -d ' ')" == 4 ]] \
  && report "each OS/architecture has a distinct cache key" ok || report "each OS/architecture has a distinct cache key" fail
for platform in "Linux ARM64" "Windows ARM64" "macOS X86" "FreeBSD X64"; do
  # shellcheck disable=SC2086
  if resolve $platform 1.2.3 >"$WORK/err" 2>&1; then
    report "unsupported platform $platform fails" fail
  else
    grep -Fq "unsupported platform" "$WORK/err" && report "unsupported platform $platform fails" ok || report "unsupported platform $platform fails" fail
  fi
done

# Miss: download, verify, store
resolve Linux X64 1.2.3 >/dev/null
DIR="$(out cache-dir)"
export WRIGHT_SETUP_DIR="$DIR" WRIGHT_SETUP_VERSION=1.2.3 WRIGHT_SETUP_TARGET=x86_64-unknown-linux-gnu WRIGHT_SETUP_EXT=tar.gz
: > "$LOG"
"$HELPER" install >/dev/null
[[ -x "$DIR/wright" && -f "$DIR/wright.sha256" ]] && "$DIR/wright" --version | grep -Fq 1.2.3 \
  && grep -Fq .tar.gz.sha256 "$LOG" && grep -Fq wright-1.2.3-x86_64-unknown-linux-gnu.tar.gz "$LOG" \
  && report "cache miss downloads and installs the verified executable" ok || report "cache miss downloads and installs the verified executable" fail
[[ "$(find "$DIR" -type f | wc -l | tr -d ' ')" == 2 ]] \
  && report "cache holds only the executable and its checksum" ok || report "cache holds only the executable and its checksum" fail

# Hit: no distribution request at all
: > "$LOG"
OUT="$WORK/verify.out"; : > "$OUT"
GITHUB_OUTPUT="$OUT" "$HELPER" verify-cache >/dev/null
[[ "$(out valid)" == true && "$(requests)" == 0 ]] \
  && report "valid cache hit makes no distribution request" ok || report "valid cache hit makes no distribution request" fail

# Corrupt executable
printf '#!/bin/sh\necho "wright 1.2.3"\necho tampered\n' > "$DIR/wright"
: > "$OUT"
GITHUB_OUTPUT="$OUT" "$HELPER" verify-cache >/dev/null 2>&1
[[ "$(out valid)" == false && ! -e "$DIR" ]] \
  && report "corrupt cached executable is rejected and removed" ok || report "corrupt cached executable is rejected and removed" fail
"$HELPER" install >/dev/null
: > "$OUT"
GITHUB_OUTPUT="$OUT" "$HELPER" verify-cache >/dev/null
[[ "$(out valid)" == true ]] && report "recovery reinstalls a verified executable" ok || report "recovery reinstalls a verified executable" fail

# Missing sidecar, and a matching sidecar over the wrong version
rm "$DIR/wright.sha256"
: > "$OUT"; GITHUB_OUTPUT="$OUT" "$HELPER" verify-cache >/dev/null 2>&1
[[ "$(out valid)" == false ]] && report "cache without checksum sidecar is rejected" ok || report "cache without checksum sidecar is rejected" fail
mkdir -p "$DIR"
printf '#!/bin/sh\necho "wright 9.9.9"\n' > "$DIR/wright"; chmod +x "$DIR/wright"
shasum -a 256 "$DIR/wright" | awk '{print $1}' > "$DIR/wright.sha256"
: > "$OUT"; GITHUB_OUTPUT="$OUT" "$HELPER" verify-cache >/dev/null 2>&1
[[ "$(out valid)" == false ]] && report "executable of another version is rejected" ok || report "executable of another version is rejected" fail

# Distribution checksum mismatch installs nothing
make_release 1.2.3 x86_64-unknown-linux-gnu
printf '%064d  wright-1.2.3-x86_64-unknown-linux-gnu.tar.gz\n' 0 \
  > "$WORK/mock/wright/releases/1.2.3/wright-1.2.3-x86_64-unknown-linux-gnu.tar.gz.sha256"
if "$HELPER" install >"$WORK/err" 2>&1; then
  report "checksum mismatch fails" fail
else
  grep -Fq "checksum mismatch" "$WORK/err" && [[ ! -e "$DIR/wright" ]] \
    && report "checksum mismatch fails and installs nothing" ok || report "checksum mismatch fails and installs nothing" fail
fi

# Missing release fails explicitly
WRIGHT_SETUP_VERSION=7.7.7 "$HELPER" install >"$WORK/err" 2>&1 \
  && report "missing release fails" fail \
  || { grep -Fq "does release 7.7.7 exist" "$WORK/err" && report "missing release fails" ok || report "missing release fails" fail; }

# PATH activation
GITHUB_PATH="$WORK/path" "$HELPER" activate
grep -Fxq "$DIR" "$WORK/path" && report "activate adds the executable directory to PATH" ok || report "activate adds the executable directory to PATH" fail

echo "setup-wright: $PASS passed, $FAIL failed"
[[ "$FAIL" == 0 ]]
