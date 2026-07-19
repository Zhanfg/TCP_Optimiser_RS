#!/system/bin/sh

ui_print "- TCP Optimiser: validating Rust runtime"

[ -d "$MODPATH" ] || abort "! Module staging directory is missing"
[ -w "$MODPATH" ] || abort "! Module staging directory is not writable"
AVAILABLE_KB="$(df -Pk "$MODPATH" 2>/dev/null | awk 'END {print $4}')"
case "$AVAILABLE_KB" in
    ''|*[!0-9]*) ui_print "- Warning: cannot determine free staging space" ;;
    *) [ "$AVAILABLE_KB" -ge 16384 ] || abort "! At least 16 MiB free space is required" ;;
esac

case "$(getprop ro.product.cpu.abi 2>/dev/null)" in
    arm64-v8a) RUST_ABI="arm64-v8a" ;;
    armeabi-v7a|armeabi) RUST_ABI="armeabi-v7a" ;;
    x86_64) RUST_ABI="x86_64" ;;
    *) abort "! Unsupported device ABI" ;;
esac

RUST_BIN="$MODPATH/bin/$RUST_ABI/tcp_optimiser"
[ -f "$RUST_BIN" ] || abort "! Missing Rust binary for $RUST_ABI"
chmod 0755 "$RUST_BIN" || abort "! Cannot make Rust binary executable"

export TCP_OPTIMISER_MODULE_DIR="$MODPATH"
export PATH="/data/adb/ksu/bin:/system/bin:/system/xbin:$PATH"
"$RUST_BIN" verify-module "$MODPATH" || abort "! Module signature or file hash verification failed"
"$RUST_BIN" install || abort "! Rust installer failed"

[ -s "$MODPATH/available_qdiscs" ] || printf '%s\n' "fq fq_codel cake pfifo_fast codel fq_pie pfifo pie pfifo_head_drop" > "$MODPATH/available_qdiscs"

set_perm "$MODPATH/service.sh" 0 0 0755
set_perm "$MODPATH/post-fs-data.sh" 0 0 0755
set_perm "$MODPATH/uninstall.sh" 0 0 0755
set_perm_recursive "$MODPATH/bin" 0 0 0755 0755

ui_print "- TCP Optimiser: installation complete"
