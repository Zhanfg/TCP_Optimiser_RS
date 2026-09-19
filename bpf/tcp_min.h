/* SPDX-License-Identifier: GPL-2.0 */
#ifndef TCP_OPT_BPF_TCP_MIN_H
#define TCP_OPT_BPF_TCP_MIN_H

#include <linux/types.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_core_read.h>
#include <bpf/bpf_tracing.h>

#define BPF_STRUCT_OPS(name, args...) \
SEC("struct_ops/"#name) \
BPF_PROG(name, args)

#define TCP_CA_NAME_MAX 16

struct sock {
    unsigned char _opaque;
} __attribute__((preserve_access_index));

struct tcp_sock {
    struct {
        struct {
            struct sock sk;
        } icsk_inet;
    } inet_conn;
    __u32 snd_cwnd;
    __u32 snd_cwnd_cnt;
    __u32 snd_cwnd_clamp;
    __u32 snd_ssthresh;
} __attribute__((preserve_access_index));

static __always_inline struct tcp_sock *tcp_sk(const struct sock *sk)
{
    return (struct tcp_sock *)sk;
}

struct ack_sample {
    __u32 pkts_acked;
    __s32 rtt_us;
    __u32 in_flight;
} __attribute__((preserve_access_index));

struct rate_sample {
    __u64 prior_mstamp;
    __u32 prior_delivered;
    __s32 delivered;
    long interval_us;
    __u32 snd_interval_us;
    __u32 rcv_interval_us;
    long rtt_us;
    int losses;
    __u32 acked_sacked;
    __u32 prior_in_flight;
    _Bool is_app_limited;
    _Bool is_retrans;
    _Bool is_ack_delayed;
} __attribute__((preserve_access_index));

struct tcp_congestion_ops {
    char name[TCP_CA_NAME_MAX];
    __u32 flags;
    void (*init)(struct sock *sk);
    void (*release)(struct sock *sk);
    __u32 (*ssthresh)(struct sock *sk);
    void (*cong_avoid)(struct sock *sk, __u32 ack, __u32 acked);
    void (*set_state)(struct sock *sk, __u8 new_state);
    void (*cwnd_event)(struct sock *sk, int ev);
    void (*in_ack_event)(struct sock *sk, __u32 flags);
    __u32 (*undo_cwnd)(struct sock *sk);
    void (*pkts_acked)(struct sock *sk, const struct ack_sample *sample);
    __u32 (*min_tso_segs)(struct sock *sk);
    __u32 (*sndbuf_expand)(struct sock *sk);
    void (*cong_control)(struct sock *sk, const struct rate_sample *rs);
    void *owner;
} __attribute__((preserve_access_index));

#endif
