import { exec, toast, shellQuote } from './kernelsu.js';
import { haptic } from './motion.js';
import I18N from './i18n.js';
import router_state from './router.js';
import { addLog } from './logs.js';
import { fetchIsConfigFile, getDefaultQdisc, getQdiscCapabilities, setDefaultQdisc } from './common.js';
import { ALL_ALGOS, ALL_QDISCS, getAlgorithmDescription, getQdiscDescription } from './capabilities.js';
import { setDynamicColorEnabled, setThemeMode } from './theme.js';

let advancedInitialized = false;

async function getSelectedAlgorithm(prefix) {
	if (!['wlan', 'rmnet_data'].includes(prefix)) return null;
	try {
		const dir = router_state.moduleInformation.moduleDir;
		const { stdout } = await exec(`for f in ${shellQuote(dir)}/${prefix}_*; do [ -f "$f" ] && printf '%s\n' "\${f##*/}"; done`);
		const selected = stdout.split('\n')
			.map(name => name.trim())
			.filter(name => name.startsWith(`${prefix}_`))
			.map(name => name.slice(prefix.length + 1));
		const trimmed = ALL_ALGOS.find(algo => selected.includes(algo)) || '';
		if (prefix === "wlan") router_state.settingsPageParams.wlanAlgo = trimmed;
		else if (prefix === "rmnet_data") router_state.settingsPageParams.rmnetAlgo = trimmed;
		return trimmed;
	} catch (error) {
		console.error('Error fetching algorithms:', error);
		return null;
	}
}

async function checkAndGetPrefixValueExists(prefix) {
	if (prefix === "wlan")
		return router_state.settingsPageParams.wlanAlgo ?? await getSelectedAlgorithm(prefix);
	if (prefix === "rmnet_data")
		return router_state.settingsPageParams.rmnetAlgo ?? await getSelectedAlgorithm(prefix);
}

const fetchAvailableAlgorithms = async (force = false) => {
	if (!force && router_state.available_algorithms.length > 0) return;
	try {
		const { stdout: output } = await exec('cat /proc/sys/net/ipv4/tcp_available_congestion_control 2>/dev/null');
		const algos = output.trim().split(/\s+/).filter(a => ALL_ALGOS.includes(a));
		if (algos.length > 0) {
			router_state.available_algorithms = algos;
			// Update cache for next time
			const dir = router_state.moduleInformation?.moduleDir || '/data/adb/modules/tcp_optimiser';
			await exec(`printf '%s\\n' ${shellQuote(algos.join(' '))} > ${shellQuote(`${dir}/available_algos`)} 2>/dev/null`).catch(() => {});
			return;
		}
	} catch (e) {}
	// Fallback: try cached file
	try {
		const dir = router_state.moduleInformation?.moduleDir || '/data/adb/modules/tcp_optimiser';
		const { stdout: cached } = await exec(`cat ${shellQuote(`${dir}/available_algos`)} 2>/dev/null`);
		const algos = cached.trim().split(/\s+/).filter(a => ALL_ALGOS.includes(a));
		if (algos.length > 0) {
			router_state.available_algorithms = algos;
			return;
		}
	} catch (e) {}
	toast(I18N.t('toast_no_congestion_algo'));
};

function buildAlgoChips(containerId, selectedAlgo, onClick) {
	const container = document.getElementById(containerId);
	if (!container) return;
	container.innerHTML = '';

	const avail = router_state.available_algorithms;
	if (!avail) {
		container.innerHTML = '<span style="color:var(--md-sys-color-on-surface-variant);font-size:0.8rem;">' + I18N.t('label_loading') + '</span>';
		return;
	}
	const supported = new Set(avail);
	const description = document.getElementById(containerId === 'wifi-algo-chips'
		? 'wifi-algo-description' : 'cell-algo-description');
	const updateDescription = (algo) => {
		if (description) description.textContent = getAlgorithmDescription(algo, I18N.currentLang);
	};
	const countId = containerId === 'wifi-algo-chips' ? 'wifi-capability-count' : 'cell-capability-count';
	const count = document.getElementById(countId);
	if (count) count.textContent = I18N.t('capability_available_count', {
		available: supported.size,
		total: ALL_ALGOS.length,
	});

	ALL_ALGOS.forEach(algo => {
		const chip = document.createElement('button');
		chip.className = 'algo-chip';
		chip.dataset.algo = algo;
		const capabilityLabel = I18N.t(supported.has(algo) ? 'capability_supported' : 'capability_unsupported');
		const algorithmDescription = getAlgorithmDescription(algo, I18N.currentLang);
		chip.title = `${algorithmDescription} · ${capabilityLabel}`;
		chip.setAttribute('aria-label', `${algo}: ${capabilityLabel}. ${algorithmDescription}`);
		const label = document.createElement('span');
		label.textContent = algo;
		chip.appendChild(label);
		if (!supported.has(algo)) {
			chip.classList.add('unsupported');
			chip.dataset.unavailable = 'true';
			const mark = document.createElement('span');
			mark.className = 'capability-mark';
			mark.textContent = '×';
			mark.setAttribute('aria-hidden', 'true');
			chip.appendChild(mark);
		}
		if (algo === selectedAlgo) {
			chip.classList.add('selected');
			updateDescription(algo);
		}
		chip.addEventListener('click', () => {
			if (!supported.has(algo)) {
				toast(I18N.t('algo_not_supported', { algo }));
				return;
			}
			if (chip.classList.contains('selected')) return;
			container.querySelectorAll('.algo-chip.selected').forEach(c => c.classList.remove('selected'));
			chip.classList.add('selected');
			updateDescription(algo);
			onClick(algo);
		});
		container.appendChild(chip);
	});
}

function refreshCapabilityLabels() {
	const algorithmCount = I18N.t('capability_available_count', {
		available: router_state.available_algorithms.length,
		total: ALL_ALGOS.length,
	});
	for (const id of ['wifi-capability-count', 'cell-capability-count']) {
		const count = document.getElementById(id);
		if (count) count.textContent = algorithmCount;
	}
	document.querySelectorAll('[data-algo].algo-chip').forEach(chip => {
		const stateKey = chip.classList.contains('unsupported') ? 'capability_unsupported' : 'capability_supported';
		chip.title = `${getAlgorithmDescription(chip.dataset.algo, I18N.currentLang)} · ${I18N.t(stateKey)}`;
		chip.setAttribute('aria-label', `${chip.dataset.algo}: ${I18N.t(stateKey)}. ${getAlgorithmDescription(chip.dataset.algo, I18N.currentLang)}`);
	});
	for (const [id, algo] of [
		['wifi-algo-description', router_state.settingsPageParams.wlanAlgo],
		['cell-algo-description', router_state.settingsPageParams.rmnetAlgo],
	]) {
		const description = document.getElementById(id);
		if (description && algo) description.textContent = getAlgorithmDescription(algo, I18N.currentLang);
	}

	const supportedQdiscs = router_state.qdiscCapabilities.filter(item => item.state === 'supported').length;
	const qdiscCount = document.getElementById('qdisc-capability-count');
	if (qdiscCount) qdiscCount.textContent = I18N.t('capability_available_count', {
		available: supportedQdiscs,
		total: ALL_QDISCS.length,
	});
	document.querySelectorAll('[data-qdisc].algo-chip').forEach(chip => {
		const state = chip.dataset.capability || 'unknown';
		const stateKey = state === 'supported' ? 'capability_supported'
			: state === 'unsupported' ? 'capability_unsupported' : 'capability_unknown';
		chip.title = `${getQdiscDescription(chip.dataset.qdisc, I18N.currentLang)} · ${I18N.t(stateKey)}`;
		chip.setAttribute('aria-label', `${chip.dataset.qdisc}: ${I18N.t(stateKey)}. ${getQdiscDescription(chip.dataset.qdisc, I18N.currentLang)}`);
	});
	const qdiscDescription = document.getElementById('qdisc-description');
	const activeQdisc = document.querySelector('[data-qdisc].algo-chip.selected')?.dataset.qdisc;
	if (qdiscDescription && activeQdisc) qdiscDescription.textContent = getQdiscDescription(activeQdisc, I18N.currentLang);
}

function algorithmMarkerCommand(dir, prefix, algorithm) {
	const selected = shellQuote(`${dir}/${prefix}_${algorithm}`);
	return `touch ${selected} && for f in ${shellQuote(dir)}/${prefix}_*; do [ "$f" = ${selected} ] || rm -f "$f"; done`;
}

function initThemeSettings() {
	const savedMode = localStorage.getItem('tcp_themeMode') || 'auto';

	const modeButtons = document.querySelectorAll('.theme-mode-btn');
	modeButtons.forEach(btn => {
		btn.classList.toggle('selected', btn.dataset.mode === savedMode);
		btn.addEventListener('click', () => {
			const mode = btn.dataset.mode;
			setThemeMode(mode);
			modeButtons.forEach(b => b.classList.toggle('selected', b.dataset.mode === mode));
		});
	});

	const dynamicColorToggle = document.getElementById('dynamic-color-toggle');
	if (dynamicColorToggle) {
		dynamicColorToggle.addEventListener('change', async () => {
			dynamicColorToggle.disabled = true;
			await setDynamicColorEnabled(dynamicColorToggle.checked);
			dynamicColorToggle.disabled = false;
		});
	}
}

export async function initSettings() {
	const killConnections = document.getElementById('kill-connections');
	const initcwndInitrwnd = document.getElementById('initcwnd-initrwnd');
	const applyBtn = document.getElementById('apply-btn');
	const forceApplyBtn = document.getElementById('force-apply-btn');

	initThemeSettings();

	// Language selector
	const savedLang = localStorage.getItem('tcp_lang') || 'en';
	document.querySelectorAll('.lang-btn').forEach(btn => {
		btn.classList.toggle('selected', btn.dataset.lang === savedLang);
		btn.addEventListener('click', async () => {
			const lang = btn.dataset.lang;
			await I18N.switchTo(lang);
			refreshCapabilityLabels();
			document.querySelectorAll('.lang-btn').forEach(b => b.classList.remove('selected'));
			btn.classList.add('selected');
		});
	});

	if (router_state.available_algorithms.length === 0) await fetchAvailableAlgorithms();
	if (router_state.settingsPageParams.killConnections == null)
		router_state.settingsPageParams.killConnections = await fetchIsConfigFile("kill_connections");
	if (router_state.settingsPageParams.initcwndInitrwnd == null)
		router_state.settingsPageParams.initcwndInitrwnd = await fetchIsConfigFile("initcwnd_initrwnd");

	killConnections.checked = router_state.settingsPageParams.killConnections;
	initcwndInitrwnd.checked = router_state.settingsPageParams.initcwndInitrwnd;

	// Build algo chip selectors
	const wlanAlgo = await checkAndGetPrefixValueExists("wlan");
	const rmnetAlgo = await checkAndGetPrefixValueExists("rmnet_data");

	buildAlgoChips('wifi-algo-chips', wlanAlgo || 'cubic', (algo) => {
		router_state.settingsPageParams.wlanAlgo = algo;
	});

	buildAlgoChips('cell-algo-chips', rmnetAlgo || 'cubic', (algo) => {
		router_state.settingsPageParams.rmnetAlgo = algo;
	});

	async function applySettings() {
		const dir = router_state.moduleInformation.moduleDir;
		const wifiAlgorithm = router_state.settingsPageParams.wlanAlgo || 'cubic';
		const cellAlgorithm = router_state.settingsPageParams.rmnetAlgo || 'cubic';
		const settings = {
			wifiAlgorithm,
			cellularAlgorithm: cellAlgorithm,
			killOnChange: killConnections.checked,
			setInitcwndInitrwndOnChange: initcwndInitrwnd.checked,
		};

		applyBtn.disabled = forceApplyBtn.disabled = true;
		applyBtn.textContent = I18N.t('settings_applying');

		try {
			const validAlgos = new Set(router_state.available_algorithms);
			if (!validAlgos.has(settings.wifiAlgorithm) || !validAlgos.has(settings.cellularAlgorithm)) {
				toast(I18N.t('toast_invalid_algo'));
				return false;
			}
			const killCommand = settings.killOnChange
				? `touch ${shellQuote(`${dir}/kill_connections`)}`
				: `rm -f ${shellQuote(`${dir}/kill_connections`)}`;
			const initCommand = settings.setInitcwndInitrwndOnChange
				? `touch ${shellQuote(`${dir}/initcwnd_initrwnd`)}`
				: `rm -f ${shellQuote(`${dir}/initcwnd_initrwnd`)}`;

			await exec(
				algorithmMarkerCommand(dir, 'wlan', settings.wifiAlgorithm) +
				` && ${algorithmMarkerCommand(dir, 'rmnet_data', settings.cellularAlgorithm)}` +
				` && rm -f ${shellQuote(`${dir}/pacing_ca`)} ${shellQuote(`${dir}/pacing_ss`)}` +
				` && ${killCommand} && ${initCommand}`
			);

			router_state.settingsPageParams.killConnections = settings.killOnChange;
			router_state.settingsPageParams.initcwndInitrwnd = settings.setInitcwndInitrwndOnChange;
			await addLog(`Settings: WiFi=${settings.wifiAlgorithm}, Cellular=${settings.cellularAlgorithm}`);
			toast(I18N.t('toast_settings_applied'));
			haptic('success');
			return true;
		} catch (error) {
			console.error('Error applying settings:', error);
			toast(I18N.t('toast_error'));
			haptic('error');
			return false;
		} finally {
			applyBtn.disabled = forceApplyBtn.disabled = false;
			applyBtn.textContent = I18N.t('settings_apply_btn');
		}
	}

	applyBtn.addEventListener('click', async () => {
		if (await applySettings()) toast(I18N.t('toast_toggle_connection'));
	});

	forceApplyBtn.addEventListener('click', async () => {
		if (!await applySettings()) return;
		const dir = router_state.moduleInformation.moduleDir;
		try {
			await exec(`touch ${shellQuote(`${dir}/force_apply`)} && chmod 644 ${shellQuote(`${dir}/force_apply`)}`);
			toast(I18N.t('toast_wait_5s'));
		} catch (error) {
			console.error('Error forcing settings apply:', error);
			toast(I18N.t('toast_error'));
		}
	});

	// Qdisc selector
	const currentQdisc = await getDefaultQdisc();
	const qdiscCapabilities = await getQdiscCapabilities();
	router_state.qdiscCapabilities = qdiscCapabilities;
	const qdiscContainer = document.getElementById('qdisc-chips');
	if (qdiscContainer) {
		const supportedCount = qdiscCapabilities.filter(item => item.state === 'supported').length;
		const qdiscCount = document.getElementById('qdisc-capability-count');
		if (qdiscCount) qdiscCount.textContent = I18N.t('capability_available_count', {
			available: supportedCount,
			total: ALL_QDISCS.length,
		});
		qdiscContainer.innerHTML = '';
		ALL_QDISCS.forEach(q => {
			const state = qdiscCapabilities.find(item => item.name === q)?.state || 'unknown';
			const chip = document.createElement('button');
			chip.className = 'algo-chip';
			chip.dataset.qdisc = q;
			chip.dataset.capability = state;
			const stateLabel = I18N.t(state === 'supported' ? 'capability_supported' : state === 'unsupported' ? 'capability_unsupported' : 'capability_unknown');
			chip.setAttribute('aria-label', `${q}: ${stateLabel}. ${getQdiscDescription(q, I18N.currentLang)}`);
			const label = document.createElement('span');
			label.textContent = q;
			chip.appendChild(label);
			chip.title = `${getQdiscDescription(q, I18N.currentLang)} · ${I18N.t(state === 'supported' ? 'capability_supported' : state === 'unsupported' ? 'capability_unsupported' : 'capability_unknown')}`;
			if (state !== 'supported') {
				chip.classList.add(state === 'unsupported' ? 'unsupported' : 'capability-unknown');
				chip.dataset.unavailable = 'true';
				const mark = document.createElement('span');
				mark.className = 'capability-mark';
				mark.textContent = state === 'unsupported' ? '×' : '?';
				mark.setAttribute('aria-hidden', 'true');
				chip.appendChild(mark);
			}
			if (q === currentQdisc) chip.classList.add('selected');
			chip.addEventListener('click', async () => {
				if (state !== 'supported') {
					toast(`${q}: ${getQdiscDescription(q, I18N.currentLang)}`);
					return;
				}
				if (chip.classList.contains('selected')) return;
				const ok = await setDefaultQdisc(q);
				if (ok) {
					qdiscContainer.querySelectorAll('.algo-chip.selected').forEach(c => c.classList.remove('selected'));
					chip.classList.add('selected');
					const description = document.getElementById('qdisc-description');
					if (description) description.textContent = getQdiscDescription(q, I18N.currentLang);
					addLog(`Global qdisc changed: ${q}`);
					toast(I18N.t('toast_qdisc_set', { qdisc: q }));
					haptic('success');
				} else {
					toast(I18N.t('toast_qdisc_fail', { qdisc: q }));
					haptic('error');
				}
			});
			qdiscContainer.appendChild(chip);
		});
		const description = document.getElementById('qdisc-description');
		if (description && currentQdisc) description.textContent = getQdiscDescription(currentQdisc, I18N.currentLang);
	}

	// Preset management
	initPresets();
	// Debug capture is global, so initialise its persisted state even when the
	// advanced page is disabled and never opened.
	await initDebugToggle();

	// Advanced kernel toggle
	initAdvancedToggle();
	if (isAdvancedEnabled()) await initAdvancedKnobs();

}

function isAdvancedEnabled() {
	return localStorage.getItem('tcp_adv_enabled') === 'true';
}

function setAdvancedNavVisible(v) {
	const nav = document.getElementById('nav-adv');
	const advPage = document.getElementById('adv-page');
	if (nav) nav.classList.toggle('hidden', !v);
	if (advPage && !v) advPage.hidden = true;
	reorderNav(v);
}

export function syncAdvancedNavVisibility() {
	setAdvancedNavVisible(isAdvancedEnabled());
}

function reorderNav(advEnabled) {
	const bar = document.getElementById('nav-bar');
	if (!bar) return;
	const items = { home: null, stats: null, settings: null, logs: null, adv: null };
	bar.querySelectorAll('.nav-item').forEach(n => { items[n.dataset.page] = n; });

	// Clear bar
	while (bar.firstChild) bar.removeChild(bar.firstChild);

	// Keep the primary navigation stable when the optional Advanced tab appears.
	if (items.home) bar.appendChild(items.home);
	if (items.stats) bar.appendChild(items.stats);
	if (items.settings) bar.appendChild(items.settings);
	if (items.logs) bar.appendChild(items.logs);
	if (items.adv) {
		items.adv.classList.toggle('hidden', !advEnabled);
		bar.appendChild(items.adv);
	}
}

function initAdvancedToggle() {
	const toggle = document.getElementById('adv-toggle');
	const enabled = isAdvancedEnabled();
	if (toggle) toggle.checked = enabled;
	setAdvancedNavVisible(enabled);
	reorderNav(enabled);

	toggle?.addEventListener('change', () => {
		if (toggle.checked) {
			showAdvancedWarning();
		} else {
			disableAdvanced();
		}
	});
}

function showAdvancedWarning() {
	const overlay = document.getElementById('adv-warning-overlay');
	const countdownEl = document.getElementById('adv-countdown');
	const confirmBtn = document.getElementById('adv-confirm-btn');
	const cancelBtn = document.getElementById('adv-cancel-btn');
	const toggle = document.getElementById('adv-toggle');

	overlay.hidden = false;
	confirmBtn.disabled = true;
	let seconds = 5;
	countdownEl.textContent = seconds;
	confirmBtn.textContent = I18N.t('adv_confirm', { s: seconds });

	const timer = setInterval(() => {
		seconds--;
		countdownEl.textContent = seconds;
		confirmBtn.textContent = I18N.t('adv_confirm', { s: seconds });
		if (seconds <= 0) {
			clearInterval(timer);
			confirmBtn.disabled = false;
			confirmBtn.textContent = I18N.t('adv_confirm_done');
		}
	}, 1000);

	const cleanup = () => {
		clearInterval(timer);
		overlay.hidden = true;
		confirmBtn.removeEventListener('click', onConfirm);
		cancelBtn.removeEventListener('click', onCancel);
	};

	function onConfirm() {
		if (confirmBtn.disabled) return;
		cleanup();
		localStorage.setItem('tcp_adv_enabled', 'true');
		setAdvancedNavVisible(true);
		reorderNav(true);
		void initAdvancedKnobs();
		toast(I18N.t('toast_adv_enabled'));
	}

	function onCancel() {
		cleanup();
		if (toggle) toggle.checked = false;
	}

	confirmBtn.addEventListener('click', onConfirm);
	cancelBtn.addEventListener('click', onCancel);
}

function disableAdvanced() {
	localStorage.setItem('tcp_adv_enabled', 'false');
	setAdvancedNavVisible(false);
	reorderNav(false);
	if (router_state.current_active_page === 'adv') {
		document.querySelector('.nav-item[data-page="settings"]')?.click();
	}
}

async function initAdvancedKnobs() {
	if (advancedInitialized) return;
	advancedInitialized = true;
	try {
		const entries = ADVANCED_SYSCTLS.map(item => shellQuote(`${item.key}|${item.path}`)).join(' ');
		const { stdout } = await exec(`# advanced-sysctl-probe
for entry in ${entries}; do
	key=\${entry%%|*}; path=\${entry#*|}
	if [ -r "$path" ]; then printf '%s=' "$key"; cat "$path" 2>/dev/null || printf '__UNAVAILABLE__\\n'
	else printf '%s=__UNSUPPORTED__\\n' "$key"
	fi
done`);
		const values = parseKeyValueOutput(stdout);
		renderAdvancedControls(values);
		bindAdvancedApply();
	} catch (error) {
		console.error('Failed to probe advanced sysctls:', error);
		const container = document.getElementById('advanced-sysctl-groups');
		if (container) container.innerHTML = `<div class="settings-card"><span class="stat-dim">${I18N.t('home_status_unknown')}</span></div>`;
	} finally {
		initBasebandBackup();
	}
}

const ADVANCED_SYSCTLS = [
	{ group: 'lifecycle', key: 'tcp_keepalive_time', path: '/proc/sys/net/ipv4/tcp_keepalive_time', min: 10, max: 7200, step: 10 },
	{ group: 'lifecycle', key: 'tcp_keepalive_intvl', path: '/proc/sys/net/ipv4/tcp_keepalive_intvl', min: 1, max: 300, step: 1 },
	{ group: 'lifecycle', key: 'tcp_keepalive_probes', path: '/proc/sys/net/ipv4/tcp_keepalive_probes', min: 1, max: 30, step: 1 },
	{ group: 'lifecycle', key: 'tcp_fin_timeout', path: '/proc/sys/net/ipv4/tcp_fin_timeout', min: 5, max: 120, step: 1 },
	{ group: 'lifecycle', key: 'tcp_syn_retries', path: '/proc/sys/net/ipv4/tcp_syn_retries', min: 1, max: 10, step: 1 },
	{ group: 'lifecycle', key: 'tcp_synack_retries', path: '/proc/sys/net/ipv4/tcp_synack_retries', min: 1, max: 10, step: 1 },
	{ group: 'lifecycle', key: 'tcp_retries2', path: '/proc/sys/net/ipv4/tcp_retries2', min: 3, max: 20, step: 1 },
	{ group: 'memory', key: 'rmem_max', path: '/proc/sys/net/core/rmem_max', min: 65536, max: 134217728, step: 65536 },
	{ group: 'memory', key: 'wmem_max', path: '/proc/sys/net/core/wmem_max', min: 65536, max: 134217728, step: 65536 },
	{ group: 'memory', key: 'optmem_max', path: '/proc/sys/net/core/optmem_max', min: 10240, max: 4194304, step: 1024 },
	{ group: 'memory', key: 'tcp_notsent_lowat', path: '/proc/sys/net/ipv4/tcp_notsent_lowat', min: 0, max: 4294967295, step: 4096 },
	{ group: 'queue', key: 'somaxconn', path: '/proc/sys/net/core/somaxconn', min: 128, max: 65535, step: 128 },
	{ group: 'queue', key: 'netdev_max_backlog', path: '/proc/sys/net/core/netdev_max_backlog', min: 256, max: 65535, step: 256 },
	{ group: 'queue', key: 'tcp_max_syn_backlog', path: '/proc/sys/net/ipv4/tcp_max_syn_backlog', min: 128, max: 65535, step: 128 },
	{ group: 'queue', key: 'netdev_budget', path: '/proc/sys/net/core/netdev_budget', min: 64, max: 4096, step: 64 },
	{ group: 'queue', key: 'netdev_budget_usecs', path: '/proc/sys/net/core/netdev_budget_usecs', min: 500, max: 50000, step: 500 },
	{ group: 'recovery', key: 'tcp_mtu_probing', path: '/proc/sys/net/ipv4/tcp_mtu_probing', min: 0, max: 2, step: 1 },
	{ group: 'recovery', key: 'tcp_sack', path: '/proc/sys/net/ipv4/tcp_sack', min: 0, max: 1, step: 1, boolean: true },
	{ group: 'recovery', key: 'tcp_dsack', path: '/proc/sys/net/ipv4/tcp_dsack', min: 0, max: 1, step: 1, boolean: true },
	{ group: 'recovery', key: 'tcp_ecn', path: '/proc/sys/net/ipv4/tcp_ecn', min: 0, max: 2, step: 1 },
	{ group: 'recovery', key: 'tcp_no_metrics_save', path: '/proc/sys/net/ipv4/tcp_no_metrics_save', min: 0, max: 1, step: 1, boolean: true },
	{ group: 'recovery', key: 'tcp_slow_start_after_idle', path: '/proc/sys/net/ipv4/tcp_slow_start_after_idle', min: 0, max: 1, step: 1, boolean: true },
	{ group: 'recovery', key: 'tcp_fastopen', path: '/proc/sys/net/ipv4/tcp_fastopen', min: 0, max: 3, step: 1 },
	{ group: 'recovery', key: 'tcp_tw_reuse', path: '/proc/sys/net/ipv4/tcp_tw_reuse', min: 0, max: 2, step: 1 },
	{ group: 'recovery', key: 'tcp_autocorking', path: '/proc/sys/net/ipv4/tcp_autocorking', min: 0, max: 1, step: 1, boolean: true },
	{ group: 'recovery', key: 'tcp_early_retrans', path: '/proc/sys/net/ipv4/tcp_early_retrans', min: 0, max: 4, step: 1 },
	{ group: 'recovery', key: 'tcp_thin_linear_timeouts', path: '/proc/sys/net/ipv4/tcp_thin_linear_timeouts', min: 0, max: 1, step: 1, boolean: true },
	{ group: 'recovery', key: 'tcp_thin_dupack', path: '/proc/sys/net/ipv4/tcp_thin_dupack', min: 0, max: 1, step: 1, boolean: true },
	{ group: 'recovery', key: 'tcp_rto_max_ms', path: '/proc/sys/net/ipv4/tcp_rto_max_ms', min: 1000, max: 120000, step: 1000 },
	{ group: 'plb', key: 'tcp_plb_enabled', path: '/proc/sys/net/ipv4/tcp_plb_enabled', min: 0, max: 1, step: 1, boolean: true },
	{ group: 'plb', key: 'tcp_plb_idle_rehash_rounds', path: '/proc/sys/net/ipv4/tcp_plb_idle_rehash_rounds', min: 0, max: 31, step: 1 },
	{ group: 'plb', key: 'tcp_plb_rehash_rounds', path: '/proc/sys/net/ipv4/tcp_plb_rehash_rounds', min: 0, max: 31, step: 1 },
	{ group: 'plb', key: 'tcp_plb_suspend_rto_sec', path: '/proc/sys/net/ipv4/tcp_plb_suspend_rto_sec', min: 0, max: 255, step: 1 },
	{ group: 'plb', key: 'tcp_plb_cong_thresh', path: '/proc/sys/net/ipv4/tcp_plb_cong_thresh', min: 0, max: 256, step: 1 },
	{ group: 'latency', key: 'busy_poll', path: '/proc/sys/net/core/busy_poll', min: 0, max: 100000, step: 50 },
	{ group: 'latency', key: 'busy_read', path: '/proc/sys/net/core/busy_read', min: 0, max: 100000, step: 50 },
	{ group: 'conntrack', key: 'nf_conntrack_max', path: '/proc/sys/net/netfilter/nf_conntrack_max', min: 1024, max: 1048576, step: 1024 },
	{ group: 'conntrack', key: 'nf_conntrack_tcp_timeout_established', path: '/proc/sys/net/netfilter/nf_conntrack_tcp_timeout_established', min: 60, max: 432000, step: 60 },
	{ group: 'conntrack', key: 'nf_conntrack_tcp_timeout_time_wait', path: '/proc/sys/net/netfilter/nf_conntrack_tcp_timeout_time_wait', min: 1, max: 600, step: 1 },
];

function parseKeyValueOutput(output) {
	const values = new Map();
	for (const line of output.split('\n')) {
		const separator = line.indexOf('=');
		if (separator > 0) values.set(line.slice(0, separator), line.slice(separator + 1).trim());
	}
	return values;
}

function renderAdvancedControls(values) {
	const container = document.getElementById('advanced-sysctl-groups');
	if (!container) return;
	container.replaceChildren();
	const groups = ['lifecycle', 'memory', 'queue', 'recovery', 'plb', 'latency', 'conntrack'];
	let supported = 0;
	for (const group of groups) {
		const items = ADVANCED_SYSCTLS.filter(item => item.group === group);
		const available = items.filter(item => /^\d+$/.test(values.get(item.key) || '')).length;
		supported += available;
		const details = document.createElement('details');
		details.className = 'settings-group';
		details.open = group === 'lifecycle';
		details.innerHTML = `<summary class="settings-group__summary"><span class="settings-group__icon ui-icon icon-sliders" aria-hidden="true"></span><span class="settings-group__copy"><strong>${I18N.t(`advanced_group_${group}`)}</strong><small>${I18N.t('advanced_group_count', { supported: available, total: items.length })}</small></span><span class="collapsible-chevron ui-icon icon-chevron-down" aria-hidden="true"></span></summary>`;
		const body = document.createElement('div');
		body.className = 'settings-group__body';
		const grid = document.createElement('div');
		grid.className = 'knob-row';
		for (const item of items) grid.appendChild(createAdvancedControl(item, values.get(item.key)));
		body.appendChild(grid);
		details.appendChild(body);
		container.appendChild(details);
	}
	const count = document.getElementById('advanced-supported-count');
	if (count) count.textContent = I18N.t('advanced_supported_count', { supported, total: ADVANCED_SYSCTLS.length });
}

function createAdvancedControl(item, rawValue) {
	const supported = /^\d+$/.test(rawValue || '');
	const wrapper = document.createElement('div');
	wrapper.className = 'knob advanced-control';
	wrapper.dataset.supported = supported ? 'true' : 'false';
	const label = document.createElement('label');
	label.htmlFor = `adv-${item.key}`;
	label.textContent = I18N.t(`advanced_${item.key}`);
	const hint = document.createElement('small');
	hint.className = 'advanced-control__hint';
	hint.textContent = supported ? item.key : I18N.t('advanced_unsupported');
	wrapper.append(label, hint);
	const input = document.createElement('input');
	input.id = `adv-${item.key}`;
	input.dataset.sysctlKey = item.key;
	input.disabled = !supported;
	if (item.boolean) {
		input.type = 'checkbox';
		input.checked = rawValue === '1';
		input.dataset.initialValue = input.checked ? '1' : '0';
		input.setAttribute('aria-label', label.textContent);
		const toggle = document.createElement('label');
		toggle.className = 'toggle advanced-control__toggle';
		const slider = document.createElement('span');
		slider.className = 'slider';
		toggle.append(input, slider);
		wrapper.appendChild(toggle);
	} else {
		input.type = 'number';
		input.min = item.min;
		input.max = item.max;
		input.step = item.step;
		input.value = supported ? rawValue : '';
		input.dataset.initialValue = supported ? rawValue : '';
		wrapper.appendChild(input);
	}
	return wrapper;
}

function bindAdvancedApply() {
	document.getElementById('apply-advanced-btn')?.addEventListener('click', applyAdvancedSettings);
}

async function applyAdvancedSettings() {
	const btn = document.getElementById('apply-advanced-btn');
	btn.disabled = true;
	btn.textContent = I18N.t('settings_applying');
	try {
		const values = [];
		for (const item of ADVANCED_SYSCTLS) {
			const input = document.getElementById(`adv-${item.key}`);
			if (!input || input.disabled) continue;
			const value = item.boolean ? (input.checked ? 1 : 0) : Number.parseInt(input.value, 10);
			if (!Number.isInteger(value) || value < item.min || value > item.max) throw new Error(`Invalid value for ${item.key}`);
			values.push({ item, input, value });
		}
		if (values.length === 0) throw new Error('No supported sysctls');
		const dir = router_state.moduleInformation.moduleDir;
		const writes = values.map(({ item, value }) => `printf '%s\\n' ${value} > ${shellQuote(item.path)} && actual=$(cat ${shellQuote(item.path)} 2>/dev/null) && [ "$actual" = "${value}" ] || exit 1; printf '${item.key}=%s\\n' "$actual"`).join('\n');
		const dedicatedKeys = new Set(['tcp_ecn', 'tcp_fastopen']);
		const config = values.filter(({ item }) => !dedicatedKeys.has(item.key)).map(({ item, value }) => `printf '%s\\n' ${shellQuote(`${item.key}=${value}`)}`).join('\n');
		const dedicatedWrites = values.filter(({ item }) => dedicatedKeys.has(item.key)).map(({ item, value }) => `printf '%s\\n' ${value} > ${shellQuote(`${dir}/${item.key}`)}`).join(' && ');
		const configPath = shellQuote(`${dir}/advanced.conf`);
		const { stdout } = await exec(`# advanced-sysctl-apply
${writes}
{ ${config}; } > ${configPath}.tmp && mv ${configPath}.tmp ${configPath}${dedicatedWrites ? ` && ${dedicatedWrites}` : ''} && touch ${shellQuote(`${dir}/force_apply`)}`);
		const verified = parseKeyValueOutput(stdout);
		for (const { item, input, value } of values) {
			if (verified.get(item.key) !== String(value)) throw new Error(`Readback failed for ${item.key}`);
			input.dataset.initialValue = String(value);
		}
		addLog(`Advanced kernel controls applied: ${values.length} verified values`);
		toast(I18N.t('toast_advanced_applied_count', { count: values.length }));
	} catch (error) {
		console.error('Failed to apply advanced settings:', error);
		toast(I18N.t('toast_error'));
	} finally {
		btn.disabled = false;
		btn.textContent = I18N.t('settings_apply_advanced');
	}
}

async function initDebugToggle() {
	const toggle = document.getElementById('debug-toggle');
	if (!toggle) return;

	const dir = router_state.moduleInformation.moduleDir;
	const enabled = await fetchIsConfigFile('debug_mode');
	localStorage.setItem('tcp_debug_enabled', enabled ? 'true' : 'false');
	toggle.checked = enabled;
	setDebugMode(enabled);

	toggle.addEventListener('change', async () => {
		const on = toggle.checked;
		try {
			await exec(on ? `touch ${shellQuote(`${dir}/debug_mode`)}` : `rm -f ${shellQuote(`${dir}/debug_mode`)}`);
		} catch (error) {
			toggle.checked = !on;
			toast(I18N.t('toast_error'));
			return;
		}
		localStorage.setItem('tcp_debug_enabled', on ? 'true' : 'false');
		setDebugMode(on);
		toast(on ? I18N.t('debug_on') : I18N.t('debug_off'));
	});
}

function setDebugMode(on) {
	const fab = document.getElementById('debug-toggle-btn');
	if (fab) fab.style.display = on ? '' : 'none';
	if (!window._debug) return;
	window._debug.enabled = on;
	if (!on) {
		document.getElementById('debug-overlay').hidden = true;
		window._debug.visible = false;
		window._debug.entries = [];
	}
}

async function initBasebandBackup() {
	const MODEM_PARTS = ['modem', 'modemst1', 'modemst2', 'fsg', 'fsc', 'nvdata', 'nvram', 'radio', 'persist', 'dsp', 'mdtp', 'mcfg'];
	const listEl = document.getElementById('bb-partition-list');
	const statusEl = document.getElementById('bb-status');
	const backupBtn = document.getElementById('bb-backup-btn');
	const pathInput = document.getElementById('bb-path-input');
	const browseBtn = document.getElementById('bb-browse-btn');

	let partitionInfos = [];

	// Detect partitions
	try {
		const { stdout } = await exec('ls -1 /dev/block/by-name/ 2>/dev/null');
		const allParts = stdout.split('\n').filter(Boolean);
		const modemHits = allParts.filter(n => MODEM_PARTS.includes(n));

		for (const name of modemHits) {
			try {
				const { stdout: sizeOut } = await exec(`blockdev --getsize64 /dev/block/by-name/${name} 2>/dev/null`);
				const bytes = parseInt(sizeOut.trim());
				partitionInfos.push({ name, bytes: bytes || 0 });
			} catch (e) {
				partitionInfos.push({ name, bytes: 0 });
			}
		}

		// Also try /dev/block/bootdevice/by-name/ for Qualcomm
		if (partitionInfos.length === 0) {
			const { stdout: qcom } = await exec('ls -1 /dev/block/bootdevice/by-name/ 2>/dev/null');
			const qcomParts = qcom.split('\n').filter(Boolean);
			const qcomHits = qcomParts.filter(n => MODEM_PARTS.includes(n));
			for (const name of qcomHits) {
				try {
					const { stdout: sizeOut } = await exec(`blockdev --getsize64 /dev/block/bootdevice/by-name/${name} 2>/dev/null`);
					partitionInfos.push({ name, bytes: parseInt(sizeOut.trim()) || 0 });
				} catch (e) {
					partitionInfos.push({ name, bytes: 0 });
				}
			}
		}
	} catch (e) { /* no partitions found */ }

	if (listEl) {
		if (partitionInfos.length === 0) {
			listEl.innerHTML = `<span class="stat-dim">${I18N.t('bb_none_found')}</span>`;
		} else {
			listEl.innerHTML = partitionInfos.map((p, i) => {
				const size = p.bytes > 0
					? p.bytes >= 1048576 ? `${(p.bytes / 1048576).toFixed(1)} MB` : `${(p.bytes / 1024).toFixed(1)} KB`
					: '?';
				return `<button class="bb-partition-chip" data-idx="${i}" title="${p.name} (${size})">
					<span>${p.name.toUpperCase()}</span>
					<span class="bb-size">${size}</span>
				</button>`;
			}).join('');

			listEl.querySelectorAll('.bb-partition-chip').forEach(chip => {
				chip.addEventListener('click', () => {
					chip.classList.toggle('checked');
				});
			});
		}
	}

	// Default path
	if (pathInput) {
		const saved = localStorage.getItem('tcp_bb_path');
		pathInput.value = saved || '/sdcard/Download/baseband_backup';
	}

	// Browse button — use a simple input or system file picker
	if (browseBtn) {
		browseBtn.addEventListener('click', async () => {
			// Use Android file manager via am start to pick directory
			// Simpler: show a prompt
			const p = prompt(I18N.t('bb_path_prompt'), pathInput?.value || '/sdcard/Download/baseband_backup');
			if (p && pathInput) {
				pathInput.value = p;
				localStorage.setItem('tcp_bb_path', p);
			}
		});
	}

	if (pathInput) {
		pathInput.addEventListener('change', () => {
			localStorage.setItem('tcp_bb_path', pathInput.value);
		});
	}

	if (backupBtn) {
		backupBtn.addEventListener('click', async () => {
			const chips = listEl?.querySelectorAll('.bb-partition-chip.checked');
			if (!chips || chips.length === 0) {
				toast(I18N.t('bb_select_warn'));
				return;
			}

			const path = normalizeBackupPath(pathInput?.value || '/sdcard/Download/baseband_backup');
			if (!path) {
				toast(I18N.t('toast_error'));
				return;
			}
			const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
			const dir = `${path}/${stamp}`;

			backupBtn.disabled = true;
			backupBtn.textContent = I18N.t('bb_backing_up');
			if (statusEl) { statusEl.hidden = false; statusEl.className = 'bb-status'; statusEl.textContent = ''; }

			const results = [];
			const completed = [];
			try {
				await exec(`mkdir -p ${shellQuote(dir)}`);
				for (const chip of chips) {
					const idx = Number.parseInt(chip.dataset.idx, 10);
					const p = partitionInfos[idx];
					if (!p || !MODEM_PARTS.includes(p.name)) continue;
					const outFile = `${dir}/${p.name}.img`;
					results.push(`  Dumping ${p.name.toUpperCase()}...`);

					try {
						const src = await findPartitionPath(p.name);
						await exec(`dd if=${shellQuote(src)} of=${shellQuote(outFile)} bs=4M`);
						const { stdout: savedSize } = await exec(`stat -c %s ${shellQuote(outFile)}`);
						const actual = Number.parseInt(savedSize.trim(), 10);
						if (!Number.isFinite(actual) || actual <= 0 || (p.bytes > 0 && actual !== p.bytes)) {
							throw new Error(`size mismatch (${actual || 0}/${p.bytes || '?'})`);
						}
						completed.push({ partition: p, source: src });
						results.push(`    \u2713 OK`);
					} catch (error) {
						results.push(`    \u2717 Failed: ${error.message || error}`);
					}
				}

				if (completed.length > 0) {
					const restoreSh = `${dir}/restore.sh`;
					const scriptLines = ['#!/system/bin/sh', '# Review device and partition paths before running.', 'set -eu', ''];
					for (const { partition, source } of completed) {
						scriptLines.push(`dd if=${shellQuote(`${dir}/${partition.name}.img`)} of=${shellQuote(source)} bs=4M`);
					}
					await exec(`printf '%s\n' ${shellQuote(scriptLines.join('\n'))} > ${shellQuote(restoreSh)} && chmod 700 ${shellQuote(restoreSh)}`);
					results.push(`  restore.sh \u2713 created`);
				}

				if (statusEl) {
					statusEl.className = completed.length === chips.length ? 'bb-status success' : 'bb-status';
					statusEl.textContent = `${I18N.t('bb_complete')}\n${dir}\n\n` + results.join('\n');
				}
				toast(completed.length > 0 ? I18N.t('bb_toast_done') : I18N.t('toast_error'));
			} catch (error) {
				console.error('Baseband backup failed:', error);
				if (statusEl) statusEl.textContent = `${I18N.t('toast_error')}: ${error.message || error}`;
				toast(I18N.t('toast_error'));
			} finally {
				backupBtn.disabled = false;
				backupBtn.textContent = I18N.t('bb_backup_btn');
			}
		});
	}
}

function normalizeBackupPath(value) {
	const path = String(value).trim().replace(/\/+$/, '');
	if (path.length < 2 || path.length > 180 || /[\0\r\n]/.test(path)) return null;
	if (!['/sdcard/', '/storage/emulated/0/', '/data/media/0/'].some(prefix => path.startsWith(prefix))) return null;
	if (path.split('/').includes('..')) return null;
	return path;
}

async function findPartitionPath(name) {
	if (!/^[A-Za-z0-9_.-]+$/.test(name)) throw new Error('Invalid partition name');
	try {
		const { stdout } = await exec(`readlink -f /dev/block/by-name/${name} 2>/dev/null`);
		if (isSafeBlockPath(stdout.trim())) return stdout.trim();
	} catch (e) {}
	try {
		const { stdout } = await exec(`readlink -f /dev/block/bootdevice/by-name/${name} 2>/dev/null`);
		if (isSafeBlockPath(stdout.trim())) return stdout.trim();
	} catch (e) {}
	return `/dev/block/by-name/${name}`;
}

function isSafeBlockPath(path) {
	return path.startsWith('/dev/block/') && !path.split('/').includes('..') && /^\/[A-Za-z0-9_./:-]+$/.test(path);
}

function getBuiltinPresets() {
	return [
		{ name: 'Balanced', wlanAlgo: 'cubic', cellAlgo: 'cubic', killConnections: false, initcwndInitrwnd: true, qdisc: 'fq_codel', pacing_ca: 150, pacing_ss: 200, tcp_fastopen: 3, tcp_ecn: 1, desc: 'Stable defaults — good for most users' },
		{ name: 'Gaming', wlanAlgo: 'bbr', cellAlgo: 'bbr', killConnections: true, initcwndInitrwnd: true, qdisc: 'fq', pacing_ca: 200, pacing_ss: 300, tcp_fastopen: 3, tcp_ecn: 1, desc: 'Low latency — aggressive BBR + FQ' },
		{ name: 'Streaming', wlanAlgo: 'bbr', cellAlgo: 'cubic', killConnections: false, initcwndInitrwnd: true, qdisc: 'fq_codel', pacing_ca: 180, pacing_ss: 250, tcp_fastopen: 3, tcp_ecn: 1, desc: 'High throughput — BBR for Wi-Fi, cubic for cell' },
		{ name: 'Battery Saver', wlanAlgo: 'vegas', cellAlgo: 'westwood', killConnections: false, initcwndInitrwnd: false, qdisc: 'fq_codel', pacing_ca: 120, pacing_ss: 180, tcp_fastopen: 1, tcp_ecn: 0, desc: 'Power efficient — delay-based, reduced pacing' },
		{ name: 'High-Speed', wlanAlgo: 'bbr3', cellAlgo: 'bbr3', killConnections: true, initcwndInitrwnd: true, qdisc: 'fq', pacing_ca: 220, pacing_ss: 320, tcp_fastopen: 3, tcp_ecn: 1, desc: 'Maximum throughput — BBR v3 experimental' },
	];
}

function getSavedPresets() {
	try {
		const value = JSON.parse(localStorage.getItem('tcp_presets') || '[]');
		return Array.isArray(value) ? value.map(normalizePreset).filter(Boolean) : [];
	} catch (e) { return []; }
}

function savePresets(presets) {
	localStorage.setItem('tcp_presets', JSON.stringify(presets));
}

function initPresets() {
	const builtin = getBuiltinPresets();
	const saved = getSavedPresets();
	const all = [...builtin, ...saved];

	const container = document.getElementById('preset-list');
	if (!container) return;
	container.innerHTML = '';

	all.forEach((preset, idx) => {
		const isBuiltin = idx < builtin.length;
		const qdiscSupported = router_state.qdiscCapabilities
			.some(item => item.name === preset.qdisc && item.state === 'supported');
		const compatible = router_state.available_algorithms.includes(preset.wlanAlgo)
			&& router_state.available_algorithms.includes(preset.cellAlgo)
			&& qdiscSupported;
		const chip = document.createElement('button');
		chip.className = 'preset-chip';
		chip.dataset.name = preset.name;
		chip.title = `${preset.desc || preset.name}${compatible ? '' : ` · ${I18N.t('capability_unsupported')}`}`;
		const dot = document.createElement('span');
		dot.className = 'preset-dot';
		dot.style.background = isBuiltin ? 'var(--md-sys-color-primary)' : 'var(--md-sys-color-tertiary)';
		const label = document.createElement('span');
		label.textContent = preset.name;
		chip.append(dot, label);
		if (compatible) {
			chip.addEventListener('click', async () => {
				await applyPreset(preset);
			});
		} else {
			chip.classList.add('unsupported');
			chip.disabled = true;
			const mark = document.createElement('span');
			mark.className = 'capability-mark';
			mark.textContent = '×';
			mark.setAttribute('aria-hidden', 'true');
			chip.appendChild(mark);
		}
		container.appendChild(chip);
	});

	// Restore active preset marker
	const activeName = localStorage.getItem('tcp_active_preset');
	if (activeName) {
		const el = [...container.querySelectorAll('.preset-chip')].find(chip => chip.dataset.name === activeName);
		if (el) el.classList.add('active');
	}

	// Export button
	const exportBtn = document.getElementById('preset-export-btn');
	if (exportBtn) exportBtn.onclick = async () => {
		const current = await buildCurrentPreset();
		const json = JSON.stringify(current, null, 2);
		const textarea = document.getElementById('preset-json-area');
		if (textarea) {
			textarea.style.display = 'block';
			textarea.value = json;
			textarea.select();
		}
		toast(I18N.t('toast_preset_exported'));
	};

	// Import button
	const importBtn = document.getElementById('preset-import-btn');
	if (importBtn) importBtn.onclick = () => {
		const textarea = document.getElementById('preset-json-area');
		if (!textarea) return;
		if (textarea.style.display === 'none') {
			textarea.style.display = 'block';
			textarea.value = '';
			textarea.focus();
			return;
		}
		try {
			const rawPreset = JSON.parse(textarea.value);
			if (!rawPreset.name) { toast(I18N.t('toast_missing_name')); return; }
			const preset = normalizePreset(rawPreset);
			if (!preset) { toast(I18N.t('toast_invalid_json')); return; }
			const all = getSavedPresets();
			all.push(preset);
			savePresets(all);
			textarea.style.display = 'none';
			initPresets();
			toast(I18N.t('toast_preset_imported', { name: preset.name }));
		} catch (e) {
			toast(I18N.t('toast_invalid_json'));
		}
	};
}

function normalizePreset(value) {
	if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
	const name = typeof value.name === 'string' ? value.name.trim() : '';
	if (!name || name.length > 40 || /[\0\r\n]/.test(name)) return null;
	if (!ALL_ALGOS.includes(value.wlanAlgo) || !ALL_ALGOS.includes(value.cellAlgo)) return null;
	if (!ALL_QDISCS.includes(value.qdisc)) return null;
	const pacingCa = value.pacing_ca == null ? null : Number(value.pacing_ca);
	const pacingSs = value.pacing_ss == null ? null : Number(value.pacing_ss);
	const fastOpen = Number(value.tcp_fastopen);
	const ecn = Number(value.tcp_ecn);
	if ((pacingCa == null) !== (pacingSs == null)) return null;
	if (![pacingCa, pacingSs].filter(number => number != null).every(number => Number.isInteger(number) && number >= 1 && number <= 1000)) return null;
	if (!Number.isInteger(fastOpen) || fastOpen < 0 || fastOpen > 3) return null;
	if (!Number.isInteger(ecn) || ecn < 0 || ecn > 2) return null;
	return {
		name,
		wlanAlgo: value.wlanAlgo,
		cellAlgo: value.cellAlgo,
		killConnections: value.killConnections === true,
		initcwndInitrwnd: value.initcwndInitrwnd === true,
		qdisc: value.qdisc,
		pacing_ca: pacingCa,
		pacing_ss: pacingSs,
		tcp_fastopen: fastOpen,
		tcp_ecn: ecn,
		desc: typeof value.desc === 'string' ? value.desc.slice(0, 160) : '',
	};
}

async function readInteger(command, fallback, min, max) {
	try {
		const { stdout } = await exec(command);
		const value = Number.parseInt(stdout.trim(), 10);
		return Number.isInteger(value) && value >= min && value <= max ? value : fallback;
	} catch (error) {
		return fallback;
	}
}

async function buildCurrentPreset() {
	const p = router_state.settingsPageParams;
	const dir = router_state.moduleInformation.moduleDir;
	const [pacingCa, pacingSs, tcpFastopen, tcpEcn, qdisc] = await Promise.all([
		readInteger(`cat ${shellQuote(`${dir}/pacing_ca`)} 2>/dev/null`, null, 1, 1000),
		readInteger(`cat ${shellQuote(`${dir}/pacing_ss`)} 2>/dev/null`, null, 1, 1000),
		readInteger('cat /proc/sys/net/ipv4/tcp_fastopen 2>/dev/null', 3, 0, 3),
		readInteger('cat /proc/sys/net/ipv4/tcp_ecn 2>/dev/null', 1, 0, 2),
		getDefaultQdisc(),
	]);
	return {
		name: 'Current',
		wlanAlgo: p.wlanAlgo || 'cubic',
		cellAlgo: p.rmnetAlgo || 'cubic',
		killConnections: p.killConnections || false,
		initcwndInitrwnd: p.initcwndInitrwnd || false,
		qdisc,
		pacing_ca: pacingCa,
		pacing_ss: pacingSs,
		tcp_fastopen: tcpFastopen,
		tcp_ecn: tcpEcn,
		desc: 'Exported from current configuration',
	};
}

async function applyPreset(preset) {
	preset = normalizePreset(preset);
	if (!preset || !router_state.available_algorithms.includes(preset.wlanAlgo) || !router_state.available_algorithms.includes(preset.cellAlgo)) {
		toast(I18N.t('toast_invalid_algo'));
		return;
	}
	const dir = router_state.moduleInformation.moduleDir;
	const killFile = shellQuote(`${dir}/kill_connections`);
	const initcwndFile = shellQuote(`${dir}/initcwnd_initrwnd`);
	const killFiles = preset.killConnections ? ` && touch ${killFile}` : ` && rm -f ${killFile}`;
	const initcwndFiles = preset.initcwndInitrwnd ? ` && touch ${initcwndFile}` : ` && rm -f ${initcwndFile}`;
	const pacingFiles = preset.pacing_ca == null
		? ` && rm -f ${shellQuote(`${dir}/pacing_ca`)} ${shellQuote(`${dir}/pacing_ss`)}`
		: ` && printf '%s\n' ${preset.pacing_ca} > ${shellQuote(`${dir}/pacing_ca`)}` +
			` && printf '%s\n' ${preset.pacing_ss} > ${shellQuote(`${dir}/pacing_ss`)}`;

	try {
		await exec(
			algorithmMarkerCommand(dir, 'wlan', preset.wlanAlgo) +
			` && ${algorithmMarkerCommand(dir, 'rmnet_data', preset.cellAlgo)}` +
			killFiles + initcwndFiles + pacingFiles
		);
		if (!await setDefaultQdisc(preset.qdisc)) throw new Error('qdisc rejected');
		await exec(`printf '%s\n' ${preset.tcp_fastopen} > /proc/sys/net/ipv4/tcp_fastopen && printf '%s\n' ${preset.tcp_ecn} > /proc/sys/net/ipv4/tcp_ecn && printf '%s\n' ${preset.tcp_fastopen} > ${shellQuote(`${dir}/tcp_fastopen`)} && printf '%s\n' ${preset.tcp_ecn} > ${shellQuote(`${dir}/tcp_ecn`)} && touch ${shellQuote(`${dir}/force_apply`)}`);
	} catch (error) {
		console.error('Failed to apply preset:', error);
		toast(I18N.t('toast_error'));
		return;
	}

	router_state.settingsPageParams.wlanAlgo = preset.wlanAlgo;
	router_state.settingsPageParams.rmnetAlgo = preset.cellAlgo;
	router_state.settingsPageParams.killConnections = preset.killConnections;
	router_state.settingsPageParams.initcwndInitrwnd = preset.initcwndInitrwnd;

	// Mark preset as active
	localStorage.setItem('tcp_active_preset', preset.name);
	document.querySelectorAll('#preset-list .preset-chip').forEach(c => c.classList.remove('active'));
	const activeEl = [...document.querySelectorAll('#preset-list .preset-chip')].find(chip => chip.dataset.name === preset.name);
	if (activeEl) activeEl.classList.add('active');

	addLog(`Preset applied: ${preset.name} (WiFi=${preset.wlanAlgo}, Cell=${preset.cellAlgo})`);
	toast(I18N.t('toast_preset_applied', { name: preset.name }));
}
