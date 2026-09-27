/* SPDX-License-Identifier: GPL-2.0 */
/*
 * OnePlus 13 / PJZ110 runtime bridge for GKI-trimmed TCP registration symbols.
 *
 * CONFIG_TRIM_UNUSED_KSYMS removes tcp_{un,}register_congestion_control from
 * the stock module export table even though the functions remain in vmlinux.
 * The privileged userspace runtime resolves their live KASLR addresses from
 * /proc/kallsyms and passes them as module parameters at insmod time.
 *
 * Keep this bridge deliberately tiny: high-frequency BBR data-path helpers
 * must never be called through raw addresses. Optional PLB/__tcp_send_ack
 * integration is disabled in the PJZ110 compatibility map instead.
 */
#pragma once

#include <linux/errno.h>
#include <linux/module.h>
#include <linux/moduleparam.h>
#include <linux/types.h>
#include <net/tcp.h>

static unsigned long bbr3_register_addr;
static unsigned long bbr3_unregister_addr;

module_param(bbr3_register_addr, ulong, 0400);
MODULE_PARM_DESC(bbr3_register_addr,
		 "live address of tcp_register_congestion_control");
module_param(bbr3_unregister_addr, ulong, 0400);
MODULE_PARM_DESC(bbr3_unregister_addr,
		 "live address of tcp_unregister_congestion_control");

typedef int (*bbr3_register_fn_t)(struct tcp_congestion_ops *);
typedef void (*bbr3_unregister_fn_t)(struct tcp_congestion_ops *);

static inline int
bbr3_register_congestion_control(struct tcp_congestion_ops *ops)
{
	bbr3_register_fn_t fn;

	if (!bbr3_register_addr || (bbr3_register_addr & 0x3))
		return -EINVAL;
	fn = (bbr3_register_fn_t)bbr3_register_addr;
	return fn(ops);
}

static inline void
bbr3_unregister_congestion_control(struct tcp_congestion_ops *ops)
{
	bbr3_unregister_fn_t fn;

	if (!bbr3_unregister_addr || (bbr3_unregister_addr & 0x3))
		return;
	fn = (bbr3_unregister_fn_t)bbr3_unregister_addr;
	fn(ops);
}
