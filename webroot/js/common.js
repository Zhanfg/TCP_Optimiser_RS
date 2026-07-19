import { exec, toast, moduleInfo, shellQuote, isBridgeAvailable } from './kernelsu.js';
import I18N from './i18n.js';
import router_state from './router.js';
import { addLog } from './logs.js';
import { ALL_QDISCS } from './capabilities.js';

async function readModuleProp() {
	try {
		const { stdout: details } = await exec(`cat /data/adb/modules/tcp_optimiser/module.prop`);
		const lines = details.trim().split('\n').filter(line => line);
		let moduleInfo = lines.reduce((acc, line) => {
			const [key, ...rest] = line.split('=');
			acc[key.trim()] = rest.join('=').trim();
			return acc;
		}, {});
		moduleInfo["moduleDir"] = '/data/adb/modules/tcp_optimiser';
		return moduleInfo;
	} catch (error) {
		console.error('Error reading module.prop:', error);
		return null;
	}
}

export async function updateModuleInformation() {
	if (!isBridgeAvailable()) {
		router_state.moduleInformation = null;
		return;
	}
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
	// KernelSU's moduleInfo() exposes metadata, not a filesystem path. Keep the
	// path tied to the immutable module id instead of trusting displayed fields.
	router_state.moduleInformation.moduleDir = '/data/adb/modules/tcp_optimiser';
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
		const pidFile = `${router_state.moduleInformation.moduleDir}/daemon.pid`;
		const { stdout: flag } = await exec(`pid=$(cat ${shellQuote(pidFile)} 2>/dev/null); case "$pid" in ''|*[!0-9]*) exit 0 ;; esac; [ -r "/proc/$pid/cmdline" ] && tr '\\000' ' ' < "/proc/$pid/cmdline" | grep -q 'tcp_optimiser' && printf '1\\n'`);
		return flag.trim() === "1";
	} catch (error) {
		return false;
	}
}

export async function get_active_iface() {
	try {
		const { stdout: active_iface } = await exec(`# active-interface-probe
iface=$(ip -4 route get 1.1.1.1 2>/dev/null | awk '/dev/ {for(i=1;i<=NF;i++) if($i=="dev"){print $(i+1); exit}}')
case "$iface" in ''|lo|tun*|tap*|wg*|dummy*|ifb*)
	iface=$(ip -4 route show table main 2>/dev/null | awk '$1=="default"{for(i=1;i<=NF;i++)if($i=="dev" && $(i+1)!~/^(lo|tun|tap|wg|dummy|ifb)/){print $(i+1); exit}}')
	;;
esac
if [ -z "$iface" ]; then
	for path in /sys/class/net/*; do
		[ -r "$path/operstate" ] || continue
		name=\${path##*/}
		case "$name" in lo|tun*|tap*|wg*|dummy*|ifb*) continue ;; esac
		[ "$(cat "$path/operstate" 2>/dev/null)" = up ] && { iface=$name; break; }
	done
fi
printf '%s\\n' "$iface"`);
		return active_iface.trim() || 'unknown';
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
	try {
		const { stdout } = await exec("dumpsys telephony.registry 2>/dev/null | grep -m1 mImsRegistered");
		if (stdout.includes('true')) return true;
		if (stdout.includes('false')) return false;
		return false;
	} catch (error) {
		return false;
	}
}

export async function fetchIsConfigFile(file_name) {
	if (!/^[A-Za-z0-9_.-]{1,64}$/.test(file_name)) return false;
	try {
		const path = `${router_state.moduleInformation.moduleDir}/${file_name}`;
		const { stdout: output } = await exec(`[ -f ${shellQuote(path)} ] && echo "exist" || echo ""`);
		return output.trim() === "exist";
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

let proxyStatusCache = null;
let proxyStatusCheckedAt = 0;

export async function getProxyStatus(force = false) {
	const now = Date.now();
	if (!force && proxyStatusCache && now - proxyStatusCheckedAt < 30000) return proxyStatusCache;
	try {
		const { stdout } = await exec(`# proxy-status-probe
proc_rows=$(
	for file in /proc/[0-9]*/cmdline; do
		pid=\${file#/proc/}; pid=\${pid%/cmdline}
		[ "$pid" = "$$" ] && continue
		[ -r "$file" ] || continue
		cmdline=$(tr '\\000' ' ' < "$file" 2>/dev/null)
		exe=$(readlink -f "/proc/$pid/exe" 2>/dev/null)
		name=$(basename "$exe" 2>/dev/null)
		[ -n "$name" ] || { name=$(printf '%s' "$cmdline" | awk '{print $1}'); name=$(basename "$name" 2>/dev/null); }
		lower=$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]')
		case "$lower" in
			mihomo) family=mihomo ;;
			clash|clash.*|clash-*) family=clash ;;
			sing-box) family=sing-box ;;
			v2ray|xray) family=v2ray ;;
			ss-local|ss-redir|shadowsocks*) family=shadowsocks ;;
			nekobox|nekoray|hiddify|trojan|naive|hysteria|tuic|surfing) family=other ;;
			*) continue ;;
		esac
		printf '%s|%s|%s\\n' "$pid" "$family" "$cmdline"
	done
)
has_mihomo=0; has_clash=0; has_singbox=0; has_v2ray=0; has_ss=0; has_other=0
printf '%s\\n' "$proc_rows" | grep -q '|mihomo|' && has_mihomo=1
printf '%s\\n' "$proc_rows" | grep -q '|clash|' && has_clash=1
printf '%s\\n' "$proc_rows" | grep -q '|sing-box|' && has_singbox=1
printf '%s\\n' "$proc_rows" | grep -q '|v2ray|' && has_v2ray=1
printf '%s\\n' "$proc_rows" | grep -q '|shadowsocks|' && has_ss=1
printf '%s\\n' "$proc_rows" | grep -q '|other|' && has_other=1
count=$((has_mihomo + has_clash + has_singbox + has_v2ray + has_ss + has_other))
core_key=
core_label=
[ "$has_mihomo" -eq 1 ] && { core_key=mihomo; core_label=Mihomo; }
[ -z "$core_key" ] && [ "$has_clash" -eq 1 ] && { core_key=clash; core_label=Clash; }
[ -z "$core_key" ] && [ "$has_singbox" -eq 1 ] && { core_key=sing-box; core_label=sing-box; }
[ -z "$core_key" ] && [ "$has_v2ray" -eq 1 ] && { core_key=v2ray; core_label='V2Ray / Xray'; }
[ -z "$core_key" ] && [ "$has_ss" -eq 1 ] && { core_key=shadowsocks; core_label=Shadowsocks; }
[ -z "$core_key" ] && [ "$has_other" -eq 1 ] && { core_key=other; core_label='Other proxy'; }
core_pid=
core_cmd=
if [ "$count" -eq 1 ]; then
	core_row=$(printf '%s\\n' "$proc_rows" | awk -F'|' -v family="$core_key" '$2 == family { print; exit }')
	core_pid=$(printf '%s' "$core_row" | cut -d'|' -f1)
	core_cmd=$(printf '%s' "$core_row" | cut -d'|' -f3-)
fi
tproxy=0
rules=$({ iptables-save -t mangle 2>/dev/null || iptables -t mangle -S 2>/dev/null; ip6tables-save -t mangle 2>/dev/null; nft list ruleset 2>/dev/null; })
printf '%s\\n' "$rules" | grep -qiE 'TPROXY|--tproxy-mark' && tproxy=1
tun=0
ip -o link show 2>/dev/null | grep -qiE '(^|: )[[:alnum:]_.-]*(tun|tap|wg)[[:alnum:]_.-]*:' && tun=1
if [ "$count" -gt 1 ]; then result=multiple
elif [ "$has_mihomo" -eq 1 ]; then [ "$tproxy" -eq 1 ] && result=mihomo_tproxy || result=mihomo
elif [ "$has_clash" -eq 1 ]; then [ "$tproxy" -eq 1 ] && result=clash_tproxy || result=clash
elif [ "$has_singbox" -eq 1 ]; then [ "$tproxy" -eq 1 ] && result=sing-box_tproxy || result=sing-box
elif [ "$has_v2ray" -eq 1 ]; then [ "$tproxy" -eq 1 ] && result=v2ray_tproxy || result=v2ray
elif [ "$has_ss" -eq 1 ]; then [ "$tproxy" -eq 1 ] && result=shadowsocks_tproxy || result=shadowsocks
elif [ "$has_other" -eq 1 ]; then [ "$tproxy" -eq 1 ] && result=other_tproxy || result=other
elif [ "$tproxy" -eq 1 ]; then result=tproxy
elif [ "$tun" -eq 1 ]; then result=vpn
else result=none
fi

package_name=
app_name=
manager_type=
manager_id=
if [ -n "$core_pid" ] && [ -r "/proc/$core_pid/status" ]; then
	uid=$(awk '/^Uid:/{print $2; exit}' "/proc/$core_pid/status" 2>/dev/null)
	case "$uid" in ''|*[!0-9]*) uid= ;; esac
	if [ -n "$uid" ] && [ "$uid" -ge 10000 ]; then
		package_name=$({ cmd package list packages -U 2>/dev/null || pm list packages -U 2>/dev/null; } | awk -v target="$uid" '$NF == "uid:" target { sub(/^package:/, "", $1); print $1; exit }')
		[ -n "$package_name" ] && manager_type=app
	fi
fi
if [ -n "$package_name" ]; then
	app_name=$(dumpsys package "$package_name" 2>/dev/null | sed -n -e 's/.*nonLocalizedLabel=//p' -e 's/.*application-label://p' | grep -v '^null$' | head -1)
	case "$package_name" in
		*clash*meta*) [ -n "$app_name" ] || app_name='Clash Meta for Android' ;;
		*clash*) [ -n "$app_name" ] || app_name='Clash for Android' ;;
		io.nekohasekai.sfa*) [ -n "$app_name" ] || app_name='sing-box' ;;
		io.nekohasekai.sagernet*) [ -n "$app_name" ] || app_name='SagerNet' ;;
		com.v2ray.ang*) [ -n "$app_name" ] || app_name='v2rayNG' ;;
		com.github.shadowsocks*) [ -n "$app_name" ] || app_name='Shadowsocks' ;;
		*nekobox*) [ -n "$app_name" ] || app_name='NekoBox' ;;
		*hiddify*) [ -n "$app_name" ] || app_name='Hiddify' ;;
		*box*) [ -n "$app_name" ] || app_name='Box' ;;
	esac
fi
if [ -z "$package_name" ] && [ -n "$core_key" ]; then
	for dir in /data/adb/modules/*; do
		[ -f "$dir/module.prop" ] || continue
		[ -e "$dir/disable" ] && continue
		id=$(sed -n 's/^id=//p' "$dir/module.prop" | head -1)
		name=$(sed -n 's/^name=//p' "$dir/module.prop" | head -1)
		case "$core_key:$id:$name" in
			mihomo:*[Bb]ox*|mihomo:*[Mm]ihomo*|mihomo:*[Cc]lash*|clash:*[Bb]ox*|clash:*[Cc]lash*|sing-box:*[Bb]ox*|sing-box:*[Ss]ing*) ;;
			*) continue ;;
		esac
		manager_type=module
		manager_id=$id
		app_name=$name
		break
	done
fi

core_version=
if [ "$count" -eq 1 ] && [ -n "$core_pid" ]; then
	exe=$(readlink -f "/proc/$core_pid/exe" 2>/dev/null)
	[ -x "$exe" ] || exe=\${core_cmd%% *}
	if [ -x "$exe" ]; then
		case "$core_key" in
			mihomo|clash) version_args='-v' ;;
			sing-box|v2ray) version_args='version' ;;
			shadowsocks|other) version_args='--version' ;;
		esac
		if command -v timeout >/dev/null 2>&1; then
			core_version=$(timeout 2 "$exe" $version_args 2>&1 | head -1)
		else
			core_version=$("$exe" $version_args 2>&1 | head -1)
		fi
	fi
fi
core_version=$(printf '%s' "$core_version" | tr '\\r\\t' '  ')
mode=process
[ "$tproxy" -eq 1 ] && mode=TPROXY
[ "$tun" -eq 1 ] && [ "$tproxy" -eq 0 ] && mode=VPN
[ "$result" = none ] && mode=none

printf 'status=%s\\n' "$result"
printf 'core=%s\\n' "$core_label"
printf 'version=%s\\n' "$core_version"
printf 'package=%s\\n' "$package_name"
printf 'app=%s\\n' "$app_name"
printf 'manager_type=%s\\n' "$manager_type"
printf 'manager_id=%s\\n' "$manager_id"
printf 'mode=%s\\n' "$mode"`);
		const fields = {};
		for (const line of stdout.split('\n')) {
			const separator = line.indexOf('=');
			if (separator > 0) fields[line.slice(0, separator)] = line.slice(separator + 1).trim();
		}
		const valid = /^(mihomo|mihomo_tproxy|clash|clash_tproxy|sing-box|sing-box_tproxy|v2ray|v2ray_tproxy|shadowsocks|shadowsocks_tproxy|other|other_tproxy|tproxy|vpn|multiple|none)$/.test(fields.status);
		proxyStatusCache = {
			status: valid ? fields.status : 'unknown',
			coreName: fields.core || '',
			coreVersion: fields.version || '',
			packageName: fields.package || '',
			appName: fields.app || '',
			managerType: fields.manager_type || '',
			managerId: fields.manager_id || '',
			mode: fields.mode || '',
		};
		proxyStatusCheckedAt = now;
		return proxyStatusCache;
	} catch (error) {
		console.error('Error detecting proxy:', error);
		proxyStatusCache = { status: 'unknown', coreName: '', coreVersion: '', packageName: '', appName: '', managerType: '', managerId: '', mode: '' };
		proxyStatusCheckedAt = now - 25000;
		return proxyStatusCache;
	}
}

export async function getHostsStatus() {
	try {
		const cmd = `hs=/etc/hosts; sz=0; blk=0; [ -f "$hs" ] && sz=$(wc -c < "$hs" 2>/dev/null) && blk=$(grep -cE '^[[:space:]]*(0\\.0\\.0\\.0|127\\.0\\.0\\.1)[[:space:]]+' "$hs" 2>/dev/null); [ -z "$blk" ] && blk=0; [ -d /data/adb/modules/hosts ] && echo "systemless" || [ -n "$(ps -A -o comm= 2>/dev/null | grep -iE 'birdhost')" ] && echo "birdhost" || [ -n "$(ps -A -o comm= 2>/dev/null | grep -iE 'adaway')" ] && echo "adaway" || [ -n "$(ps -A -o comm= 2>/dev/null | grep -iE 'blokada|dns66|netguard')" ] && echo "blocker" || [ "$sz" -gt 200 ] && [ "$blk" -gt 5 ] && echo "blocked:$blk" || [ "$sz" -gt 200 ] && echo "modified" || echo "none"`;
		const { stdout: result } = await exec(cmd);
		return result.trim();
	} catch (error) {
		console.error('Error checking hosts:', error);
		return 'unknown';
	}
}

let qdiscCapabilityCache = null;
let qdiscCapabilityCheckedAt = 0;

export async function getQdiscCapabilities(force = false) {
	const now = Date.now();
	if (!force && qdiscCapabilityCache && now - qdiscCapabilityCheckedAt < 60000) {
		return qdiscCapabilityCache;
	}

	const names = ALL_QDISCS.map(shellQuote).join(' ');
	const command = `# qdisc-capability-probe
current=$(cat /proc/sys/net/core/default_qdisc 2>/dev/null); for q in ${names}; do state=unknown; if [ "$q" = "$current" ]; then state=supported; elif command -v tc >/dev/null 2>&1; then probe=$(tc qdisc add dev lo root "$q" help 2>&1); rc=$?; if printf '%s' "$probe" | grep -qiE 'unknown qdisc|qdisc kind is unknown|specified qdisc.*unknown|operation not supported|not supported'; then state=unsupported; elif [ -n "$probe" ] || [ "$rc" -eq 0 ]; then state=supported; fi; fi; printf '%s:%s\n' "$q" "$state"; done`;
	try {
		const { stdout } = await exec(command);
		const byName = new Map(stdout.split('\n')
			.map(line => line.trim().split(':', 2))
			.filter(([name, state]) => ALL_QDISCS.includes(name) && ['supported', 'unsupported'].includes(state)));
		qdiscCapabilityCache = ALL_QDISCS.map(name => ({
			name,
			state: ['supported', 'unsupported'].includes(byName.get(name)) ? byName.get(name) : 'unknown',
		}));
	} catch (error) {
		qdiscCapabilityCache = ALL_QDISCS.map(name => ({ name, state: 'unknown' }));
	}
	qdiscCapabilityCheckedAt = now;
	return qdiscCapabilityCache;
}

export async function setDefaultQdisc(qdisc) {
	const capabilities = await getQdiscCapabilities();
	if (capabilities.find(item => item.name === qdisc)?.state !== 'supported') return false;
	try {
		const dir = router_state.moduleInformation?.moduleDir || '/data/adb/modules/tcp_optimiser';
		await exec(`printf '%s\n' ${shellQuote(qdisc)} > /proc/sys/net/core/default_qdisc && printf '%s\n' ${shellQuote(qdisc)} > ${shellQuote(`${dir}/qdisc`)} && touch ${shellQuote(`${dir}/force_apply`)}`);
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
	const cmd = `awk '/^Tcp:/{n++;if(n==1){for(i=1;i<=NF;i++){if($i=="RetransSegs")r=i;if($i=="InSegs")ri=i;if($i=="OutSegs")ro=i}}else if(n==2){if(r&&ri&&ro)printf "retrans:%s\\nin:%s\\nout:%s\\n",$r,$ri,$ro;exit}}' /proc/net/snmp`;
	try {
		const { stdout } = await exec(cmd);
		const out = {};
		stdout.split('\n').forEach(line => {
			if (line.startsWith('retrans:')) out.retrans = parseInt(line.split(':')[1]) || 0;
			if (line.startsWith('in:')) out.inSegs = parseInt(line.split(':')[1]) || 0;
			if (line.startsWith('out:')) out.outSegs = parseInt(line.split(':')[1]) || 0;
		});
		return ['retrans', 'inSegs', 'outSegs'].every(key => Number.isFinite(out[key])) ? out : null;
	} catch (e) { return null; }
}

export async function getIfaceBytes(iface) {
	if (!/^[A-Za-z0-9_.:-]{1,32}$/.test(iface)) return null;
	try {
		const { stdout } = await exec(`awk -v if=${shellQuote(iface)} '$1==if":"{print "rx:"$2"\\ntx:"$10}' /proc/net/dev`);
		const out = {};
		stdout.split('\n').forEach(line => {
			if (line.startsWith('rx:')) out.rxBytes = parseInt(line.split(':')[1]) || 0;
			if (line.startsWith('tx:')) out.txBytes = parseInt(line.split(':')[1]) || 0;
		});
		return Number.isFinite(out.rxBytes) && Number.isFinite(out.txBytes) ? out : null;
	} catch (e) { return null; }
}

export async function getSockStat() {
	try {
		const { stdout } = await exec("awk '/^TCP:/{print}' /proc/net/sockstat 2>/dev/null");
		const parts = stdout.trim().split(/\s+/).slice(1);
		const values = {};
		for (let i = 0; i + 1 < parts.length; i += 2) values[parts[i]] = Number.parseInt(parts[i + 1], 10);
		if (!Number.isFinite(values.inuse)) return null;
		return {
			tcpInUse: values.inuse,
			tcpOrphan: Number.isFinite(values.orphan) ? values.orphan : null,
			tcpTW: Number.isFinite(values.tw) ? values.tw : null,
			tcpAlloc: Number.isFinite(values.alloc) ? values.alloc : null,
			tcpMem: Number.isFinite(values.mem) ? values.mem : null,
		};
	} catch (e) { return null; }
}

export async function getTCPConnsCount() {
	try {
		const { stdout } = await exec("[ -r /proc/net/tcp ] || exit 1; awk 'FNR>1 && $4==\"01\"{n++} END{print n+0}' /proc/net/tcp /proc/net/tcp6 2>/dev/null");
		const count = Number.parseInt(stdout.trim(), 10);
		return Number.isFinite(count) ? count : null;
	} catch (e) { return null; }
}

export async function getDNSServers() {
	try {
		const { stdout } = await exec("getprop | grep -E '^\\[net\\.([A-Za-z0-9_.-]+\\.)?dns[0-9]+\\]' | sort");
		const servers = [];
		stdout.split('\n').forEach(line => {
			const m = line.match(/^\[net\.(?:(.+)\.)?dns\d+\]:\s*\[([^\]]+)\]$/);
			if (m) {
				const val = m[2];
				if (val && val !== '0.0.0.0' && val !== '::') {
					servers.push({ iface: m[1] || 'system', ip: val });
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
	} catch (e) { return null; }
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
		const rtts = out.filter(v => v.rtt > 0).map(v => v.rtt);
		const cwnds = out.filter(v => v.cwnd > 0).map(v => v.cwnd);
		if (rtts.length === 0) return null;
		const avgRTT = rtts.reduce((sum, value) => sum + value, 0) / rtts.length;
		const avgCWND = cwnds.length ? cwnds.reduce((sum, value) => sum + value, 0) / cwnds.length : 0;
		return { samples: rtts.length, avgRTT: Math.round(avgRTT * 100) / 100, avgCWND: Math.round(avgCWND), maxRTT: Math.max(...rtts), maxCWND: cwnds.length ? Math.max(...cwnds) : 0 };
	} catch (e) { return null; }
}
