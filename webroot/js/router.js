import { exec } from './kernelsu.js';
import I18N from './i18n.js';
import { updateModuleInformation } from './common.js';
import { updateModuleStatus, initHome, updateHomeUI } from './home.js';
import { initLogs, read_log_file, updateLogsUI } from './logs.js';
import { initSettings, syncAdvancedNavVisibility } from './settings.js';
import { updateStats, initStatsUI } from './stats.js';
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
	qdiscCapabilities: [],
	current_active_page: 'home',
	statsParams: { tcpConns: null, sockStat: null, dnsServers: null, ssInfo: null, tcpCounters: null },
};

let updateTimer = null;
let lastStatusUpdate = 0;
let settingsInitPromise = null;
const VALID_PAGES = new Set(['home', 'stats', 'settings', 'logs', 'adv']);

function ensureSettingsInitialized() {
	if (!router_state.moduleInformation) return Promise.resolve();
	if (!settingsInitPromise) {
		settingsInitPromise = initSettings().catch(error => {
			settingsInitPromise = null;
			console.error('Error initializing settings:', error);
		});
	}
	return settingsInitPromise;
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
	if (!router_state.isInitializing) void updateUI();
	if (!router_state.isInitializing && (pageName === 'settings' || pageName === 'adv')) {
		void ensureSettingsInitialized();
	}
}

window.addEventListener('popstate', (e) => {
	const page = e.state?.page || 'home';
	showPage(page, false);
});

async function updateUI() {
	switch (router_state.current_active_page) {
		case 'home': updateHomeUI(); break;
		case 'logs': updateLogsUI(); break;
		case 'stats': await updateStats(); break;
	}
}

const runRealtimeUpdate = async () => {
	try {
		const page = router_state.current_active_page;
		const now = Date.now();
		if (page === 'home' || now - lastStatusUpdate >= 15000) {
			await updateModuleStatus();
			lastStatusUpdate = now;
		}
		if (page === 'logs') {
			await read_log_file();
			updateLogsUI();
		} else if (page === 'stats') {
			await updateStats();
		} else if (page === 'home') {
			updateHomeUI();
		}
	} catch (error) {
		console.error('Error setting update loop:', error);
	} finally {
		const delay = router_state.current_active_page === 'home' ? 10000
			: router_state.current_active_page === 'settings' || router_state.current_active_page === 'adv' ? 15000
			: 5000;
		updateTimer = setTimeout(runRealtimeUpdate, delay);
	}
};

const startRealtimeUpdater = () => {
	if (updateTimer) clearTimeout(updateTimer);
	updateTimer = setTimeout(runRealtimeUpdate, 5000);
};

document.addEventListener('DOMContentLoaded', async () => {
	await I18N.init();
	syncAdvancedNavVisibility();
	initMotion();
	await initDynamicColorTheme();
	await updateModuleInformation();

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
	initLogs();
	initStatsUI();
	router_state.isInitializing = false;
	if (router_state.moduleInformation) {
		await updateModuleStatus();
		lastStatusUpdate = Date.now();
	} else {
		router_state.homePageParams.module_status = 'NotInstalled';
	}

	showPage('home', false);
	history.replaceState({ page: 'home' }, '', '#home');
	await updateUI();
	startRealtimeUpdater();

	document.addEventListener('i18n-changed', () => {
		// I18N.applyToDOM restores static placeholders (including the global
		// status chip and title), so immediately repaint their live values.
		updateHomeUI();
		showPage(router_state.current_active_page, false);
		void updateUI();
	});
});

export default router_state;
