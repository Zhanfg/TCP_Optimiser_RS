import { exec, toast, shellQuote } from './kernelsu.js';
import I18N from './i18n.js';
import { formatLocalDateTime } from './common.js';
import router_state from './router.js';

const SOURCE_MARKER = '__TCP_OPTIMISER_SOURCE__:';
const MAX_LINES_PER_SOURCE = 800;
let previousRenderSignature = null;
let filterText = '';
let followTail = true;
let toolbar = null;
let initialized = false;

function localText(english, chinese) {
	const language = I18N.currentLang || localStorage.getItem('tcp_lang') || document.documentElement.lang;
	return String(language).toLowerCase().startsWith('zh') ? chinese : english;
}

function sourceDefinitions() {
	const moduleDir = router_state.moduleInformation?.moduleDir;
	if (!moduleDir) return [];
	return [
		{ key: 'service', label: localText('Service', '服务日志'), path: `${moduleDir}/service.log` },
		{ key: 'debug', label: localText('Debug', '调试日志'), path: `${moduleDir}/debug.log` },
		{ key: 'restore', label: localText('Restore', '恢复日志'), path: `${moduleDir}/uninstall-restore.log` },
	];
}

function isSafeExportPath(value) {
	const path = String(value).trim();
	return path.length >= 2 && path.length <= 220 && !/[\0\r\n]/.test(path)
		&& ['/sdcard/', '/storage/emulated/0/', '/data/media/0/'].some(prefix => path.startsWith(prefix))
		&& !path.split('/').includes('..');
}

function parseCombinedLogs(output, sources) {
	const labels = new Map(sources.map(source => [source.key, source.label]));
	const entries = [];
	let currentSource = sources[0]?.key || 'service';
	for (const rawLine of String(output || '').split('\n')) {
		if (rawLine.startsWith(SOURCE_MARKER)) {
			currentSource = rawLine.slice(SOURCE_MARKER.length).trim() || currentSource;
			entries.push({ source: currentSource, sourceLabel: labels.get(currentSource) || currentSource, message: '', type: 'source' });
			continue;
		}
		if (!rawLine.trim()) continue;
		entries.push({
			source: currentSource,
			sourceLabel: labels.get(currentSource) || currentSource,
			message: rawLine,
			type: classifyLogLevel(rawLine),
		});
	}
	return entries;
}

function classifyLogLevel(message) {
	if (/\b(ERROR|FATAL|PANIC|FAIL(?:ED|URE)?)\b|\[失败\]|错误[:：]/i.test(message)) return 'error';
	if (/\b(WARN(?:ING)?)\b|\[警告\]|注意[:：]/i.test(message)) return 'warn';
	return 'info';
}

export async function addLog(message) {
	try {
		const moduleDir = router_state.moduleInformation?.moduleDir;
		if (!moduleDir) return;
		const logFile = `${moduleDir}/service.log`;
		await exec(`printf '%s\\n' ${shellQuote(`${formatLocalDateTime()} - ${message}`)} >> ${shellQuote(logFile)}`);
	} catch (error) {
		console.error('Error adding to log file:', error);
	}
}

export async function read_log_file(force = false) {
	const sources = sourceDefinitions();
	if (sources.length === 0) {
		router_state.logsList = [];
		router_state.logsError = localText('Module directory is unavailable.', '无法获取模块目录。');
		previousRenderSignature = null;
		return;
	}
	const command = sources.map(source => {
		const marker = `${SOURCE_MARKER}${source.key}`;
		return `if [ -r ${shellQuote(source.path)} ]; then printf '%s\\n' ${shellQuote(marker)}; tail -n ${MAX_LINES_PER_SOURCE} ${shellQuote(source.path)}; fi`;
	}).join('; ');
	try {
		const { stdout } = await exec(command);
		router_state.logsList = parseCombinedLogs(stdout, sources);
		router_state.logsError = null;
		if (force) previousRenderSignature = null;
	} catch (error) {
		console.error('Error reading log files:', error);
		router_state.logsError = localText(
			'Log files could not be read. Root bridge or file permissions may be unavailable.',
			'无法读取日志。Root 桥接或文件权限可能不可用。',
		);
		if (force) previousRenderSignature = null;
	}
}

function filteredEntries() {
	const query = filterText.trim().toLocaleLowerCase();
	if (!query) return router_state.logsList;
	return router_state.logsList.filter(entry =>
		entry.type === 'source'
		|| entry.message.toLocaleLowerCase().includes(query)
		|| entry.sourceLabel.toLocaleLowerCase().includes(query));
}

function addLogToScreen(entry, container) {
	const logEntry = document.createElement('div');
	logEntry.className = 'log-entry';
	logEntry.dataset.level = entry.type;
	if (entry.type === 'source') {
		logEntry.textContent = entry.sourceLabel;
		container.appendChild(logEntry);
		return;
	}
	logEntry.textContent = entry.message;
	if (entry.message.length > 180 || entry.message.includes('\t')) {
		logEntry.classList.add('is-collapsible');
		logEntry.tabIndex = 0;
		logEntry.setAttribute('role', 'button');
		logEntry.setAttribute('aria-expanded', 'false');
		const toggle = () => {
			const expanded = logEntry.classList.toggle('is-expanded');
			logEntry.setAttribute('aria-expanded', String(expanded));
		};
		logEntry.addEventListener('click', toggle);
		logEntry.addEventListener('keydown', event => {
			if (event.key !== 'Enter' && event.key !== ' ') return;
			event.preventDefault();
			toggle();
		});
	}
	container.appendChild(logEntry);
}

function createEmptyState(container, filtered = false) {
	const empty = document.createElement('div');
	empty.className = 'log-empty';
	const icon = document.createElement('span');
	icon.className = 'log-empty__icon';
	icon.setAttribute('aria-hidden', 'true');
	const glyph = document.createElement('span');
	glyph.className = 'ui-icon icon-scroll-text';
	icon.appendChild(glyph);
	const title = document.createElement('strong');
	title.textContent = filtered
		? localText('No matching entries', '没有匹配的日志')
		: I18N.t('logs_empty');
	const description = document.createElement('small');
	description.textContent = filtered
		? localText('Change or clear the search filter.', '请更改或清除搜索条件。')
		: I18N.t('logs_empty_desc');
	empty.append(icon, title, description);
	container.appendChild(empty);
}

function updateToolbarLanguage() {
	if (!toolbar) return;
	const search = toolbar.querySelector('#log-filter-input');
	if (search) {
		search.placeholder = localText('Filter logs', '筛选日志');
		search.setAttribute('aria-label', localText('Filter log entries', '筛选日志条目'));
	}
	const refresh = toolbar.querySelector('#log-refresh-btn');
	if (refresh) refresh.textContent = localText('Refresh', '刷新');
	const copy = toolbar.querySelector('#log-copy-btn');
	if (copy) copy.textContent = localText('Copy', '复制');
	const follow = toolbar.querySelector('#log-follow-btn');
	if (follow) {
		follow.textContent = localText('Follow', '跟随');
		follow.title = localText('Keep the newest log entry visible', '自动保持最新日志可见');
	}
}

function ensureToolbar() {
	if (toolbar?.isConnected) return toolbar;
	const header = document.querySelector('#logs-page .log-header');
	if (!header) return null;
	toolbar = document.createElement('div');
	toolbar.className = 'log-toolbar';
	toolbar.innerHTML = `
		<input id="log-filter-input" class="log-toolbar__search" type="search" autocomplete="off" spellcheck="false">
		<div class="log-toolbar__actions">
			<button type="button" id="log-refresh-btn" class="log-toolbar__button"></button>
			<button type="button" id="log-copy-btn" class="log-toolbar__button"></button>
			<button type="button" id="log-follow-btn" class="log-toolbar__button" aria-pressed="true"></button>
		</div>`;
	header.after(toolbar);
	updateToolbarLanguage();
	const filter = toolbar.querySelector('#log-filter-input');
	filter.addEventListener('input', () => {
		filterText = filter.value;
		previousRenderSignature = null;
		updateLogsUI();
	});
	toolbar.querySelector('#log-refresh-btn').addEventListener('click', async () => {
		const button = toolbar.querySelector('#log-refresh-btn');
		button.disabled = true;
		try {
			await read_log_file(true);
			updateLogsUI();
		} finally {
			button.disabled = false;
		}
	});
	toolbar.querySelector('#log-copy-btn').addEventListener('click', async () => {
		const text = exportText(filteredEntries());
		try {
			if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(text);
			else fallbackCopy(text);
			toast(localText('Logs copied', '日志已复制'));
		} catch (error) {
			console.error('Log copy failed:', error);
			toast(localText('Copy failed', '复制失败'));
		}
	});
	toolbar.querySelector('#log-follow-btn').addEventListener('click', event => {
		followTail = !followTail;
		event.currentTarget.setAttribute('aria-pressed', String(followTail));
		if (followTail) {
			const content = document.getElementById('log-content');
			content.scrollTop = content.scrollHeight;
		}
	});
	return toolbar;
}

function fallbackCopy(text) {
	const textarea = document.createElement('textarea');
	textarea.value = text;
	textarea.style.position = 'fixed';
	textarea.style.opacity = '0';
	document.body.appendChild(textarea);
	textarea.select();
	if (!document.execCommand('copy')) throw new Error('copy command failed');
	textarea.remove();
}

function exportText(entries) {
	return entries.map(entry => entry.type === 'source'
		? `\n===== ${entry.sourceLabel} =====`
		: entry.message).join('\n').trimStart();
}

export function updateLogsUI() {
	if (router_state.isInitializing) return;
	ensureToolbar();
	const logContent = document.getElementById('log-content');
	const logsHeading = document.getElementById('logs-heading');
	if (!logContent || !logsHeading) return;
	const entries = filteredEntries();
	const lineCount = router_state.logsList.filter(entry => entry.type !== 'source').length;
	logsHeading.textContent = `${I18N.t('logs_heading')}${lineCount > 0 ? ` (${lineCount})` : ''}`;

	const signature = JSON.stringify({
		entries: entries.map(entry => [entry.source, entry.type, entry.message]),
		error: router_state.logsError,
		filterText,
		language: I18N.currentLang,
	});
	if (signature === previousRenderSignature) return;

	const distanceFromBottom = logContent.scrollHeight - logContent.scrollTop - logContent.clientHeight;
	const wasNearBottom = distanceFromBottom < 96;
	const previousScrollTop = logContent.scrollTop;
	logContent.replaceChildren();
	if (router_state.logsError) {
		const error = document.createElement('div');
		error.className = 'log-error-state';
		error.textContent = router_state.logsError;
		logContent.appendChild(error);
	} else if (entries.filter(entry => entry.type !== 'source').length === 0) {
		createEmptyState(logContent, Boolean(filterText.trim()));
	} else {
		for (const entry of entries) addLogToScreen(entry, logContent);
	}
	if (followTail && wasNearBottom) logContent.scrollTop = logContent.scrollHeight;
	else logContent.scrollTop = Math.min(previousScrollTop, Math.max(0, logContent.scrollHeight - logContent.clientHeight));
	previousRenderSignature = signature;
}

export function initLogs() {
	if (initialized) return;
	initialized = true;
	ensureToolbar();
	const clearBtn = document.getElementById('clear-logs-btn');
	clearBtn?.addEventListener('click', async () => {
		const confirmed = window.confirm(localText(
			'Clear service, debug and restoration logs?',
			'确定清除服务、调试与恢复日志吗？',
		));
		if (!confirmed) return;
		const sources = sourceDefinitions();
		try {
			const files = sources.map(source => shellQuote(source.path)).join(' ');
			await exec(`rm -f ${files}; : > ${shellQuote(sources[0].path)}`);
			router_state.logsList = [];
			router_state.logsError = null;
			previousRenderSignature = null;
			updateLogsUI();
			toast(localText('Logs cleared', '日志已清除'));
		} catch (error) {
			console.error('Error clearing log files:', error);
			toast(I18N.t('toast_clear_fail'));
		}
	});

	const pathInput = document.getElementById('log-path-input');
	if (pathInput) {
		pathInput.value = localStorage.getItem('tcp_log_path') || '/sdcard/Download/tcp_optimiser.log';
		pathInput.spellcheck = false;
		pathInput.autocomplete = 'off';
		pathInput.addEventListener('change', () => localStorage.setItem('tcp_log_path', pathInput.value));
	}
	document.getElementById('log-save-btn')?.addEventListener('click', async () => {
		const destination = pathInput?.value?.trim() || '/sdcard/Download/tcp_optimiser.log';
		if (!isSafeExportPath(destination)) {
			toast(I18N.t('toast_error'));
			return;
		}
		const parent = destination.slice(0, destination.lastIndexOf('/')) || '/sdcard/Download';
		const content = exportText(router_state.logsList);
		try {
			await exec(`mkdir -p ${shellQuote(parent)} && printf '%s' ${shellQuote(content)} > ${shellQuote(destination)}`);
			toast(`${I18N.t('logs_saved_to')} ${destination}`);
		} catch (error) {
			console.error('Error saving logs:', error);
			toast(localText('Save failed', '保存失败'));
		}
	});

	document.addEventListener('i18n-changed', () => {
		updateToolbarLanguage();
		previousRenderSignature = null;
		updateLogsUI();
	});
	document.addEventListener('tcp:page-change', event => {
		if (event.detail?.page !== 'logs') return;
		void read_log_file(true).then(updateLogsUI);
	});
	void read_log_file(true).then(updateLogsUI);
}
