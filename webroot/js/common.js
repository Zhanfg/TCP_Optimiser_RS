import { exec, toast, moduleInfo } from './kernelsu.js';
import I18N from './i18n.js';
import router_state from './router.js';
import { addLog } from './logs.js';

async function readModuleProp() {
	try {
		const { stdout: details } = await exec(`cat /data/adb/modules/tcp_optimiser/module.prop`);
		const lines = details.trim().split('\n').filter(line => line);
		let moduleInfo = lines.reduce((acc, line) => {
			const [key, ...rest] = line.split('=');
			acc[key.trim()] = rest.join('=').trim();
			return acc;
		}, {});
		moduleInfo["moduleDir"] = `/data/adb/modules/${moduleInfo.id}`;
		return moduleInfo;
	} catch (error) {
		console.error('Error reading module.prop:', error);
		return null;
	}
}

export async function updateModuleInformation() {
	try {
		const raw = moduleInfo();
		router_state.moduleInformation = typeof raw === 'string' ? JSON.parse(raw) : (raw || null);
		if (!router_state.moduleInformation || Object.keys(router_state.moduleInformation).length === 0) {
			router_state.moduleInformation = await readModuleProp();
		}
	} catch (error) {
		console.error('Error updating module info:', error);
		toast(I18N.t('toast_fetch_module_fail'));
	}
	if (!router_state.moduleInformation) return;
	var versionStr = router_state.moduleInformation.version ? 'v' + router_state.moduleInformation.version : '';
	var versionCodeStr = router_state.moduleInformation.versionCode ? router_state.moduleInformation.versionCode : '';
	var finalVersionStr = versionStr && versionCodeStr ? `${versionStr} (${versionCodeStr})` : "Loading...";
	['version-value', 'version-badge', 'about-version-text'].forEach(id => {
		const el = document.getElementById(id);
		if (el) el.textContent = finalVersionStr;
	});

	// Live description from module.prop (updated by service.sh)
	var descEl = document.getElementById('about-live-desc');
	if (descEl) {
		descEl.textContent = router_state.moduleInformation.description || '';
	}
}

export async function getModuleActiveState() {
	try {
		const { stdout: file_exists } = await exec(`ls "/dev/.tcp_module_log_cleared"`);
		return file_exists != "";
	} catch (error) {
		console.error('Error updating module state:', error);
		return false;
	}
}

export async function get_active_iface() {
	try {
		const { stdout: active_iface } = await exec(`ip route get 192.0.2.1 2>/dev/null | awk '/dev/ {for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}'`);
		return active_iface.trim();
	} catch (error) {
		console.error('Error fetching active interface:', error);
		addLog('Error fetching active interface.');
		return "error";
	}
}

export async function get_active_algorithm() {
	try {
		const { stdout: active_algo } = await exec(`cat /proc/sys/net/ipv4/tcp_congestion_control`);
		return active_algo.trim();
	} catch (error) {
		console.error('Error fetching active algorithm:', error);
		addLog('Error fetching active algorithm.');
		return "error";
	}
}

export async function getInitcwndInitrwndValue() {
	try {
		const { stdout: initcwndInitrwndValueOutput } = await exec(`ip route show | grep -o 'initcwnd [0-9]* initrwnd [0-9]*'`);
		const initcwndInitrwndValues = initcwndInitrwndValueOutput.trim().split(/\s+/).filter((_, i) => i % 2 === 1);
		return initcwndInitrwndValues;
	} catch (error) {
		console.error('Error fetching initcwnd/initrwnd:', error);
		return [];
	}
}

export async function get_wifi_calling_state() {
	const DUMPSYS_TMP_FILE = `${router_state.moduleInformation.moduleDir}/dumpsys.tmp`;
	try {
		await exec(`dumpsys activity service SystemUIService > "${DUMPSYS_TMP_FILE}" 2>/dev/null`);
		const { stdout: returnCode } = await exec(`grep -qE "slot='vowifi'.*visible user=.*" "${DUMPSYS_TMP_FILE}" && echo $?`);
		await exec(`rm -f "${DUMPSYS_TMP_FILE}"`);
		return returnCode.trim() === '0';
	} catch (error) {
		console.error('Error checking VoWiFi state:', error);
		return false;
	}
}

export async function fetchIsConfigFile(file_name) {
	try {
		const { stdout: output } = await exec(`[ -f "${router_state.moduleInformation.moduleDir}/${file_name}" ] && echo "exist" || echo ""`);
		return output == "exist";
	} catch (error) {
		console.error('Error fetching config file status:', error);
		return false;
	}
}

export async function getDefaultQdisc() {
	try {
		const { stdout: qdisc } = await exec('cat /proc/sys/net/core/default_qdisc');
		return qdisc.trim();
	} catch (error) {
		console.error('Error reading default_qdisc:', error);
		return 'unknown';
	}
}

export async function getProxyStatus() {
	try {
		// Check system VPN first
		const { stdout: vpnOut } = await exec("ip link show 2>/dev/null | grep -c 'tun[0-9]'");
		const vpnCount = parseInt(vpnOut.trim()) || 0;

		const { stdout } = await exec("ps -A -o comm= 2>/dev/null");
		const procs = stdout.toLowerCase();

		const families = [
			{ name: 'clash', re: /clash|mihomo/ },
			{ name: 'surfing', re: /surfing/ },
			{ name: 'v2ray', re: /v2ray|xray/ },
			{ name: 'sing-box', re: /sing-box/ },
			{ name: 'shadowsocks', re: /ss-local|ss-redir|shadowsocks/ },
			{ name: 'other', re: /nekobox|nekoray|hiddify|trojan|naive|hysteria|tuic/ },
		];
		const hits = families.filter(f => f.re.test(procs));

		if (vpnCount > 0 && hits.length === 0) return 'vpn';
		if (hits.length === 0) return 'none';
		if (hits.length > 1) return 'multiple';
		return hits[0].name;
	} catch (error) {
		console.error('Error detecting proxy:', error);
		return 'unknown';
	}
}

export async function getHostsStatus() {
	try {
		const cmd = `hs=/etc/hosts; sz=0; blk=0; [ -f "$hs" ] && sz=$(wc -c < "$hs" 2>/dev/null) && blk=$(grep -cE '^\\s*(0\\\\.0\\\\.0\\\\.0|127\\\\.0\\\\.0\\\\.1)\\\\s+' "$hs" 2>/dev/null); [ -z "$blk" ] && blk=0; [ -d /data/adb/modules/hosts ] && echo "systemless" || [ -n "$(ps -A -o comm= 2>/dev/null | grep -iE 'birdhost')" ] && echo "birdhost" || [ -n "$(ps -A -o comm= 2>/dev/null | grep -iE 'adaway')" ] && echo "adaway" || [ -n "$(ps -A -o comm= 2>/dev/null | grep -iE 'blokada|dns66|netguard')" ] && echo "blocker" || [ "$sz" -gt 200 ] && [ "$blk" -gt 5 ] && echo "blocked:$blk" || [ "$sz" -gt 200 ] && echo "modified" || echo "none"`;
		const { stdout: result } = await exec(cmd);
		return result.trim();
	} catch (error) {
		console.error('Error checking hosts:', error);
		return 'unknown';
	}
}

export function getKnownQdiscs() {
	return ['fq', 'fq_codel', 'cake', 'pfifo_fast', 'codel', 'fq_pie', 'pfifo'];
}

export async function setDefaultQdisc(qdisc) {
	try {
		await exec(`echo "${qdisc}" > /proc/sys/net/core/default_qdisc`);
		return true;
	} catch (error) {
		console.error('Error setting default_qdisc:', error);
		return false;
	}
}

export function formatLocalDateTime(date = new Date()) {
	const pad = (n) => n.toString().padStart(2, '0');
	const yyyy = date.getFullYear();
	const mm = pad(date.getMonth() + 1);
	const dd = pad(date.getDate());
	const hh = pad(date.getHours());
	const min = pad(date.getMinutes());
	const ss = pad(date.getSeconds());
	return `${yyyy}-${mm}-${dd} ${hh}:${min}:${ss}`;
}

// ---- Network Stats ----

export async function getTCPStatCounters() {
	const cmd = `awk '/^Tcp:/{for(i=1;i<=NF;i++){if($i=="RetransSegs")r=i;if($i=="InSegs")ri=i;if($i=="OutSegs")ro=i}if(r)print "retrans:"$r;if(ri)print "in:"$ri;if(ro)print "out:"$ro}' /proc/net/snmp`;
	try {
		const { stdout } = await exec(cmd);
		const out = {};
		stdout.split('\n').forEach(line => {
			if (line.startsWith('retrans:')) out.retrans = parseInt(line.split(':')[1]) || 0;
			if (line.startsWith('in:')) out.inSegs = parseInt(line.split(':')[1]) || 0;
			if (line.startsWith('out:')) out.outSegs = parseInt(line.split(':')[1]) || 0;
		});
		return out;
	} catch (e) { return { retrans: 0, inSegs: 0, outSegs: 0 }; }
}

export async function getIfaceBytes(iface) {
	try {
		const { stdout } = await exec(`awk -v if="${iface}" '$1==if":"{print "rx:"$2"\\ntx:"$10}' /proc/net/dev`);
		const out = {};
		stdout.split('\n').forEach(line => {
			if (line.startsWith('rx:')) out.rxBytes = parseInt(line.split(':')[1]) || 0;
			if (line.startsWith('tx:')) out.txBytes = parseInt(line.split(':')[1]) || 0;
		});
		return out;
	} catch (e) { return { rxBytes: 0, txBytes: 0 }; }
}

export async function getSockStat() {
	try {
		const { stdout } = await exec("awk '/^TCP:/{print $3}' /proc/net/sockstat 2>/dev/null");
		const parts = stdout.trim().split(/\s+/);
		return {
			tcpInUse: parseInt(parts[0]) || 0,
			tcpOrphan: parseInt(parts[1]) || 0,
			tcpTW: parseInt(parts[2]) || 0,
			tcpAlloc: parseInt(parts[3]) || 0,
			tcpMem: parseInt(parts[4]) || 0,
		};
	} catch (e) { return { tcpInUse: 0, tcpOrphan: 0, tcpTW: 0, tcpAlloc: 0, tcpMem: 0 }; }
}

export async function getTCPConnsCount() {
	try {
		const { stdout } = await exec("ss -Htn state established 2>/dev/null | wc -l");
		return parseInt(stdout.trim()) || 0;
	} catch (e) { return 0; }
}

export async function getDNSServers() {
	try {
		const { stdout } = await exec("getprop | grep -E 'net\\.dns[0-9]|net\\.eth[0-9]+\\.dns' | sort");
		const servers = [];
		stdout.split('\n').forEach(line => {
			const m = line.match(/\[(.+?)\]:\s*\[(.+?)\]/);
			if (m) {
				const val = m[2];
				if (val && val !== '0.0.0.0' && val !== '::') {
					servers.push({ iface: m[1].replace('net.', '').replace('.dns', ''), ip: val });
				}
			}
		});
		if (servers.length === 0) {
			const { stdout: resolv } = await exec("cat /etc/resolv.conf 2>/dev/null | grep '^nameserver' | awk '{print $2}'");
			resolv.split('\n').filter(Boolean).forEach(ip => {
				servers.push({ iface: 'system', ip: ip.trim() });
			});
		}
		return servers;
	} catch (e) { return []; }
}

export async function getSSInfo() {
	try {
		const { stdout } = await exec("ss -tino 2>/dev/null | grep -v 'LISTEN\\|CLOSE-WAIT\\|TIME-WAIT' | head -20");
		const lines = stdout.trim().split('\n').filter(Boolean);
		const out = [];
		lines.forEach(line => {
			const cwnd = (line.match(/cwnd:(\d+)/) || [])[1];
			const rtt = (line.match(/rtt:([\d.]+)/) || [])[1];
			if (cwnd || rtt) out.push({
				cwnd: cwnd ? parseInt(cwnd) : 0,
				rtt: rtt ? parseFloat(rtt) : 0,
			});
		});
		if (out.length === 0) return null;
		const avgRTT = out.reduce((s, v) => s + v.rtt, 0) / out.length;
		const avgCWND = out.reduce((s, v) => s + v.cwnd, 0) / out.length;
		return { samples: out.length, avgRTT: Math.round(avgRTT * 100) / 100, avgCWND: Math.round(avgCWND), maxRTT: Math.max(...out.map(v => v.rtt)), maxCWND: Math.max(...out.map(v => v.cwnd)) };
	} catch (e) { return null; }
}
