#!/system/bin/sh

MODDIR="${0%/*}"
pkill -f "$MODDIR/bin/.*/tcp_optimiser" 2>/dev/null

[ -w /proc/sys/net/ipv4/tcp_congestion_control ] && printf '%s\n' cubic > /proc/sys/net/ipv4/tcp_congestion_control 2>/dev/null
[ -w /proc/sys/net/core/default_qdisc ] && printf '%s\n' fq_codel > /proc/sys/net/core/default_qdisc 2>/dev/null

rm -f "$MODDIR/service.log" "$MODDIR/debug.log" /dev/.tcp_module_log_cleared 2>/dev/null
