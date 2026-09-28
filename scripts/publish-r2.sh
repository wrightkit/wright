#!/usr/bin/env bash
# Publish verified stable or nightly archives and the stable installer scripts to R2.

set -euo pipefail

: "${RELEASE_CHANNEL:?RELEASE_CHANNEL is required}"
: "${RELEASE_VERSION:?RELEASE_VERSION is required}"
: "${RELEASE_COMMIT:?RELEASE_COMMIT is required}"
: "${R2_BUCKET:?R2_BUCKET is required}"
: "${R2_ENDPOINT:?R2_ENDPOINT is required}"
: "${R2_PUBLIC_BASE_URL:?R2_PUBLIC_BASE_URL is required}"
: "${AWS_ACCESS_KEY_ID:?AWS_ACCESS_KEY_ID is required}"
: "${AWS_SECRET_ACCESS_KEY:?AWS_SECRET_ACCESS_KEY is required}"
: "${GITHUB_WORKSPACE:?GITHUB_WORKSPACE is required}"
: "${ARTIFACTS_DIR:?ARTIFACTS_DIR is required}"

version="$RELEASE_VERSION"
installer_public_base_url=
case "$RELEASE_CHANNEL" in
  stable)
    : "${R2_INSTALLER_PUBLIC_BASE_URL:?R2_INSTALLER_PUBLIC_BASE_URL is required for stable releases}"
    installer_public_base_url="$R2_INSTALLER_PUBLIC_BASE_URL"
    object_prefix="wright/releases/$version"
    pointer_key="wright/latest/version"
    pointer_value="$version"
    ;;
  nightly)
    object_prefix="wright/nightly/$RELEASE_COMMIT"
    pointer_key="wright/nightly/version"
    pointer_value="$RELEASE_COMMIT"
    ;;
  *)
    echo "unsupported R2 release channel: $RELEASE_CHANNEL" >&2
    exit 1
    ;;
esac

release_dir="$GITHUB_WORKSPACE/r2-$RELEASE_CHANNEL"
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
  local existing
  existing="$GITHUB_WORKSPACE/existing-$(basename "$key")"
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
  local base_url="$1" key="$2" source="$3" cache_pattern="$4" content_type_pattern="$5"
  local downloaded
  downloaded="$GITHUB_WORKSPACE/downloaded-$(basename "$key")"
  curl --http2 --fail --silent --show-error --location --output "$downloaded" "$base_url/$key"
  cmp --silent "$source" "$downloaded"
  curl --http2 --fail --silent --show-error --head "$base_url/$key" | \
    grep --ignore-case --extended-regexp "^cache-control:.*$cache_pattern" >/dev/null
  curl --http2 --fail --silent --show-error --head "$base_url/$key" | \
    grep --ignore-case --extended-regexp "^content-type:.*$content_type_pattern" >/dev/null
  rm -f "$downloaded"
}

put_mutable() {
  local source="$1" key="$2" content_type="$3" base_url="$4"
  aws s3api put-object --bucket "$R2_BUCKET" --key "$key" --body "$source" \
    --cache-control 'no-store, max-age=0' --content-type "$content_type" \
    --endpoint-url "$R2_ENDPOINT" >/dev/null
  verify_public "$base_url" "$key" "$source" 'no-store.*max-age=0' "$content_type"
}

for archive in "$release_dir"/wright-*.tar.gz "$release_dir"/wright-*.zip; do
  [[ -e "$archive" ]] || continue
  checksum="$archive.sha256"
  expected_hash="$(awk 'NR == 1 { print $1 }' "$checksum")"
  actual_hash="$(sha256sum "$archive" | awk 'NR == 1 { print $1 }')"
  test "$actual_hash" = "$expected_hash"
  name="$(basename "$archive")"
  put_immutable "$archive" "$object_prefix/$name" 'public, max-age=31536000, immutable' 'application/octet-stream'
  put_immutable "$checksum" "$object_prefix/$name.sha256" 'public, max-age=31536000, immutable' 'text/plain; charset=utf-8'
  verify_public "$R2_PUBLIC_BASE_URL" "$object_prefix/$name" "$archive" 'max-age=31536000.*immutable' 'application/octet-stream'
  verify_public "$R2_PUBLIC_BASE_URL" "$object_prefix/$name.sha256" "$checksum" 'max-age=31536000.*immutable' 'text/plain'
done

if [[ "$RELEASE_CHANNEL" == stable ]]; then
  for script in install.sh install.ps1; do
    source="$GITHUB_WORKSPACE/$script"
    test -s "$source" || { echo "missing canonical $script" >&2; exit 1; }
    put_mutable "$source" "wright/$script" 'text/plain; charset=utf-8' "$installer_public_base_url"
  done
fi

pointer_file="$GITHUB_WORKSPACE/r2-$RELEASE_CHANNEL-pointer"
printf '%s\n' "$pointer_value" > "$pointer_file"
put_mutable "$pointer_file" "$pointer_key" 'text/plain; charset=utf-8' "$R2_PUBLIC_BASE_URL"
