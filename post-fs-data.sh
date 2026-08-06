#!/system/bin/sh

MODDIR="${0%/*}"
case "$(getprop ro.product.cpu.abi 2>/dev/null)" in
    arm64-v8a) RUST_ABI="arm64-v8a" ;;
    armeabi-v7a|armeabi) RUST_ABI="armeabi-v7a" ;;
    x86_64) RUST_ABI="x86_64" ;;
    *) exit 0 ;;
esac

RUST_BIN="$MODDIR/bin/$RUST_ABI/tcp_optimiser"
[ -x "$RUST_BIN" ] || exit 0

export TCP_OPTIMISER_MODULE_DIR="$MODDIR"
export PATH="/data/adb/ksu/bin:/system/bin:/system/xbin:$PATH"

# Refuse the early one-shot write path if the original kernel state cannot be
# captured or the preserved baseline no longer parses correctly.
if ! "$RUST_BIN" capture-baseline >/dev/null 2>> "$MODDIR/service.log"; then
    printf '%s - [ERROR] Kernel baseline is unavailable; early tuning skipped\n' "$(date '+%Y-%m-%d %H:%M:%S')" >> "$MODDIR/service.log"
    exit 1
fi

"$RUST_BIN" once
