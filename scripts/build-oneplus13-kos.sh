#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
DEST=${1:-"$REPO_ROOT/oneplus13_kernel_modules"}
PROFILE="$REPO_ROOT/scripts/device-profiles/oneplus13-pjz110.json"
SOURCE_REPO=https://github.com/OnePlusOSS/android_kernel_common_oneplus_sm8750.git
SOURCE_REV=e1b346b6b4f4096eb342ae3684838a942fd6f6c4
BBR_REPO=https://github.com/hrimfaxi/tcp_bbr_modules.git
BBR_REV=c5c557584175b5fed8939bf91ec249aed158597d
JOBS=${TCP_OPTIMISER_BUILD_JOBS:-2}

WORK=$(mktemp -d)
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

KERNEL="$WORK/kernel"
BBR="$WORK/tcp_bbr_modules"
OUT="$WORK/out"

git init "$KERNEL"
git -C "$KERNEL" remote add origin "$SOURCE_REPO"
git -C "$KERNEL" fetch --depth=1 origin "$SOURCE_REV"
git -C "$KERNEL" checkout --detach FETCH_HEAD

KBUILD=(ARCH=arm64 LLVM=-18 LLVM_IAS=1)
make -C "$KERNEL" "${KBUILD[@]}" gki_defconfig

CONFIG="$KERNEL/.config"
"$KERNEL/scripts/config" --file "$CONFIG" --set-str LOCALVERSION "-android15-8-o-4k"
"$KERNEL/scripts/config" --file "$CONFIG" --disable LOCALVERSION_AUTO
"$KERNEL/scripts/config" --file "$CONFIG" --enable MODULES
"$KERNEL/scripts/config" --file "$CONFIG" --enable MODVERSIONS
"$KERNEL/scripts/config" --file "$CONFIG" --enable MODULE_UNLOAD
"$KERNEL/scripts/config" --file "$CONFIG" --enable CFI_CLANG
"$KERNEL/scripts/config" --file "$CONFIG" --enable TCP_CONG_ADVANCED
"$KERNEL/scripts/config" --file "$CONFIG" --enable TCP_CONG_BBR
"$KERNEL/scripts/config" --file "$CONFIG" --enable NET_SCH_FQ
"$KERNEL/scripts/config" --file "$CONFIG" --enable NET_SCH_CODEL
"$KERNEL/scripts/config" --file "$CONFIG" --enable NET_SCH_FQ_CODEL
"$KERNEL/scripts/config" --file "$CONFIG" --module NET_SCH_CAKE
"$KERNEL/scripts/config" --file "$CONFIG" --module NET_SCH_PIE
"$KERNEL/scripts/config" --file "$CONFIG" --module NET_SCH_FQ_PIE
make -C "$KERNEL" "${KBUILD[@]}" olddefconfig

grep -qx 'CONFIG_MODVERSIONS=y' "$CONFIG"
grep -qx 'CONFIG_CFI_CLANG=y' "$CONFIG"
grep -qx 'CONFIG_TCP_CONG_BBR=y' "$CONFIG"
grep -qx 'CONFIG_NET_SCH_CAKE=m' "$CONFIG"
grep -qx 'CONFIG_NET_SCH_PIE=m' "$CONFIG"
grep -qx 'CONFIG_NET_SCH_FQ_PIE=m' "$CONFIG"

# Generate the source CRC table using the pinned OnePlus kernel. Do not emit
# any device bundle unless all 65 live-device CRC fingerprints match.
make -C "$KERNEL" -j"$JOBS" "${KBUILD[@]}" vmlinux
test -s "$KERNEL/Module.symvers"
python3 "$REPO_ROOT/scripts/verify-device-symvers.py"   "$PROFILE" "$KERNEL/Module.symvers" --report "$WORK/device-abi-report.json"

make -C "$KERNEL" -j"$JOBS" "${KBUILD[@]}" M=net/sched   net/sched/sch_cake.ko net/sched/sch_pie.ko net/sched/sch_fq_pie.ko

git clone --filter=blob:none "$BBR_REPO" "$BBR"
git -C "$BBR" checkout --detach "$BBR_REV"
python3 - "$BBR/Makefile" <<'PY'
from pathlib import Path
import sys
p=Path(sys.argv[1])
text=p.read_text()
for line in text.splitlines():
    if line.startswith("obj-m"):
        text=text.replace(line,"obj-m          := tcp_bbr3.o",1)
        break
p.write_text(text)
PY

make -C "$BBR"   KDIR="$KERNEL" ARCH=arm64 LLVM=-18 LLVM_IAS=1   CC_PROBE=clang-18 PROBE_J="$JOBS" probe
make -C "$BBR"   KDIR="$KERNEL" ARCH=arm64 LLVM=-18 LLVM_IAS=1   CC_PROBE=clang-18 PROBE_J="$JOBS"

rm -rf "$DEST"
mkdir -p "$DEST/6.6-android15-8/aarch64" "$DEST/device_profile"
install -m 0644 "$KERNEL/net/sched/sch_cake.ko" "$DEST/6.6-android15-8/aarch64/"
install -m 0644 "$KERNEL/net/sched/sch_pie.ko" "$DEST/6.6-android15-8/aarch64/"
install -m 0644 "$KERNEL/net/sched/sch_fq_pie.ko" "$DEST/6.6-android15-8/aarch64/"
install -m 0644 "$BBR/tcp_bbr3.ko" "$DEST/6.6-android15-8/aarch64/"
install -m 0644 "$PROFILE" "$DEST/device_profile/oneplus13-pjz110.json"
install -m 0644 "$WORK/device-abi-report.json" "$DEST/device_profile/abi-report.json"

# Every output must carry symbol versions. Android's module loader ignores the
# kernel-release prefix in vermagic when __versions is present, but it still
# checks the remainder and every imported symbol CRC.
for ko in "$DEST"/6.6-android15-8/aarch64/*.ko; do
  readelf -S "$ko" | grep -q '__versions'
  modinfo -F vermagic "$ko" | tee -a "$DEST/device_profile/vermagic.txt"
done

python3 - "$DEST" "$KERNEL/Module.symvers" "$SOURCE_REV" "$BBR_REV" <<'PY'
import hashlib,json,subprocess,sys
from pathlib import Path
root=Path(sys.argv[1])
symvers=Path(sys.argv[2])
source_rev=sys.argv[3]
bbr_rev=sys.argv[4]
mods=[]
for p in sorted((root/"6.6-android15-8/aarch64").glob("*.ko")):
    mods.append({
        "name":p.stem,
        "kmi":"6.6-android15-8",
        "arch":"aarch64",
        "file":p.relative_to(root).as_posix(),
        "sha256":hashlib.sha256(p.read_bytes()).hexdigest(),
        "kernel_release":None,
    })
manifest={
    "schema":1,
    "scope":"device-kmi",
    "device_profile":"oneplus13-pjz110",
    "requires_paired_kernel":False,
    "kmi":"6.6-android15-8",
    "kernel_source":{
        "repository":"https://github.com/OnePlusOSS/android_kernel_common_oneplus_sm8750",
        "revision":source_rev,
    },
    "bbr3_source":{
        "repository":"https://github.com/hrimfaxi/tcp_bbr_modules",
        "revision":bbr_rev,
    },
    "builtin_capabilities":["tcp_bbr","sch_fq","sch_codel","sch_fq_codel"],
    "modules":mods,
}
(root/"manifest.json").write_text(json.dumps(manifest,indent=2)+"\n")
PY

cp "$KERNEL/vmlinux.symvers" "$DEST/device_profile/vmlinux.symvers"
cp "$KERNEL/Module.symvers" "$DEST/device_profile/Module.symvers"
printf 'built OnePlus 13 PJZ110 KO-only bundle with %s modules\n'   "$(find "$DEST/6.6-android15-8/aarch64" -name '*.ko' | wc -l)"
