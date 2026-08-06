#!/system/bin/sh

MODDIR="${0%/*}"
case "$(getprop ro.product.cpu.abi 2>/dev/null)" in
    arm64-v8a) RUST_ABI="arm64-v8a" ;;
    armeabi-v7a|armeabi) RUST_ABI="armeabi-v7a" ;;
    x86_64) RUST_ABI="x86_64" ;;
    *) RUST_ABI="" ;;
esac

RUST_BIN="$MODDIR/bin/$RUST_ABI/tcp_optimiser"
if [ -z "$RUST_ABI" ] || [ ! -x "$RUST_BIN" ]; then
    printf '%s - [ERROR] Rust daemon unavailable for ABI %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$(getprop ro.product.cpu.abi)" >> "$MODDIR/service.log"
    exit 1
fi

export TCP_OPTIMISER_MODULE_DIR="$MODDIR"
export PATH="/data/adb/ksu/bin:/system/bin:/system/xbin:$PATH"

# Never apply kernel tuning unless an exact, parseable rollback baseline exists.
if ! "$RUST_BIN" capture-baseline >/dev/null 2>> "$MODDIR/service.log"; then
    printf '%s - [ERROR] Kernel baseline is unavailable; daemon startup refused\n' "$(date '+%Y-%m-%d %H:%M:%S')" >> "$MODDIR/service.log"
    exit 1
fi

exec "$RUST_BIN" daemon
