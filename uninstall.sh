#!/system/bin/sh

MODDIR="${0%/*}"
pkill -f "$MODDIR/bin/.*/tcp_optimiser" 2>/dev/null

case "$(getprop ro.product.cpu.abi 2>/dev/null)" in
    arm64-v8a) RUST_ABI="arm64-v8a" ;;
    armeabi-v7a|armeabi) RUST_ABI="armeabi-v7a" ;;
    x86_64) RUST_ABI="x86_64" ;;
    *) RUST_ABI="" ;;
esac

RUST_BIN="$MODDIR/bin/$RUST_ABI/tcp_optimiser"
BASELINE="$MODDIR/.sysctl-baseline.json"
export TCP_OPTIMISER_MODULE_DIR="$MODDIR"
export PATH="/data/adb/ksu/bin:/system/bin:/system/xbin:$PATH"

if [ -f "$BASELINE" ] && [ -n "$RUST_ABI" ] && [ -x "$RUST_BIN" ]; then
    "$RUST_BIN" restore-baseline >/dev/null 2>&1 ||         printf '%s\n' "[TCP Optimiser] Warning: some baseline sysctls could not be restored" >&2
else
    printf '%s\n' "[TCP Optimiser] No captured baseline; leaving kernel defaults unchanged" >&2
fi

rm -f     "$MODDIR/service.log"     "$MODDIR/debug.log"     "$MODDIR/daemon.pid"     "$MODDIR/force_apply"     /dev/.tcp_module_log_cleared 2>/dev/null
rm -f "$MODDIR"/.route_cache_* 2>/dev/null
