import { exec, toast, shellQuote } from './kernelsu.js';
import I18N from './i18n.js';
import { formatLocalDateTime } from './common.js';
import router_state from './router.js';

let previousLogSignature = null;

function isSafeExportPath(value) {
	const path = String(value).trim();
	return path.length >= 2 && path.length <= 220 && !/[\0\r\n]/.test(path)
		&& ['/sdcard/', '/storage/emulated/0/', '/data/media/0/'].some(prefix => path.startsWith(prefix))
		&& !path.split('/').includes('..');
}

export async function addLog(message) {
	try {
		const LOGFILE = `${router_state.moduleInformation.moduleDir}/service.log`;
		await exec(`printf '%s\\n' ${shellQuote(`${formatLocalDateTime()} - ${message}`)} >> ${shellQuote(LOGFILE)}`);
		// Log rotation is handled by utils.sh log_print()
	} catch (error) {
		console.error('Error adding to log file:', error);
	}
}

export async function read_log_file() {
	try {
		const { stdout: logs } = await exec(`cat ${shellQuote(`${router_state.moduleInformation.moduleDir}/service.log`)}`);
		router_state.logsList = logs.trim().split('\n').filter(line => line.length > 0);
	} catch (error) {
		console.error('Error reading log file:', error);
		router_state.logsList = [];
	}
}

function addLogToScreen(message) {
	const logEntry = document.createElement('div');
	logEntry.className = 'log-entry';
	logEntry.textContent = message;
	if (message.length > 120) {
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
	const logContent = document.getElementById('log-content');
	logContent.appendChild(logEntry);
	logContent.scrollTop = logContent.scrollHeight;
}

export function updateLogsUI() {
	if (router_state.isInitializing) return;
	const logContent = document.getElementById('log-content');
	const logsHeading = document.getElementById('logs-heading');

	const signature = router_state.logsList.join('\n');
	if (signature !== previousLogSignature) {
		const label = I18N.t('logs_heading');
		logsHeading.textContent = `${label}${router_state.logsList.length > 0 ? ` (${router_state.logsList.length})` : ""}`;
		logContent.innerHTML = '';
		if (router_state.logsList.length === 0) {
			const emptyDiv = document.createElement('div');
			emptyDiv.className = 'log-empty';
			const icon = document.createElement('span');
			icon.className = 'log-empty__icon';
			icon.setAttribute('aria-hidden', 'true');
			const glyph = document.createElement('span');
			glyph.className = 'ui-icon icon-scroll-text';
			icon.appendChild(glyph);
			const title = document.createElement('strong');
			title.textContent = I18N.t('logs_empty');
			const description = document.createElement('small');
			description.textContent = I18N.t('logs_empty_desc');
			emptyDiv.append(icon, title, description);
			logContent.appendChild(emptyDiv);
		} else {
			router_state.logsList.forEach(log => addLogToScreen(log));
		}
		previousLogSignature = signature;
	}
}

export async function initLogs() {
	const clearBtn = document.getElementById('clear-logs-btn');
	clearBtn.addEventListener('click', async () => {
		try {
			await exec(`rm -f ${shellQuote(`${router_state.moduleInformation.moduleDir}/service.log`)}`);
			router_state.logsList = [];
			previousLogSignature = null;
			updateLogsUI();
		} catch (error) {
			console.error('Error clearing log file:', error);
			toast(I18N.t('toast_clear_fail'));
		}
	});

	// Log save
	const pathInput = document.getElementById('log-path-input');
	if (pathInput) {
		const saved = localStorage.getItem('tcp_log_path');
		pathInput.value = saved || '/sdcard/Download/tcp_optimiser.log';
		pathInput.addEventListener('change', () => {
			localStorage.setItem('tcp_log_path', pathInput.value);
		});
	}
	const saveBtn = document.getElementById('log-save-btn');
	saveBtn?.addEventListener('click', async () => {
		const dest = pathInput?.value?.trim() || '/sdcard/Download/tcp_optimiser.log';
		const src = `${router_state.moduleInformation.moduleDir}/service.log`;
		if (!isSafeExportPath(dest)) {
			toast(I18N.t('toast_error'));
			return;
		}
		try {
			await exec(`cp ${shellQuote(src)} ${shellQuote(dest)}`);
			toast(`${I18N.t('logs_saved_to')} ${dest}`);
		} catch (e) {
			toast(I18N.t('toast_clear_fail'));
		}
	});

	updateLogsUI();
}
