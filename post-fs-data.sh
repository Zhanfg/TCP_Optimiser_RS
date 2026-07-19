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
"$RUST_BIN" once
