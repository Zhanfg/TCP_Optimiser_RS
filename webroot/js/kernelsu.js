// Mock ksu for browser preview
window.ksu = {
	exec(command, options, callbackName) {
		const fn = window[callbackName];
		const mockData = mockExec(command);
		if (fn) fn(mockData.errno, mockData.stdout, mockData.stderr);
	},
	spawn(command, args, options, callbackName) {
		const child = window[callbackName];
		if (child) setTimeout(() => child.emit('exit', 0), 10);
	},
	fullScreen(isFullScreen) {},
	toast(message) {
		console.log('[toast]', message);
		const el = document.createElement('div');
		el.className = 'toast show';
		el.textContent = message;
		document.body.appendChild(el);
		setTimeout(() => { el.style.transform = 'translate(-50%,120px)'; setTimeout(() => el.remove(), 400); }, 2000);
	},
	moduleInfo() {
		return JSON.stringify({
			id: 'tcp_optimiser',
			name: 'TCP Optimiser',
version: '3.0',
versionCode: '30',
			author: 'fatalcoder524 & axymorrsen',
			description: 'TCP Optimisations & update tcp_cong_algo based on interface',
		});
	},
};

function mockExec(cmd) {
	// Simulate realistic return values for preview
	if (cmd.includes('tcp_available_congestion_control'))
		return { errno: 0, stdout: 'bbr bbr2 bbr3 cubic westwood westwood_plus reno htcp vegas yeah illinois dctcp cdg bic highspeed hybla nv scalable lp', stderr: '' };
	if (cmd.includes('tcp_congestion_control'))
		return { errno: 0, stdout: 'bbr', stderr: '' };
	if (cmd.includes('default_qdisc'))
		return { errno: 0, stdout: 'fq_codel', stderr: '' };
	if (cmd.includes('ip route get') && cmd.includes('192.0.2.1'))
		return { errno: 0, stdout: 'wlan0', stderr: '' };
	if (cmd.includes('/dev/.tcp_module_log_cleared'))
		return { errno: 0, stdout: '1', stderr: '' };
	if (cmd.includes('dumpsys') && cmd.includes('vowifi'))
		return { errno: 0, stdout: '0', stderr: '' };
	if (cmd.includes('ps -A') && cmd.includes('comm='))
		return { errno: 0, stdout: 'system_server\ncom.android.phone\nclash.meta\nsshd\n', stderr: '' };
	if (cmd.includes('ip link show') && cmd.includes('tun'))
		return { errno: 0, stdout: '0', stderr: '' };
	if (cmd.includes('/proc/net/snmp'))
		return { errno: 0, stdout: `Tcp: RtoAlgorithm RtoMin RtoMax MaxConn ActiveOpens PassiveOpens AttemptFails EstabResets CurrEstab InSegs OutSegs RetransSegs InErrs OutRsts InCsumErrors\nTcp: 1 200 120000 -1 15234 8934 12 45 67 15234567 12345678 1234 0 89 0`, stderr: '' };
	if (cmd.includes('/proc/net/dev') && cmd.includes('awk'))
		return { errno: 0, stdout: 'rx:1234567890\ntx:987654321', stderr: '' };
	if (cmd.includes('/proc/net/sockstat'))
		return { errno: 0, stdout: 'TCP: inuse 89 orphan 2 tw 12 alloc 256 mem 5', stderr: '' };
	if (cmd.includes('ss -Htn') && cmd.includes('established'))
		return { errno: 0, stdout: 'conn1\nconn2\nconn3\nconn4\nconn5\nconn6\nconn7', stderr: '' };
	if (cmd.includes('ss -tino'))
		return { errno: 0, stdout: `ESTAB 0 0 192.168.1.100:45678 1.2.3.4:443 uid:1000 ino:12345 sk:1 <->
	 ts sack cubic wscale:7,7 rto:204 rtt:12.345/6.789 ato:40 mss:1460 pmtu:1500 rcvmss:1460 advmss:1460 cwnd:10 ssthresh:7 bytes_acked:123456 bytes_received:654321 segs_out:890 segs_in:567 data_segs_out:445 data_segs_in:223 send 1.2Mbps lastsnd:1234 lastrcv:5678 lastack:5678 pacing_rate 2.4Mbps delivery_rate 1.8Mbps busy:100ms rwnd_limited:50ms(5%) sndbuf_limited:0ms(0%) retrans:0/3 dsack_dups:2 reordering:3`, stderr: '' };
	if (cmd.includes('getprop') && cmd.includes('dns'))
		return { errno: 0, stdout: '[net.dns1]: [8.8.8.8]\n[net.dns2]: [8.8.4.4]\n[net.dns3]: [1.1.1.1]', stderr: '' };
	if (cmd.includes('ls /dev/block/by-name'))
		return { errno: 0, stdout: 'modem\nmodemst1\nmodemst2\nfsg\nfsc\nboot\nsystem\nvendor\n', stderr: '' };
	if (cmd.includes('blockdev --getsize64'))
		return { errno: 0, stdout: cmd.includes('modem') ? '125829120' : '2097152', stderr: '' };
	if (cmd.includes('cat /etc/hosts'))
		return { errno: 0, stdout: '127.0.0.1 localhost\n::1 ip6-localhost\n0.0.0.0 example.com\n', stderr: '' };
	if (cmd.includes('/etc/hosts'))
		return { errno: 0, stdout: '127.0.0.1 localhost\n::1 ip6-localhost\n0.0.0.0 example.com\n', stderr: '' };
	if (cmd.includes('kill_connections') || cmd.includes('initcwnd_initrwnd'))
		return { errno: 0, stdout: 'exist', stderr: '' };
	if (cmd.includes('scaling_governor'))
		return { errno: 0, stdout: 'schedutil', stderr: '' };
	if (cmd.includes('ip route show'))
		return { errno: 0, stdout: '192.168.1.0/24 dev wlan0 proto kernel scope link src 192.168.1.100 initcwnd 10 initrwnd 78\n', stderr: '' };
	if (cmd.includes('tcp_keepalive_time'))
		return { errno: 0, stdout: '7200', stderr: '' };
	if (cmd.includes('tcp_keepalive_intvl') && !cmd.includes('probes'))
		return { errno: 0, stdout: '75', stderr: '' };
	if (cmd.includes('tcp_keepalive_probes'))
		return { errno: 0, stdout: '9', stderr: '' };
	if (cmd.includes('busy_poll'))
		return { errno: 0, stdout: '0', stderr: '' };
	if (cmd.includes('busy_read'))
		return { errno: 0, stdout: '0', stderr: '' };
	if (cmd.includes('somaxconn'))
		return { errno: 0, stdout: '4096', stderr: '' };
	if (cmd.includes('netdev_max_backlog'))
		return { errno: 0, stdout: '1000', stderr: '' };
	if (cmd.includes('tcp_mtu_probing'))
		return { errno: 0, stdout: '1', stderr: '' };
	if (cmd.includes('tcp_slow_start'))
		return { errno: 0, stdout: '1', stderr: '' };
	if (cmd.includes('tcp_fastopen'))
		return { errno: 0, stdout: '3', stderr: '' };
	if (cmd.includes('nf_conntrack_max'))
		return { errno: 0, stdout: '65536', stderr: '' };
	if (cmd.includes('dumpsys wallpaper') && cmd.includes('primaryColor'))
		return { errno: 0, stdout: 'primaryColor=#6750a4', stderr: '' };
	if (cmd.includes('dumpsys telephony') && cmd.includes('mImsRegistered'))
		return { errno: 0, stdout: 'mImsRegistered=false', stderr: '' };
	if (cmd.includes('echo')) return { errno: 0, stdout: '', stderr: '' };
	return { errno: 0, stdout: '', stderr: '' };
}

export function exec(command, options) {
	return mockExec(command);
}

export function toast(message) { ksu.toast(message); }
export function moduleInfo() { return ksu.moduleInfo(); }
