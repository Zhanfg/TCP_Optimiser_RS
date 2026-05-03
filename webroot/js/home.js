import { exec, toast } from './kernelsu.js';
import I18N from './i18n.js';
import { get_active_iface, get_active_algorithm, getInitcwndInitrwndValue, get_wifi_calling_state, getModuleActiveState, getDefaultQdisc, getProxyStatus, getHostsStatus } from './common.js';
import router_state from './router.js';

const ALL_ALGOS = ['bbr', 'bbr2', 'bbr3', 'cubic', 'westwood', 'westwood_plus', 'reno',
	'htcp', 'vegas', 'yeah', 'illinois', 'dctcp', 'cdg', 'bic', 'highspeed',
	'hybla', 'nv', 'scalable', 'lp'];
let _lastAlgoSet = '';
let _lastActiveAlgo = '';
let _lastEnabled = false;

export async function updateModuleStatus() {
	try {
		const [enabled, iface, algo, initcwndInitrwnd, defaultQdisc, proxy, hosts] = await Promise.all([
			getModuleActiveState(),
			get_active_iface(),
			get_active_algorithm(),
			getInitcwndInitrwndValue(),
			getDefaultQdisc(),
			getProxyStatus(),
			getHostsStatus(),
		]);
		const isActive = enabled;
		router_state.homePageParams.module_status = isActive ? "Enabled" : "Disabled";
		router_state.homePageParams.active_iface = iface || "None";
		router_state.homePageParams.active_iface_type = (iface || "").startsWith("rmnet") || (iface || "").startsWith("ccmni") ? "Cellular"
			: (iface || "").startsWith("wlan") || (iface || "").startsWith("tun") ? "Wi-Fi" : "Unknown";
		router_state.homePageParams.active_algorithm = algo || "Unknown";
		router_state.homePageParams.active_InitcwndInitrwndValue = initcwndInitrwnd;
		router_state.homePageParams.default_qdisc = defaultQdisc;
		router_state.homePageParams.proxy_status = proxy;
		router_state.homePageParams.hosts_status = hosts;
	} catch (error) {
		console.error('Error updating status:', error);
	}
}

function updateAlgoChips() {
	const container = document.getElementById('home-algo-chips');
	if (!container) return;
	const available = new Set(router_state.available_algorithms);
	const active = router_state.homePageParams.active_algorithm;
	const enabled = router_state.homePageParams.module_status === "Enabled";

	const curSet = [...available].sort().join(',');
	if (curSet === _lastAlgoSet && active === _lastActiveAlgo && enabled === _lastEnabled) return;
	_lastAlgoSet = curSet;
	_lastActiveAlgo = active;
	_lastEnabled = enabled;

	container.innerHTML = '';
	ALL_ALGOS.forEach(algo => {
		if (!available.has(algo)) return;
		const chip = document.createElement('button');
		chip.className = 'algo-chip';
		chip.dataset.algo = algo;
		chip.textContent = algo;
		if (enabled && algo === active) chip.classList.add('selected');
		chip.addEventListener('click', () => {
			toast(algo === active ? I18N.t('algo_active', { algo }) : I18N.t('algo_available', { algo }));
		});
		container.appendChild(chip);
	});
}

function showDetail(title, body) {
	const overlay = document.getElementById('detail-overlay');
	const elTitle = document.getElementById('detail-title');
	const elBody = document.getElementById('detail-body');
	if (!overlay || !elTitle || !elBody) return;
	elTitle.textContent = title;
	elBody.innerHTML = body;
	overlay.hidden = false;
}

function cardLongDesc(id) {
	const p = router_state.homePageParams;
	switch (id) {
		case 'iface-type':
			return `<b>${p.active_iface_type}</b><br>Wi-Fi: 2.4/5/6 GHz freq-based pacing scaling<br>Cellular: independent algorithm selection<br><i>Detected by ip route get 192.0.2.1</i>`;
		case 'iface-name':
			return `<b>${p.active_iface}</b><br>wlan* → Wi-Fi<br>rmnet*/ccmni* → Cellular<br>tun* → VPN/VoWiFi<br><i>Active route determines interface</i>`;
		case 'tcp-algo':
			return `<b>${p.active_algorithm}</b><br>Set via /proc/sys/net/ipv4/tcp_congestion_control<br>All available: ${router_state.available_algorithms.join(', ')}<br><i>Changes apply to new connections</i>`;
		case 'qdisc':
			return `<b>${p.default_qdisc}</b><br>fq = Fair Queue (low latency paired with BBR)<br>fq_codel = Fair Queuing + CoDel (balanced)<br>cake = Common Applications Kept Enhanced<br>pfifo_fast = Default kernel FIFO<br><i>Affects all new socket connections</i>`;
		case 'proxy':
			const s = p.proxy_status;
			if (s === 'none') return '<b>No proxy detected</b><br><i>Checked via ps -A for clash/v2ray/sing-box/shadowsocks processes</i>';
			if (s === 'vpn') return '<b>System VPN</b><br>A tun interface is active (Android VPN API)<br><i>Check ip link show grep tun</i>';
			return `<b>Transparent Proxy</b><br>Type: ${s}<br>Detected process: ${s === 'clash' ? 'clash/mihomo' : s === 'v2ray' ? 'v2ray/xray' : s === 'sing-box' ? 'sing-box' : s === 'surfing' ? 'Surfing' : s === 'shadowsocks' ? 'Shadowsocks' : s}<br><i>Module-level transparent proxy active</i>`;
		case 'hosts':
			const h = p.hosts_status;
			if (h === 'none') return '<b>Default hosts</b><br>2 entries (localhost + ip6-localhost)<br><i>No modifications detected</i>';
			if (h === 'systemless') return '<b>Systemless Hosts</b><br>Magisk module at /data/adb/modules/hosts<br><i>Overlays /etc/hosts at boot</i>';
			if (h === 'birdhost') return '<b>BirdHost</b><br>DNS-rewriting hosts app running<br><i>Process detected via ps</i>';
			if (h === 'adaway') return '<b>AdAway</b><br>Ad-blocking hosts manager active<br><i>Process detected via ps</i>';
			if (h === 'blocker') return '<b>DNS Blocker App</b><br>Blokada/DNS66/NetGuard type blocker<br><i>Process detected via ps</i>';
			if (h.startsWith('blocked:')) return `<b>${h.split(':')[1]} entries blocked</b><br>Hosts file size > 200 bytes<br>0.0.0.0/127.0.0.1 redirects active`;
			if (h === 'modified') return '<b>Hosts modified</b><br>Content differs from stock Android<br><i>Review /etc/hosts manually</i>';
			return `<b>${h}</b>`;
		default: return '';
	}
}

export function updateHomeUI() {
	if (router_state.isInitializing) return;
	const p = router_state.homePageParams;
	const enabled = p.module_status === "Enabled";

	document.getElementById('module-status-value').textContent = enabled ? I18N.t('status_active') : I18N.t('status_disabled');
	document.getElementById('hero-status-card').classList.toggle('disabled', !enabled);
	document.getElementById('status-chip').classList.toggle('enabled', enabled);
	document.getElementById('status-chip').classList.toggle('disabled', !enabled);
	document.getElementById('status-chip-label').textContent = enabled ? I18N.t('status_active') : I18N.t('status_inactive');

	document.getElementById('iface-type-value').textContent = enabled ? p.active_iface_type : "\u2014";
	document.getElementById('iface-name-value').textContent = enabled ? p.active_iface : "\u2014";
	document.getElementById('tcp-algo-value').textContent = enabled ? p.active_algorithm : "\u2014";
	document.getElementById('qdisc-value').textContent = enabled ? p.default_qdisc : "\u2014";

	const proxyLabel = {
		none: '\u2014', vpn: 'System VPN', clash: 'Clash', surfing: 'Surfing', v2ray: 'V2Ray / Xray',
		'sing-box': 'sing-box', shadowsocks: 'Shadowsocks', other: 'Transparent', multiple: 'Multiple', unknown: '...'
	};
	const proxyEl = document.getElementById('proxy-value');
	if (proxyEl) proxyEl.textContent = proxyLabel[p.proxy_status] || p.proxy_status;

	const hostsEl = document.getElementById('hosts-value');
	if (hostsEl) {
		const h = p.hosts_status;
		if (h === 'none') hostsEl.textContent = '\u2014';
		else if (h === 'systemless') hostsEl.textContent = 'Systemless';
		else if (h === 'birdhost') hostsEl.textContent = 'BirdHost';
		else if (h === 'adaway') hostsEl.textContent = 'AdAway';
		else if (h === 'blocker') hostsEl.textContent = 'Blocker';
		else if (h.startsWith('blocked:')) hostsEl.textContent = h.split(':')[1] + ' blocked';
		else if (h === 'modified') hostsEl.textContent = 'Modified';
		else hostsEl.textContent = h;
	}

	updateAlgoChips();
}

export async function initHome() {
	// Long press on info cards → detail popup
	['iface-type', 'iface-name', 'tcp-algo', 'qdisc', 'proxy', 'hosts'].forEach(id => {
		const card = document.getElementById(id + '-card');
		if (!card) return;
		let timer = null;
		card.addEventListener('pointerdown', async () => {
			const handler = async () => {
				if (id === 'hosts') {
					try {
						const { stdout } = await exec('cat /etc/hosts 2>/dev/null | head -50');
						const content = stdout.trim() || '(empty)';
						showDetail(I18N.t('home_hosts'), `<pre style="font-size:0.65rem;max-height:200px;overflow:auto;white-space:pre-wrap;font-family:monospace">${content}</pre>`);
					} catch (e) {
						showDetail(I18N.t('home_hosts'), cardLongDesc(id));
					}
				} else {
					showDetail(I18N.t('home_' + id.replace('-', '_')), cardLongDesc(id));
				}
			};
			timer = setTimeout(handler, 500);
		});
		card.addEventListener('pointerup', () => clearTimeout(timer));
		card.addEventListener('pointerleave', () => clearTimeout(timer));
		card.addEventListener('pointercancel', () => clearTimeout(timer));
	});

	// Detail overlay close
	document.getElementById('detail-overlay')?.addEventListener('click', (e) => {
		if (e.target.id === 'detail-overlay') e.target.hidden = true;
	});

	// About modal handlers
	document.getElementById('about-close-btn')?.addEventListener('click', () => {
		document.getElementById('about-overlay').hidden = true;
	});
	document.getElementById('about-overlay')?.addEventListener('click', (e) => {
		if (e.target.id === 'about-overlay') e.target.hidden = true;
	});

	// Pull-to-about: touch overscroll at bottom of home page
	let overscrollY = 0;
	let touchStartY = 0;
	let overscrollTriggered = false;

	document.addEventListener('touchstart', (e) => {
		if (router_state.current_active_page !== 'home') return;
		touchStartY = e.touches[0].clientY;
		overscrollY = 0;
		overscrollTriggered = false;
	}, { passive: true });

	document.addEventListener('touchmove', (e) => {
		if (router_state.current_active_page !== 'home') return;
		if (overscrollTriggered) return;
		const atBottom = window.scrollY + window.innerHeight >= document.documentElement.scrollHeight - 2;
		if (!atBottom) { overscrollY = 0; return; }
		const deltaY = touchStartY - e.touches[0].clientY; // positive = finger moving UP (pull down content)
		if (deltaY > 0) {
			overscrollY = deltaY;
			if (overscrollY > 80) {
				overscrollTriggered = true;
				document.getElementById('about-overlay').hidden = false;
				overscrollY = 0;
			}
		}
	}, { passive: true });

	document.getElementById('pull-about')?.addEventListener('click', () => {
		document.getElementById('about-overlay').hidden = false;
	});

	router_state.isInitializing = false;
	updateHomeUI();
}
