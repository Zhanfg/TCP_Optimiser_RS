#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
KMI=${1:-android15-6.6}
DEST=${2:-"$REPO_ROOT/kernel_modules"}
BBR_SOURCE_REV=c5c557584175b5fed8939bf91ec249aed158597d
CONFIG_SPEC="$REPO_ROOT/scripts/kernel-module-config.txt"
JOBS=${TCP_OPTIMISER_BUILD_JOBS:-$(nproc)}
KMI_PREFLIGHT=${TCP_OPTIMISER_KMI_PREFLIGHT:-1}
PREFLIGHT_ONLY=${TCP_OPTIMISER_PREFLIGHT_ONLY:-0}
MINIMAL_KERNEL_BUILD=${TCP_OPTIMISER_MINIMAL_KERNEL_BUILD:-0}
SYMVERS_CACHE=${KERNEL_SYMVERS_CACHE:-}
THINLTO_CACHE=${KERNEL_THINLTO_CACHE:-}
BBR_KCONFIG_CACHE=${BBR_KCONFIG_CACHE:-}
PREFLIGHT_REPORT=${TCP_OPTIMISER_PREFLIGHT_REPORT:-}
PREFLIGHT_CACHE=${TCP_OPTIMISER_PREFLIGHT_CACHE:-}

case "$KMI" in
  android12-5.10|android13-5.15|android14-6.1|android15-6.6) ;;
  *)
    printf 'unsupported KMI target: %s\n' "$KMI" >&2
    exit 2
    ;;
esac

test -s "$CONFIG_SPEC"

# A workflow-level cache key guards this report with the exact kernel revision,
# compiler identity, module config and audit/build scripts. When present, reuse
# the deterministic compatibility result before cloning/preparing a kernel.
if [[ -n "$PREFLIGHT_CACHE" && -s "$PREFLIGHT_CACHE" ]]; then
  set +e
  python3 - "$PREFLIGHT_CACHE" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1]))
modules = data.get("modules", [])
if not modules:
    raise SystemExit(2)
bad = [m for m in modules if not m.get("compatible", False)]
print(f"cached KMI preflight: checked={len(modules)} incompatible={len(bad)}")
for item in bad:
    print(f"  {item['file']}: {', '.join(item.get('non_kmi_symbols', []))}")
raise SystemExit(1 if bad else 0)
PY
  cached_rc=$?
  set -e
  if [[ "$cached_rc" -eq 1 ]]; then
    printf 'cached KMI preflight rejects %s; skipping kernel preparation/build\n' "$KMI" >&2
    exit 1
  elif [[ "$cached_rc" -eq 0 ]]; then
    printf 'cached KMI preflight accepts %s\n' "$KMI"
    if [[ "$PREFLIGHT_ONLY" == "1" ]]; then
      exit 0
    fi
    KMI_PREFLIGHT=0
  else
    printf '[WARN] cached KMI preflight report is invalid; recomputing\n' >&2
  fi
fi

WORK=$(mktemp -d)
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

KERNEL_DIR="$WORK/kernel"
BBR_DIR="$WORK/tcp_bbr_modules"
PREFLIGHT_DIR="$WORK/kmi-preflight"

git clone --filter=blob:none --depth=1 --branch "$KMI" \
  https://android.googlesource.com/kernel/common "$KERNEL_DIR"

if [[ -n "$THINLTO_CACHE" ]]; then
  mkdir -p "$THINLTO_CACHE"
  rm -rf "$KERNEL_DIR/.thinlto-cache"
  ln -s "$THINLTO_CACHE" "$KERNEL_DIR/.thinlto-cache"
fi

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
while IFS='=' read -r option value; do
  [[ -z "$option" || "$option" == \#* ]] && continue
  case "$value" in
    m) "$KERNEL_DIR/scripts/config" --file "$CONFIG" --module "$option" ;;
    y) "$KERNEL_DIR/scripts/config" --file "$CONFIG" --enable "$option" ;;
    n) "$KERNEL_DIR/scripts/config" --file "$CONFIG" --disable "$option" ;;
    *)
      printf 'unsupported kernel config value: %s=%s\n' "$option" "$value" >&2
      exit 2
      ;;
  esac
done < "$CONFIG_SPEC"
make -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" olddefconfig

# modules_prepare is inexpensive and produces generated headers and host tools
# required for targeted module compilation. It intentionally does not generate
# Module.symvers when CONFIG_MODVERSIONS is enabled.
make -C "$KERNEL_DIR" -j"$JOBS" "${KBUILD_ARGS[@]}" modules_prepare

SYMVERS_HIT=0
if [[ -n "$SYMVERS_CACHE" && -s "$SYMVERS_CACHE" ]]; then
  install -m 0644 "$SYMVERS_CACHE" "$KERNEL_DIR/Module.symvers"
  SYMVERS_HIT=1
  printf 'using cached exact Module.symvers: %s\n' "$SYMVERS_CACHE"
fi

git clone --filter=blob:none https://github.com/hrimfaxi/tcp_bbr_modules.git "$BBR_DIR"
git -C "$BBR_DIR" checkout --detach "$BBR_SOURCE_REV"

# BBR v1 comes from the Android kernel tree. Build only the OOT BBRv3 object.
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

prepare_bbr_probe() {
  if [[ -n "$BBR_KCONFIG_CACHE" && -s "$BBR_KCONFIG_CACHE" ]]; then
    install -m 0644 "$BBR_KCONFIG_CACHE" "$BBR_DIR/kernel_config.h"
    # Fresh checkout mtimes would otherwise make Make regenerate the probe.
    touch "$BBR_DIR/kernel_config.h"
    printf 'using cached BBR3 API probe: %s\n' "$BBR_KCONFIG_CACHE"
    return
  fi

  make -C "$BBR_DIR" \
    KDIR="$KERNEL_DIR" ARCH=arm64 LLVM=1 LLVM_IAS=1 \
    CROSS_COMPILE=aarch64-linux-gnu- CROSS_COMPILE_COMPAT=arm-linux-gnueabi- \
    "${BBR_CC_ARGS[@]}" CC_PROBE=clang PROBE_J="$JOBS" probe

  if [[ -n "$BBR_KCONFIG_CACHE" ]]; then
    mkdir -p "$(dirname "$BBR_KCONFIG_CACHE")"
    install -m 0644 "$BBR_DIR/kernel_config.h" "$BBR_KCONFIG_CACHE"
  fi
}

prepare_bbr_probe

build_in_tree_targets() {
  local warn=$1
  local -a extra=()
  if [[ "$warn" == "1" ]]; then
    extra+=(KBUILD_MODPOST_WARN=1)
  fi
  make -C "$KERNEL_DIR" -j"$JOBS" "${KBUILD_ARGS[@]}" "${extra[@]}" \
    M=net/ipv4 tcp_bbr.ko
  make -C "$KERNEL_DIR" -j"$JOBS" "${KBUILD_ARGS[@]}" "${extra[@]}" \
    M=net/sched \
    sch_fq.ko sch_codel.ko sch_fq_codel.ko sch_cake.ko sch_pie.ko sch_fq_pie.ko
}

build_bbr3() {
  local warn=$1
  local -a extra=()
  if [[ "$warn" == "1" ]]; then
    extra+=(KBUILD_MODPOST_WARN=1)
  fi
  make -C "$BBR_DIR" \
    KDIR="$KERNEL_DIR" ARCH=arm64 LLVM=1 LLVM_IAS=1 \
    CROSS_COMPILE=aarch64-linux-gnu- CROSS_COMPILE_COMPAT=arm-linux-gnueabi- \
    "${BBR_CC_ARGS[@]}" "${extra[@]}" CC_PROBE=clang PROBE_J="$JOBS"
}

stage_modules() {
  local root=$1
  rm -rf "$root"
  mkdir -p "$root"
  install -m 0644 "$KERNEL_DIR/net/ipv4/tcp_bbr.ko" "$root/tcp_bbr.ko"
  install -m 0644 "$BBR_DIR/tcp_bbr3.ko" "$root/tcp_bbr3.ko"
  install -m 0644 "$KERNEL_DIR/net/sched/sch_fq.ko" "$root/sch_fq.ko"
  install -m 0644 "$KERNEL_DIR/net/sched/sch_codel.ko" "$root/sch_codel.ko"
  install -m 0644 "$KERNEL_DIR/net/sched/sch_fq_codel.ko" "$root/sch_fq_codel.ko"
  install -m 0644 "$KERNEL_DIR/net/sched/sch_cake.ko" "$root/sch_cake.ko"
  install -m 0644 "$KERNEL_DIR/net/sched/sch_pie.ko" "$root/sch_pie.ko"
  install -m 0644 "$KERNEL_DIR/net/sched/sch_fq_pie.ko" "$root/sch_fq_pie.ko"
}

clean_target_outputs() {
  make -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" M=net/ipv4 clean
  make -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" M=net/sched clean
  make -C "$BBR_DIR" \
    KDIR="$KERNEL_DIR" ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- clean || true
}

# Cheap KMI gate first. On a cache miss, KBUILD_MODPOST_WARN allows producing
# temporary KOs without CRC data solely so undefined symbol names can be
# compared against Android's GKI KMI lists. If this fails, a full GKI build
# cannot make the module generically GKI-loadable, so stop before expensive LTO.
if [[ "$KMI_PREFLIGHT" == "1" ]]; then
  if [[ "$SYMVERS_HIT" == "1" ]]; then
    build_in_tree_targets 0
    build_bbr3 0
  else
    build_in_tree_targets 1
    build_bbr3 1
  fi
  stage_modules "$PREFLIGHT_DIR"
  preflight_json="$WORK/kmi-preflight-$KMI.json"
  set +e
  python3 "$REPO_ROOT/scripts/audit-gki-symbols.py" \
    "$KERNEL_DIR" "$PREFLIGHT_DIR" \
    --json "$preflight_json" --strict
  preflight_rc=$?
  set -e
  if [[ -n "$PREFLIGHT_REPORT" && -s "$preflight_json" ]]; then
    mkdir -p "$(dirname "$PREFLIGHT_REPORT")"
    install -m 0644 "$preflight_json" "$PREFLIGHT_REPORT"
  fi
  if [[ -n "$PREFLIGHT_CACHE" && -s "$preflight_json" ]]; then
    mkdir -p "$(dirname "$PREFLIGHT_CACHE")"
    install -m 0644 "$preflight_json" "$PREFLIGHT_CACHE"
  fi
  if [[ "$preflight_rc" -ne 0 ]]; then
    printf 'KMI preflight rejected %s before full GKI build\n' "$KMI" >&2
    exit "$preflight_rc"
  fi
  if [[ "$PREFLIGHT_ONLY" == "1" ]]; then
    printf 'KMI preflight-only mode completed for %s\n' "$KMI"
    exit 0
  fi
fi

if [[ "$SYMVERS_HIT" != "1" ]]; then
  clean_target_outputs

  if [[ "$MINIMAL_KERNEL_BUILD" == "1" ]]; then
    printf 'trying minimal vmlinux-only build for Module.symvers\n'
    make -C "$KERNEL_DIR" -j"$JOBS" "${KBUILD_ARGS[@]}" vmlinux
  fi

  if [[ ! -s "$KERNEL_DIR/Module.symvers" ]]; then
    printf 'Module.symvers unavailable; falling back to full GKI Image+modules build\n'
    make -C "$KERNEL_DIR" -j"$JOBS" "${KBUILD_ARGS[@]}" Image modules
  else
    # A vmlinux-only build has not built our selected modular targets.
    build_in_tree_targets 0
  fi

  test -s "$KERNEL_DIR/Module.symvers"
  if [[ -n "$SYMVERS_CACHE" ]]; then
    mkdir -p "$(dirname "$SYMVERS_CACHE")"
    install -m 0644 "$KERNEL_DIR/Module.symvers" "$SYMVERS_CACHE"
    printf 'stored exact Module.symvers cache candidate: %s\n' "$SYMVERS_CACHE"
  fi

  # The preflight BBR3 module was built without exact CRCs. Rebuild it against
  # the exact target Module.symvers.
  make -C "$BBR_DIR" \
    KDIR="$KERNEL_DIR" ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- clean || true
  build_bbr3 0
elif [[ "$KMI_PREFLIGHT" != "1" ]]; then
  # Cache hit + preflight disabled: compile just the eight required modules.
  build_in_tree_targets 0
  build_bbr3 0
fi

python3 "$REPO_ROOT/scripts/audit-module-exports.py" \
  "$KERNEL_DIR" "$BBR_DIR/tcp_bbr3.ko" \
  --json "$WORK/bbr3-export-audit.json"

KERNEL_RELEASE=$(make -s -C "$KERNEL_DIR" "${KBUILD_ARGS[@]}" kernelrelease)
KERNEL_REV=$(git -C "$KERNEL_DIR" rev-parse HEAD)

rm -rf "$DEST"
mkdir -p "$DEST/$KMI/aarch64"
stage_modules "$DEST/$KMI/aarch64"

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
