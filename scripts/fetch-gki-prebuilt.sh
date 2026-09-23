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
if "vmlinux.symvers" not in files:
    raise SystemExit("official GKI build is missing required artifact: vmlinux.symvers")
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

curl --fail --location --retry 3 --retry-all-errors   "$base/vmlinux.symvers" -o "$OUT/vmlinux.symvers"
curl --fail --location --retry 3 --retry-all-errors   "$base/$official_module" -o "$OUT/official-module.ko"

if grep -Fxq 'gki-info.txt' "$OUT/artifacts.txt"; then
  curl --fail --location --retry 3 --retry-all-errors     "$base/gki-info.txt" -o "$OUT/gki-info.txt"
fi

official_vermagic=$(modinfo -F vermagic "$OUT/official-module.ko")
kernel_release=${official_vermagic%% *}
if [[ -z "$kernel_release" ]]; then
  printf 'official module has empty vermagic release: %s\n' "$official_module" >&2
  exit 1
fi

gki_info_release=""
if [[ -s "$OUT/gki-info.txt" ]]; then
  gki_info_release=$(python3 - "$OUT/gki-info.txt" <<'PY'
import sys
for raw in open(sys.argv[1], errors="replace"):
    line = "".join(raw.split())
    if line.startswith("kernel_release="):
        print(line.split("=", 1)[1])
        break
PY
)
  if [[ -n "$gki_info_release" && "$gki_info_release" != "$kernel_release" ]]; then
    printf 'official artifact mismatch: gki-info=%s module-vermagic=%s\n'       "$gki_info_release" "$kernel_release" >&2
    exit 1
  fi
fi

python3 - "$OUT" "$KMI" "$TAG" "$SHA1" "$KERNEL_BID" "$ARTIFACT_TARGET"   "$kernel_release" "$official_module" "$official_vermagic" <<'PY'
import hashlib, json, pathlib, sys
out = pathlib.Path(sys.argv[1])
gki_info = out / "gki-info.txt"
meta = {
    "schema": 1,
    "kmi": sys.argv[2],
    "tag": sys.argv[3],
    "sha1": sys.argv[4],
    "kernel_bid": sys.argv[5],
    "artifact_target": sys.argv[6],
    "kernel_release": sys.argv[7],
    "kernel_release_source": "official_module_vermagic",
    "official_module": sys.argv[8],
    "official_vermagic": sys.argv[9],
    "vmlinux_symvers_sha256": hashlib.sha256((out / "vmlinux.symvers").read_bytes()).hexdigest(),
    "official_module_sha256": hashlib.sha256((out / "official-module.ko").read_bytes()).hexdigest(),
}
if gki_info.is_file():
    meta["gki_info_sha256"] = hashlib.sha256(gki_info.read_bytes()).hexdigest()
else:
    meta["gki_info_sha256"] = None
(out / "metadata.json").write_text(json.dumps(meta, indent=2, sort_keys=True) + "\n")
PY

printf 'official GKI: %s %s BID=%s release=%s\n' "$KMI" "$TAG" "$KERNEL_BID" "$kernel_release"
if [[ -s "$OUT/gki-info.txt" ]]; then
  printf 'downloaded vmlinux.symvers + gki-info.txt + %s\n' "$official_module"
else
  printf 'downloaded vmlinux.symvers + %s (no gki-info.txt on this GKI generation)\n' "$official_module"
fi
