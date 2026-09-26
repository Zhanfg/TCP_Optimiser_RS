#!/usr/bin/env bash
set -euo pipefail

ARCHIVE=${1:?usage: validate-package.sh <module.zip>}
test -s "$ARCHIVE"
unzip -tq "$ARCHIVE"

ENTRIES=$(unzip -Z1 "$ARCHIVE")
read -r -a PACKAGE_ABIS <<< "${TCP_OPTIMISER_PACKAGE_ABIS:-arm64-v8a armeabi-v7a x86_64}"
[ "${#PACKAGE_ABIS[@]}" -gt 0 ] || {
  printf 'TCP_OPTIMISER_PACKAGE_ABIS resolved to an empty ABI set\n' >&2
  exit 1
}

if printf '%s\n' "$ENTRIES" | grep -Eq '(^/|(^|/)\.\.(/|$)|\\)'; then
  printf 'unsafe archive path detected\n' >&2
  exit 1
fi

for REQUIRED in \
  module.prop customize.sh service.sh post-fs-data.sh uninstall.sh LICENSE checksums.sha256 checksums.sig \
  webroot/index.html webroot/js/common.js webroot/js/settings.js; do
  if ! printf '%s\n' "$ENTRIES" | grep -Fxq "$REQUIRED"; then
    printf 'missing package entry: %s\n' "$REQUIRED" >&2
    exit 1
  fi
done

for ABI in "${PACKAGE_ABIS[@]}"; do
  case "$ABI" in
    arm64-v8a|armeabi-v7a|x86_64) ;;
    *)
      printf 'unsupported expected ABI: %s\n' "$ABI" >&2
      exit 1
      ;;
  esac
  REQUIRED="bin/$ABI/tcp_optimiser"
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

BINARY_COUNT="$(printf '%s\n' "$ENTRIES" | grep -Ec '^bin/[^/]+/tcp_optimiser$')"
if [ "$BINARY_COUNT" -ne "${#PACKAGE_ABIS[@]}" ]; then
  printf 'package contains %s Android binaries, expected %s\n' "$BINARY_COUNT" "${#PACKAGE_ABIS[@]}" >&2
  exit 1
fi

EXTRACTED=$(mktemp -d)
trap 'rm -rf "$EXTRACTED"' EXIT
unzip -q "$ARCHIVE" -d "$EXTRACTED"

openssl pkeyutl -verify -pubin -rawin -inkey keys/module-signing-public.pem \
  -in "$EXTRACTED/checksums.sha256" -sigfile "$EXTRACTED/checksums.sig"
(cd "$EXTRACTED" && sha256sum -c checksums.sha256)

if [ -f "$EXTRACTED/kernel_modules/manifest.json" ]; then
  python3 - "$EXTRACTED/kernel_modules" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
data = json.loads((root / "manifest.json").read_text())
if data.get("schema") != 1:
    raise SystemExit("unsupported kernel module manifest schema")

referenced = set()
identities = set()
for entry in data.get("modules", []):
    relative = Path(entry["file"])
    if relative.is_absolute() or ".." in relative.parts:
        raise SystemExit(f"unsafe kernel module path: {entry['file']}")
    identity = (entry["name"], entry["kmi"], entry["arch"])
    if identity in identities:
        raise SystemExit(f"duplicate kernel module identity: {identity}")
    identities.add(identity)
    path = root / relative
    if not path.is_file():
        raise SystemExit(f"missing kernel module: {entry['file']}")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest.lower() != entry["sha256"].lower():
        raise SystemExit(f"kernel module hash mismatch: {entry['file']}")
    referenced.add(relative.as_posix())

actual = {
    path.relative_to(root).as_posix()
    for path in root.glob("**/*.ko")
}
if actual != referenced:
    missing = sorted(referenced - actual)
    extra = sorted(actual - referenced)
    raise SystemExit(f"kernel module manifest mismatch: missing={missing} extra={extra}")
if not referenced:
    raise SystemExit("kernel module manifest contains no modules")
print(f"validated {len(referenced)} packaged kernel modules")
PY
fi

cmp "$EXTRACTED/webroot/index.html" webroot/index.html
cmp "$EXTRACTED/webroot/js/common.js" webroot/js/common.js
cmp "$EXTRACTED/webroot/js/settings.js" webroot/js/settings.js

check_machine() {
  local binary=$1
  local machine=$2
  readelf -h "$binary" | grep -F 'Machine:' | grep -F "$machine" >/dev/null
}

for ABI in "${PACKAGE_ABIS[@]}"; do
  case "$ABI" in
    arm64-v8a) MACHINE='AArch64' ;;
    armeabi-v7a) MACHINE='ARM' ;;
    x86_64) MACHINE='X86-64' ;;
  esac
  check_machine "$EXTRACTED/bin/$ABI/tcp_optimiser" "$MACHINE"
done

if [ -n "${TCP_OPTIMISER_DEVICE_PROFILE_FILE:-}" ]; then
  test -s "$EXTRACTED/device_profile/target.properties"
  cmp "$EXTRACTED/device_profile/target.properties" "$TCP_OPTIMISER_DEVICE_PROFILE_FILE"
fi

if [ "${GITHUB_ACTIONS:-false}" = "true" ]; then
  grep -Fq "https://github.com/${GITHUB_REPOSITORY}" "$EXTRACTED/build-info.json"
  grep -Fq "${GITHUB_SHA}" "$EXTRACTED/build-info.json"
fi

if grep -RniE 'stealth|幽灵|隐身' "$EXTRACTED/module.prop" "$EXTRACTED/webroot" >/dev/null; then
  printf 'removed stealth feature is still present in package\n' >&2
  exit 1
fi

printf 'package validation: %s entries, signed manifest and %s verified Android ELF(s)\n' \
  "$(printf '%s\n' "$ENTRIES" | wc -l)" "${#PACKAGE_ABIS[@]}"
