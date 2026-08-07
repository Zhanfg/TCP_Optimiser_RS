#!/usr/bin/env bash
set -euo pipefail

ARCHIVE=${1:?usage: validate-package.sh <module.zip>}
test -s "$ARCHIVE"
unzip -tq "$ARCHIVE"

ENTRIES=$(unzip -Z1 "$ARCHIVE")
if printf '%s\n' "$ENTRIES" | grep -Eq '(^/|(^|/)\.\.(/|$)|\\)'; then
  printf 'unsafe archive path detected\n' >&2
  exit 1
fi

for REQUIRED in \
  module.prop customize.sh service.sh post-fs-data.sh uninstall.sh LICENSE checksums.sha256 checksums.sig \
  webroot/index.html webroot/js/common.js webroot/js/settings.js \
  bin/arm64-v8a/tcp_optimiser bin/armeabi-v7a/tcp_optimiser bin/x86_64/tcp_optimiser; do
  if ! printf '%s\n' "$ENTRIES" | grep -Fxq "$REQUIRED"; then
    printf 'missing package entry: %s\n' "$REQUIRED" >&2
    exit 1
  fi
done

if [ "${GITHUB_ACTIONS:-false}" = "true" ]; then
  if ! printf '%s\n' "$ENTRIES" | grep -Fxq 'build-info.json'; then
    printf 'official package is missing build-info.json\n' >&2
    exit 1
  fi
fi

if [ "$(printf '%s\n' "$ENTRIES" | grep -Ec '^bin/[^/]+/tcp_optimiser$')" -ne 3 ]; then
  printf 'package must contain exactly three Android binaries\n' >&2
  exit 1
fi

EXTRACTED=$(mktemp -d)
trap 'rm -rf "$EXTRACTED"' EXIT
unzip -q "$ARCHIVE" -d "$EXTRACTED"
openssl pkeyutl -verify -pubin -rawin -inkey keys/module-signing-public.pem \
  -in "$EXTRACTED/checksums.sha256" -sigfile "$EXTRACTED/checksums.sig"
(cd "$EXTRACTED" && sha256sum -c checksums.sha256)
cmp "$EXTRACTED/webroot/index.html" webroot/index.html
cmp "$EXTRACTED/webroot/js/common.js" webroot/js/common.js
cmp "$EXTRACTED/webroot/js/settings.js" webroot/js/settings.js

check_machine() {
  local binary=$1
  local machine=$2
  readelf -h "$binary" | grep -F 'Machine:' | grep -F "$machine" >/dev/null
}

check_machine "$EXTRACTED/bin/arm64-v8a/tcp_optimiser" 'AArch64'
check_machine "$EXTRACTED/bin/armeabi-v7a/tcp_optimiser" 'ARM'
check_machine "$EXTRACTED/bin/x86_64/tcp_optimiser" 'X86-64'

if [ "${GITHUB_ACTIONS:-false}" = "true" ]; then
  PACKAGE_CHANNEL=${TCP_OPTIMISER_PACKAGE_CHANNEL:?TCP_OPTIMISER_PACKAGE_CHANNEL is required}
  PACKAGE_OFFICIAL=${TCP_OPTIMISER_PACKAGE_OFFICIAL:?TCP_OPTIMISER_PACKAGE_OFFICIAL is required}
  grep -Fq "https://github.com/${GITHUB_REPOSITORY}" "$EXTRACTED/build-info.json"
  grep -Fq "${GITHUB_SHA}" "$EXTRACTED/build-info.json"
  grep -Fq "\"channel\": \"${PACKAGE_CHANNEL}\"" "$EXTRACTED/build-info.json"
  grep -Fq "\"official\": ${PACKAGE_OFFICIAL}" "$EXTRACTED/build-info.json"
  for ABI in arm64-v8a armeabi-v7a x86_64; do
    strings "$EXTRACTED/bin/$ABI/tcp_optimiser" | grep -F "channel=${PACKAGE_CHANNEL}" >/dev/null
  done
fi

if grep -RniE 'stealth|幽灵|隐身' "$EXTRACTED/module.prop" "$EXTRACTED/webroot" >/dev/null; then
  printf 'removed stealth feature is still present in package\n' >&2
  exit 1
fi

printf 'package validation: %s entries, signed manifest, matched provenance and three verified Android ELFs\n' "$(printf '%s\n' "$ENTRIES" | wc -l)"
