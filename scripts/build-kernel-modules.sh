#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
KMI=${1:-android15-6.6}
DEST=${2:-"$REPO_ROOT/kernel_modules"}
BBR_SOURCE_REV=c5c557584175b5fed8939bf91ec249aed158597d

case "$KMI" in
  android15-6.6) ;;
  *)
    printf 'unsupported initial KMI target: %s\n' "$KMI" >&2
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

make -C "$KERNEL_DIR" ARCH=arm64 LLVM=1 gki_defconfig

CONFIG="$KERNEL_DIR/.config"
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module TCP_CONG_BBR
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_FQ
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_CODEL
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_FQ_CODEL
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_CAKE
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_PIE
"$KERNEL_DIR/scripts/config" --file "$CONFIG" --module NET_SCH_FQ_PIE
make -C "$KERNEL_DIR" ARCH=arm64 LLVM=1 olddefconfig

# A full GKI build is intentional: CONFIG_MODVERSIONS modules need the exact
# Module.symvers/CRC data. modules_prepare alone can create a .ko that compiles
# but is not a trustworthy loadable artifact.
make -C "$KERNEL_DIR" -j"$(nproc)" ARCH=arm64 LLVM=1 Image modules

KERNEL_RELEASE=$(make -s -C "$KERNEL_DIR" ARCH=arm64 LLVM=1 kernelrelease)
KERNEL_REV=$(git -C "$KERNEL_DIR" rev-parse HEAD)

git clone https://github.com/hrimfaxi/tcp_bbr_modules.git "$BBR_DIR"
git -C "$BBR_DIR" checkout --detach "$BBR_SOURCE_REV"

# Only build the BBR modules we actually ship in this Android bundle.
python3 - "$BBR_DIR/Makefile" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
text = path.read_text()
for line in text.splitlines():
    if line.startswith("obj-m"):
        text = text.replace(line, "obj-m          := tcp_bbr1.o tcp_bbr3.o", 1)
        break
path.write_text(text)
PY

make -C "$BBR_DIR" \
  KDIR="$KERNEL_DIR" ARCH=arm64 LLVM=1 CC_PROBE=clang \
  PROBE_J="$(nproc)"

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

python3 - "$DEST" "$KMI" "$KERNEL_RELEASE" "$KERNEL_REV" "$BBR_SOURCE_REV" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
kmi, release, kernel_rev, bbr_rev = sys.argv[2:]
module_dir = root / kmi / "aarch64"

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
