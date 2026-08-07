import { exec, shellQuote } from './kernelsu.js';
import I18N from './i18n.js';
import { formatLocalDateTime } from './common.js';
import router_state from './router.js';
import { initRuntimeControlUI } from './runtime-control-ui.js';

let panel = null;
let status = null;
let lastError = null;
let loading = false;
let initialized = false;

function localText(english, chinese) {
	const language = I18N.currentLang || localStorage.getItem('tcp_lang') || document.documentElement.lang;
	return String(language).toLowerCase().startsWith('zh') ? chinese : english;
}

function previewAllowed() {
	const localHost = ['localhost', '127.0.0.1', '[::1]'].includes(location.hostname);
	return ['http:', 'https:'].includes(location.protocol) && localHost;
}

function previewStatus() {
	return {
		healthy: true,
		version: 1,
		captured_at_epoch: Math.floor(Date.now() / 1000) - 86400,
		sysctl_count: 42,
		interface_count: 2,
		interface_names: ['wlan0', 'rmnet_data0'],
		file_size_bytes: 4096,
		path: '/data/adb/modules/tcp_optimiser/baseline-v1.json',
	};
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
		? localText('Valid', '有效')
		: state === 'loading'
			? localText('Reading', '读取中')
			: localText('Unavailable', '不可用');
	row.append(text, badge);
	return row;
}

function ensurePanel() {
	if (panel?.isConnected) return panel;
	const home = document.getElementById('home-page');
	if (!home) return null;
	panel = document.createElement('details');
	panel.id = 'baseline-health-panel';
	panel.className = 'capability-panel collapsible-panel baseline-health-panel';
	panel.innerHTML = `
		<summary class="panel-heading capability-heading collapsible-summary">
			<div>
				<p class="panel-kicker" id="baseline-eyebrow"></p>
				<h3 id="baseline-title"></h3>
			</div>
			<span class="collapsible-summary__actions">
				<span id="baseline-health-count" class="capability-count"></span>
				<span class="collapsible-chevron ui-icon icon-chevron-down" aria-hidden="true"></span>
			</span>
		</summary>
		<div class="collapsible-panel__body">
			<p id="baseline-description" class="capability-help"></p>
			<div id="baseline-health-list" class="verification-list" aria-live="polite"></div>
			<button type="button" id="baseline-refresh-btn" class="btn-outline"></button>
		</div>`;
	const capability = document.getElementById('home-capability-panel');
	if (capability) capability.after(panel);
	else home.appendChild(panel);
	const saved = sessionStorage.getItem('tcp_details_baseline-health-panel');
	if (saved !== null) panel.open = saved === 'true';
	panel.addEventListener('toggle', () => {
		sessionStorage.setItem('tcp_details_baseline-health-panel', String(panel.open));
		if (panel.open && !status && !loading) void refreshBaselineStatus();
	});
	panel.querySelector('#baseline-refresh-btn')?.addEventListener('click', () => void refreshBaselineStatus(true));
	updateLabels();
	render();
	return panel;
}

function updateLabels() {
	if (!panel) return;
	panel.querySelector('#baseline-eyebrow').textContent = localText('Rollback evidence', '回滚证据');
	panel.querySelector('#baseline-title').textContent = localText('Kernel baseline', '内核原始基线');
	panel.querySelector('#baseline-description').textContent = localText(
		'Read-only validation of the original kernel state recorded before tuning.',
		'只读校验调参前记录的原始内核状态，不会创建或修改快照。',
	);
	panel.querySelector('#baseline-refresh-btn').textContent = localText('Check baseline', '检查基线');
}

function render() {
	if (!ensurePanel()) return;
	updateLabels();
	const count = panel.querySelector('#baseline-health-count');
	const list = panel.querySelector('#baseline-health-list');
	const refresh = panel.querySelector('#baseline-refresh-btn');
	refresh.disabled = loading;
	list.replaceChildren();
	if (loading) {
		count.textContent = localText('Reading…', '读取中…');
		panel.dataset.state = 'loading';
		list.appendChild(createRow(
			localText('Baseline file', '基线文件'),
			localText('Validating the existing snapshot', '正在校验现有快照'),
			'loading',
		));
		return;
	}
	if (lastError || !status) {
		count.textContent = localText('Unavailable', '不可用');
		panel.dataset.state = 'unavailable';
		list.appendChild(createRow(
			localText('Baseline file', '基线文件'),
			lastError || localText('No result yet', '尚未检查'),
			'unavailable',
		));
		return;
	}
	count.textContent = localText('Valid snapshot', '快照有效');
	panel.dataset.state = 'match';
	const captureTime = formatLocalDateTime(new Date(status.captured_at_epoch * 1000));
	const interfaces = status.interface_names?.length
		? status.interface_names.join(', ')
		: localText('No interface qdisc recorded yet', '尚未记录接口队列');
	list.append(
		createRow(localText('Schema', '快照格式'), `v${status.version} · ${status.file_size_bytes} B`),
		createRow(localText('Captured', '捕获时间'), captureTime),
		createRow(localText('Managed sysctls', '受管 sysctl'), String(status.sysctl_count)),
		createRow(localText('Interface qdiscs', '接口队列'), `${status.interface_count} · ${interfaces}`),
	);
}

function baselineCommand() {
	const moduleDir = router_state.moduleInformation?.moduleDir;
	if (!moduleDir) throw new Error(localText('Module is not installed.', '模块尚未安装。'));
	const binaryRoot = shellQuote(`${moduleDir}/bin`);
	return `abi=$(getprop ro.product.cpu.abi 2>/dev/null); case "$abi" in arm64-v8a) abi=arm64-v8a ;; armeabi-v7a|armeabi) abi=armeabi-v7a ;; x86_64) abi=x86_64 ;; *) exit 64 ;; esac; bin=${binaryRoot}/$abi/tcp_optimiser; [ -x "$bin" ] || exit 65; "$bin" baseline-status`;
}

export async function refreshBaselineStatus(force = false) {
	if (loading) return;
	if (!force && status && !lastError) return;
	loading = true;
	lastError = null;
	render();
	try {
		const { stdout } = await exec(baselineCommand());
		const line = stdout.trim().split('\n').filter(Boolean).at(-1);
		const parsed = JSON.parse(line || '{}');
		if (parsed.healthy !== true || !Number.isFinite(parsed.captured_at_epoch)) {
			if (previewAllowed()) status = previewStatus();
			else throw new Error(localText('Baseline status is incomplete.', '基线状态不完整。'));
		} else {
			status = parsed;
		}
	} catch (error) {
		if (previewAllowed()) {
			status = previewStatus();
			lastError = null;
		} else {
			console.error('Baseline status unavailable:', error);
			status = null;
			lastError = String(error?.message || error).replace(/^tcp_optimiser:\s*error:\s*/i, '');
		}
	} finally {
		loading = false;
		render();
	}
}

export function initBaselineUI() {
	if (initialized) return;
	initialized = true;
	initRuntimeControlUI();
	ensurePanel();
	document.addEventListener('i18n-changed', () => {
		updateLabels();
		render();
	});
	document.addEventListener('tcp:page-change', event => {
		if (event.detail?.page === 'home') void refreshBaselineStatus();
	});
	document.addEventListener('tcp:refresh', () => void refreshBaselineStatus(true));
	void refreshBaselineStatus();
}
