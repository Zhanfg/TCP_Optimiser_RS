// SPDX-License-Identifier: GPL-2.0
/*
 * Minimal TCP congestion-control struct_ops probe.
 *
 * This is deliberately NOT BBRv2. It delegates the three mandatory
 * congestion-control callbacks to the kernel's Reno kfuncs so we can test
 * whether a target kernel accepts a TCP struct_ops provider before spending
 * payload/engineering budget on a BBRv2-compatible implementation.
 */
#include "vmlinux.h"
#include "bbr2_state_layout.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>

char LICENSE[] SEC("license") = "GPL";

struct {
    __uint(type, BPF_MAP_TYPE_SK_STORAGE);
    __uint(map_flags, BPF_F_NO_PREALLOC);
    __type(key, int);
    __type(value, struct tcpopt_bbr2_cold_state);
} tcpopt_bbr2_cold SEC(".maps");

extern __u32 tcp_reno_ssthresh(struct sock *sk) __ksym;
extern void tcp_reno_cong_avoid(struct sock *sk, __u32 ack, __u32 acked) __ksym;
extern __u32 tcp_reno_undo_cwnd(struct sock *sk) __ksym;

#define TCPOPT_PROBE_MAGIC 0x5450434fu

struct tcpopt_probe_ca {
    __u32 magic;
    __u32 init_count;
};

static __always_inline struct tcpopt_probe_ca *tcpopt_probe_ca(struct sock *sk)
{
    struct inet_connection_sock *icsk = (struct inet_connection_sock *)sk;

    return (void *)icsk->icsk_ca_priv;
}

SEC("struct_ops")
void BPF_PROG(tcpopt_probe_init, struct sock *sk)
{
    struct tcpopt_probe_ca *ca = tcpopt_probe_ca(sk);
    struct tcpopt_bbr2_cold_state *cold;

    ca->magic = TCPOPT_PROBE_MAGIC;
    ca->init_count++;

    cold = bpf_sk_storage_get(
        &tcpopt_bbr2_cold, sk, 0, BPF_LOCAL_STORAGE_GET_F_CREATE);
    if (cold)
        cold->prior_cwnd = tcp_sk(sk)->snd_cwnd;
}

SEC("struct_ops")
__u32 BPF_PROG(tcpopt_probe_ssthresh, struct sock *sk)
{
    struct tcpopt_probe_ca *ca = tcpopt_probe_ca(sk);
    struct tcpopt_bbr2_cold_state *cold;

    if (ca->magic != TCPOPT_PROBE_MAGIC)
        ca->magic = TCPOPT_PROBE_MAGIC;

    cold = bpf_sk_storage_get(&tcpopt_bbr2_cold, sk, 0, 0);
    if (cold)
        cold->undo_inflight_hi = tcp_sk(sk)->snd_cwnd;

    return tcp_reno_ssthresh(sk);
}

SEC("struct_ops")
void BPF_PROG(tcpopt_probe_cong_avoid, struct sock *sk, __u32 ack, __u32 acked)
{
    tcp_reno_cong_avoid(sk, ack, acked);
}

SEC("struct_ops")
__u32 BPF_PROG(tcpopt_probe_undo_cwnd, struct sock *sk)
{
    return tcp_reno_undo_cwnd(sk);
}

SEC(".struct_ops")
struct tcp_congestion_ops tcpopt_probe = {
    .init = (void *)tcpopt_probe_init,
    .ssthresh = (void *)tcpopt_probe_ssthresh,
    .cong_avoid = (void *)tcpopt_probe_cong_avoid,
    .undo_cwnd = (void *)tcpopt_probe_undo_cwnd,
    .name = "tcpopt_probe",
};
