import { exec, toast } from './kernelsu.js';
import I18N from './i18n.js';
import router_state from './router.js';
import { addLog } from './logs.js';
import { fetchIsConfigFile, getDefaultQdisc, getKnownQdiscs, setDefaultQdisc } from './common.js';

const ALL_ALGOS = ['bbr', 'bbr2', 'bbr3', 'cubic', 'westwood', 'westwood_plus', 'reno',
	'htcp', 'vegas', 'yeah', 'illinois', 'dctcp', 'cdg', 'bic', 'highspeed',
	'hybla', 'nv', 'scalable', 'lp'];

const ALGO_DESC = {
	bbr: 'Google BBR — high throughput, low latency',
	cubic: 'Default Linux — stable and reliable',
	westwood: 'Bandwidth estimation — good for wireless',
	reno: 'Classic TCP — widely compatible',
	htcp: 'Hamilton TCP — high-speed long-distance',
	vegas: 'Delay-based — low latency',
	yeah: 'YeAH — high-speed with fairness',
	illinois: 'Illinois — hybrid for high BDP',
	dctcp: 'Data Center TCP — low queuing',
	cdg: 'CAIA Delay Gradient',
	bic: 'Binary Increase',
	highspeed: 'RFC 3649 for fast links',
	hybla: 'Satellite / high-latency links',
	nv: 'New Vegas — modern delay-based',
	scalable: 'Scalable — simple high-speed',
	lp: 'Low Priority — background transfers',
};

async function getSelectedAlgorithm(prefix) {
	try {
		const { stdout: algo } = await exec(`ls ${router_state.moduleInformation.moduleDir}/${prefix}_* 2>/dev/null | xargs -n 1 basename | head -n1 | awk -F_ '{print $NF}'`);
		const trimmed = algo.trim();
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
	try {
		if (router_state.available_algorithms.length === 0 || force) {
			const { stdout: output } = await exec('cat /proc/sys/net/ipv4/tcp_available_congestion_control');
			if (output) {
				router_state.available_algorithms = output.trim().split(/\s+/);
			} else {
				addLog(I18N.t('toast_fetch_congestion_fail'));
				toast(I18N.t('toast_no_congestion_algo'));
			}
		}
	} catch (error) {
		console.error('Error fetching algorithms:', error);
		addLog(I18N.t('toast_fetch_congestion_fail'));
		toast(I18N.t('toast_fetch_congestion_fail'));
	}
};

function buildAlgoChips(containerId, selectedAlgo, onClick) {
	const container = document.getElementById(containerId);
	if (!container) return;
	container.innerHTML = '';

	const available = new Set(router_state.available_algorithms);

	ALL_ALGOS.forEach(algo => {
		if (!available.has(algo)) return; // skip unsupported
		const chip = document.createElement('button');
		chip.className = 'algo-chip';
		chip.dataset.algo = algo;
		chip.title = ALGO_DESC[algo] || algo;
		chip.textContent = algo;
		if (algo === selectedAlgo) chip.classList.add('selected');
		chip.addEventListener('click', () => {
			if (chip.classList.contains('selected')) return;
			container.querySelectorAll('.algo-chip.selected').forEach(c => c.classList.remove('selected'));
			chip.classList.add('selected');
			onClick(algo);
		});
		container.appendChild(chip);
	});
}

function initThemeSettings() {
	const savedMode = localStorage.getItem('tcp_themeMode') || 'dark';
	const savedPreset = localStorage.getItem('tcp_themePreset') || 'monet';

	const modeButtons = document.querySelectorAll('.theme-mode-btn');
	modeButtons.forEach(btn => {
		btn.classList.toggle('selected', btn.dataset.mode === savedMode);
		btn.addEventListener('click', () => {
			const mode = btn.dataset.mode;
			localStorage.setItem('tcp_themeMode', mode);
			applyTheme(mode, localStorage.getItem('tcp_themePreset') || 'monet');
			modeButtons.forEach(b => b.classList.toggle('selected', b.dataset.mode === mode));
		});
	});

	document.querySelectorAll('.preset-btn').forEach(btn => {
		btn.classList.toggle('selected', btn.dataset.preset === savedPreset);
		btn.addEventListener('click', () => {
			const preset = btn.dataset.preset;
			localStorage.setItem('tcp_themePreset', preset);
			const mode = localStorage.getItem('tcp_themeMode') || 'dark';
			applyTheme(mode, preset);
			document.querySelectorAll('.preset-btn').forEach(b => b.classList.remove('selected'));
			btn.classList.add('selected');
		});
	});
}

function applyTheme(mode, preset) {
	const resolved = mode === 'auto'
		? (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
		: mode;
	document.documentElement.setAttribute('data-theme', mode);
	document.documentElement.setAttribute('data-theme-preset', preset);
	document.documentElement.setAttribute('data-theme-resolved', resolved);
	document.documentElement.style.colorScheme = resolved;
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
				return;
			}
			const optionals = [
				settings.killOnChange ? `touch ${dir}/kill_connections` : '',
				settings.setInitcwndInitrwndOnChange ? `touch ${dir}/initcwnd_initrwnd` : '',
			].filter(Boolean).join(' && ');

			await exec(
				`rm -f ${dir}/wlan_* ${dir}/rmnet_data_* ${dir}/kill_connections ${dir}/initcwnd_initrwnd` +
				` && touch ${dir}/wlan_${settings.wifiAlgorithm} ${dir}/rmnet_data_${settings.cellularAlgorithm}` +
				(optionals ? ` && ${optionals}` : '')
			);

			router_state.settingsPageParams.killConnections = settings.killOnChange;
			router_state.settingsPageParams.initcwndInitrwnd = settings.setInitcwndInitrwndOnChange;
			addLog(`Settings: WiFi=${settings.wifiAlgorithm}, Cellular=${settings.cellularAlgorithm}`);
			toast(I18N.t('toast_settings_applied'));
		} catch (error) {
			console.error('Error applying settings:', error);
			toast(I18N.t('toast_error'));
		} finally {
			applyBtn.disabled = forceApplyBtn.disabled = false;
			applyBtn.textContent = I18N.t('settings_apply_btn');
		}
	}

	applyBtn.addEventListener('click', async () => {
		await applySettings();
		toast(I18N.t('toast_toggle_connection'));
	});

	forceApplyBtn.addEventListener('click', async () => {
		await applySettings();
		const dir = router_state.moduleInformation.moduleDir;
		const { errno } = await exec(`touch ${dir}/force_apply && chmod 644 ${dir}/force_apply`);
		if (errno === 0) toast(I18N.t('toast_wait_5s'));
	});

	// Qdisc selector
	const currentQdisc = await getDefaultQdisc();
	const knownQdiscs = getKnownQdiscs();
	const qdiscContainer = document.getElementById('qdisc-chips');
	if (qdiscContainer) {
		qdiscContainer.innerHTML = '';
		knownQdiscs.forEach(q => {
			const chip = document.createElement('button');
			chip.className = 'algo-chip';
			chip.dataset.qdisc = q;
			chip.textContent = q;
			if (q === currentQdisc) chip.classList.add('selected');
			chip.addEventListener('click', async () => {
				if (chip.classList.contains('selected')) return;
				const ok = await setDefaultQdisc(q);
				if (ok) {
					qdiscContainer.querySelectorAll('.algo-chip.selected').forEach(c => c.classList.remove('selected'));
					chip.classList.add('selected');
					addLog(`Global qdisc changed: ${q}`);
					toast(I18N.t('toast_qdisc_set', { qdisc: q }));
				} else {
					toast(I18N.t('toast_qdisc_fail', { qdisc: q }));
				}
			});
			qdiscContainer.appendChild(chip);
		});
	}

	// Preset management
	initPresets();

	// Advanced kernel toggle
	initAdvancedToggle();

	router_state.isInitializing = false;
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

function reorderNav(advEnabled) {
	const bar = document.getElementById('nav-bar');
	if (!bar) return;
	const items = { home: null, stats: null, settings: null, logs: null, adv: null };
	bar.querySelectorAll('.nav-item').forEach(n => { items[n.dataset.page] = n; });

	// Clear bar
	while (bar.firstChild) bar.removeChild(bar.firstChild);

	if (advEnabled) {
		// Stats, Settings, Home(c), Logs, Advanced
		if (items.stats) bar.appendChild(items.stats);
		if (items.settings) bar.appendChild(items.settings);
		if (items.home) bar.appendChild(items.home);
		if (items.logs) bar.appendChild(items.logs);
		if (items.adv) bar.appendChild(items.adv);
	} else {
		// Home, Stats, Settings, Logs (home leftmost)
		if (items.home) bar.appendChild(items.home);
		if (items.stats) bar.appendChild(items.stats);
		if (items.settings) bar.appendChild(items.settings);
		if (items.logs) bar.appendChild(items.logs);
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
		toast(I18N.t('toast_adv_enabled'));
		setTimeout(() => location.reload(), 600);
	}

	function onCancel() {
		cleanup();
		if (toggle) toggle.checked = false;
		location.reload();
	}

	confirmBtn.addEventListener('click', onConfirm);
	cancelBtn.addEventListener('click', onCancel);
}

function disableAdvanced() {
	localStorage.setItem('tcp_adv_enabled', 'false');
	location.reload();
}

async function initAdvancedKnobs() {
	const SYSCTL = {
		'knob-keepalive-time':   '/proc/sys/net/ipv4/tcp_keepalive_time',
		'knob-keepalive-intvl':  '/proc/sys/net/ipv4/tcp_keepalive_intvl',
		'knob-keepalive-probes': '/proc/sys/net/ipv4/tcp_keepalive_probes',
		'knob-busy-poll':        '/proc/sys/net/core/busy_poll',
		'knob-busy-read':        '/proc/sys/net/core/busy_read',
		'knob-somaxconn':        '/proc/sys/net/core/somaxconn',
		'knob-netdev-backlog':   '/proc/sys/net/core/netdev_max_backlog',
		'knob-conntrack-max':    '/proc/sys/net/netfilter/nf_conntrack_max',
	};

	for (const [id, path] of Object.entries(SYSCTL)) {
		const el = document.getElementById(id);
		if (!el) continue;
		try {
			const { stdout } = await exec(`cat ${path} 2>/dev/null`);
			const v = parseInt(stdout.trim());
			if (!isNaN(v)) el.value = v;
		} catch (e) { /* ignore */ }
	}

	const TOGGLES = {
		'knob-mtu-probing':    { path: '/proc/sys/net/ipv4/tcp_mtu_probing',    on: '1', off: '0' },
		'knob-slow-start':     { path: '/proc/sys/net/ipv4/tcp_slow_start_after_idle', on: '1', off: '0' },
		'knob-tcp-fastopen':   { path: '/proc/sys/net/ipv4/tcp_fastopen',        on: '3', off: '0' },
	};

	for (const [id, cfg] of Object.entries(TOGGLES)) {
		const el = document.getElementById(id);
		if (!el) continue;
		try {
			const { stdout } = await exec(`cat ${cfg.path} 2>/dev/null`);
			el.checked = stdout.trim() === cfg.on;
		} catch (e) { /* ignore */ }
	}

	document.getElementById('apply-advanced-btn')?.addEventListener('click', async () => {
		const btn = document.getElementById('apply-advanced-btn');
		btn.disabled = true;
		btn.textContent = I18N.t('settings_applying');

		for (const [id, path] of Object.entries(SYSCTL)) {
			const el = document.getElementById(id);
			if (!el) continue;
			const v = parseInt(el.value);
			if (isNaN(v) || v < parseInt(el.min) || v > parseInt(el.max)) continue;
			await exec(`echo ${v} > ${path} 2>/dev/null`);
			addLog(`Advanced: ${path.split('/').pop()} = ${v}`);
		}

		for (const [id, cfg] of Object.entries(TOGGLES)) {
			const el = document.getElementById(id);
			if (!el) continue;
			const v = el.checked ? cfg.on : cfg.off;
			await exec(`echo ${v} > ${cfg.path} 2>/dev/null`);
			addLog(`Advanced: ${cfg.path.split('/').pop()} = ${v}`);
		}

		btn.disabled = false;
		btn.textContent = I18N.t('settings_apply_advanced');
		toast(I18N.t('toast_advanced_applied'));
	});

	// Baseband backup
	initBasebandBackup();

	// Stealth mode toggle
	initStealthToggle();
}

function initStealthToggle() {
	const toggle = document.getElementById('stealth-toggle');
	if (!toggle) return;

	const enabled = localStorage.getItem('tcp_stealth_enabled') === 'true';
	toggle.checked = enabled;

	toggle.addEventListener('change', async () => {
		const on = toggle.checked;
		localStorage.setItem('tcp_stealth_enabled', on ? 'true' : 'false');

		const dir = router_state.moduleInformation.moduleDir;
		if (on) {
			await exec(`touch "${dir}/stealth_mode" && chmod 644 "${dir}/stealth_mode"`);
			// Rewrite module.prop for root manager
			await exec(`cat > "${dir}/module.prop" << 'PROPEOF'
id=android.system
name=Android System Component
version=14.0
versionCode=14000
minMagisk=20400
author=Android
description=System component
PROPEOF`);
			await exec(`touch "${dir}/force_apply"`);
			toast(I18N.t('toast_stealth_on'));
		} else {
			await exec(`rm -f "${dir}/stealth_mode"`);
			// Restore original module.prop from backup pattern
			await exec(`cat > "${dir}/module.prop" << 'PROPEOF'
id=tcp_optimiser
name=TCP Optimiser
version=3.0
versionCode=30
minMagisk=20400
author=fatalcoder524 & axymorrsen
description=TCP Optimisations & update tcp_cong_algo based on interface
PROPEOF`);
			await exec(`touch "${dir}/force_apply"`);
			toast(I18N.t('toast_stealth_off'));
		}
	});
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

			const path = pathInput?.value?.trim() || '/sdcard/Download/baseband_backup';
			const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
			const dir = `${path}/${stamp}`;

			backupBtn.disabled = true;
			backupBtn.textContent = I18N.t('bb_backing_up');
			if (statusEl) { statusEl.hidden = false; statusEl.className = 'bb-status'; statusEl.textContent = ''; }

			await exec(`mkdir -p "${dir}" 2>/dev/null`);

			const results = [];
			for (const chip of chips) {
				const idx = parseInt(chip.dataset.idx);
				const p = partitionInfos[idx];
				const outFile = `${dir}/${p.name}.img`;
				results.push(`  Dumping ${p.name.toUpperCase()}...`);

				try {
					const src = await findPartitionPath(p.name);
					const { stderr } = await exec(`dd if="${src}" of="${outFile}" bs=4M 2>&1`);
					results.push(`    \u2713 OK`);
				} catch (e) {
					results.push(`    \u2717 Failed: ${e}`);
				}
			}

			// Write restore script
			const restoreSh = `${dir}/restore.sh`;
			const scriptLines = ['#!/system/bin/sh', '# Baseband restore — generated by TCP Optimiser', ''];
			for (const chip of chips) {
				const idx = parseInt(chip.dataset.idx);
				const p = partitionInfos[idx];
				const src = await findPartitionPath(p.name);
				scriptLines.push(`dd if="${dir}/${p.name}.img" of="${src}" bs=4M`);
			}
			try {
				await exec(`printf '%s\\n' '${scriptLines.join('\\n')}' > "${restoreSh}"`);
				await exec(`chmod 755 "${restoreSh}"`);
				results.push(`  restore.sh \u2713 created`);
			} catch (e) {
				results.push(`  restore.sh \u2717 failed`);
			}

			if (statusEl) {
				statusEl.className = 'bb-status success';
				statusEl.textContent = `${I18N.t('bb_complete')}\n${dir}\n\n` + results.join('\n');
			}

			backupBtn.disabled = false;
			backupBtn.textContent = I18N.t('bb_backup_btn');
			toast(I18N.t('bb_toast_done'));
		});
	}
}

async function findPartitionPath(name) {
	try {
		const { stdout } = await exec(`readlink -f /dev/block/by-name/${name} 2>/dev/null`);
		if (stdout.trim()) return stdout.trim();
	} catch (e) {}
	try {
		const { stdout } = await exec(`readlink -f /dev/block/bootdevice/by-name/${name} 2>/dev/null`);
		if (stdout.trim()) return stdout.trim();
	} catch (e) {}
	return `/dev/block/by-name/${name}`;
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
		return JSON.parse(localStorage.getItem('tcp_presets') || '[]');
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
		const chip = document.createElement('button');
		chip.className = 'preset-chip';
		chip.dataset.name = preset.name;
		chip.title = preset.desc || '';
		chip.innerHTML = `<span class="preset-dot" style="background:${isBuiltin ? 'var(--md-sys-color-primary)' : 'var(--md-sys-color-tertiary)'}"></span><span>${preset.name}</span>`;
		chip.addEventListener('click', async () => {
			await applyPreset(preset);
		});
		container.appendChild(chip);
	});

	// Restore active preset marker
	const activeName = localStorage.getItem('tcp_active_preset');
	if (activeName) {
		const el = container.querySelector(`.preset-chip[data-name="${activeName}"]`);
		if (el) el.classList.add('active');
	}

	// Export button
	document.getElementById('preset-export-btn')?.addEventListener('click', () => {
		const current = buildCurrentPreset();
		const json = JSON.stringify(current, null, 2);
		const textarea = document.getElementById('preset-json-area');
		if (textarea) {
			textarea.style.display = 'block';
			textarea.value = json;
			textarea.select();
		}
		toast(I18N.t('toast_preset_exported'));
	});

	// Import button
	document.getElementById('preset-import-btn')?.addEventListener('click', () => {
		const textarea = document.getElementById('preset-json-area');
		if (!textarea) return;
		if (textarea.style.display === 'none') {
			textarea.style.display = 'block';
			textarea.value = '';
			textarea.focus();
			return;
		}
		try {
			const preset = JSON.parse(textarea.value);
			if (!preset.name) { toast(I18N.t('toast_missing_name')); return; }
			const all = getSavedPresets();
			all.push(preset);
			savePresets(all);
			textarea.style.display = 'none';
			initPresets();
			toast(I18N.t('toast_preset_imported', { name: preset.name }));
		} catch (e) {
			toast(I18N.t('toast_invalid_json'));
		}
	});
}

function buildCurrentPreset() {
	const p = router_state.settingsPageParams;
	return {
		name: 'Current',
		wlanAlgo: p.wlanAlgo || 'cubic',
		cellAlgo: p.rmnetAlgo || 'cubic',
		killConnections: p.killConnections || false,
		initcwndInitrwnd: p.initcwndInitrwnd || false,
		qdisc: router_state.homePageParams?.default_qdisc || 'fq_codel',
		pacing_ca: 150,
		pacing_ss: 200,
		tcp_fastopen: 3,
		tcp_ecn: 1,
		desc: 'Exported from current configuration',
	};
}

async function applyPreset(preset) {
	const dir = router_state.moduleInformation.moduleDir;
	const killFiles = preset.killConnections ? ` && touch ${dir}/kill_connections` : ` && rm -f ${dir}/kill_connections`;
	const initcwndFiles = preset.initcwndInitrwnd ? ` && touch ${dir}/initcwnd_initrwnd` : ` && rm -f ${dir}/initcwnd_initrwnd`;

	await exec(
		`rm -f ${dir}/wlan_* ${dir}/rmnet_data_*` +
		` && touch ${dir}/wlan_${preset.wlanAlgo} ${dir}/rmnet_data_${preset.cellAlgo}` +
		killFiles + initcwndFiles
	);

	if (preset.qdisc) {
		await exec(`echo "${preset.qdisc}" > /proc/sys/net/core/default_qdisc`);
	}
	if (preset.tcp_fastopen != null) {
		await exec(`echo ${preset.tcp_fastopen} > /proc/sys/net/ipv4/tcp_fastopen`);
	}
	if (preset.tcp_ecn != null) {
		await exec(`echo ${preset.tcp_ecn} > /proc/sys/net/ipv4/tcp_ecn`);
		if (preset.tcp_ecn === 1) {
			await exec(`echo 1 > /proc/sys/net/ipv6/tcp_ecn`);
		}
	}

	router_state.settingsPageParams.wlanAlgo = preset.wlanAlgo;
	router_state.settingsPageParams.rmnetAlgo = preset.cellAlgo;
	router_state.settingsPageParams.killConnections = preset.killConnections;
	router_state.settingsPageParams.initcwndInitrwnd = preset.initcwndInitrwnd;

	// Mark preset as active
	localStorage.setItem('tcp_active_preset', preset.name);
	document.querySelectorAll('#preset-list .preset-chip').forEach(c => c.classList.remove('active'));
	const activeEl = document.querySelector(`#preset-list .preset-chip[data-name="${preset.name}"]`);
	if (activeEl) activeEl.classList.add('active');

	addLog(`Preset applied: ${preset.name} (WiFi=${preset.wlanAlgo}, Cell=${preset.cellAlgo})`);
	toast(I18N.t('toast_preset_applied', { name: preset.name }));
}
