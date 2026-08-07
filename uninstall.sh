#!/system/bin/sh

MODDIR="${0%/*}"
LOG_FILE="$MODDIR/uninstall-restore.log"
PID_FILE="$MODDIR/daemon.pid"
PROVENANCE_FILE="$MODDIR/baseline-provenance-v1.json"

# Stop the exact daemon instance first; retain pkill only as a compatibility
# fallback for installations created before PID journaling was introduced.
if [ -r "$PID_FILE" ]; then
    DAEMON_PID="$(sed -n '1p' "$PID_FILE" 2>/dev/null)"
    case "$DAEMON_PID" in
        ''|*[!0-9]*) ;;
        *) kill "$DAEMON_PID" 2>/dev/null || true ;;
    esac
fi
sleep 1
pkill -f "$MODDIR/bin/.*/tcp_optimiser" 2>/dev/null || true

case "$(getprop ro.product.cpu.abi 2>/dev/null)" in
    arm64-v8a) RUST_ABI="arm64-v8a" ;;
    armeabi-v7a|armeabi) RUST_ABI="armeabi-v7a" ;;
    x86_64) RUST_ABI="x86_64" ;;
    *) RUST_ABI="" ;;
esac

RUST_BIN="$MODDIR/bin/$RUST_ABI/tcp_optimiser"
export TCP_OPTIMISER_MODULE_DIR="$MODDIR"
export PATH="/data/adb/ksu/bin:/system/bin:/system/xbin:$PATH"

BASELINE_KIND="exact_pre_module"
if [ -s "$PROVENANCE_FILE" ] && grep -Fq '"provenance": "legacy_upgrade_snapshot"' "$PROVENANCE_FILE"; then
    BASELINE_KIND="legacy_upgrade_snapshot"
fi

if [ -n "$RUST_ABI" ] && [ -x "$RUST_BIN" ] && [ -s "$MODDIR/baseline-v1.json" ]; then
    if "$RUST_BIN" restore-baseline > "$LOG_FILE" 2>&1; then
        if [ "$BASELINE_KIND" = "legacy_upgrade_snapshot" ]; then
            printf '%s\n' "[WARN] Restored the state captured immediately before the legacy in-place upgrade; this was not verified as the vendor default." >> "$LOG_FILE"
            command -v log >/dev/null 2>&1 && log -t TCP_Optimiser "Pre-upgrade compatibility snapshot restored"
        else
            command -v log >/dev/null 2>&1 && log -t TCP_Optimiser "Original kernel baseline restored"
        fi
    else
        command -v log >/dev/null 2>&1 && log -p e -t TCP_Optimiser "Kernel baseline restoration reported errors; see $LOG_FILE before module removal completes"
    fi
else
    printf '%s\n' "[ERROR] Baseline restoration unavailable; no generic TCP defaults were forced" > "$LOG_FILE"
    command -v log >/dev/null 2>&1 && log -p e -t TCP_Optimiser "Kernel baseline unavailable; refusing unsafe cubic/fq_codel fallback"
fi

# Logs are retained only until the module manager removes the module directory,
# allowing the uninstall UI or recovery shell to inspect the restoration report.
rm -f /dev/.tcp_module_log_cleared 2>/dev/null
exit 0
