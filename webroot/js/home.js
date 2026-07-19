import { exec, toast } from './kernelsu.js';
import I18N from './i18n.js';
import { get_active_iface, get_active_algorithm, getInitcwndInitrwndValue, getModuleActiveState, getDefaultQdisc, getProxyStatus, getHostsStatus, getQdiscCapabilities, getRuntimeSnapshot, repairRuntimePolicy, formatLocalDateTime } from './common.js';
import router_state from './router.js';
import { ALL_ALGOS, getAlgorithmDescription, getQdiscDescription } from './capabilities.js';
import { haptic, setAnimatedText } from './motion.js';

let _lastAlgoSet = '';
let _lastActiveAlgo = '';
let _lastEnabled = false;

export async function updateModuleStatus(force = false) {
	try {
		// Check via KSU API (already loaded by updateModuleInformation)
		if (!router_state.moduleInformation) {
			router_state.homePageParams.module_status = "NotInstalled";
			return;
		}

		let snapshot = null;
		try {
			snapshot = await getRuntimeSnapshot(force);
		} catch (error) {
			console.warn('Unified runtime snapshot unavailable, using compatibility probes:', error);
		}

		let running, iface, algo, initcwndInitrwnd, defaultQdisc, hosts;
		const [proxy, qdiscCapabilities] = await Promise.all([
			getProxyStatus(force), getQdiscCapabilities(force),
		]);
		if (snapshot) {
			running = snapshot.module_active;
			iface = snapshot.active_iface;
			algo = snapshot.algorithm;
			initcwndInitrwnd = snapshot.init_windows || [];
			defaultQdisc = snapshot.default_qdisc;
			hosts = snapshot.hosts || 'unknown';
			router_state.available_algorithms = snapshot.available_algorithms || [];
			router_state.runtimeSnapshot = snapshot;
			router_state.verification = snapshot.verification;
		} else {
			[running, iface, algo, initcwndInitrwnd, defaultQdisc, hosts] = await Promise.all([
				getModuleActiveState(), get_active_iface(), get_active_algorithm(),
				getInitcwndInitrwndValue(), getDefaultQdisc(), getHostsStatus(),
			]);
			router_state.runtimeSnapshot = null;
			router_state.verification = null;
		}

		router_state.homePageParams.module_status = running ? "Enabled" : "Disabled";
		router_state.homePageParams.active_iface = iface || "None";
		const ifaceName = iface || "";
		router_state.homePageParams.active_iface_type = classifyInterface(ifaceName);
		router_state.homePageParams.active_algorithm = algo || "Unknown";
		router_state.homePageParams.active_InitcwndInitrwndValue = initcwndInitrwnd;
		router_state.homePageParams.default_qdisc = defaultQdisc;
		router_state.homePageParams.proxy_status = proxy?.status || 'unknown';
		router_state.homePageParams.proxy_info = proxy || null;
		router_state.homePageParams.hosts_status = hosts;
		router_state.qdiscCapabilities = qdiscCapabilities;
	} catch (error) {
		console.error('Error updating status:', error);
	}
}

function verificationCheckLabel(key) {
	const known = {
		interface_mode: 'verification_check_interface',
		configured_policy: 'verification_check_policy',
		congestion_algorithm: 'verification_check_algorithm',
		default_qdisc: 'verification_check_default_qdisc',
		interface_qdisc: 'verification_check_interface_qdisc',
		tcp_pacing_ca_ratio: 'verification_check_pacing_ca',
		tcp_pacing_ss_ratio: 'verification_check_pacing_ss',
	};
	return known[key] ? I18N.t(known[key]) : key.replace(/^advanced\./, '').replaceAll('_', ' ');
}

function renderVerification() {
	const snapshot = router_state.verification;
	const list = document.getElementById('verification-list');
	const count = document.getElementById('verification-count');
	const panel = document.getElementById('verification-panel');
	const lastRepair = document.getElementById('verification-last-repair');
	const repairButton = document.getElementById('verification-repair-btn');
	if (!list || !count || !panel || !lastRepair) return;

	list.replaceChildren();
	if (!snapshot?.summary || !Array.isArray(snapshot.checks)) {
		if (repairButton) repairButton.disabled = true;
		count.textContent = I18N.t('verification_unavailable');
		panel.dataset.state = 'unavailable';
		lastRepair.textContent = I18N.t('verification_requires_core');
		return;
	}
	if (repairButton) repairButton.disabled = router_state.homePageParams.module_status !== 'Enabled';

	const summary = snapshot.summary;
	count.textContent = summary.drifted > 0
		? I18N.t('verification_drift_count', { count: summary.drifted })
		: summary.unavailable > 0
			? I18N.t('verification_unavailable_count', { count: summary.unavailable })
			: I18N.t('verification_match_count', { matched: summary.matched, total: summary.total });
	panel.dataset.state = summary.drifted > 0 ? 'drift' : (summary.unavailable > 0 ? 'unavailable' : 'match');

	for (const check of snapshot.checks) {
		const row = document.createElement('div');
		row.className = 'verification-row';
		row.dataset.state = check.state;
		const text = document.createElement('div');
		text.className = 'verification-row__text';
		const title = document.createElement('strong');
		title.textContent = verificationCheckLabel(check.key);
		const values = document.createElement('span');
		values.textContent = check.state === 'match'
			? check.actual
			: `${check.actual ?? I18N.t('verification_value_unavailable')} → ${check.expected}`;
		text.append(title, values);
		const state = document.createElement('span');
		state.className = 'verification-row__state';
		state.textContent = I18N.t(`verification_state_${check.state}`);
		row.append(text, state);
		list.appendChild(row);
	}

	const repair = snapshot.last_repair;
	lastRepair.textContent = repair
		? I18N.t(repair.success ? 'verification_last_repair_success' : 'verification_last_repair_failed', {
			time: formatLocalDateTime(new Date(repair.timestamp_epoch * 1000)),
		})
		: I18N.t('verification_never_repaired');
}

function classifyInterface(ifaceName) {
	if (/^(wlan|swlan|wifi|ap)/i.test(ifaceName)) return 'Wi-Fi';
	if (/^(rmnet|ccmni|ccemni|pdp|wwan|v4-rmnet|rev_rmnet)/i.test(ifaceName)) return 'Cellular';
	if (/^(eth|en)[A-Za-z0-9_.-]*/i.test(ifaceName)) return 'Ethernet';
	if (/^(rndis|usb)[A-Za-z0-9_.-]*/i.test(ifaceName)) return 'USB';
	if (/^(tun|tap|wg)[A-Za-z0-9_.-]*/i.test(ifaceName)) return 'VPN';
	return 'Unknown';
}

function updateAlgoChips() {
	const container = document.getElementById('home-algo-chips');
	if (!container) return;
	const avail = router_state.available_algorithms;
	const active = router_state.homePageParams.active_algorithm;
	const enabled = router_state.homePageParams.module_status === "Enabled";
	const count = document.getElementById('algo-capability-count');
	if (count) setAnimatedText(count, avail?.length ? I18N.t('capability_available_count', {
		available: avail.length,
		total: ALL_ALGOS.length,
	}) : I18N.t('home_status_unknown'));

	const curSet = [...(avail || [])].sort().join(',');
	if (curSet === _lastAlgoSet && active === _lastActiveAlgo && enabled === _lastEnabled) return;
	_lastAlgoSet = curSet;
	_lastActiveAlgo = active;
	_lastEnabled = enabled;

	container.innerHTML = '';
	if (!avail || avail.length === 0) {
		container.innerHTML = '<span style="color:var(--md-sys-color-on-surface-variant);font-size:0.8rem;">' + I18N.t('home_status_unknown') + '</span>';
		return;
	}
	const supported = new Set(avail || []);
	ALL_ALGOS.forEach(algo => {
		const chip = document.createElement('button');
		chip.className = 'algo-chip';
		chip.dataset.algo = algo;
		chip.title = getAlgorithmDescription(algo, I18N.currentLang);
		const label = document.createElement('span');
		label.textContent = algo;
		chip.appendChild(label);
		chip.setAttribute('aria-label', `${algo}: ${I18N.t(supported.has(algo) ? 'capability_supported' : 'capability_unsupported')}`);
		if (!supported.has(algo)) {
			chip.classList.add('unsupported');
			chip.disabled = true;
			chip.title = I18N.t('capability_unsupported');
			const mark = document.createElement('span');
			mark.className = 'capability-mark';
			mark.textContent = '×';
			mark.setAttribute('aria-hidden', 'true');
			chip.appendChild(mark);
		} else {
			chip.title = I18N.t('capability_supported');
		}
		if (enabled && algo === active) chip.classList.add('selected');
		chip.addEventListener('click', () => {
			document.querySelector('.nav-item[data-page="settings"]')?.click();
			requestAnimationFrame(() => {
				const group = document.getElementById('network-policy-group');
				if (group) group.open = true;
				document.querySelector(`#wifi-algo-chips [data-algo="${algo}"]`)?.focus({ preventScroll: true });
				group?.scrollIntoView({ behavior: 'smooth', block: 'start' });
			});
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

function escapeHtml(value) {
	return String(value).replace(/[&<>"']/g, char => ({
		'&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
	})[char]);
}

function detailBlock(primary, lines = [], note = '') {
	const parts = [`<b>${escapeHtml(primary)}</b>`];
	for (const line of lines.filter(Boolean)) parts.push(escapeHtml(line));
	if (note) parts.push(`<i>${escapeHtml(note)}</i>`);
	return parts.join('<br>');
}

function cardLongDesc(id) {
	const p = router_state.homePageParams;
	switch (id) {
		case 'iface-type':
			return detailBlock(I18N.t('detail_iface_summary', { type: p.active_iface_type, name: p.active_iface }), [
				I18N.t('detail_iface_basis'),
			], I18N.t('detail_live_device'));
		case 'iface-name':
			return detailBlock(p.active_iface, [I18N.t('detail_iface_basis')], I18N.t('detail_live_device'));
		case 'tcp-algo':
			return detailBlock(p.active_algorithm, [
				getAlgorithmDescription(p.active_algorithm, I18N.currentLang),
				I18N.t('detail_algo_source'),
			], I18N.t('detail_algo_note'));
		case 'qdisc':
			return detailBlock(p.default_qdisc, [
				getQdiscDescription(p.default_qdisc, I18N.currentLang),
				I18N.t('detail_qdisc_source'),
			], I18N.t('detail_live_device'));
		case 'proxy':
			return proxyDetail(p.proxy_status, p.proxy_info);
		case 'hosts':
			return detailBlock(document.getElementById('hosts-value')?.textContent || p.hosts_status, [
				I18N.t('detail_hosts_basis'),
			], I18N.t('detail_live_device'));
		default: return '';
	}
}

function proxyDetail(status, info = {}) {
	if (status === 'unknown') return detailBlock(I18N.t('home_status_unknown'), [
		I18N.t('detail_proxy_unknown'),
	]);
	if (status === 'none') return detailBlock(I18N.t('home_proxy_none'), [
		I18N.t('detail_proxy_none'),
	], I18N.t('detail_live_device'));
	if (status === 'vpn') return detailBlock(I18N.t('home_proxy_vpn'), [
		I18N.t('detail_proxy_vpn'),
	], I18N.t('detail_live_device'));

	const transparent = status === 'tproxy' || status.endsWith('_tproxy');
	const family = status.replace(/_tproxy$/, '');
	const names = {
		mihomo: 'Mihomo', clash: 'Clash', 'sing-box': 'sing-box',
		v2ray: 'V2Ray / Xray', shadowsocks: 'Shadowsocks', other: I18N.t('home_proxy_other'),
	};
	const coreName = info?.coreName || (status === 'tproxy' ? '' : names[family] || family);
	const primary = info?.appName || coreName || (status === 'tproxy' ? 'TPROXY' : family);
	const lines = [];
	if (info?.managerType === 'app') {
		if (info.appName) lines.push(I18N.t('detail_proxy_app', { name: info.appName }));
		if (info.packageName) lines.push(I18N.t('detail_proxy_package', { name: info.packageName }));
	} else if (info?.managerType === 'module') {
		lines.push(I18N.t('detail_proxy_module', { name: info.appName || info.managerId || '—' }));
		if (info.managerId) lines.push(I18N.t('detail_proxy_module_id', { id: info.managerId }));
		lines.push(I18N.t('detail_proxy_package_module'));
	}
	if (coreName) lines.push(I18N.t('detail_proxy_core', { name: coreName }));
	lines.push(info?.coreVersion
		? I18N.t('detail_proxy_version', { version: info.coreVersion })
		: I18N.t('detail_proxy_version_unknown'));
	if (transparent) lines.push(I18N.t('detail_proxy_tproxy'));
	return detailBlock(primary, lines, I18N.t('detail_proxy_note'));
}

export function updateHomeUI() {
	if (router_state.isInitializing) return;
	const p = router_state.homePageParams;
	const notInstalled = p.module_status === "NotInstalled";
	const enabled = p.module_status === "Enabled";

	const statusVal = document.getElementById('module-status-value');
	const heroCard = document.getElementById('hero-status-card');
	const statusChip = document.getElementById('status-chip');
	const chipLabel = document.getElementById('status-chip-label');

	if (notInstalled) {
		setAnimatedText(statusVal, I18N.t('status_not_installed'));
		heroCard.classList.add('disabled');
		setAnimatedText(chipLabel, I18N.t('status_not_installed'));
		statusChip.classList.remove('enabled');
		statusChip.classList.add('disabled');
	} else {
		setAnimatedText(statusVal, enabled ? I18N.t('status_active') : I18N.t('status_disabled'));
		heroCard.classList.toggle('disabled', !enabled);
		setAnimatedText(chipLabel, enabled ? I18N.t('status_active') : I18N.t('status_inactive'));
		statusChip.classList.toggle('enabled', enabled);
		statusChip.classList.toggle('disabled', !enabled);
	}

	setAnimatedText(document.getElementById('iface-type-value'), notInstalled ? "\u2014" : (enabled ? p.active_iface_type : "\u2014"));
	setAnimatedText(document.getElementById('iface-name-value'), notInstalled ? "\u2014" : (enabled ? p.active_iface : "\u2014"));
	setAnimatedText(document.getElementById('tcp-algo-value'), notInstalled ? "\u2014" : (enabled ? p.active_algorithm : "\u2014"));
	setAnimatedText(document.getElementById('qdisc-value'), notInstalled ? "\u2014" : (enabled ? p.default_qdisc : "\u2014"));
	document.querySelector('.network-panel')?.classList.toggle('is-live', enabled && !notInstalled);

	const proxyLabel = {
		none: I18N.t('home_proxy_none'), vpn: I18N.t('home_proxy_vpn'), tproxy: 'TPROXY',
		mihomo: 'Mihomo', mihomo_tproxy: 'Mihomo · TPROXY',
		clash: 'Clash', clash_tproxy: 'Clash · TPROXY',
		v2ray: 'V2Ray / Xray', v2ray_tproxy: 'V2Ray / Xray · TPROXY',
		'sing-box': 'sing-box', 'sing-box_tproxy': 'sing-box · TPROXY',
		shadowsocks: 'Shadowsocks', shadowsocks_tproxy: 'Shadowsocks · TPROXY',
		other: I18N.t('home_proxy_other'), other_tproxy: `${I18N.t('home_proxy_other')} · TPROXY`,
		multiple: I18N.t('home_status_multiple'), unknown: I18N.t('home_status_unknown')
	};
	const proxyEl = document.getElementById('proxy-value');
	const proxyCard = document.getElementById('proxy-card');
	const coreLabel = proxyLabel[p.proxy_status] || p.proxy_info?.coreName || p.proxy_status;
	const managerLabel = p.proxy_info?.appName;
	const hasDistinctManager = managerLabel && p.proxy_info?.coreName
		&& managerLabel.toLowerCase() !== p.proxy_info.coreName.toLowerCase();
	const displayedProxy = hasDistinctManager ? `${managerLabel} · ${coreLabel}` : coreLabel;
	if (proxyEl) setAnimatedText(proxyEl, displayedProxy);
	if (proxyCard) {
		proxyCard.dataset.state = p.proxy_status === 'unknown' ? 'unknown' : (p.proxy_status === 'none' ? 'neutral' : 'active');
		proxyCard.setAttribute('aria-label', `${I18N.t('home_proxy')}: ${proxyEl?.textContent || I18N.t('home_status_unknown')}`);
	}

	const hostsEl = document.getElementById('hosts-value');
	if (hostsEl) {
		const h = p.hosts_status;
		let hostsLabel = h;
		if (h === 'none') hostsLabel = I18N.t('home_hosts_default');
		else if (h === 'systemless') hostsLabel = 'Systemless';
		else if (h === 'birdhost') hostsLabel = 'BirdHost';
		else if (h === 'adaway') hostsLabel = 'AdAway';
		else if (h === 'blocker') hostsLabel = 'Blocker';
		else if (h.startsWith('blocked:')) hostsLabel = I18N.t('home_hosts_blocked', { count: h.split(':')[1] });
		else if (h === 'modified') hostsLabel = I18N.t('home_hosts_modified');
		else if (h === 'unknown') hostsLabel = I18N.t('home_status_unknown');
		setAnimatedText(hostsEl, hostsLabel);
		const hostsCard = document.getElementById('hosts-card');
		if (hostsCard) {
			hostsCard.dataset.state = h === 'unknown' ? 'unknown' : (h === 'none' ? 'neutral' : 'active');
			hostsCard.setAttribute('aria-label', `${I18N.t('home_hosts')}: ${hostsEl.textContent}`);
		}
	}

	updateAlgoChips();
	renderVerification();
}

export async function initHome() {
	document.getElementById('verification-refresh-btn')?.addEventListener('click', async event => {
		const button = event.currentTarget;
		button.disabled = true;
		try {
			await updateModuleStatus(true);
			updateHomeUI();
			haptic('selection');
		} finally {
			button.disabled = false;
		}
	});
	document.getElementById('verification-repair-btn')?.addEventListener('click', async event => {
		const button = event.currentTarget;
		button.disabled = true;
		try {
			const record = await repairRuntimePolicy();
			await updateModuleStatus(true);
			updateHomeUI();
			toast(I18N.t(record.success ? 'verification_repair_success' : 'verification_repair_failed', {
				count: record.errors.length,
			}));
			haptic(record.success ? 'success' : 'error');
		} catch (error) {
			console.error('Policy repair failed:', error);
			toast(I18N.t('verification_repair_error'));
			haptic('error');
		} finally {
			button.disabled = false;
		}
	});

	// Status cards are directly tappable and keyboard accessible.
	['iface-type', 'iface-name', 'tcp-algo', 'qdisc', 'proxy', 'hosts'].forEach(id => {
		const card = document.getElementById(id + '-card');
		if (!card) return;
		card.classList.add('detail-card');
		card.tabIndex = 0;
		card.setAttribute('role', 'button');
		const openDetail = async () => {
			if (id === 'hosts') {
				try {
					const { stdout } = await exec('cat /etc/hosts 2>/dev/null | head -50');
					const content = stdout.trim() || '(empty)';
					showDetail(I18N.t('home_hosts'), `<pre style="font-size:0.75rem;max-height:240px;overflow:auto;white-space:pre-wrap;font-family:monospace">${escapeHtml(content)}</pre>`);
				} catch (e) {
					showDetail(I18N.t('home_hosts'), cardLongDesc(id));
				}
			} else {
				showDetail(I18N.t('home_' + id.replace('-', '_')), cardLongDesc(id));
			}
		};
		card.addEventListener('click', openDetail);
		card.addEventListener('keydown', (event) => {
			if (event.key !== 'Enter' && event.key !== ' ') return;
			event.preventDefault();
			void openDetail();
		});
	});

	// Detail overlay close
	document.getElementById('detail-overlay')?.addEventListener('click', (e) => {
		if (e.target.id === 'detail-overlay') e.target.hidden = true;
	});
	document.addEventListener('keydown', (event) => {
		if (event.key === 'Escape') document.getElementById('detail-overlay').hidden = true;
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

	updateHomeUI();
}
