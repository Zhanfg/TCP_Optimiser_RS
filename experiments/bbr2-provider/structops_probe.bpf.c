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
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>

char LICENSE[] SEC("license") = "GPL";

extern __u32 tcp_reno_ssthresh(struct sock *sk) __ksym;
extern void tcp_reno_cong_avoid(struct sock *sk, __u32 ack, __u32 acked) __ksym;
extern __u32 tcp_reno_undo_cwnd(struct sock *sk) __ksym;

SEC("struct_ops")
__u32 BPF_PROG(tcpopt_probe_ssthresh, struct sock *sk)
{
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
    .ssthresh = (void *)tcpopt_probe_ssthresh,
    .cong_avoid = (void *)tcpopt_probe_cong_avoid,
    .undo_cwnd = (void *)tcpopt_probe_undo_cwnd,
    .name = "tcpopt_probe",
};
