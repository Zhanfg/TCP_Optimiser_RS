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

KBUILD_ARGS=(ARCH=arm64 LLVM=1 LLVM_IAS=1 CROSS_COMPILE=aarch64-linux-gnu- CROSS_COMPILE_COMPAT=arm-linux-gnueabi- KCFLAGS=-D__ANDROID_COMMON_KERNEL__)
make -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" gki_defconfig
make -C "$KERNEL_DIR" -j"$(nproc)" "${KBUILD_ARGS[@]}" modules_prepare

install -m 0644 "$OUT/prebuilt/vmlinux.symvers" "$KERNEL_DIR/Module.symvers"

# modules_prepare derives a local development release. For this experiment,
# pin UTS_RELEASE to the exact CI release that produced the official symvers.
# This is safe only after the tag SHA + gki-info + official KO vermagic checks.
printf '%s\n' "$OFFICIAL_RELEASE" > "$KERNEL_DIR/include/config/kernel.release"
printf '#define UTS_RELEASE "%s"\n' "$OFFICIAL_RELEASE" > "$KERNEL_DIR/include/generated/utsrelease.h"

# Validate the mixed-build mechanism itself with a deliberately tiny external
# module that imports only the base GKI ABI. The TCP/qdisc targets are audited
# separately: if they reference non-KMI/module-to-module symbols, they should
# fail the KMI gate rather than being used to judge the prebuilt-symvers path.
PROBE_DIR="$WORK/gki-probe"
mkdir -p "$PROBE_DIR"
cat > "$PROBE_DIR/Makefile" <<'EOF'
obj-m := tcpopt_gki_probe.o
EOF
cat > "$PROBE_DIR/tcpopt_gki_probe.c" <<'EOF'
#include <linux/init.h>
#include <linux/module.h>

static int __init tcpopt_gki_probe_init(void)
{
    return 0;
}

static void __exit tcpopt_gki_probe_exit(void)
{
}

module_init(tcpopt_gki_probe_init);
module_exit(tcpopt_gki_probe_exit);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("TCP Optimiser GKI prebuilt symvers smoke probe");
EOF

make -C "$KERNEL_DIR" -j"$(nproc)" "${KBUILD_ARGS[@]}" \
  KERNELRELEASE="$OFFICIAL_RELEASE" M="$PROBE_DIR" modules

PROBE_KO="$PROBE_DIR/tcpopt_gki_probe.ko"
local_vermagic=$(modinfo -F vermagic "$PROBE_KO")
local_release=${local_vermagic%% *}
if [[ "$local_release" != "$OFFICIAL_RELEASE" ]]; then
  printf 'local KO vermagic mismatch: expected=%s actual=%s\n' "$OFFICIAL_RELEASE" "$local_release" >&2
  exit 1
fi

python3 "$ROOT/scripts/audit-module-exports.py" \
  "$KERNEL_DIR" "$PROBE_KO" \
  --json "$OUT/export-audit.json"

install -m 0644 "$PROBE_KO" "$OUT/tcpopt_gki_probe.ko"
python3 - "$OUT" "$official_vermagic" "$local_vermagic" <<'PY'
import json, pathlib, sys
out=pathlib.Path(sys.argv[1])
m=json.load(open(out/"prebuilt/metadata.json"))
m["official_vermagic"]=sys.argv[2]
m["local_vermagic"]=sys.argv[3]
m["vermagic_match"]=sys.argv[2].split()[0] == sys.argv[3].split()[0] == m["kernel_release"]
(out/"validation.json").write_text(json.dumps(m, indent=2, sort_keys=True)+"\n")
PY

printf 'official symvers/vermagic mixed-build smoke passed: %s\n' "$OFFICIAL_RELEASE"
printf 'official vermagic: %s\n' "$official_vermagic"
printf 'local vermagic:    %s\n' "$local_vermagic"
