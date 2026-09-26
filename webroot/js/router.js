import { exec, toast } from './kernelsu.js';
import I18N from './i18n.js';
import { getBuildInfo, signalWebUiActive, updateModuleInformation, verifyInstalledModule } from './common.js';
import { updateModuleStatus, initHome, updateHomeUI } from './home.js';
import { initDynamicColorTheme } from './theme.js';
import { initMotion } from './motion.js';

const router_state = {
	moduleInformation: null,
	isInitializing: true,
	homePageParams: {
		module_status: "Loading...",
		active_iface_type: "None",
		active_iface: "Unknown",
		active_algorithm: "Unknown",
		active_InitcwndInitrwndValue: [],
		wifi_calling_state: false,
		default_qdisc: "unknown",
		proxy_status: "unknown",
		proxy_info: null,
		hosts_status: "unknown",
	},
	settingsPageParams: {
		wlanAlgo: null,
		rmnetAlgo: null,
		killConnections: null,
		initcwndInitrwnd: null,
	},
	logsList: [],
	available_algorithms: [],
	native_algorithms: [],
	bundled_algorithms: [],
	bundled_qdiscs: [],
	kernelBundle: null,
	autoTuningEnabled: null,
	qdiscPolicy: 'unknown',
	networkProfile: null,
	qdiscCapabilities: [],
	runtimeSnapshot: null,
	verification: null,
	current_active_page: 'home',
	statsParams: { tcpConns: null, sockStat: null, dnsServers: null, ssInfo: null, tcpCounters: null },
};

let updateTimer = null;
let realtimeBusy = false;
let lastStatusUpdate = 0;
let settingsInitPromise = null;
let settingsModulePromise = null;
let statsModulePromise = null;
let logsModulePromise = null;
let statsInitialized = false;
let logsInitialized = false;
const VALID_PAGES = new Set(['home', 'stats', 'settings', 'logs', 'adv']);

function loadSettingsModule() {
	return settingsModulePromise ||= import('./settings.js');
}

function loadStatsModule() {
	return statsModulePromise ||= import('./stats.js');
}

function loadLogsModule() {
	return logsModulePromise ||= import('./logs.js');
}

function syncAdvancedNavVisibilityFast() {
	const enabled = localStorage.getItem('tcp_adv_enabled') === 'true';
	document.getElementById('nav-adv')?.classList.toggle('hidden', !enabled);
	const page = document.getElementById('adv-page');
	if (page && !enabled) page.hidden = true;
}

async function ensureSettingsInitialized(includeAdvanced = false) {
	if (!router_state.moduleInformation) return null;
	const settings = await loadSettingsModule();
	if (!settingsInitPromise) {
		settingsInitPromise = settings.initSettings().catch(error => {
			settingsInitPromise = null;
			console.error('Error initializing settings:', error);
			throw error;
		});
	}
	await settingsInitPromise;
	if (includeAdvanced) await settings.ensureAdvancedInitialized?.();
	return settings;
}

async function ensureStatsInitialized() {
	const stats = await loadStatsModule();
	if (!statsInitialized) {
		stats.initStatsUI();
		statsInitialized = true;
	}
	return stats;
}

async function ensureLogsInitialized() {
	const logs = await loadLogsModule();
	if (!logsInitialized) {
		await logs.initLogs();
		logsInitialized = true;
	}
	return logs;
}

function showPage(pageName, push = true) {
	if (!VALID_PAGES.has(pageName)) pageName = 'home';
	document.querySelectorAll('#pages > section').forEach(s => s.hidden = true);
	const page = document.getElementById(pageName + '-page');
	if (page) page.hidden = false;
	router_state.current_active_page = pageName;

	document.querySelectorAll('.nav-item').forEach(n => {
		const isActive = n.dataset.page === pageName;
		n.classList.toggle('active', isActive);
		if (isActive) n.setAttribute('aria-current', 'page');
		else n.removeAttribute('aria-current');
	});

	// Update top bar title to current tab name
	const titleMap = { home: 'nav_home', stats: 'nav_stats', settings: 'nav_settings', logs: 'nav_logs', adv: 'nav_advanced' };
	const titleKey = titleMap[pageName] || 'app_title';
	const titleEl = document.querySelector('.app-title');
	if (titleEl) titleEl.textContent = I18N.t(titleKey);

	if (push) {
		history.pushState({ page: pageName }, '', `#${pageName}`);
	}
	if (!router_state.isInitializing) {
		void updateUI();
		if (pageName === 'settings' || pageName === 'adv') {
			void ensureSettingsInitialized(pageName === 'adv');
		}
		startRealtimeUpdater(1200);
	}
}

window.addEventListener('popstate', (e) => {
	const page = e.state?.page || 'home';
	showPage(page, false);
});

async function updateUI() {
	if (document.hidden) return;
	switch (router_state.current_active_page) {
		case 'home':
			updateHomeUI();
			break;
		case 'logs': {
			const logs = await ensureLogsInitialized();
			if (router_state.moduleInformation) await logs.read_log_file();
			logs.updateLogsUI();
			break;
		}
		case 'stats': {
			const stats = await ensureStatsInitialized();
			await stats.updateStats();
			break;
		}
	}
}

function realtimeDelay() {
	switch (router_state.current_active_page) {
		case 'home': return 12000;
		case 'stats': return 5000;
		case 'logs': return 8000;
		case 'settings':
		case 'adv': return 20000;
		default: return 12000;
	}
}

const runRealtimeUpdate = async () => {
	if (document.hidden) {
		updateTimer = null;
		return;
	}
	if (realtimeBusy) {
		updateTimer = setTimeout(runRealtimeUpdate, realtimeDelay());
		return;
	}
	realtimeBusy = true;
	try {
		void signalWebUiActive();
		const page = router_state.current_active_page;
		const now = Date.now();
		if (page === 'home' || now - lastStatusUpdate >= 30000) {
			await updateModuleStatus();
			lastStatusUpdate = now;
		}
		await updateUI();
	} catch (error) {
		console.error('Error setting update loop:', error);
	} finally {
		realtimeBusy = false;
		if (!document.hidden) updateTimer = setTimeout(runRealtimeUpdate, realtimeDelay());
	}
};

const startRealtimeUpdater = (delay = 5000) => {
	if (updateTimer) clearTimeout(updateTimer);
	if (document.hidden) {
		updateTimer = null;
		return;
	}
	updateTimer = setTimeout(runRealtimeUpdate, delay);
};

async function initAboutDiagnostics() {
	const version = document.getElementById('about-build-version');
	const channel = document.getElementById('about-build-channel');
	const source = document.getElementById('about-build-source');
	const revision = document.getElementById('about-build-revision');
	try {
		const info = await getBuildInfo();
		if (version) version.textContent = info.version || '—';
		if (channel) channel.textContent = info.channel || '—';
		if (source) source.textContent = info.source || '—';
		if (revision) revision.textContent = info.revision || '—';
	} catch (error) {
		console.warn('Build provenance unavailable:', error);
		for (const element of [version, channel, source, revision]) {
			if (element) element.textContent = I18N.t('home_status_unknown');
		}
	}

	const integrityButton = document.getElementById('about-integrity-btn');
	integrityButton?.addEventListener('click', async () => {
		if (integrityButton.disabled) return;
		const status = document.getElementById('about-integrity-status');
		integrityButton.disabled = true;
		if (status) status.textContent = I18N.t('integrity_check_running');
		try {
			await verifyInstalledModule();
			if (status) status.textContent = I18N.t('integrity_check_ok');
			toast(I18N.t('integrity_check_ok'));
		} catch (error) {
			console.error('Installed module integrity check failed:', error);
			if (status) status.textContent = I18N.t('integrity_check_failed');
			toast(I18N.t('integrity_check_failed'));
		} finally {
			integrityButton.disabled = false;
		}
	});
}

document.addEventListener('DOMContentLoaded', async () => {
	await I18N.init();
	syncAdvancedNavVisibilityFast();
	initMotion();
	void initDynamicColorTheme();
	await updateModuleInformation();
	void signalWebUiActive(true);

	document.querySelectorAll('.link-chip').forEach(chip => {
		chip.addEventListener('click', async (e) => {
			e.preventDefault();
			const url = chip.dataset.url;
			if (/^https?:\/\/[^\s"'`$;|&<>()\\]+$/.test(url)) {
				await exec(`am start -a android.intent.action.VIEW -d "${url}"`);
			}
		});
	});

	document.querySelectorAll('.nav-item').forEach(item => {
		item.addEventListener('click', (e) => {
			e.preventDefault();
			showPage(item.dataset.page);
		});
	});

	window._debug?.init();
	await initHome();
	router_state.isInitializing = false;
	if (!router_state.moduleInformation) {
		router_state.homePageParams.module_status = 'NotInstalled';
	}

	// Paint the shell immediately. Runtime data arrives from the daemon's
	// persisted snapshot in the background instead of blocking first paint.
	showPage('home', false);
	history.replaceState({ page: 'home' }, '', '#home');
	updateHomeUI();
	void signalWebUiActive(true);
	void updateUI();
	startRealtimeUpdater(750);

	if (router_state.moduleInformation) {
		void updateModuleStatus().then(() => {
			lastStatusUpdate = Date.now();
			updateHomeUI();
		});
	}

	const idle = window.requestIdleCallback || ((fn) => setTimeout(fn, 1800));
	idle(() => {
		void loadStatsModule();
		void loadLogsModule();
		void initAboutDiagnostics();
	});

	document.addEventListener('i18n-changed', () => {
		// I18N.applyToDOM restores static placeholders (including the global
		// status chip and title), so immediately repaint their live values.
		updateHomeUI();
		showPage(router_state.current_active_page, false);
		void updateUI();
	});
});

document.addEventListener('visibilitychange', () => {
	if (document.hidden) {
		if (updateTimer) clearTimeout(updateTimer);
		updateTimer = null;
		return;
	}
	void updateUI();
	startRealtimeUpdater(750);
});

export default router_state;
