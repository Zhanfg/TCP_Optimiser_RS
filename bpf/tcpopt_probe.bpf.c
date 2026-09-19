// SPDX-License-Identifier: GPL-2.0
#include "tcp_min.h"

char _license[] SEC("license") = "GPL";

extern __u32 tcp_slow_start(struct tcp_sock *tp, __u32 acked) __ksym;
extern void tcp_cong_avoid_ai(struct tcp_sock *tp, __u32 w, __u32 acked) __ksym;

SEC("struct_ops/tcpopt_probe_ssthresh")
__u32 BPF_PROG(tcpopt_probe_ssthresh, struct sock *sk)
{
    struct tcp_sock *tp = tcp_sk(sk);
    return tp->snd_cwnd > 2 ? tp->snd_cwnd / 2 : 2;
}

SEC("struct_ops/tcpopt_probe_cong_avoid")
void BPF_PROG(tcpopt_probe_cong_avoid, struct sock *sk, __u32 ack, __u32 acked)
{
    struct tcp_sock *tp = tcp_sk(sk);

    if (tp->snd_cwnd < tp->snd_ssthresh)
        acked = tcp_slow_start(tp, acked);
    if (acked)
        tcp_cong_avoid_ai(tp, tp->snd_cwnd, acked);
}

SEC("struct_ops/tcpopt_probe_undo_cwnd")
__u32 BPF_PROG(tcpopt_probe_undo_cwnd, struct sock *sk)
{
    return tcp_sk(sk)->snd_cwnd;
}

SEC(".struct_ops")
struct tcp_congestion_ops tcpopt_probe = {
    .ssthresh = (void *)tcpopt_probe_ssthresh,
    .cong_avoid = (void *)tcpopt_probe_cong_avoid,
    .undo_cwnd = (void *)tcpopt_probe_undo_cwnd,
    .name = "tcpopt_probe",
};
