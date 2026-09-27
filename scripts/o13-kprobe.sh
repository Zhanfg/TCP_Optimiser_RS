#!/system/bin/sh
# O13 Kernel ABI Probe — read-only.

OUTDIR="/sdcard/Download"
STAMP="$(date +%Y%m%d_%H%M%S 2>/dev/null)"
[ -n "$STAMP" ] || STAMP="unknown"
WORK="/data/local/tmp/o13_kprobe_$STAMP"
LOG="$WORK/o13_kernel_probe_$STAMP.txt"
TAR="$OUTDIR/O13_KProbe_$STAMP.tar.gz"
mkdir -p "$WORK" "$OUTDIR" 2>/dev/null
exec >"$LOG" 2>&1

echo "=== O13 Kernel ABI Probe ==="
echo "time: $(date 2>/dev/null)"
echo "uid: $(id -u 2>/dev/null)"
echo
echo "=== Device ==="
echo "manufacturer: $(getprop ro.product.manufacturer 2>/dev/null)"
echo "brand: $(getprop ro.product.brand 2>/dev/null)"
echo "model: $(getprop ro.product.model 2>/dev/null)"
echo "device: $(getprop ro.product.device 2>/dev/null)"
echo "product: $(getprop ro.product.name 2>/dev/null)"
echo "android: $(getprop ro.build.version.release 2>/dev/null)"
echo "sdk: $(getprop ro.build.version.sdk 2>/dev/null)"
echo "build_display: $(getprop ro.build.display.id 2>/dev/null)"
echo "fingerprint: $(getprop ro.build.fingerprint 2>/dev/null)"
echo
echo "=== Kernel identity ==="
echo "osrelease: $(cat /proc/sys/kernel/osrelease 2>/dev/null)"
echo "uname_a: $(uname -a 2>/dev/null)"
echo "proc_version: $(cat /proc/version 2>/dev/null)"
echo "kptr_restrict: $(cat /proc/sys/kernel/kptr_restrict 2>/dev/null)"
echo "modules_disabled: $(cat /proc/sys/kernel/modules_disabled 2>/dev/null)"
echo "module_sig_enforce: $(cat /proc/sys/kernel/module_sig_enforce 2>/dev/null)"
echo
echo "=== TCP state ==="
echo "available_cc: $(cat /proc/sys/net/ipv4/tcp_available_congestion_control 2>/dev/null)"
echo "current_cc: $(cat /proc/sys/net/ipv4/tcp_congestion_control 2>/dev/null)"
echo "default_qdisc: $(cat /proc/sys/net/core/default_qdisc 2>/dev/null)"
echo
echo "=== Kernel config summary ==="
if [ -r /proc/config.gz ]; then
  cp /proc/config.gz "$WORK/proc_config.gz" 2>/dev/null
  zcat /proc/config.gz 2>/dev/null | grep -E '^(CONFIG_MODULES|CONFIG_MODVERSIONS|CONFIG_MODULE_SIG|CONFIG_MODULE_SIG_FORCE|CONFIG_KALLSYMS|CONFIG_KALLSYMS_ALL|CONFIG_DEBUG_INFO_BTF|CONFIG_TCP_CONG_ADVANCED|CONFIG_TCP_CONG_BBR|CONFIG_NET_SCH_FQ|CONFIG_NET_SCH_FQ_CODEL)='
else
  echo "/proc/config.gz: unavailable"
fi
echo
echo "=== BTF ==="
if [ -r /sys/kernel/btf/vmlinux ]; then
  ls -l /sys/kernel/btf/vmlinux 2>/dev/null
  if command -v sha256sum >/dev/null 2>&1; then sha256sum /sys/kernel/btf/vmlinux 2>/dev/null; fi
else
  echo "/sys/kernel/btf/vmlinux: unavailable"
fi
echo

SYMS="tcp_register_congestion_control tcp_unregister_congestion_control __tcp_send_ack tcp_plb_update_state tcp_plb_update_state_upon_rto tcp_plb_check_rehash"
echo "=== Target symbol presence/export evidence ==="
for sym in $SYMS; do
  echo "--- $sym ---"
  if [ -r /proc/kallsyms ]; then
    grep -w "$sym" /proc/kallsyms 2>/dev/null | sed -E 's/^[0-9a-fA-F]+[[:space:]]+/<ADDR> /'
    grep -w "__ksymtab_$sym" /proc/kallsyms 2>/dev/null | sed -E 's/^[0-9a-fA-F]+[[:space:]]+/<ADDR> /'
    grep -w "__kstrtab_$sym" /proc/kallsyms 2>/dev/null | sed -E 's/^[0-9a-fA-F]+[[:space:]]+/<ADDR> /'
    grep -w "__crc_$sym" /proc/kallsyms 2>/dev/null | sed -E 's/^[0-9a-fA-F]+[[:space:]]+/<ADDR> /'
  else
    echo "/proc/kallsyms: unreadable"
  fi
  echo "loaded-module CRC consumers:"
  found_crc=0
  for p in /sys/module/*/versions/"$sym"; do
    if [ -f "$p" ]; then
      found_crc=1
      printf '%s: ' "$p"
      cat "$p" 2>/dev/null
    fi
  done
  [ "$found_crc" -eq 1 ] || echo "(none)"
done
echo

echo "=== Loaded modules (TCP/BBR/network subset) ==="
if [ -r /proc/modules ]; then
  grep -Ei '(^|_)(tcp|bbr|net|oplus).*' /proc/modules 2>/dev/null || true
else
  echo "/proc/modules: unreadable"
fi
echo

echo "=== DLKM module directories ==="
for d in /vendor_dlkm/lib/modules /system_dlkm/lib/modules /odm_dlkm/lib/modules /vendor/lib/modules /system/lib/modules; do
  if [ -d "$d" ]; then
    echo "--- $d ---"
    find "$d" -maxdepth 2 -type f -name '*.ko' 2>/dev/null | sort
  fi
done
echo

MODROOT="/data/adb/modules/tcp_optimiser"
echo "=== tcp_optimiser installed bundle ==="
if [ -d "$MODROOT" ]; then
  echo "module_dir: $MODROOT"
  [ -f "$MODROOT/module.prop" ] && cat "$MODROOT/module.prop"
  echo
  if [ -f "$MODROOT/kernel_modules/manifest.json" ]; then
    echo "--- manifest.json ---"
    cat "$MODROOT/kernel_modules/manifest.json"
    cp "$MODROOT/kernel_modules/manifest.json" "$WORK/manifest.json" 2>/dev/null
  else
    echo "manifest.json: absent"
  fi
  echo
  echo "--- bundled KO files ---"
  find "$MODROOT/kernel_modules" -type f -name '*.ko' 2>/dev/null | sort
else
  echo "tcp_optimiser module directory not found"
fi
echo

KO="$(find "$MODROOT/kernel_modules" -type f -name 'tcp_bbr3.ko' 2>/dev/null | head -n 1)"
echo "=== Existing tcp_bbr3.ko metadata ==="
if [ -n "$KO" ] && [ -f "$KO" ]; then
  echo "ko: $KO"
  if command -v modinfo >/dev/null 2>&1; then
    modinfo "$KO" 2>&1
  else
    echo "modinfo: unavailable"
    strings "$KO" 2>/dev/null | grep -E '^(vermagic=|name=|depends=|srcversion=|description=)' || true
  fi
  echo
  echo "--- module imported symbol versions (best effort) ---"
  if command -v modprobe >/dev/null 2>&1; then
    modprobe --show-modversions "$KO" 2>&1 || modprobe --dump-modversions "$KO" 2>&1 || true
  else
    echo "modprobe: unavailable"
  fi
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$KO" 2>/dev/null; fi
else
  echo "tcp_bbr3.ko: not found"
fi
echo

echo "=== Recent module loader errors ==="
dmesg 2>/dev/null | grep -Ei 'tcp_bbr3|unknown symbol|module.*version|vermagic|disagrees about version|invalid module|module verification' | tail -n 160 || true
echo
echo "=== Probe summary ==="
echo "Read-only: no modules were loaded/unloaded and no TCP settings were changed."

exec >/dev/null 2>&1
if command -v tar >/dev/null 2>&1; then
  tar -czf "$TAR" -C "$WORK" . 2>/dev/null
else
  TAR=""
fi

echo "log: $LOG"
if [ -n "$TAR" ] && [ -f "$TAR" ]; then
  echo "archive: $TAR"
else
  cp "$LOG" "$OUTDIR/O13_KProbe_$STAMP.txt" 2>/dev/null
  echo "archive unavailable; text: $OUTDIR/O13_KProbe_$STAMP.txt"
fi
