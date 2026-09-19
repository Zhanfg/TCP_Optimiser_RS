// SPDX-License-Identifier: GPL-2.0
#ifndef TCPOPT_BBR2_STATE_LAYOUT_H
#define TCPOPT_BBR2_STATE_LAYOUT_H

#include "vmlinux.h"

/*
 * Experimental BBRv2 state split for stock GKI.
 *
 * Stock Android GKI commonly provides 104 bytes in icsk_ca_priv, while
 * Google's v2alpha kernel expands ICSK_CA_PRIV_SIZE to 224 bytes. Keep the
 * per-ACK working set in the native CA private area and move cold recovery /
 * ECN bookkeeping to BPF sk_storage.
 *
 * This header defines layout only. It is not yet a BBRv2 implementation.
 */
struct tcpopt_bbr2_hot_state {
    /*
     * Put the two 64-bit clocks first so the 104-byte ABI does not grow from
     * alignment padding when compiled for BPF/64-bit Android kernels.
     */
    __u64 cycle_mstamp;
    __u64 ack_epoch_mstamp;

    __u32 min_rtt_us;
    __u32 min_rtt_stamp;
    __u32 probe_rtt_done_stamp;
    __u32 probe_rtt_min_us;
    __u32 probe_rtt_min_stamp;
    __u32 next_rtt_delivered;

    /* Original BBR bitfields packed explicitly for stable BPF layout. */
    __u32 mode_flags;
    __u32 gain_flags;
    __u32 full_bw;
    __u32 ack_epoch_flags;

    __u32 bw_latest;
    __u32 bw_lo;
    __u32 bw_hi[2];
    __u32 inflight_latest;
    __u32 inflight_lo;
    __u32 inflight_hi;

    __u32 bw_probe_up_cnt;
    __u32 bw_probe_up_acks;
    __u32 ecn_flags;
    __u32 loss_round_delivered;

    __u16 extra_acked[2];
}

struct tcpopt_bbr2_cold_state {
    __u32 prior_rcv_nxt;
    __u32 prior_cwnd;
    __u32 undo_bw_lo;
    __u32 undo_inflight_lo;
    __u32 undo_inflight_hi;
    __u32 probe_wait_us;
    __u32 alpha_last_delivered;
    __u32 alpha_last_delivered_ce;

    /* Reserved for optional PLB state without perturbing the hot layout. */
    __u32 plb_flags;
    __u32 plb_pause_until;
};

_Static_assert(sizeof(struct tcpopt_bbr2_hot_state) == 104,
               "BBRv2 hot state must fit stock GKI icsk_ca_priv");
_Static_assert(sizeof(struct tcpopt_bbr2_cold_state) == 40,
               "unexpected BBRv2 cold-state layout");

#endif
