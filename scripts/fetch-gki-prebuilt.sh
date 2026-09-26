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

python3 - "$OUT/BUILD_INFO" "$OUT/artifacts.txt" "$OUT/selection.json" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
files = data["target"]["dir_list"]
open(sys.argv[2], "w").write("\n".join(files) + "\n")
if "vmlinux.symvers" not in files:
    raise SystemExit("official GKI build is missing required artifact: vmlinux.symvers")
if "vmlinux" not in files:
    raise SystemExit("official GKI build is missing required artifact: vmlinux")

mods = sorted(x for x in files if x.endswith(".ko"))
image = "Image" if "Image" in files else None
if not mods and image is None:
    raise SystemExit(
        "official GKI build exposes neither an individual .ko nor an uncompressed Image "
        "for exact kernel-release validation"
    )

selection = {
    "official_module": mods[0] if mods else None,
    "official_image": image,
    "vmlinux": "vmlinux",
    "gki_info": "gki-info.txt" if "gki-info.txt" in files else None,
}
open(sys.argv[3], "w").write(json.dumps(selection, sort_keys=True) + "\n")
PY

readarray -t selected < <(python3 - "$OUT/selection.json" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
for key in ("official_module", "official_image", "vmlinux", "gki_info"):
    print(data.get(key) or "")
PY
)
official_module=${selected[0]}
official_image=${selected[1]}
vmlinux=${selected[2]}
gki_info=${selected[3]}

curl --fail --location --retry 3 --retry-all-errors   "$base/vmlinux.symvers" -o "$OUT/vmlinux.symvers"
curl --fail --location --retry 3 --retry-all-errors   "$base/$vmlinux" -o "$OUT/vmlinux"

if [[ -n "$official_module" ]]; then
  curl --fail --location --retry 3 --retry-all-errors     "$base/$official_module" -o "$OUT/official-module.ko"
fi

if [[ -n "$official_image" ]]; then
  curl --fail --location --retry 3 --retry-all-errors     "$base/$official_image" -o "$OUT/official-kernel-image"
fi

if [[ -n "$gki_info" ]]; then
  curl --fail --location --retry 3 --retry-all-errors     "$base/$gki_info" -o "$OUT/gki-info.txt"
fi

official_vermagic=""
kernel_release=""
kernel_release_source=""

if [[ -s "$OUT/official-module.ko" ]]; then
  official_vermagic=$(modinfo -F vermagic "$OUT/official-module.ko")
  kernel_release=${official_vermagic%% *}
  kernel_release_source="official_module_vermagic"
fi

if [[ -z "$kernel_release" && -s "$OUT/official-kernel-image" ]]; then
  kernel_release=$(python3 - "$OUT/official-kernel-image" <<'PY'
import re, sys
data = open(sys.argv[1], "rb").read()
matches = re.findall(
    rb"Linux version ([0-9]+\.[0-9]+\.[0-9]+-[^\x00\n\r ]+)",
    data,
)
if not matches:
    raise SystemExit("official Image has no readable concrete Linux version banner")
values = []
for value in matches:
    text = value.decode("ascii", "strict")
    if text not in values:
        values.append(text)
if len(values) != 1:
    raise SystemExit(f"official Image has ambiguous concrete Linux version banners: {values}")
print(values[0])
PY
)
  kernel_release_source="official_image_banner"
fi

if [[ -z "$kernel_release" ]]; then
  printf 'cannot determine exact kernel release from official GKI artifacts\n' >&2
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
    printf 'official artifact mismatch: gki-info=%s authoritative-release=%s\n'       "$gki_info_release" "$kernel_release" >&2
    exit 1
  fi
fi

python3 - "$OUT" "$KMI" "$TAG" "$SHA1" "$KERNEL_BID" "$ARTIFACT_TARGET"   "$kernel_release" "$kernel_release_source" "$official_module" "$official_image"   "$official_vermagic" <<'PY'
import hashlib, json, pathlib, sys

out = pathlib.Path(sys.argv[1])
gki_info = out / "gki-info.txt"
official_module_path = out / "official-module.ko"
official_image_path = out / "official-kernel-image"
vmlinux_path = out / "vmlinux"

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None

meta = {
    "schema": 2,
    "kmi": sys.argv[2],
    "tag": sys.argv[3],
    "sha1": sys.argv[4],
    "kernel_bid": sys.argv[5],
    "artifact_target": sys.argv[6],
    "kernel_release": sys.argv[7],
    "kernel_release_source": sys.argv[8],
    "official_module": sys.argv[9] or None,
    "official_image": sys.argv[10] or None,
    "official_vermagic": sys.argv[11] or None,
    "vmlinux_symvers_sha256": digest(out / "vmlinux.symvers"),
    "official_module_sha256": digest(official_module_path),
    "official_image_sha256": digest(official_image_path),
    "vmlinux_sha256": digest(vmlinux_path),
    "gki_info_sha256": digest(gki_info),
}
(out / "metadata.json").write_text(json.dumps(meta, indent=2, sort_keys=True) + "\n")
PY

printf 'official GKI: %s %s BID=%s release=%s source=%s\n'   "$KMI" "$TAG" "$KERNEL_BID" "$kernel_release" "$kernel_release_source"
if [[ -n "$official_module" ]]; then
  printf 'release witness: %s\n' "$official_module"
else
  printf 'release witness: %s (legacy GKI publishes no individual KO)\n' "$official_image"
fi
