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
VERIFY_OUTPUT="$("$RUST_BIN" verify-module "$MODPATH" 2>&1)"
VERIFY_RC=$?
if [ "$VERIFY_RC" -ne 0 ]; then
    [ -n "$VERIFY_OUTPUT" ] && ui_print "! $VERIFY_OUTPUT"
    abort "! Module signature or file hash verification failed"
fi

TARGET_PROFILE="$MODPATH/device_profile/target.properties"
if [ -f "$TARGET_PROFILE" ]; then
    # The profile is covered by checksums.sha256/checksums.sig and is sourced
    # only after the signed payload has been verified above.
    # shellcheck disable=SC1090
    . "$TARGET_PROFILE"

    [ -n "${TARGET_MODEL:-}" ] || abort "! Device profile is missing TARGET_MODEL"
    [ -n "${TARGET_DEVICE:-}" ] || abort "! Device profile is missing TARGET_DEVICE"
    [ -n "${TARGET_ABI:-}" ] || abort "! Device profile is missing TARGET_ABI"
    [ -n "${TARGET_KMI:-}" ] || abort "! Device profile is missing TARGET_KMI"

    [ "$RUST_ABI" = "$TARGET_ABI" ] || abort "! This package requires ABI $TARGET_ABI"

    MODEL_MATCH=0
    DEVICE_MATCH=0
    IDENTITY_MATCH=0

    for PROP in \
        ro.product.model ro.product.vendor.model ro.product.product.model ro.product.odm.model \
        ro.product.name ro.product.vendor.name ro.product.product.name ro.product.odm.name; do
        [ "$(getprop "$PROP" 2>/dev/null)" = "$TARGET_MODEL" ] && MODEL_MATCH=1
    done

    for PROP in \
        ro.product.device ro.product.vendor.device ro.product.product.device ro.product.odm.device \
        ro.build.product; do
        [ "$(getprop "$PROP" 2>/dev/null)" = "$TARGET_DEVICE" ] && DEVICE_MATCH=1
    done

    BUILD_FINGERPRINT="$(getprop ro.build.fingerprint 2>/dev/null)"
    case "$BUILD_FINGERPRINT" in
        OnePlus/"$TARGET_MODEL"/"$TARGET_DEVICE":*) IDENTITY_MATCH=1 ;;
    esac

    if [ "$IDENTITY_MATCH" -ne 1 ] && { [ "$MODEL_MATCH" -ne 1 ] || [ "$DEVICE_MATCH" -ne 1 ]; }; then
        ui_print "! Detected model: $(getprop ro.product.model 2>/dev/null)"
        ui_print "! Detected name: $(getprop ro.product.name 2>/dev/null)"
        ui_print "! Detected device: $(getprop ro.product.device 2>/dev/null)"
        ui_print "! Detected build.product: $(getprop ro.build.product 2>/dev/null)"
        ui_print "! Detected fingerprint: $BUILD_FINGERPRINT"
        abort "! This package requires OnePlus $TARGET_MODEL ($TARGET_DEVICE)"
    fi

    KERNEL_RELEASE="$(cat /proc/sys/kernel/osrelease 2>/dev/null)"
    VERSION_PART="${KERNEL_RELEASE%%-*}"
    ANDROID_REST="${KERNEL_RELEASE#*-}"
    ANDROID_PART="${ANDROID_REST%%-*}"
    KMI_REST="${ANDROID_REST#*-}"
    KMI_GEN="${KMI_REST%%-*}"
    MAJOR_MINOR="$(printf '%s\n' "$VERSION_PART" | awk -F. '{print $1 "." $2}')"
    CURRENT_KMI="$MAJOR_MINOR-$ANDROID_PART-$KMI_GEN"
    [ "$CURRENT_KMI" = "$TARGET_KMI" ] || abort "! Kernel KMI mismatch: expected $TARGET_KMI, got $CURRENT_KMI"

    ui_print "- Device gate: $TARGET_MODEL ($TARGET_DEVICE) / $TARGET_KMI verified"
fi

"$RUST_BIN" install || abort "! Rust installer failed"
ln -sf "$RUST_ABI/tcp_optimiser" "$MODPATH/bin/tcp_optimiser" || abort "! Cannot select active Rust binary"

[ -s "$MODPATH/available_qdiscs" ] || printf '%s\n' "fq fq_codel cake pfifo_fast codel fq_pie pfifo pie pfifo_head_drop" > "$MODPATH/available_qdiscs"

set_perm "$MODPATH/service.sh" 0 0 0755
set_perm "$MODPATH/post-fs-data.sh" 0 0 0755
set_perm "$MODPATH/uninstall.sh" 0 0 0755
set_perm_recursive "$MODPATH/bin" 0 0 0755 0755

ui_print "- TCP Optimiser: installation complete"
