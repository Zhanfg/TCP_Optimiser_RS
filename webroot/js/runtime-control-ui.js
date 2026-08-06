import { exec, shellQuote, toast } from './kernelsu.js';
import I18N from './i18n.js';
import { formatLocalDateTime } from './common.js';
import router_state from './router.js';

const COMMANDS = new Set([
	'control-status',
	'checkpoint-status',
	'diff',
	'reload',
	'pause',
	'resume',
	'safe-mode',
	'safe-mode --disable',
	'restore-checkpoint',
]);

let panel = null;
let controlStatus = null;
let checkpointStatus = null;
let policyDiff = null;
let lastError = null;
let loading = false;
let actionBusy = false;
let initialized = false;

function localText(english, chinese) {
	const language = I18N.currentLang || localStorage.getItem('tcp_lang') || document.documentElement.lang;
	return String(language).toLowerCase().startsWith('zh') ? chinese : english;
}

function previewAllowed() {
	const requested = new URLSearchParams(location.search).get('preview') === '1';
	return requested || ['localhost', '127.0.0.1', '[::1]'].includes(location.hostname);
}

function previewData() {
	return {
		control: {
			format_version: 1,
			generation: 4,
			mode: 'active',
			requested_action: 'reload',
			updated_at_epoch: Math.floor(Date.now() / 1000) - 180,
			reason: 'preview',
		},
		checkpoint: {
			available: true,
			checkpoint: {
				captured_at_epoch: Math.floor(Date.now() / 1000) - 3600,
				interface: 'wlan0',
				interface_mode: 'Wi-Fi',
				algorithm: 'bbr',
				qdisc: 'fq',
				pacing_ca: 200,
				pacing_ss: 300,
			},
			consecutive_failures: 0,
			automatic_safe_mode_threshold: 3,
		},
		diff: {
			interface: 'wlan0',
			interface_mode: 'Wi-Fi',
			runtime_mode: 'active',
			write_allowed: true,
			changes: [],
			warnings: [],
		},
	};
}

function ensureStyles() {
	if (document.getElementById('runtime-control-styles')) return;
	const style = document.createElement('style');
	style.id = 'runtime-control-styles';
	style.textContent = `
		.runtime-control-panel[data-mode="safe_mode"] { border-color: var(--md-sys-color-error); }
		.runtime-control-panel[data-mode="paused"] { border-color: #d7a72b; }
		.runtime-control-actions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin-top: 12px; }
		.runtime-control-actions button { min-height: 44px; }
		.runtime-control-actions .runtime-control-wide { grid-column: 1 / -1; }
		.runtime-plan-list { display: grid; gap: 6px; margin-top: 10px; }
		.runtime-plan-change { display: grid; grid-template-columns: minmax(90px, 0.7fr) minmax(0, 1.3fr); gap: 10px; padding: 8px 10px; border-radius: 12px; background: var(--md-sys-color-surface-container-low); font-size: .76rem; }
		.runtime-plan-change code { overflow-wrap: anywhere; text-align: end; }
		@media (max-width: 419px) { .runtime-control-actions { grid-template-columns: 1fr; } .runtime-control-actions .runtime-control-wide { grid-column: auto; } }
		@media (min-width: 960px) {
			html.product-ui #home-page > #runtime-control-panel { grid-column: 2; grid-row: 3; width: 100%; align-self: start; }
			html.product-ui #home-page > #baseline-health-panel { grid-row: 4; }
		}
	`;
	document.head.appendChild(style);
}

function createRow(label, value, state = 'match') {
	const row = document.createElement('div');
	row.className = 'verification-row';
	row.dataset.state = state;
	const text = document.createElement('div');
	text.className = 'verification-row__text';
	const title = document.createElement('strong');
	title.textContent = label;
	const detail = document.createElement('span');
	detail.textContent = value;
	text.append(title, detail);
	const badge = document.createElement('span');
	badge.className = 'verification-row__state';
	badge.textContent = state === 'match'
		? localText('Ready', '正常')
		: state === 'loading'
			? localText('Reading', '读取中')
			: state === 'warn'
				? localText('Attention', '需注意')
				: localText('Unavailable', '不可用');
	row.append(text, badge);
	return row;
}

function ensurePanel() {
	if (panel?.isConnected) return panel;
	const home = document.getElementById('home-page');
	if (!home) return null;
	ensureStyles();
	panel = document.createElement('details');
	panel.id = 'runtime-control-panel';
	panel.className = 'capability-panel collapsible-panel runtime-control-panel';
	panel.innerHTML = `
		<summary class="panel-heading capability-heading collapsible-summary">
			<div>
				<p class="panel-kicker" id="runtime-control-eyebrow"></p>
				<h3 id="runtime-control-title"></h3>
			</div>
			<span class="collapsible-summary__actions">
				<span id="runtime-control-count" class="capability-count"></span>
				<span class="collapsible-chevron ui-icon icon-chevron-down" aria-hidden="true"></span>
			</span>
		</summary>
		<div class="collapsible-panel__body">
			<p id="runtime-control-description" class="capability-help"></p>
			<div id="runtime-control-list" class="verification-list" aria-live="polite"></div>
			<div id="runtime-plan-list" class="runtime-plan-list"></div>
			<div class="runtime-control-actions">
				<button type="button" id="runtime-reload-btn" class="btn-primary"></button>
				<button type="button" id="runtime-pause-btn" class="btn-outline"></button>
				<button type="button" id="runtime-resume-btn" class="btn-outline"></button>
				<button type="button" id="runtime-safe-btn" class="btn-outline"></button>
				<button type="button" id="runtime-restore-btn" class="btn-danger runtime-control-wide"></button>
				<button type="button" id="runtime-refresh-btn" class="btn-text runtime-control-wide"></button>
			</div>
		</div>`;
	const verification = document.getElementById('verification-panel');
	if (verification) verification.after(panel);
	else home.appendChild(panel);
	const saved = sessionStorage.getItem('tcp_details_runtime-control-panel');
	if (saved !== null) panel.open = saved === 'true';
	panel.addEventListener('toggle', () => {
		sessionStorage.setItem('tcp_details_runtime-control-panel', String(panel.open));
		if (panel.open && !controlStatus && !loading) void refreshRuntimeControl(true);
	});
	bindActions();
	updateLabels();
	render();
	return panel;
}

function updateLabels() {
	if (!panel) return;
	panel.querySelector('#runtime-control-eyebrow').textContent = localText('Runtime guard', '运行保护');
	panel.querySelector('#runtime-control-title').textContent = localText('Policy control', '策略运行控制');
	panel.querySelector('#runtime-control-description').textContent = localText(
		'Hot reload, read-only pause, safe mode, planned changes and the last verified policy checkpoint.',
		'管理热重载、只读暂停、安全模式、计划差异与最近一次已验证策略检查点。',
	);
	panel.querySelector('#runtime-reload-btn').textContent = localText('Reload policy', '热重载策略');
	panel.querySelector('#runtime-pause-btn').textContent = localText('Pause writes', '暂停写入');
	panel.querySelector('#runtime-resume-btn').textContent = localText('Resume', '恢复运行');
	panel.querySelector('#runtime-safe-btn').textContent = controlStatus?.mode === 'safe_mode'
		? localText('Leave safe mode', '退出安全模式')
		: localText('Enter safe mode', '进入安全模式');
	panel.querySelector('#runtime-restore-btn').textContent = localText('Restore last known good', '恢复最近正常策略');
	panel.querySelector('#runtime-refresh-btn').textContent = localText('Refresh control state', '刷新运行状态');
}

function modeLabel(mode) {
	return mode === 'active'
		? localText('Active', '正常运行')
		: mode === 'paused'
			? localText('Paused', '已暂停写入')
			: mode === 'safe_mode'
				? localText('Safe mode', '安全模式')
				: localText('Unknown', '未知');
}

function render() {
	if (!ensurePanel()) return;
	updateLabels();
	const count = panel.querySelector('#runtime-control-count');
	const list = panel.querySelector('#runtime-control-list');
	const planList = panel.querySelector('#runtime-plan-list');
	list.replaceChildren();
	planList.replaceChildren();
	if (loading) {
		panel.dataset.mode = 'loading';
		count.textContent = localText('Reading…', '读取中…');
		list.appendChild(createRow(localText('Runtime mode', '运行模式'), localText('Reading control state', '正在读取控制状态'), 'loading'));
		syncButtons();
		return;
	}
	if (lastError || !controlStatus) {
		panel.dataset.mode = 'unavailable';
		count.textContent = localText('Unavailable', '不可用');
		list.appendChild(createRow(localText('Runtime control', '运行控制'), lastError || localText('No result yet', '尚未检查'), 'unavailable'));
		syncButtons();
		return;
	}

	panel.dataset.mode = controlStatus.mode;
	count.textContent = modeLabel(controlStatus.mode);
	const modeState = controlStatus.mode === 'active' ? 'match' : 'warn';
	list.appendChild(createRow(
		localText('Runtime mode', '运行模式'),
		`${modeLabel(controlStatus.mode)} · gen ${controlStatus.generation}`,
		modeState,
	));
	list.appendChild(createRow(
		localText('Last command', '最近命令'),
		`${controlStatus.requested_action || 'initial'} · ${controlStatus.reason || '-'}`,
		'match',
	));

	const checkpoint = checkpointStatus?.checkpoint;
	const failureCount = checkpointStatus?.consecutive_failures || 0;
	const failureThreshold = checkpointStatus?.automatic_safe_mode_threshold || 3;
	list.appendChild(createRow(
		localText('Verified checkpoint', '已验证检查点'),
		checkpoint
			? `${checkpoint.algorithm} + ${checkpoint.qdisc} · ${checkpoint.interface}`
			: localText('No verified policy recorded yet', '尚未记录已验证策略'),
		checkpoint ? 'match' : 'warn',
	));
	list.appendChild(createRow(
		localText('Policy failures', '策略失败计数'),
		`${failureCount} / ${failureThreshold}`,
		failureCount > 0 ? 'warn' : 'match',
	));
	if (checkpoint?.captured_at_epoch) {
		list.appendChild(createRow(
			localText('Checkpoint time', '检查点时间'),
			formatLocalDateTime(new Date(checkpoint.captured_at_epoch * 1000)),
			'match',
		));
	}

	const changes = Array.isArray(policyDiff?.changes) ? policyDiff.changes : [];
	list.appendChild(createRow(
		localText('Planned differences', '计划差异'),
		changes.length === 0
			? localText('Current kernel state matches the configured policy', '当前内核状态与配置策略一致')
			: localText(`${changes.length} value(s) would change`, `将修改 ${changes.length} 项`),
		changes.length === 0 ? 'match' : 'warn',
	));
	for (const change of changes.slice(0, 6)) {
		const row = document.createElement('div');
		row.className = 'runtime-plan-change';
		const key = document.createElement('strong');
		key.textContent = change.key;
		const values = document.createElement('code');
		values.textContent = `${change.current ?? 'unavailable'} → ${change.planned ?? 'none'}`;
		row.append(key, values);
		planList.appendChild(row);
	}
	for (const warning of policyDiff?.warnings || []) {
		const row = document.createElement('div');
		row.className = 'runtime-plan-change';
		const key = document.createElement('strong');
		key.textContent = localText('Warning', '警告');
		const value = document.createElement('span');
		value.textContent = warning;
		row.append(key, value);
		planList.appendChild(row);
	}
	syncButtons();
}

function syncButtons() {
	if (!panel) return;
	const mode = controlStatus?.mode;
	const disabled = loading || actionBusy || !controlStatus;
	panel.querySelector('#runtime-reload-btn').disabled = disabled || mode !== 'active';
	panel.querySelector('#runtime-pause-btn').disabled = disabled || mode !== 'active';
	panel.querySelector('#runtime-resume-btn').disabled = disabled || mode !== 'paused';
	panel.querySelector('#runtime-safe-btn').disabled = disabled;
	panel.querySelector('#runtime-restore-btn').disabled = disabled || checkpointStatus?.available !== true;
	panel.querySelector('#runtime-refresh-btn').disabled = loading || actionBusy;
}

function binaryCommand(subcommand) {
	if (!COMMANDS.has(subcommand)) throw new Error('Unsupported runtime command');
	const moduleDir = router_state.moduleInformation?.moduleDir;
	if (!moduleDir) throw new Error(localText('Module is not installed.', '模块尚未安装。'));
	const binaryRoot = shellQuote(`${moduleDir}/bin`);
	return `abi=$(getprop ro.product.cpu.abi 2>/dev/null); case "$abi" in arm64-v8a) abi=arm64-v8a ;; armeabi-v7a|armeabi) abi=armeabi-v7a ;; x86_64) abi=x86_64 ;; *) exit 64 ;; esac; bin=${binaryRoot}/$abi/tcp_optimiser; [ -x "$bin" ] || exit 65; "$bin" ${subcommand}`;
}

async function readJson(subcommand) {
	const { stdout } = await exec(binaryCommand(subcommand));
	const line = stdout.trim().split('\n').filter(Boolean).at(-1);
	return JSON.parse(line || '{}');
}

async function runAction(subcommand, confirmation = null) {
	if (actionBusy) return;
	if (confirmation && !window.confirm(confirmation)) return;
	actionBusy = true;
	lastError = null;
	render();
	let accepted = false;
	try {
		await readJson(subcommand);
		accepted = true;
		toast(localText('Runtime command accepted', '运行命令已接受'));
	} catch (error) {
		console.error('Runtime command failed:', error);
		lastError = String(error?.message || error).replace(/^tcp_optimiser:\s*error:\s*/i, '');
		toast(localText('Runtime command failed', '运行命令失败'));
	}
	actionBusy = false;
	if (accepted) await refreshRuntimeControl(true);
	else render();
}

function bindActions() {
	panel.querySelector('#runtime-reload-btn')?.addEventListener('click', () => void runAction('reload'));
	panel.querySelector('#runtime-pause-btn')?.addEventListener('click', () => void runAction(
		'pause',
		localText('Pause all daemon policy writes?', '暂停 daemon 的全部策略写入吗？'),
	));
	panel.querySelector('#runtime-resume-btn')?.addEventListener('click', () => void runAction('resume'));
	panel.querySelector('#runtime-safe-btn')?.addEventListener('click', () => {
		const leaving = controlStatus?.mode === 'safe_mode';
		void runAction(
			leaving ? 'safe-mode --disable' : 'safe-mode',
			leaving
				? localText('Leave safe mode and immediately reapply the configured policy?', '退出安全模式并立即重新应用配置策略吗？')
				: localText('Enter persistent safe mode and stop all policy writes?', '进入持久安全模式并停止全部策略写入吗？'),
		);
	});
	panel.querySelector('#runtime-restore-btn')?.addEventListener('click', () => void runAction(
		'restore-checkpoint',
		localText(
			'Restore the last verified policy and remain in safe mode? The active interface must still exist.',
			'恢复最近一次已验证策略并保持安全模式吗？记录的网络接口必须仍然存在。',
		),
	));
	panel.querySelector('#runtime-refresh-btn')?.addEventListener('click', () => void refreshRuntimeControl(true));
}

export async function refreshRuntimeControl(force = false) {
	if (loading) return;
	if (!force && controlStatus && checkpointStatus && policyDiff && !lastError) return;
	loading = true;
	lastError = null;
	render();
	try {
		const [controlResult, checkpointResult, diffResult] = await Promise.allSettled([
			readJson('control-status'),
			readJson('checkpoint-status'),
			readJson('diff'),
		]);
		if (controlResult.status !== 'fulfilled') throw controlResult.reason;
		controlStatus = controlResult.value;
		checkpointStatus = checkpointResult.status === 'fulfilled'
			? checkpointResult.value
			: {
				available: false,
				checkpoint: null,
				consecutive_failures: 0,
				automatic_safe_mode_threshold: 3,
			};
		policyDiff = diffResult.status === 'fulfilled'
			? diffResult.value
			: {
				changes: [],
				warnings: [localText(
					'Policy difference is unavailable until an active physical route exists.',
					'存在活动物理网络路由后才能读取策略差异。',
				)],
			};
	} catch (error) {
		if (previewAllowed()) {
			const preview = previewData();
			controlStatus = preview.control;
			checkpointStatus = preview.checkpoint;
			policyDiff = preview.diff;
		} else {
			console.error('Runtime control status unavailable:', error);
			lastError = String(error?.message || error).replace(/^tcp_optimiser:\s*error:\s*/i, '');
		}
	} finally {
		loading = false;
		render();
	}
}

export function initRuntimeControlUI() {
	if (initialized) return;
	initialized = true;
	ensurePanel();
	document.addEventListener('i18n-changed', render);
	document.addEventListener('tcp:page-change', event => {
		if (event.detail?.page === 'home') void refreshRuntimeControl();
	});
	document.addEventListener('tcp:refresh', () => void refreshRuntimeControl(true));
	void refreshRuntimeControl();
}
