#!/usr/bin/env bash
# Publish verified release archives to immutable R2 objects and advance latest.

set -euo pipefail

: "${RELEASE_TAG:?RELEASE_TAG is required}"
: "${R2_BUCKET:?R2_BUCKET is required}"
: "${R2_ENDPOINT:?R2_ENDPOINT is required}"
: "${R2_PUBLIC_BASE_URL:?R2_PUBLIC_BASE_URL is required}"
: "${AWS_ACCESS_KEY_ID:?AWS_ACCESS_KEY_ID is required}"
: "${AWS_SECRET_ACCESS_KEY:?AWS_SECRET_ACCESS_KEY is required}"
: "${GITHUB_WORKSPACE:?GITHUB_WORKSPACE is required}"
: "${ARTIFACTS_DIR:?ARTIFACTS_DIR is required}"

version="${RELEASE_TAG#v}"
release_dir="$GITHUB_WORKSPACE/r2-release"
mkdir -p "$release_dir"
for triple in x86_64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc; do
  ext="tar.gz"
  [[ "$triple" == "x86_64-pc-windows-msvc" ]] && ext="zip"
  archive="wright-$version-$triple.$ext"
  test -f "$ARTIFACTS_DIR/$archive" || { echo "missing $archive" >&2; exit 1; }
  test -f "$ARTIFACTS_DIR/$archive.sha256" || { echo "missing $archive.sha256" >&2; exit 1; }
  cp "$ARTIFACTS_DIR/$archive" "$release_dir/"
  cp "$ARTIFACTS_DIR/$archive.sha256" "$release_dir/"
done

put_immutable() {
  local source="$1" key="$2" cache_control="$3" content_type="$4"
  local existing="$GITHUB_WORKSPACE/existing-$(basename "$key")"
  if aws s3api head-object --bucket "$R2_BUCKET" --key "$key" --endpoint-url "$R2_ENDPOINT" >/dev/null 2>&1; then
    aws s3api get-object --bucket "$R2_BUCKET" --key "$key" --endpoint-url "$R2_ENDPOINT" "$existing" >/dev/null
    cmp --silent "$source" "$existing" || {
      echo "error: immutable R2 object $key differs from this release artifact" >&2
      exit 1
    }
    rm -f "$existing"
    return
  fi
  aws s3api put-object --bucket "$R2_BUCKET" --key "$key" --body "$source" \
    --if-none-match '*' --cache-control "$cache_control" --content-type "$content_type" \
    --endpoint-url "$R2_ENDPOINT" >/dev/null
}

verify_public() {
  local key="$1" source="$2" cache_pattern="$3"
  local downloaded="$GITHUB_WORKSPACE/downloaded-$(basename "$key")"
  curl --fail --silent --show-error --location --output "$downloaded" "$R2_PUBLIC_BASE_URL/$key"
  cmp --silent "$source" "$downloaded"
  curl --fail --silent --show-error --head "$R2_PUBLIC_BASE_URL/$key" | \
    grep --ignore-case --extended-regexp "^cache-control:.*$cache_pattern" >/dev/null
  rm -f "$downloaded"
}

for archive in "$release_dir"/wright-*.tar.gz "$release_dir"/wright-*.zip; do
  [[ -e "$archive" ]] || continue
  checksum="$archive.sha256"
  expected_hash="$(awk 'NR == 1 { print $1 }' "$checksum")"
  actual_hash="$(sha256sum "$archive" | awk 'NR == 1 { print $1 }')"
  test "$actual_hash" = "$expected_hash"
  name="$(basename "$archive")"
  put_immutable "$archive" "wright/releases/$version/$name" 'public, max-age=31536000, immutable' 'application/octet-stream'
  put_immutable "$checksum" "wright/releases/$version/$name.sha256" 'public, max-age=31536000, immutable' 'text/plain; charset=utf-8'
  verify_public "wright/releases/$version/$name" "$archive" 'max-age=31536000.*immutable'
  verify_public "wright/releases/$version/$name.sha256" "$checksum" 'max-age=31536000.*immutable'
done

latest_version="$GITHUB_WORKSPACE/latest-version"
printf '%s\n' "$version" > "$latest_version"
aws s3api put-object --bucket "$R2_BUCKET" --key wright/latest/version --body "$latest_version" \
  --cache-control 'no-store, max-age=0' --content-type 'text/plain; charset=utf-8' \
  --endpoint-url "$R2_ENDPOINT" >/dev/null
verify_public wright/latest/version "$latest_version" 'no-store'
rm -f "$latest_version"
