import { exec } from './kernelsu.js';
import I18N from './i18n.js';
import { updateModuleInformation } from './common.js';
import { updateModuleStatus, initHome, updateHomeUI } from './home.js';
import { initLogs, read_log_file, updateLogsUI } from './logs.js';
import { initSettings, syncAdvancedNavVisibility } from './settings.js';
import { updateStats, initStatsUI } from './stats.js';
import { initDynamicColorTheme } from './theme.js';
import { initMotion } from './motion.js';
import { initBaselineUI } from './baseline-ui.js';
import {
	announce,
	initProductUI,
	rememberPageScroll,
	restorePageScroll,
	setGlobalBusy,
	syncProductUILanguage,
} from './product-ui.js';

const router_state = {
	moduleInformation: null,
	isInitializing: true,
	homePageParams: {
		module_status: 'Loading...',
		active_iface_type: 'None',
		active_iface: 'Unknown',
		active_algorithm: 'Unknown',
		active_InitcwndInitrwndValue: [],
		wifi_calling_state: false,
		default_qdisc: 'unknown',
		proxy_status: 'unknown',
		proxy_info: null,
		hosts_status: 'unknown',
	},
	settingsPageParams: {
		wlanAlgo: null,
		rmnetAlgo: null,
		killConnections: null,
		initcwndInitrwnd: null,
	},
	logsList: [],
	logsError: null,
	available_algorithms: [],
	qdiscCapabilities: [],
	runtimeSnapshot: null,
	verification: null,
	current_active_page: 'home',
	statsParams: { tcpConns: null, sockStat: null, dnsServers: null, ssInfo: null, tcpCounters: null },
};

let updateTimer = null;
let lastStatusUpdate = 0;
let settingsInitPromise = null;
let refreshPromise = null;
const VALID_PAGES = new Set(['home', 'stats', 'settings', 'logs', 'adv']);
const PAGE_TITLE_KEYS = {
	home: 'nav_home',
	stats: 'nav_stats',
	settings: 'nav_settings',
	logs: 'nav_logs',
	adv: 'nav_advanced',
};

function localText(english, chinese) {
	const language = I18N.currentLang || localStorage.getItem('tcp_lang') || document.documentElement.lang;
	return String(language).toLowerCase().startsWith('zh') ? chinese : english;
}

function pageFromLocation() {
	try {
		const candidate = decodeURIComponent(location.hash.replace(/^#/, '').split(/[?&/]/)[0] || 'home');
		return VALID_PAGES.has(candidate) ? candidate : 'home';
	} catch (error) {
		console.warn('Ignoring malformed page hash:', error);
		return 'home';
	}
}

function pageIsAvailable(pageName) {
	if (pageName !== 'adv') return true;
	const nav = document.getElementById('nav-adv');
	return Boolean(nav && !nav.hidden && !nav.classList.contains('hidden') && getComputedStyle(nav).display !== 'none');
}

function normalizedPage(pageName) {
	if (!VALID_PAGES.has(pageName)) return 'home';
	if (!pageIsAvailable(pageName)) return 'settings';
	return pageName;
}

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

function updateDocumentTitle(pageName) {
	const appTitle = I18N.t('app_title');
	const pageTitle = I18N.t(PAGE_TITLE_KEYS[pageName] || 'nav_home');
	const title = document.querySelector('.app-title');
	const subtitle = document.querySelector('.app-subtitle');
	if (title) title.textContent = appTitle;
	if (subtitle) subtitle.textContent = pageTitle;
	document.title = pageName === 'home' ? appTitle : `${pageTitle} · ${appTitle}`;
}

function scheduleRealtimeUpdate(delay = 5000) {
	if (updateTimer) clearTimeout(updateTimer);
	updateTimer = null;
	if (document.hidden) return;
	updateTimer = setTimeout(runRealtimeUpdate, delay);
}

function showPage(pageName, options = {}) {
	const {
		historyMode = 'push',
		restoreScroll = false,
		update = true,
	} = options;
	const nextPage = normalizedPage(pageName);
	const previousPage = router_state.current_active_page;
	if (!router_state.isInitializing && previousPage && previousPage !== nextPage) rememberPageScroll(previousPage);

	document.querySelectorAll('#pages > section').forEach(section => { section.hidden = true; });
	const page = document.getElementById(`${nextPage}-page`);
	if (!page) return;
	page.hidden = false;
	router_state.current_active_page = nextPage;

	document.querySelectorAll('.nav-item').forEach(item => {
		const active = item.dataset.page === nextPage;
		item.classList.toggle('active', active);
		if (active) item.setAttribute('aria-current', 'page');
		else item.removeAttribute('aria-current');
	});

	updateDocumentTitle(nextPage);
	const targetHash = `#${nextPage}`;
	if (historyMode === 'replace') history.replaceState({ page: nextPage }, '', targetHash);
	else if (historyMode === 'push' && (location.hash !== targetHash || previousPage !== nextPage)) {
		history.pushState({ page: nextPage }, '', targetHash);
	}

	document.body.dataset.page = nextPage;
	document.dispatchEvent(new CustomEvent('tcp:page-change', {
		detail: { page: nextPage, previousPage },
	}));

	if (!router_state.isInitializing && (nextPage === 'settings' || nextPage === 'adv')) {
		void ensureSettingsInitialized();
	}
	if (!router_state.isInitializing && update) void updateUI();
	restorePageScroll(nextPage, !restoreScroll);
	scheduleRealtimeUpdate(nextPage === 'stats' || nextPage === 'logs' ? 1200 : 5000);
}

window.addEventListener('popstate', event => {
	showPage(event.state?.page || pageFromLocation(), {
		historyMode: 'none',
		restoreScroll: true,
	});
});

window.addEventListener('hashchange', () => {
	const page = normalizedPage(pageFromLocation());
	if (page !== router_state.current_active_page) {
		showPage(page, { historyMode: 'none', restoreScroll: true });
	}
});

async function updateUI() {
	switch (router_state.current_active_page) {
		case 'home':
			updateHomeUI();
			break;
		case 'logs':
			updateLogsUI();
			break;
		case 'stats':
			await updateStats();
			break;
		default:
			break;
	}
}

async function refreshCurrentPage({ announceResult = false } = {}) {
	if (refreshPromise) return refreshPromise;
	refreshPromise = (async () => {
		setGlobalBusy(true);
		try {
			await updateModuleStatus(true);
			lastStatusUpdate = Date.now();
			if (router_state.current_active_page === 'logs') await read_log_file(true);
			else if (router_state.current_active_page === 'stats') await updateStats();
			else updateHomeUI();
			if (announceResult) announce(localText('Refresh complete', '刷新完成'));
		} catch (error) {
			console.error('Manual refresh failed:', error);
			if (announceResult) announce(localText('Refresh failed', '刷新失败'));
		} finally {
			setGlobalBusy(false);
			refreshPromise = null;
		}
	})();
	return refreshPromise;
}

const runRealtimeUpdate = async () => {
	updateTimer = null;
	if (document.hidden) return;
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
		console.error('Realtime update failed:', error);
	} finally {
		const page = router_state.current_active_page;
		const delay = page === 'logs' || page === 'stats' ? 5000
			: page === 'home' ? 10000 : 15000;
		scheduleRealtimeUpdate(delay);
	}
};

document.addEventListener('visibilitychange', () => {
	if (document.hidden) {
		if (updateTimer) clearTimeout(updateTimer);
		updateTimer = null;
		return;
	}
	void refreshCurrentPage();
	scheduleRealtimeUpdate(5000);
});

document.addEventListener('tcp:refresh', () => void refreshCurrentPage({ announceResult: true }));

document.addEventListener('DOMContentLoaded', async () => {
	await I18N.init();
	initProductUI();
	syncAdvancedNavVisibility();
	initMotion();
	await initDynamicColorTheme();
	await updateModuleInformation();
	initBaselineUI();

	document.querySelectorAll('.link-chip').forEach(chip => {
		chip.addEventListener('click', async event => {
			event.preventDefault();
			const url = chip.dataset.url;
			if (/^https?:\/\/[^\s"'`$;|&<>()\\]+$/.test(url)) {
				await exec(`am start -a android.intent.action.VIEW -d "${url}"`);
			}
		});
	});

	document.querySelectorAll('.nav-item').forEach(item => {
		item.addEventListener('click', event => {
			event.preventDefault();
			showPage(item.dataset.page, { historyMode: 'push', restoreScroll: false });
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

	const initialPage = normalizedPage(pageFromLocation());
	showPage(initialPage, { historyMode: 'replace', restoreScroll: false, update: false });
	await updateUI();
	scheduleRealtimeUpdate(5000);

	document.addEventListener('i18n-changed', () => {
		updateHomeUI();
		updateDocumentTitle(router_state.current_active_page);
		syncProductUILanguage();
		void updateUI();
	});
});

export default router_state;
