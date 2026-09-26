#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
DEST=${1:-"$REPO_ROOT/wgtcp_probe"}
SOURCE_REPO=https://github.com/OnePlusOSS/android_kernel_common_oneplus_sm8750.git
SOURCE_REV=e1b346b6b4f4096eb342ae3684838a942fd6f6c4
WGTCP_REPO=https://github.com/secwest/WGTCP.git
WGTCP_REV=b6ec0eaea1a91902181205551aff963814a5383c
JOBS=${TCP_OPTIMISER_BUILD_JOBS:-$(nproc)}
SYMVERS_CACHE=${TCP_OPTIMISER_SYMVERS_CACHE:-}

WORK=$(mktemp -d)
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

KERNEL="$WORK/kernel"
WGTCP="$WORK/WGTCP"
EXT="$WORK/wgtcp"
LOG="$WORK/build.log"

git init "$KERNEL"
git -C "$KERNEL" remote add origin "$SOURCE_REPO"
git -C "$KERNEL" fetch --depth=1 origin "$SOURCE_REV"
git -C "$KERNEL" checkout --detach FETCH_HEAD

git init "$WGTCP"
git -C "$WGTCP" remote add origin "$WGTCP_REPO"
git -C "$WGTCP" fetch --depth=1 origin "$WGTCP_REV"
git -C "$WGTCP" checkout --detach FETCH_HEAD

KBUILD=(ARCH=arm64 LLVM=-18 LLVM_IAS=1)
if command -v ccache >/dev/null 2>&1; then
  export CCACHE_BASEDIR="$WORK"
  export CCACHE_NOHASHDIR=true
  export CCACHE_COMPILERCHECK=content
  KBUILD+=(CC="ccache clang-18")
fi

make -C "$KERNEL" "${KBUILD[@]}" gki_defconfig
"$KERNEL/scripts/config" --file "$KERNEL/.config" --set-str LOCALVERSION "-android15-8-o-4k"
"$KERNEL/scripts/config" --file "$KERNEL/.config" --disable LOCALVERSION_AUTO
"$KERNEL/scripts/config" --file "$KERNEL/.config" --enable MODULES
"$KERNEL/scripts/config" --file "$KERNEL/.config" --enable MODVERSIONS
"$KERNEL/scripts/config" --file "$KERNEL/.config" --enable MODULE_UNLOAD
# Keep the phone's stock WireGuard built in. The probe module receives its own
# module name, rtnl kind and generic-netlink family so the two can coexist.
"$KERNEL/scripts/config" --file "$KERNEL/.config" --enable WIREGUARD
make -C "$KERNEL" -j"$JOBS" "${KBUILD[@]}" olddefconfig modules_prepare

if [[ -n "$SYMVERS_CACHE" && -s "$SYMVERS_CACHE/vmlinux.symvers" ]]; then
  echo "Using cached verified PJZ110 vmlinux.symvers"
  install -m 0644 "$SYMVERS_CACHE/vmlinux.symvers" "$KERNEL/vmlinux.symvers"
else
  echo "PJZ110 symvers cache miss; building vmlinux once"
  make -C "$KERNEL" -j"$JOBS" "${KBUILD[@]}" vmlinux
  test -s "$KERNEL/vmlinux.symvers"
fi
python3 "$REPO_ROOT/scripts/verify-device-symvers.py" "$REPO_ROOT/scripts/device-profiles/oneplus13-pjz110.json" "$KERNEL/vmlinux.symvers" --report "$WORK/abi-report.json"
if [[ -n "$SYMVERS_CACHE" && ! -s "$SYMVERS_CACHE/vmlinux.symvers" ]]; then
  mkdir -p "$SYMVERS_CACHE"
  install -m 0644 "$KERNEL/vmlinux.symvers" "$SYMVERS_CACHE/vmlinux.symvers"
  install -m 0644 "$WORK/abi-report.json" "$SYMVERS_CACHE/abi-report.json"
fi
install -m 0644 "$KERNEL/vmlinux.symvers" "$KERNEL/Module.symvers"

mkdir -p "$EXT"
cp -a "$WGTCP/kernel" "$EXT/kernel"
cp -a "$WGTCP/include" "$EXT/include"

# Build a side-by-side module instead of replacing Android's built-in WireGuard.
python3 - "$EXT/kernel/Makefile" "$EXT/include/uapi/linux/wireguard.h" <<'PY'
from pathlib import Path
import sys

makefile = Path(sys.argv[1])
uapi = Path(sys.argv[2])

m = makefile.read_text()
m = m.replace("wireguard-y :=", "wgtcp-y :=")
m = m.replace("wireguard-y +=", "wgtcp-y +=")
m = m.replace("obj-$(CONFIG_WIREGUARD) := wireguard.o", "obj-m := wgtcp.o")
if "obj-m := wgtcp.o" not in m:
    raise SystemExit("failed to rename WGTCP module target")
makefile.write_text(m)

h = uapi.read_text()
h = h.replace('#define WG_GENL_NAME "wireguard"', '#define WG_GENL_NAME "wgtcp"')
if '#define WG_GENL_NAME "wgtcp"' not in h:
    raise SystemExit("failed to isolate WGTCP generic-netlink family")
uapi.write_text(h)
PY

mkdir -p "$DEST"
cp "$WORK/abi-report.json" "$DEST/abi-report.json"

set +e
make -C "$KERNEL" -j"$JOBS" "${KBUILD[@]}" M="$EXT/kernel" modules   2>&1 | tee "$LOG"
STRICT_RC=${PIPESTATUS[0]}
set -e

cp "$LOG" "$DEST/build.log"

if [[ "$STRICT_RC" -ne 0 ]]; then
  echo "strict WGTCP external-module build failed; collecting unresolved-symbol evidence"
  set +e
  make -C "$KERNEL" -j"$JOBS" "${KBUILD[@]}" M="$EXT/kernel"     KBUILD_MODPOST_WARN=1 modules 2>&1 | tee "$DEST/modpost-warn.log"
  WARN_RC=${PIPESTATUS[0]}
  set -e
  printf '%s\n' "$STRICT_RC" > "$DEST/strict-build-rc.txt"
  printf '%s\n' "$WARN_RC" > "$DEST/warn-build-rc.txt"
else
  printf '0\n' > "$DEST/strict-build-rc.txt"
fi

if [[ -s "$EXT/kernel/wgtcp.ko" ]]; then
  install -m 0644 "$EXT/kernel/wgtcp.ko" "$DEST/wgtcp.ko"
  modinfo "$DEST/wgtcp.ko" > "$DEST/modinfo.txt" || true
  readelf -S "$DEST/wgtcp.ko" > "$DEST/sections.txt" || true
  nm -u "$DEST/wgtcp.ko" | awk '{print $NF}' | sed '/^$/d' | sort -u > "$DEST/undefined-symbols.txt"

  python3 - "$KERNEL/Module.symvers" "$DEST/undefined-symbols.txt" "$DEST/symbol-audit.json" <<'PY'
import json,sys
from pathlib import Path

symvers=Path(sys.argv[1])
required=Path(sys.argv[2])
out=Path(sys.argv[3])

exports={}
for line in symvers.read_text(errors="replace").splitlines():
    fields=line.split()
    if len(fields)>=2:
        try:
            exports[fields[1]]=int(fields[0],16)
        except ValueError:
            pass

symbols=[x.strip() for x in required.read_text().splitlines() if x.strip()]
missing=[s for s in symbols if s not in exports]
report={
    "required_count":len(symbols),
    "exported_count":len(symbols)-len(missing),
    "missing_count":len(missing),
    "missing":missing,
}
out.write_text(json.dumps(report,indent=2)+"\n")
print(json.dumps(report,indent=2))
PY
fi

python3 - "$DEST" "$SOURCE_REV" "$WGTCP_REV" "$STRICT_RC" <<'PY'
import json,sys
from pathlib import Path
root=Path(sys.argv[1])
report={
    "schema":1,
    "target":"OnePlus 13 PJZ110",
    "mode":"coexisting-external-module-probe",
    "stock_wireguard":"built-in",
    "module_name":"wgtcp",
    "genl_family":"wgtcp",
    "oneplus_source_revision":sys.argv[2],
    "wgtcp_revision":sys.argv[3],
    "strict_build_success":int(sys.argv[4])==0,
    "flashable":False,
}
(root/"probe-manifest.json").write_text(json.dumps(report,indent=2)+"\n")
PY

# This workflow is an ABI/portability probe. Never silently promote a
# warn-only module to a phone-test artifact.
if [[ "$STRICT_RC" -ne 0 ]]; then
  exit "$STRICT_RC"
fi

test -s "$DEST/wgtcp.ko"
grep -q '__versions' "$DEST/sections.txt"
echo "WGTCP coexistence probe built successfully"
