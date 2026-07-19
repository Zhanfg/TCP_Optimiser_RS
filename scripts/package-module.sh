#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
OUTPUT=${1:-"$REPO_ROOT/dist/TCP_Optimiser_RS.zip"}
BINARY_ROOT=${2:-"$REPO_ROOT/binary-input/bin"}

case "$OUTPUT" in
  /*) ;;
  *) OUTPUT="$REPO_ROOT/$OUTPUT" ;;
esac

STAGE=$(mktemp -d)
GENERATED_KEY=
ARCHIVE_LIST=
MANIFEST=
cleanup() {
  rm -rf "$STAGE"
  if [ -n "$GENERATED_KEY" ]; then rm -f "$GENERATED_KEY"; fi
  if [ -n "$ARCHIVE_LIST" ]; then rm -f "$ARCHIVE_LIST"; fi
  if [ -n "$MANIFEST" ]; then rm -f "$MANIFEST"; fi
}
trap cleanup EXIT
mkdir -p "$(dirname "$OUTPUT")"

for ABI in arm64-v8a armeabi-v7a x86_64; do
  SOURCE="$BINARY_ROOT/$ABI/tcp_optimiser"
  if [ ! -s "$SOURCE" ]; then
    printf 'missing Android binary: %s\n' "$SOURCE" >&2
    exit 1
  fi
  install -Dm755 "$SOURCE" "$STAGE/bin/$ABI/tcp_optimiser"
done

cp -a "$REPO_ROOT/webroot" "$STAGE/webroot"
for FILE in module.prop customize.sh service.sh post-fs-data.sh uninstall.sh LICENSE; do
  install -Dm644 "$REPO_ROOT/$FILE" "$STAGE/$FILE"
done
chmod 755 "$STAGE/customize.sh" "$STAGE/service.sh" "$STAGE/post-fs-data.sh" "$STAGE/uninstall.sh"

EPOCH=${SOURCE_DATE_EPOCH:-$(git -C "$REPO_ROOT" log -1 --format=%ct)}
if [ "${GITHUB_ACTIONS:-false}" = "true" ]; then
  cat > "$STAGE/build-info.json" <<EOF
{
  "official": true,
  "channel": "official-github",
  "repository": "https://github.com/${GITHUB_REPOSITORY}",
  "revision": "${GITHUB_SHA}",
  "sourceDateEpoch": ${EPOCH}
}
EOF
fi
find "$STAGE" -exec touch -h -d "@$EPOCH" {} +

SIGNING_KEY_FILE=${MODULE_SIGNING_KEY_FILE:-}
if [ -z "$SIGNING_KEY_FILE" ] && [ -n "${MODULE_SIGNING_KEY:-}" ]; then
  GENERATED_KEY=$(mktemp)
  chmod 600 "$GENERATED_KEY"
  printf '%s\n' "$MODULE_SIGNING_KEY" > "$GENERATED_KEY"
  SIGNING_KEY_FILE=$GENERATED_KEY
fi
if [ -z "$SIGNING_KEY_FILE" ] || [ ! -s "$SIGNING_KEY_FILE" ]; then
  printf 'an Ed25519 signing key is required (MODULE_SIGNING_KEY_FILE or MODULE_SIGNING_KEY)\n' >&2
  exit 1
fi

MANIFEST=$(mktemp)
(
  cd "$STAGE"
  find . -type f ! -name checksums.sha256 ! -name checksums.sig -printf '%P\n' | LC_ALL=C sort | while IFS= read -r FILE; do
    sha256sum "$FILE"
  done
) > "$MANIFEST"
install -m 0644 "$MANIFEST" "$STAGE/checksums.sha256"
openssl pkeyutl -sign -rawin -inkey "$SIGNING_KEY_FILE" \
  -in "$STAGE/checksums.sha256" -out "$STAGE/checksums.sig"
touch -h -d "@$EPOCH" "$STAGE/checksums.sha256" "$STAGE/checksums.sig"
rm -f "$OUTPUT"
ARCHIVE_LIST=$(mktemp)
(cd "$STAGE" && find . -type f -print | LC_ALL=C sort > "$ARCHIVE_LIST")
if command -v zip >/dev/null 2>&1; then
  (cd "$STAGE" && zip -q -X -9 "$OUTPUT" -@ < "$ARCHIVE_LIST")
elif command -v jar >/dev/null 2>&1; then
  (cd "$STAGE" && jar --create --file "$OUTPUT" --no-manifest "@$ARCHIVE_LIST")
elif command -v 7z >/dev/null 2>&1; then
  (cd "$STAGE" && 7z a -tzip -mx=9 -mtc=off -bd -bso0 "$OUTPUT" "@$ARCHIVE_LIST")
else
  printf 'zip or 7z is required to assemble the module archive\n' >&2
  exit 1
fi

printf 'module package: %s\n' "$OUTPUT"
