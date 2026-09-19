#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
KMI=${1:-android15-6.6}
DEST=${2:-"$REPO_ROOT/kernel_modules"}
BBR_SOURCE_REV=c5c557584175b5fed8939bf91ec249aed158597d

case "$KMI" in
  android12-5.10|android13-5.15|android14-6.1|android15-6.6) ;;
  *)
    printf 'unsupported KMI target: %s\n' "$KMI" >&2
    exit 2
    ;;
esac

WORK=$(mktemp -d)
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

KERNEL_DIR="$WORK/kernel"
BBR_DIR="$WORK/tcp_bbr_modules"

git clone --filter=blob:none --depth=1 --branch "$KMI" \
  https://android.googlesource.com/kernel/common "$KERNEL_DIR"

KBUILD_ARGS=(ARCH=arm64 LLVM=1 LLVM_IAS=1 CROSS_COMPILE=aarch64-linux-gnu- CROSS_COMPILE_COMPAT=arm-linux-gnueabi-)
BBR_CC_ARGS=()
if command -v ccache >/dev/null 2>&1; then
  export CCACHE_BASEDIR="$WORK"
  export CCACHE_NOHASHDIR=true
  export CCACHE_COMPILERCHECK=content
  KBUILD_ARGS+=(CC="ccache clang")
  BBR_CC_ARGS+=(CC="ccache clang")
fi

make -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" gki_defconfig

CONFIG="$KERNEL_DIR/.config"
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module TCP_CONG_BBR
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_FQ
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_CODEL
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_FQ_CODEL
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_CAKE
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_PIE
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_FQ_PIE
make -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" olddefconfig

# A full GKI build is intentional: CONFIG_MODVERSIONS modules need the exact
# Module.symvers/CRC data. modules_prepare alone can create a .ko that compiles
# but is not a trustworthy loadable artifact.
make -C "$KERNEL_DIR" -j"$(nproc)" "${KBUILD_ARGS[@]}" Image modules

KERNEL_RELEASE=$(make -s -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" kernelrelease)
KERNEL_REV=$(git -C "$KERNEL_DIR" rev-parse HEAD)

git clone https://github.com/hrimfaxi/tcp_bbr_modules.git "$BBR_DIR"
git -C "$BBR_DIR" checkout --detach "$BBR_SOURCE_REV"

# BBR v1 comes from the target Android kernel tree as tcp_bbr.ko.
# Build only the out-of-tree BBRv3 module here; compiling tcp_bbr1.o is
# unnecessary and breaks on older branches whose BPF kfunc API differs.
python3 - "$BBR_DIR/Makefile" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
text = path.read_text()
for line in text.splitlines():
    if line.startswith("obj-m"):
        text = text.replace(line, "obj-m          := tcp_bbr3.o", 1)
        break
path.write_text(text)
PY

make -C "$BBR_DIR" \
  KDIR="$KERNEL_DIR" ARCH=arm64 LLVM=1 LLVM_IAS=1 CROSS_COMPILE=aarch64-linux-gnu- \
  CROSS_COMPILE_COMPAT=arm-linux-gnueabi- "${BBR_CC_ARGS[@]}" CC_PROBE=clang \
  PROBE_J="$(nproc)"

# Compile-time API probes are not enough. Verify the final KO against the
# exact full-build Module.symvers. Do not use the upstream source-grep audit
# here: its EXPORT_SYMBOL capture currently mis-parses GPL exports.
python3 "$REPO_ROOT/scripts/audit-module-exports.py" \
  "$KERNEL_DIR" "$BBR_DIR/tcp_bbr3.ko" \
  --json "$WORK/bbr3-export-audit.json"

rm -rf "$DEST"
mkdir -p "$DEST/$KMI/aarch64"

copy_module() {
  local source=$1
  local name=$2
  test -s "$source"
  install -m 0644 "$source" "$DEST/$KMI/aarch64/$name"
}

copy_module "$KERNEL_DIR/net/ipv4/tcp_bbr.ko" tcp_bbr.ko
copy_module "$BBR_DIR/tcp_bbr3.ko" tcp_bbr3.ko
copy_module "$KERNEL_DIR/net/sched/sch_fq.ko" sch_fq.ko
copy_module "$KERNEL_DIR/net/sched/sch_codel.ko" sch_codel.ko
copy_module "$KERNEL_DIR/net/sched/sch_fq_codel.ko" sch_fq_codel.ko
copy_module "$KERNEL_DIR/net/sched/sch_cake.ko" sch_cake.ko
copy_module "$KERNEL_DIR/net/sched/sch_pie.ko" sch_pie.ko
copy_module "$KERNEL_DIR/net/sched/sch_fq_pie.ko" sch_fq_pie.ko

python3 "$REPO_ROOT/scripts/audit-gki-symbols.py" \
  "$KERNEL_DIR" "$DEST/$KMI/aarch64" \
  --json "$DEST/kmi-symbol-audit-$KMI.json" --strict

python3 - "$DEST" "$KMI" "$KERNEL_RELEASE" "$KERNEL_REV" "$BBR_SOURCE_REV" <<'PY'
import hashlib
import json
from pathlib import Path
import re
import sys

root = Path(sys.argv[1])
kernel_branch, release, kernel_rev, bbr_rev = sys.argv[2:]
module_dir = root / kernel_branch / "aarch64"

match = re.match(r"^(\d+)\.(\d+)\.\d+-(android\d+)-(\d+)", release)
if not match:
    raise SystemExit(f"cannot derive Android KMI from kernel release: {release}")
kmi = f"{match.group(1)}.{match.group(2)}-{match.group(3)}-{match.group(4)}"

modules = []
for path in sorted(module_dir.glob("*.ko")):
    modules.append({
        "name": path.stem,
        "kmi": kmi,
        "arch": "aarch64",
        "file": path.relative_to(root).as_posix(),
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    })

manifest = {
    "schema": 1,
    "kmi": kmi,
    "kernel_branch": kernel_branch,
    "kernel_release": release,
    "kernel_source": {
        "repository": "https://android.googlesource.com/kernel/common",
        "revision": kernel_rev,
    },
    "bbr3_source": {
        "repository": "https://github.com/hrimfaxi/tcp_bbr_modules",
        "revision": bbr_rev,
    },
    "modules": modules,
}
(root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
PY

printf 'built %s kernel modules for %s (%s)\n' \
  "$(find "$DEST/$KMI/aarch64" -name '*.ko' | wc -l)" "$KMI" "$KERNEL_RELEASE"
