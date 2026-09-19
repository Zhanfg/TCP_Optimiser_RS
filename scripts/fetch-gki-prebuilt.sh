#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
KMI=${1:?usage: fetch-gki-prebuilt.sh <kmi> <output-dir>}
OUT=${2:?usage: fetch-gki-prebuilt.sh <kmi> <output-dir>}
PINS="${GKI_RELEASE_PINS:-$ROOT/scripts/gki-release-pins.json}"

mkdir -p "$OUT"

eval "$(python3 - "$PINS" "$KMI" <<'PY'
import json, shlex, sys
pins = json.load(open(sys.argv[1]))
try:
    item = pins["targets"][sys.argv[2]]
except KeyError:
    raise SystemExit(f"missing GKI release pin: {sys.argv[2]}")
for key in ("tag", "sha1", "kernel_bid", "artifact_target"):
    value = str(item[key])
    print(f"{key.upper()}={shlex.quote(value)}")
PY
)"

repo=https://android.googlesource.com/kernel/common
peeled=$(git ls-remote "$repo" "refs/tags/$TAG^{}" | awk 'NR==1{print $1}')
if [[ -z "$peeled" ]]; then
  peeled=$(git ls-remote "$repo" "refs/tags/$TAG" | awk 'NR==1{print $1}')
fi
if [[ "$peeled" != "$SHA1" ]]; then
  printf 'pinned GKI tag mismatch: %s expected %s got %s\n' "$TAG" "$SHA1" "$peeled" >&2
  exit 1
fi

base="https://ci.android.com/builds/submitted/$KERNEL_BID/$ARTIFACT_TARGET/latest/raw"
curl --fail --location --retry 3 --retry-all-errors "$base/BUILD_INFO" -o "$OUT/BUILD_INFO"

python3 - "$OUT/BUILD_INFO" "$OUT/artifacts.txt" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
files = data["target"]["dir_list"]
open(sys.argv[2], "w").write("\n".join(files) + "\n")
required = {"vmlinux.symvers", "gki-info.txt"}
missing = sorted(required - set(files))
if missing:
    raise SystemExit(f"official GKI build is missing required artifacts: {missing}")
mods = sorted(x for x in files if x.endswith(".ko"))
if not mods:
    raise SystemExit("official GKI build exposes no individual .ko artifact for vermagic validation")
print(mods[0])
PY

official_module=$(python3 - "$OUT/artifacts.txt" <<'PY'
import sys
mods = sorted(x.strip() for x in open(sys.argv[1]) if x.strip().endswith(".ko"))
print(mods[0])
PY
)

for file in vmlinux.symvers gki-info.txt "$official_module"; do
  curl --fail --location --retry 3 --retry-all-errors "$base/$file" -o "$OUT/$(basename "$file")"
done
mv "$OUT/$(basename "$official_module")" "$OUT/official-module.ko"

kernel_release=$(python3 - "$OUT/gki-info.txt" <<'PY'
import sys
for raw in open(sys.argv[1], errors="replace"):
    line = "".join(raw.split())
    if line.startswith("kernel_release="):
        print(line.split("=", 1)[1])
        break
else:
    raise SystemExit("gki-info.txt has no kernel_release")
PY
)

python3 - "$OUT" "$KMI" "$TAG" "$SHA1" "$KERNEL_BID" "$ARTIFACT_TARGET" "$kernel_release" "$official_module" <<'PY'
import hashlib, json, pathlib, sys
out = pathlib.Path(sys.argv[1])
meta = {
    "schema": 1,
    "kmi": sys.argv[2],
    "tag": sys.argv[3],
    "sha1": sys.argv[4],
    "kernel_bid": sys.argv[5],
    "artifact_target": sys.argv[6],
    "kernel_release": sys.argv[7],
    "official_module": sys.argv[8],
    "vmlinux_symvers_sha256": hashlib.sha256((out / "vmlinux.symvers").read_bytes()).hexdigest(),
    "gki_info_sha256": hashlib.sha256((out / "gki-info.txt").read_bytes()).hexdigest(),
    "official_module_sha256": hashlib.sha256((out / "official-module.ko").read_bytes()).hexdigest(),
}
(out / "metadata.json").write_text(json.dumps(meta, indent=2, sort_keys=True) + "\n")
PY

printf 'official GKI: %s %s BID=%s release=%s\n' "$KMI" "$TAG" "$KERNEL_BID" "$kernel_release"
printf 'downloaded vmlinux.symvers + %s\n' "$official_module"
