#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
VMLINUX_BTF=${VMLINUX_BTF:-/sys/kernel/btf/vmlinux}
OUT=${1:-"$ROOT/out/bbr2-provider"}
CLANG=${CLANG:-clang}
BPFTOOL=${BPFTOOL:-bpftool}
LLVM_STRIP=${LLVM_STRIP:-llvm-strip}

command -v "$CLANG" >/dev/null
command -v "$BPFTOOL" >/dev/null
command -v "$LLVM_STRIP" >/dev/null
test -r "$VMLINUX_BTF"

mkdir -p "$OUT"
"$BPFTOOL" btf dump file "$VMLINUX_BTF" format c > "$OUT/vmlinux.h"

ARCH_DEFINE=__TARGET_ARCH_x86
case "$(uname -m)" in
  aarch64|arm64) ARCH_DEFINE=__TARGET_ARCH_arm64 ;;
  armv7*|armv8l) ARCH_DEFINE=__TARGET_ARCH_arm ;;
  x86_64|amd64) ARCH_DEFINE=__TARGET_ARCH_x86 ;;
esac

MULTIARCH=$(gcc -print-multiarch 2>/dev/null || true)
INCLUDES=(-I"$OUT" -I/usr/include)
if [ -n "$MULTIARCH" ] && [ -d "/usr/include/$MULTIARCH" ]; then
  INCLUDES+=("-I/usr/include/$MULTIARCH")
fi

"$CLANG" -target bpf -D"$ARCH_DEFINE" -O2 -g   "${INCLUDES[@]}"   -c "$ROOT/experiments/bbr2-provider/structops_probe.bpf.c"   -o "$OUT/tcpopt_structops_probe.bpf.o"

# Preserve BTF/BTF.ext and CO-RE relocation metadata; remove ordinary debug
# sections only. Never use --strip-all on a BPF CO-RE object.
"$LLVM_STRIP" --strip-debug "$OUT/tcpopt_structops_probe.bpf.o"

test -s "$OUT/tcpopt_structops_probe.bpf.o"
printf 'object=%s\n' "$OUT/tcpopt_structops_probe.bpf.o"
printf 'bytes=%s\n' "$(stat -c %s "$OUT/tcpopt_structops_probe.bpf.o")"
