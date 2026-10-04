const now = () => Math.floor(Date.now() / 1000);

const runtime = () => ({
	generated_epoch: now(),
	module_active: true,
	active_iface: 'wlan0',
	algorithm: 'bbr',
	default_qdisc: 'fq_codel',
	available_algorithms: ['bbr','bbr3','cubic','reno'],
	native_algorithms: ['bbr','cubic','reno'],
	bundled_algorithms: ['bbr3'],
	bundled_qdiscs: ['cake','pie','fq_pie'],
	auto_tuning_enabled: true,
	qdisc_policy: 'per_algorithm',
	kernel_bundle: {
		kernel_release: '6.6.147-android15-8-preview',
		kmi: '6.6-android15-8',
		matching_mode: 'kmi',
		matched_modules: 4,
	},
	adaptive: {
		stable_state: 'stable',
		latest: { state: 'stable', confidence: 91, sample: { avg_rtt_ms: 31.4 } },
	},
});

const detailed = () => ({
	...runtime(),
	proxy_state: { family: 'mihomo', label: 'Mihomo', mode: 'tproxy' },
	dns: [{ iface: 'wlan0', ip: '1.1.1.1' }, { iface: 'system', ip: '8.8.8.8' }],
	conn_info: { avg_rtt_ms: 31.4, max_rtt_ms: 48.2, avg_cwnd: 18.2, max_cwnd: 32, samples: 7 },
	verification: {
		summary: { matched: 5, total: 5, drifted: 0, unavailable: 0 },
		checks: [
			{ key:'congestion_algorithm', state:'match', actual:'bbr', expected:'bbr' },
			{ key:'default_qdisc', state:'match', actual:'fq_codel', expected:'fq_codel' },
		],
	},
});

const stats = details => ({
	generated_epoch: now(),
	active_iface: 'wlan0',
	tcp: { retrans: 1234, in_segs: 15234567, out_segs: 12345678 },
	iface: { rx_bytes: 1234567890, tx_bytes: 987654321 },
	sock: { tcp_in_use: 89, tcp_orphan: 2, tcp_tw: 12, tcp_alloc: 256, tcp_mem: 5 },
	established: 7,
	...(details ? {
		dns: [{ iface:'wlan0', ip:'1.1.1.1' }],
		conn_info: { avg_rtt_ms:31.4, max_rtt_ms:48.2, avg_cwnd:18.2, max_cwnd:32, samples:7 },
	} : {}),
});

const advanced = {
	tcp_keepalive_time:7200,
	tcp_keepalive_intvl:75,
	tcp_keepalive_probes:9,
	tcp_fin_timeout:60,
	tcp_syn_retries:6,
	tcp_synack_retries:5,
	tcp_retries2:15,
	rmem_max:16777216,
	wmem_max:16777216,
	optmem_max:20480,
	tcp_notsent_lowat:4294967295,
	somaxconn:4096,
	netdev_max_backlog:1000,
	tcp_max_syn_backlog:4096,
	netdev_budget:300,
	netdev_budget_usecs:2000,
	tcp_mtu_probing:1,
	tcp_sack:1,
	tcp_dsack:1,
	tcp_ecn:1,
	tcp_no_metrics_save:0,
	tcp_slow_start_after_idle:1,
	tcp_fastopen:3,
	tcp_tw_reuse:1,
	tcp_autocorking:1,
	tcp_early_retrans:3,
	tcp_thin_linear_timeouts:0,
	tcp_thin_dupack:0,
	tcp_rto_max_ms:120000,
	tcp_plb_enabled:0,
	tcp_plb_idle_rehash_rounds:3,
	tcp_plb_rehash_rounds:12,
	tcp_plb_suspend_rto_sec:60,
	tcp_plb_cong_thresh:128,
	busy_poll:0,
	busy_read:0,
	nf_conntrack_max:65536,
	nf_conntrack_tcp_timeout_established:432000,
	nf_conntrack_tcp_timeout_time_wait:120,
};

export function mockExec(command) {
	if (command.includes('advanced-sysctl-probe')) {
		return { errno:0, stdout:Object.entries(advanced).map(([k,v]) => `${k}=${v}`).join('\n'), stderr:'' };
	}
	if (command.includes('advanced-sysctl-apply')) {
		const pairs = [...command.matchAll(/printf '([a-z0-9_]+)=%s\\n'/g)].map(match => match[1]);
		return { errno:0, stdout:pairs.map(key => `${key}=${advanced[key] ?? 0}`).join('\n'), stderr:'' };
	}
	if (command.includes('v41-settings-read')) {
		return { errno:0, stdout:'wifi=bbr\ncell=bbr\nqdisc=\nkill=0\ninit=0\nproxykill=0\nauto=1\n', stderr:'' };
	}
	if (command.includes('v41-settings-apply') || command.includes('v41-profile-refresh')) {
		return { errno:0, stdout:JSON.stringify({ ok:true, mode:'fast', elapsed_ms:8 }), stderr:'' };
	}
	if (command.includes('qdisc-capability-probe')) {
		return { errno:0, stdout:'fq=supported\nfq_codel=supported\ncodel=supported\ncake=supported\npie=supported\nfq_pie=supported\n', stderr:'' };
	}
	if (command.includes('runtime-stats-sample')) {
		return { errno:0, stdout:JSON.stringify(stats(command.includes('--details'))), stderr:'' };
	}
	if (command.includes('runtime-details-snapshot')) {
		return { errno:0, stdout:JSON.stringify(detailed()), stderr:'' };
	}
	if (command.includes('runtime-status-snapshot')) {
		return { errno:0, stdout:JSON.stringify(command.includes('--verify') ? detailed() : runtime()), stderr:'' };
	}
	if (command.includes('module-integrity-check')) return { errno:0, stdout:'', stderr:'' };
	if (command.includes('tail -n 160')) return { errno:0, stdout:'Preview log\nTCP Optimiser v4.1 WebUI ready\n', stderr:'' };
	if (command.includes('rm -f') && command.includes('service.log')) return { errno:0, stdout:'', stderr:'' };
	return { errno:0, stdout:'', stderr:'' };
}
