#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
OUT=${1:-"$ROOT/bpf/out"}
mkdir -p "$OUT"

CLANG=${CLANG:-clang}
if ! "$CLANG" -target bpf -dM -E - </dev/null >/dev/null 2>&1; then
  echo "clang does not support the BPF target" >&2
  exit 1
fi

"$CLANG" -target bpf -D__TARGET_ARCH_arm64 -O2 -g \
  -I"$ROOT/bpf" \
  -c "$ROOT/bpf/tcpopt_probe.bpf.c" \
  -o "$OUT/tcpopt_probe.bpf.o"

llvm-strip -g "$OUT/tcpopt_probe.bpf.o" || true
test -s "$OUT/tcpopt_probe.bpf.o"
printf 'built %s (%s bytes)\n' "$OUT/tcpopt_probe.bpf.o" "$(wc -c < "$OUT/tcpopt_probe.bpf.o")"
