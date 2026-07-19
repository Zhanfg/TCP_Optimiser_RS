// Browser preview fallback. Never replace the real KernelSU WebView bridge.
const mockKsu = {
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
			version: '3.0.0',
			versionCode: '30',
			author: 'fatalcoder524 & axymorrsen',
			description: 'TCP Optimisations & update tcp_cong_algo based on interface',
		});
	},
};

const previewRequested = new URLSearchParams(window.location.search).get('preview') === '1';
const previewHost = ['localhost', '127.0.0.1', '[::1]'].includes(window.location.hostname);
const previewAllowed = previewRequested || (['http:', 'https:'].includes(window.location.protocol) && previewHost);

const unavailableKsu = {
	exec(command, options, callbackName) {
		const fn = window[callbackName];
		if (fn) fn(127, '', 'KernelSU WebUI bridge is unavailable');
	},
	spawn(command, args, options, callbackName) {
		const child = window[callbackName];
		if (child) setTimeout(() => child.emit('exit', 127), 0);
	},
	fullScreen() {},
	toast(message) { console.warn('[toast]', message); },
	moduleInfo() { return null; },
};

// Mock device values are only legal in an explicit/local browser preview.
// Production must report unavailable instead of presenting preset data.
const bridge = window.ksu || (previewAllowed ? mockKsu : unavailableKsu);

function mockExec(cmd) {
	// Simulate realistic return values for preview
	if (cmd.includes('tcp_available_congestion_control'))
		return { errno: 0, stdout: 'bbr bbr2 cubic westwood reno htcp vegas yeah illinois dctcp cdg bic highspeed hybla nv scalable lp', stderr: '' };
	if (cmd.includes('tcp_congestion_control'))
		return { errno: 0, stdout: 'bbr', stderr: '' };
	if (cmd.includes('qdisc-capability-probe'))
		return { errno: 0, stdout: 'fq:supported\nfq_codel:supported\ncake:unsupported\npfifo_fast:unsupported\ncodel:supported\nfq_pie:unsupported\npfifo:supported\n', stderr: '' };
	if (cmd.includes('dynamic-color-palette-probe'))
		return { errno: 0, stdout: [
			'system_accent1_0=#ffffffff', 'system_accent1_100=#ff9ff2e7',
			'system_accent1_200=#ff83d5cb', 'system_accent1_600=#ff006a61',
			'system_accent1_700=#ff005049', 'system_accent1_800=#ff003732',
			'system_accent1_900=#ff00201d', 'system_accent2_100=#ffb5ccc7',
			'system_accent2_700=#ff354b47', 'system_accent2_900=#ff0b1f1c',
			'system_accent3_100=#ffb0cbd8', 'system_accent3_200=#ff95afbc',
			'system_accent3_600=#ff46636f', 'system_accent3_700=#ff2f4b57',
			'system_accent3_900=#ff071e28', 'system_neutral1_10=#fff4fbf8',
			'system_neutral1_50=#ffeef5f2', 'system_neutral1_100=#ffdde4e1',
			'system_neutral1_800=#ff2d312f', 'system_neutral1_900=#ff191c1b',
			'system_neutral2_100=#ffd9e5e1', 'system_neutral2_200=#ffbdc9c5',
			'system_neutral2_400=#ff87938f', 'system_neutral2_500=#ff6d7975',
			'system_neutral2_700=#ff3d4945', 'system_neutral2_800=#ff27332f',
			'system_neutral2_900=#ff121e1b',
		].join('\n'), stderr: '' };
	if (cmd.includes('default_qdisc'))
		return { errno: 0, stdout: 'fq_codel', stderr: '' };
	if (cmd.includes('active-interface-probe'))
		return { errno: 0, stdout: 'wlan0', stderr: '' };
	if (cmd.includes('proxy-status-probe'))
		return { errno: 0, stdout: 'status=mihomo_tproxy\ncore=Mihomo\nversion=Mihomo Meta v1.19.10 android arm64\npackage=\napp=Box for Root\nmanager_type=module\nmanager_id=box_for_root\nmode=TPROXY', stderr: '' };
	if (cmd.includes('advanced-sysctl-probe')) {
		const values = {
			tcp_keepalive_time: 7200, tcp_keepalive_intvl: 75, tcp_keepalive_probes: 9,
			tcp_fin_timeout: 60, tcp_syn_retries: 6, tcp_synack_retries: 5, tcp_retries2: 15,
			rmem_max: 16777216, wmem_max: 16777216, optmem_max: 20480, tcp_notsent_lowat: 4294967295,
			somaxconn: 4096, netdev_max_backlog: 1000, tcp_max_syn_backlog: 4096,
			netdev_budget: 300, netdev_budget_usecs: 2000, tcp_mtu_probing: 1,
			tcp_sack: 1, tcp_dsack: 1, tcp_ecn: 1, tcp_no_metrics_save: 0,
			tcp_slow_start_after_idle: 1, tcp_fastopen: 3, tcp_tw_reuse: 1,
			busy_poll: 0, busy_read: 0, nf_conntrack_max: 65536,
			nf_conntrack_tcp_timeout_established: 432000, nf_conntrack_tcp_timeout_time_wait: 120,
		};
		return { errno: 0, stdout: Object.entries(values).map(([key, value]) => `${key}=${value}`).join('\n'), stderr: '' };
	}
	if (cmd.includes('advanced-sysctl-apply')) {
		const verified = [...cmd.matchAll(/printf '%s\\n' (\d+) > '\/proc\/sys\/[^']+\/([a-z0-9_]+)'/g)]
			.map(match => `${match[2]}=${match[1]}`).join('\n');
		return { errno: 0, stdout: verified, stderr: '' };
	}
	if (cmd.includes('/dev/.tcp_module_log_cleared'))
		return { errno: 0, stdout: '1', stderr: '' };
	if (cmd.includes('daemon.pid'))
		return { errno: 0, stdout: '1', stderr: '' };
	if (cmd.includes('dumpsys') && cmd.includes('vowifi'))
		return { errno: 0, stdout: '0', stderr: '' };
	if (cmd.includes('hs=/etc/hosts'))
		return { errno: 0, stdout: 'none', stderr: '' };
	if (cmd.includes('ps -A') && cmd.includes('comm='))
		return { errno: 0, stdout: 'system_server\ncom.android.phone\nclash.meta\nsshd\n', stderr: '' };
	if (cmd.includes('ip link show') && cmd.includes('tun'))
		return { errno: 0, stdout: '0', stderr: '' };
	if (cmd.includes('/proc/net/snmp'))
		return { errno: 0, stdout: 'retrans:1234\nin:15234567\nout:12345678', stderr: '' };
	if (cmd.includes('/proc/net/dev') && cmd.includes('awk'))
		return { errno: 0, stdout: 'rx:1234567890\ntx:987654321', stderr: '' };
	if (cmd.includes('/proc/net/sockstat'))
		return { errno: 0, stdout: 'TCP: inuse 89 orphan 2 tw 12 alloc 256 mem 5', stderr: '' };
	if (cmd.includes('/proc/net/tcp') && cmd.includes('$4=="01"'))
		return { errno: 0, stdout: '7', stderr: '' };
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
	if (cmd.includes('stat -c %s'))
		return { errno: 0, stdout: '125829120', stderr: '' };
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
	if (cmd.includes('dumpsys telephony') && cmd.includes('mImsRegistered'))
		return { errno: 0, stdout: 'mImsRegistered=false', stderr: '' };
	if (cmd.includes('echo')) return { errno: 0, stdout: '', stderr: '' };
	return { errno: 0, stdout: '', stderr: '' };
}

let callbackCounter = 0;

function callbackName() {
	return `tcp_exec_${Date.now()}_${callbackCounter++}`;
}

export function shellQuote(value) {
	const text = String(value);
	if (text.includes('\0')) throw new TypeError('Shell values cannot contain NUL bytes');
	return `'${text.replace(/'/g, `'"'"'`)}'`;
}

export function exec(command, options = {}) {
	const cmdShort = command.length > 80 ? command.substring(0, 80) + '…' : command;
	if (bridge === mockKsu) {
		const result = mockExec(command);
		window._debug?.log('exec', cmdShort, `errno=${result.errno} stdout=${(result.stdout || '').substring(0, 60)}`);
		return result.errno === 0
			? Promise.resolve(result)
			: Promise.reject(new Error(result.stderr || `Command failed with errno ${result.errno}`));
	}

	return new Promise((resolve, reject) => {
		const name = callbackName();
		const timer = setTimeout(() => {
			cleanup();
			reject(new Error('KernelSU command timed out'));
		}, 15000);
		const cleanup = () => {
			clearTimeout(timer);
			delete window[name];
		};
		window[name] = (errno, stdout, stderr) => {
			cleanup();
			window._debug?.log('exec', cmdShort, `errno=${errno} stdout=${(stdout || '').substring(0, 60)}`);
			if (errno === 0) resolve({ errno, stdout, stderr });
			else reject(new Error(stderr || `Command failed with errno ${errno}`));
		};
		try {
			bridge.exec(command, JSON.stringify(options), name);
		} catch (error) {
			cleanup();
			reject(error);
		}
	});
}

export function toast(message) {
	window._debug?.log('toast', message);
	bridge.toast(message);
}
export function moduleInfo() { return bridge.moduleInfo(); }
export function isBridgeAvailable() { return bridge !== unavailableKsu; }

window._ksuExec = exec;
window._ksuToast = toast;
