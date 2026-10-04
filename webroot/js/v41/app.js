import { state, patch } from './store.js';
import { initLocale, language, setLanguage, t } from './locale.js';
import * as api from './api.js';
import {
	homeTemplate, renderHome, renderHomeDetails, renderHomeVerification,
	settingsTemplate, handleSettingsAction,
	statsTemplate, renderStats, renderStatsDetails, logsTemplate, renderLogs,
	selectChoice, applyTheme,
} from './views.js';

const outlet = () => document.getElementById('app-outlet');
let timer = null;
let pageToken = 0;
let advancedModule = null;
let interactionUntil = 0;

function navLabel(page) {
	const zh = language() === 'zh';
	return {
		home: zh ? '首页' : 'Home',
		stats: zh ? '统计' : 'Stats',
		settings: zh ? '设置' : 'Settings',
		logs: zh ? '日志' : 'Logs',
	}[page] || page;
}

function syncNavLabels() {
	document.querySelectorAll('.nav-btn').forEach(btn => {
		const label = btn.querySelector('small');
		if (label) label.textContent = navLabel(btn.dataset.page);
	});
}

function setBusy(value) {
	document.documentElement.classList.toggle('busy', value);
}

function setStatus(text, ok = true) {
	const chip = document.getElementById('shell-status');
	if (!chip) return;
	chip.textContent = text;
	chip.dataset.ok = String(ok);
}

async function mount(page) {
	clearTimeout(timer);
	timer = null;
	state.page = page;
	const token = ++pageToken;
	document.querySelectorAll('.nav-btn').forEach(btn => {
		btn.classList.toggle('active', btn.dataset.page === page);
	});
	const title = document.getElementById('shell-title');
	if (title) title.textContent = navLabel(page);

	if (page === 'home') {
		outlet().innerHTML = homeTemplate();
		renderHome();
		await refreshHome(token, true);
		schedule(4000, token, () => refreshHome(token, false));
		return;
	}

	if (page === 'settings') {
		setBusy(true);
		try {
			const [runtime, settings] = await Promise.all([
				api.runtime(false),
				api.readSettings(),
			]);
			if (token !== pageToken) return;
			patch({ runtime, settings });
			outlet().innerHTML = settingsTemplate();
		} finally { setBusy(false); }
		return;
	}

	if (page === 'stats') {
		outlet().innerHTML = statsTemplate();
		await refreshStats(token);
		schedule(3000, token, () => refreshStats(token));
		return;
	}

	if (page === 'logs') {
		outlet().innerHTML = logsTemplate();
		await refreshLogs(token);
		return;
	}
}

function inputPending() {
	try {
		return navigator.scheduling?.isInputPending?.({ includeContinuous: true }) === true;
	} catch (_) {
		return false;
	}
}

function schedule(ms, token, fn) {
	clearTimeout(timer);
	timer = setTimeout(async () => {
		if (token !== pageToken || document.hidden) return;
		if (performance.now() < interactionUntil || inputPending()) {
			schedule(450, token, fn);
			return;
		}
		try { await fn(); } catch (_) {}
		if (token === pageToken && !document.hidden) schedule(ms, token, fn);
	}, ms);
}

async function refreshHome(token, force) {
	try {
		const runtime = await api.runtime(force);
		if (token !== pageToken) return;
		patch({ runtime });
		renderHome(runtime);
		setStatus(
			runtime.module_active === false
				? t('未运行', 'Stopped')
				: t('正常', 'Live'),
			runtime.module_active !== false,
		);
	} catch (error) {
		if (token !== pageToken) return;
		setStatus(t('读取失败', 'Unavailable'), false);
	}
}

async function refreshStats(token, details = false) {
	try {
		const stats = await api.sampleStats(details);
		if (token !== pageToken) return;
		patch({ stats });
		renderStats(stats);
		if (details) renderStatsDetails(stats);
	} catch (_) {}
}

async function refreshLogs(token) {
	try {
		const text = await api.readLogs();
		if (token !== pageToken) return;
		renderLogs(text);
	} catch (_) {
		if (token === pageToken) renderLogs('');
	}
}

function routeFromHash() {
	const value = location.hash.replace(/^#/, '');
	return ['home','stats','settings','logs'].includes(value) ? value : 'home';
}

document.addEventListener('click', async event => {
	const nav = event.target.closest('.nav-btn');
	if (nav) {
		const page = nav.dataset.page;
		if (page && page !== state.page) {
			history.pushState({ page }, '', `#${page}`);
			await mount(page);
		}
		return;
	}

	const choice = event.target.closest('.choice');
	if (choice) {
		selectChoice(choice);
		return;
	}

	const languageButton = event.target.closest('[data-lang]');
	if (languageButton) {
		setLanguage(languageButton.dataset.lang);
		syncNavLabels();
		await mount(state.page);
		return;
	}

	const theme = event.target.closest('[data-theme]');
	if (theme) {
		applyTheme(theme.dataset.theme);
		return;
	}

	const actionEl = event.target.closest('[data-action]');
	if (!actionEl) return;
	const action = actionEl.dataset.action;

	try {
		if (action === 'refresh-home') await refreshHome(pageToken, true);
		else if (action === 'home-details') {
			actionEl.disabled = true;
			try {
				const details = await api.runtimeDetails();
				renderHomeDetails(details);
			} finally { actionEl.disabled = false; }
		}
		else if (action === 'home-verify') {
			actionEl.disabled = true;
			try {
				const verified = await api.runtimeVerification();
				renderHomeVerification(verified);
			} finally { actionEl.disabled = false; }
		}
		else if (action === 'refresh-stats') await refreshStats(pageToken);
		else if (action === 'stats-details') await refreshStats(pageToken, true);
		else if (action === 'refresh-logs') await refreshLogs(pageToken);
		else if (action === 'clear-logs') {
			await api.clearLogs();
			await refreshLogs(pageToken);
		}
		else if (['load-qdiscs','apply-fast','apply-full','refresh-profile','verify'].includes(action)) {
			await handleSettingsAction(action);
		}
		else if (action === 'open-advanced') {
			const host = document.getElementById('advanced-host');
			if (!host) return;
			host.hidden = false;
			actionEl.disabled = true;
			try {
				advancedModule ||= await import('./advanced.js');
				await advancedModule.mountAdvanced(host);
				host.scrollIntoView({ block: 'start', behavior: 'auto' });
			} finally {
				actionEl.disabled = false;
			}
		}
		else if (action === 'close-advanced') {
			const host = document.getElementById('advanced-host');
			if (host) {
				host.hidden = true;
				host.replaceChildren();
				delete host.dataset.loaded;
			}
		}
		else if (action === 'save-advanced') {
			const host = document.getElementById('advanced-host');
			if (!host) return;
			actionEl.disabled = true;
			try {
				advancedModule ||= await import('./advanced.js');
				await advancedModule.saveAdvanced(host);
				actionEl.textContent = t('已保存', 'Saved');
			} finally {
				setTimeout(() => {
					actionEl.disabled = false;
					actionEl.textContent = t('保存高级参数', 'Save advanced controls');
				}, 700);
			}
		}
	} catch (error) {
		setStatus(error?.message || 'Error', false);
	}
});

function markInteraction() {
	interactionUntil = performance.now() + 420;
}

window.addEventListener('scroll', markInteraction, { passive: true });
window.addEventListener('touchstart', markInteraction, { passive: true });
window.addEventListener('touchmove', markInteraction, { passive: true });

window.addEventListener('popstate', () => void mount(routeFromHash()));
document.addEventListener('visibilitychange', () => {
	if (!document.hidden) void mount(state.page);
	else {
		clearTimeout(timer);
		timer = null;
	}
});

async function start() {
	const mode = localStorage.getItem('tcp_themeMode') || 'auto';
	applyTheme(mode);
	setBusy(true);
	try {
		initLocale();
		syncNavLabels();
		patch({ module: api.getModuleInfo() });
		document.documentElement.classList.add('ready');
		await mount(routeFromHash());
	} finally {
		setBusy(false);
	}
}

void start();
