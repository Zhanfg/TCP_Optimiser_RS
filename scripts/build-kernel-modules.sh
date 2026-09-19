#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
KMI=${1:-android15-6.6}
DEST=${2:-"$REPO_ROOT/kernel_modules"}
BBR_SOURCE_REV=c5c557584175b5fed8939bf91ec249aed158597d
MODULE_CONFIG_FILE="$REPO_ROOT/scripts/gki-module-config.txt"
SDK_CACHE_DIR=${GKI_SDK_CACHE_DIR:-}
JOBS=${KBUILD_JOBS:-$(nproc)}
BUILD_STARTED=$SECONDS

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

KERNEL_REV=$(git -C "$KERNEL_DIR" rev-parse HEAD)
SOURCE_DATE_EPOCH=$(git -C "$KERNEL_DIR" show -s --format=%ct HEAD)
KBUILD_BUILD_TIMESTAMP=$(git -C "$KERNEL_DIR" show -s --format=%cD HEAD)
export SOURCE_DATE_EPOCH KBUILD_BUILD_TIMESTAMP
export KBUILD_BUILD_USER=tcp-optimiser
export KBUILD_BUILD_HOST=github-actions

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
while IFS='=' read -r symbol value; do
  case "$symbol" in
    ''|'#'*) continue ;;
  esac
  case "$value" in
    m) "$KERNEL_DIR/scripts/config" --file "$CONFIG" --module "$symbol" ;;
    y) "$KERNEL_DIR/scripts/config" --file "$CONFIG" --enable "$symbol" ;;
    n) "$KERNEL_DIR/scripts/config" --file "$CONFIG" --disable "$symbol" ;;
    *)
      printf 'invalid GKI module config entry: %s=%s\n' "$symbol" "$value" >&2
      exit 2
      ;;
  esac
done < "$MODULE_CONFIG_FILE"
make -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" olddefconfig

CONFIG_SHA=$(sha256sum "$CONFIG" | awk '{print $1}')
KERNEL_RELEASE=$(make -s -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" kernelrelease)
SDK_HIT=0

if [ -n "$SDK_CACHE_DIR" ] &&
   [ -s "$SDK_CACHE_DIR/Module.symvers" ] &&
   [ -s "$SDK_CACHE_DIR/metadata.json" ]; then
  if python3 - "$SDK_CACHE_DIR/metadata.json" "$KERNEL_REV" "$CONFIG_SHA" "$KERNEL_RELEASE" <<'PY'
import json
import sys
from pathlib import Path

meta = json.loads(Path(sys.argv[1]).read_text())
expected = {
    "schema": 1,
    "kernel_revision": sys.argv[2],
    "config_sha256": sys.argv[3],
    "kernel_release": sys.argv[4],
}
raise SystemExit(0 if all(meta.get(key) == value for key, value in expected.items()) else 1)
PY
  then
    SDK_HIT=1
  fi
fi

if [ "$SDK_HIT" -eq 1 ]; then
  printf 'GKI SDK cache hit for %s: skipping vmlinux/LTO bootstrap\n' "$KMI"
  cp "$SDK_CACHE_DIR/Module.symvers" "$KERNEL_DIR/Module.symvers"
  make -C "$KERNEL_DIR" -j"$JOBS" "${KBUILD_ARGS[@]}" modules_prepare

  # Module BTF generation only needs the base BTF, not the hundreds-of-MiB
  # vmlinux ELF. libbpf/pahole accept a detached raw BTF file here.
  if [ -s "$SDK_CACHE_DIR/vmlinux.btf" ]; then
    cp "$SDK_CACHE_DIR/vmlinux.btf" "$KERNEL_DIR/vmlinux"
  fi
  BUILD_MODE=sdk-cache
else
  printf 'GKI SDK cache miss for %s: bootstrapping vmlinux once\n' "$KMI"

  # CONFIG_MODVERSIONS requires a real Module.symvers. Build vmlinux to run
  # modpost, but do not build Image or every unrelated kernel module.
  make -C "$KERNEL_DIR" -j"$JOBS" "${KBUILD_ARGS[@]}" vmlinux
  test -s "$KERNEL_DIR/Module.symvers"

  if [ -n "$SDK_CACHE_DIR" ]; then
    mkdir -p "$SDK_CACHE_DIR"
    install -m 0644 "$KERNEL_DIR/Module.symvers" "$SDK_CACHE_DIR/Module.symvers"

    # Keep only the compact base-BTF payload needed to BTF-finalize modules.
    if llvm-readelf -S "$KERNEL_DIR/vmlinux" | grep -q '[.]BTF'; then
      llvm-objcopy --dump-section .BTF="$SDK_CACHE_DIR/vmlinux.btf" "$KERNEL_DIR/vmlinux"
    else
      rm -f "$SDK_CACHE_DIR/vmlinux.btf"
    fi

    python3 - "$SDK_CACHE_DIR/metadata.json" "$KERNEL_REV" "$CONFIG_SHA" "$KERNEL_RELEASE" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
path.write_text(json.dumps({
    "schema": 1,
    "kernel_revision": sys.argv[2],
    "config_sha256": sys.argv[3],
    "kernel_release": sys.argv[4],
}, indent=2, sort_keys=True) + "\n")
PY
  fi
  BUILD_MODE=cold-vmlinux
fi

# Build only the seven in-tree modules we ship. Supplying all qdisc targets
# together lets modpost resolve sch_fq_pie -> sch_pie exports without building
# every other driver/module selected by gki_defconfig.
TARGET_MODULES=(
  net/ipv4/tcp_bbr.ko
  net/sched/sch_fq.ko
  net/sched/sch_codel.ko
  net/sched/sch_fq_codel.ko
  net/sched/sch_cake.ko
  net/sched/sch_pie.ko
  net/sched/sch_fq_pie.ko
)
TARGET_BUILD_STARTED=$SECONDS
make -C "$KERNEL_DIR" -j"$JOBS" "${KBUILD_ARGS[@]}" "${TARGET_MODULES[@]}"
TARGET_BUILD_SECONDS=$((SECONDS - TARGET_BUILD_STARTED))

mkdir -p "$BBR_DIR"
git -C "$BBR_DIR" init -q
git -C "$BBR_DIR" remote add origin https://github.com/hrimfaxi/tcp_bbr_modules.git
git -C "$BBR_DIR" fetch -q --depth=1 origin "$BBR_SOURCE_REV"
git -C "$BBR_DIR" checkout -q --detach FETCH_HEAD

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
  PROBE_J="$JOBS"

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

TOTAL_SECONDS=$((SECONDS - BUILD_STARTED))
printf 'built %s kernel modules for %s (%s); mode=%s target_modules=%ss total=%ss\n' \
  "$(find "$DEST/$KMI/aarch64" -name '*.ko' | wc -l)" "$KMI" "$KERNEL_RELEASE" \
  "$BUILD_MODE" "$TARGET_BUILD_SECONDS" "$TOTAL_SECONDS"

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    printf '### GKI build %s\n\n' "$KMI"
    printf -- '- Build mode: %s\n' "$BUILD_MODE"
    printf -- '- Target-module phase: %ss\n' "$TARGET_BUILD_SECONDS"
    printf -- '- Total script time: %ss\n' "$TOTAL_SECONDS"
    if [ -n "$SDK_CACHE_DIR" ] && [ -s "$SDK_CACHE_DIR/Module.symvers" ]; then
      printf -- '- Reusable Module.symvers SDK: ready\n'
    fi
  } >> "$GITHUB_STEP_SUMMARY"
fi
