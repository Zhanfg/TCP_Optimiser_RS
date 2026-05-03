import { exec } from './kernelsu.js';
import I18N from './i18n.js';
import { updateModuleInformation } from './common.js';
import { updateModuleStatus, initHome, updateHomeUI } from './home.js';
import { initLogs, addLog, read_log_file, updateLogsUI } from './logs.js';
import { initSettings } from './settings.js';
import { updateStats, initStatsUI } from './stats.js';

// Floating nav scroll hide/show
let _lastScrollY = 0;
let _navScrollTicking = false;

function onScroll() {
	if (_navScrollTicking) return;
	_navScrollTicking = true;
	requestAnimationFrame(() => {
		const nav = document.getElementById('nav-bar');
		if (!nav) return;
		const currentY = window.scrollY;
		const delta = currentY - _lastScrollY;
		if (currentY > 60 && delta > 8) {
			nav.classList.add('scroll-hidden');
		} else if (delta < -4 || currentY < 10) {
			nav.classList.remove('scroll-hidden');
		}
		_lastScrollY = currentY;
		_navScrollTicking = false;
	});
}

window.addEventListener('scroll', onScroll, { passive: true });

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
	current_active_page: 'home',
	statsParams: { tcpConns: 0, sockStat: null, dnsServers: [], ssInfo: null, tcpCounters: null },
};

let updateInterval = null;

let _initialPopState = true;
let _ignoreNextPop = false;

function showPage(pageName, push = true) {
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
}

window.addEventListener('popstate', (e) => {
	if (_initialPopState) {
		_initialPopState = false;
		return;
	}
	if (_ignoreNextPop) {
		_ignoreNextPop = false;
		return;
	}
	const page = e.state?.page || 'home';
	showPage(page, false);
});

function updateUI() {
	switch (router_state.current_active_page) {
		case 'home': updateHomeUI(); break;
		case 'logs': updateLogsUI(); break;
		case 'stats': updateStats(); break;
	}
}

const startRealtimeUpdater = async () => {
	try {
		if (updateInterval) clearInterval(updateInterval);
		updateInterval = setInterval(async () => {
			await updateModuleStatus();
			await read_log_file();
			updateUI();
		}, 5000);
	} catch (error) {
		console.error('Error setting update loop:', error);
	}
};

document.addEventListener('DOMContentLoaded', async () => {
	await I18N.init();
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

	initHome();
	initLogs();
	await initSettings();
	initStatsUI();

	showPage('home', false);
	history.replaceState({ page: 'home' }, '', '#home');
	startRealtimeUpdater();

	document.addEventListener('i18n-changed', () => updateUI());
});

export default router_state;
