import { exec, toast } from './kernelsu.js';
import I18N from './i18n.js';
import { formatLocalDateTime } from './common.js';
import router_state from './router.js';

const logHeadingDefaultValue = "Logs";
let prev_logs_count = 0;

export async function addLog(message) {
	try {
		const LOGFILE = `${router_state.moduleInformation.moduleDir}/service.log`;
		const safeMsg = message.replace(/'/g, "'\\''");
		await exec(`printf '%s\\n' '${formatLocalDateTime()} - ${safeMsg}' >> "${LOGFILE}"`);
		// Log rotation is handled by utils.sh log_print()
	} catch (error) {
		console.error('Error adding to log file:', error);
	}
}

export async function read_log_file() {
	try {
		const { stdout: logs } = await exec(`cat "${router_state.moduleInformation.moduleDir}/service.log"`);
		router_state.logsList = logs.trim().split('\n').filter(line => line.length > 0);
	} catch (error) {
		console.error('Error reading log file:', error);
		router_state.logsList = [];
	}
}

function addLogToScreen(message) {
	const logEntry = document.createElement('div');
	logEntry.textContent = message;
	const logContent = document.getElementById('log-content');
	logContent.appendChild(logEntry);
	logContent.scrollTop = logContent.scrollHeight;
}

export function updateLogsUI() {
	if (router_state.isInitializing) return;
	const logContent = document.getElementById('log-content');
	const logsHeading = document.getElementById('logs-heading');

	if (router_state.logsList.length !== prev_logs_count) {
		const label = I18N.t('logs_heading');
	logsHeading.textContent = `${label}${router_state.logsList.length > 0 ? ` (${router_state.logsList.length})` : ""}`;
		logContent.innerHTML = '';
		if (router_state.logsList.length === 0) {
			const emptyDiv = document.createElement('div');
			emptyDiv.className = 'log-empty';
			emptyDiv.textContent = I18N.t('logs_empty');
			logContent.appendChild(emptyDiv);
		} else {
			router_state.logsList.forEach(log => addLogToScreen(log));
		}
		prev_logs_count = router_state.logsList.length;
	}
}

export async function initLogs() {
	const clearBtn = document.getElementById('clear-logs-btn');
	clearBtn.addEventListener('click', async () => {
		try {
			await exec(`rm -f "${router_state.moduleInformation.moduleDir}/service.log"`);
			document.getElementById('log-content').innerHTML = `<div class="log-empty">${I18N.t('logs_empty')}</div>`;
			document.getElementById('logs-heading').textContent = logHeadingDefaultValue;
			router_state.logsList = [];
			prev_logs_count = 0;
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
		try {
			await exec(`cp "${src}" "${dest}"`);
			toast(`${I18N.t('logs_saved_to')} ${dest}`);
		} catch (e) {
			toast(I18N.t('toast_clear_fail'));
		}
	});

	router_state.isInitializing = false;
	updateLogsUI();
}
