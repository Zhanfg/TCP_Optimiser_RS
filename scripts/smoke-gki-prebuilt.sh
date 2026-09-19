#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
KMI=${1:-android15-6.6}
OUT=${2:-"$ROOT/out/gki-prebuilt-smoke"}
WORK=$(mktemp -d)
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

mkdir -p "$OUT"
bash "$ROOT/scripts/fetch-gki-prebuilt.sh" "$KMI" "$OUT/prebuilt"

readarray -t meta < <(python3 - "$OUT/prebuilt/metadata.json" <<'PY'
import json, sys
m=json.load(open(sys.argv[1]))
for k in ("tag","sha1","kernel_release"):
    print(m[k])
PY
)
TAG=${meta[0]}
SHA1=${meta[1]}
OFFICIAL_RELEASE=${meta[2]}

official_vermagic=$(modinfo -F vermagic "$OUT/prebuilt/official-module.ko")
official_module_release=${official_vermagic%% *}
if [[ "$official_module_release" != "$OFFICIAL_RELEASE" ]]; then
  printf 'official artifact mismatch: gki-info=%s module-vermagic=%s\n' \
    "$OFFICIAL_RELEASE" "$official_module_release" >&2
  exit 1
fi

KERNEL_DIR="$WORK/kernel"
git clone --filter=blob:none --depth=1 --branch "$TAG" \
  https://android.googlesource.com/kernel/common "$KERNEL_DIR"
actual_sha=$(git -C "$KERNEL_DIR" rev-parse HEAD)
test "$actual_sha" = "$SHA1"

KBUILD_ARGS=(ARCH=arm64 LLVM=1 LLVM_IAS=1 CROSS_COMPILE=aarch64-linux-gnu- CROSS_COMPILE_COMPAT=arm-linux-gnueabi-)
make -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" gki_defconfig
CONFIG="$KERNEL_DIR/.config"
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_CODEL
make -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" olddefconfig
make -C "$KERNEL_DIR" -j"$(nproc)" "${KBUILD_ARGS[@]}" modules_prepare

install -m 0644 "$OUT/prebuilt/vmlinux.symvers" "$KERNEL_DIR/Module.symvers"

# modules_prepare derives a local development release. For this experiment,
# pin UTS_RELEASE to the exact CI release that produced the official symvers.
# This is safe only after the tag SHA + gki-info + official KO vermagic checks.
printf '%s\n' "$OFFICIAL_RELEASE" > "$KERNEL_DIR/include/config/kernel.release"
printf '#define UTS_RELEASE "%s"\n' "$OFFICIAL_RELEASE" > "$KERNEL_DIR/include/generated/utsrelease.h"

make -C "$KERNEL_DIR" -j"$(nproc)" "${KBUILD_ARGS[@]}" \
  KERNELRELEASE="$OFFICIAL_RELEASE" M=net/sched sch_codel.ko

local_vermagic=$(modinfo -F vermagic "$KERNEL_DIR/net/sched/sch_codel.ko")
local_release=${local_vermagic%% *}
if [[ "$local_release" != "$OFFICIAL_RELEASE" ]]; then
  printf 'local KO vermagic mismatch: expected=%s actual=%s\n' "$OFFICIAL_RELEASE" "$local_release" >&2
  exit 1
fi

python3 "$ROOT/scripts/audit-module-exports.py" \
  "$KERNEL_DIR" "$KERNEL_DIR/net/sched/sch_codel.ko" \
  --json "$OUT/export-audit.json"

install -m 0644 "$KERNEL_DIR/net/sched/sch_codel.ko" "$OUT/sch_codel.ko"
python3 - "$OUT" "$official_vermagic" "$local_vermagic" <<'PY'
import json, pathlib, sys
out=pathlib.Path(sys.argv[1])
m=json.load(open(out/"prebuilt/metadata.json"))
m["official_vermagic"]=sys.argv[2]
m["local_vermagic"]=sys.argv[3]
m["vermagic_match"]=sys.argv[2].split()[0] == sys.argv[3].split()[0] == m["kernel_release"]
(out/"validation.json").write_text(json.dumps(m, indent=2, sort_keys=True)+"\n")
PY

printf 'official symvers smoke passed: %s\n' "$OFFICIAL_RELEASE"
printf 'official vermagic: %s\n' "$official_vermagic"
printf 'local vermagic:    %s\n' "$local_vermagic"
