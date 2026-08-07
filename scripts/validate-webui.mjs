import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const root = process.cwd();
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8');
const exists = relative => fs.existsSync(path.join(root, relative));
const fail = message => {
	console.error(`webui validation: ${message}`);
	process.exitCode = 1;
};
const requireText = (source, needle, description) => {
	if (!source.includes(needle)) fail(description);
};
const rejectText = (source, needle, description) => {
	if (source.includes(needle)) fail(description);
};

function parseFlatLanguageFile(relative) {
	const source = read(relative);
	const seen = new Map();
	for (const match of source.matchAll(/^\s*"((?:\\.|[^"])*)"\s*:/gm)) {
		const key = JSON.parse(`"${match[1]}"`);
		const line = source.slice(0, match.index).split(/\r?\n/).length;
		if (seen.has(key)) fail(`${relative}:${line} duplicates ${key} (first at ${seen.get(key)})`);
		else seen.set(key, line);
	}
	try {
		return JSON.parse(source);
	} catch (error) {
		fail(`${relative} is invalid JSON: ${error.message}`);
		return {};
	}
}

function quotedValues(source, declaration) {
	const block = source.match(new RegExp(`${declaration}[^=]*=\\s*&?\\[([\\s\\S]*?)\\];`));
	if (!block) {
		fail(`cannot find ${declaration}`);
		return [];
	}
	return [...block[1].matchAll(/["']([a-z0-9_]+)["']/g)].map(match => match[1]);
}

const english = parseFlatLanguageFile('webroot/lang/source/string.json');
const chinese = parseFlatLanguageFile('webroot/lang/zh.json');
for (const key of Object.keys(english)) if (!(key in chinese)) fail(`zh.json is missing ${key}`);
for (const key of Object.keys(chinese)) if (!(key in english)) fail(`zh.json has unknown key ${key}`);

const html = read('webroot/index.html');
const javascriptDirectory = path.join(root, 'webroot/js');
const javascriptDirectoryEntries = fs.readdirSync(javascriptDirectory);
const forbiddenJavascriptArtifacts = javascriptDirectoryEntries.filter(file =>
	file.endsWith('.fixed')
	|| file.endsWith('.tmp')
	|| /^\.(?:cleanup(?:-marker)?|finalize|noop|stop)$/.test(file));
for (const file of forbiddenJavascriptArtifacts) fail(`temporary WebUI artifact must not be packaged: webroot/js/${file}`);
const javascriptFiles = javascriptDirectoryEntries.filter(file => file.endsWith('.js'));
const javascript = javascriptFiles.map(file => read(`webroot/js/${file}`)).join('\n');
const referencedKeys = new Set([
	...[...html.matchAll(/data-i18n(?:-placeholder|-aria-label)?="([^"]+)"/g)].map(match => match[1]),
	...[...javascript.matchAll(/I18N\.t\(\s*['"]([A-Za-z0-9_]+)['"]\s*(?:[,\)])/g)].map(match => match[1]),
]);
for (const key of referencedKeys) if (!(key in english)) fail(`translation key ${key} is referenced but undefined`);

const ids = new Map();
for (const match of html.matchAll(/\sid="([^"]+)"/g)) {
	const id = match[1];
	const line = html.slice(0, match.index).split(/\r?\n/).length;
	if (ids.has(id)) fail(`webroot/index.html:${line} duplicates id ${id} (first at ${ids.get(id)})`);
	else ids.set(id, line);
}

for (const match of html.matchAll(/<script[^>]+src="([^"]+)"/g)) {
	const relative = path.join('webroot', match[1]);
	if (!exists(relative)) fail(`missing script ${relative}`);
}

for (const file of javascriptFiles) {
	const source = read(`webroot/js/${file}`);
	for (const match of source.matchAll(/(?:import|export)\s+(?:[\s\S]*?\s+from\s+)?['"](\.\.?\/[^'"]+)['"]/g)) {
		const imported = path.normalize(path.join('webroot/js', path.dirname(file), match[1]));
		if (!exists(imported)) fail(`webroot/js/${file} imports missing ${match[1]}`);
	}
}

const rust = read('src/config.rs');
const capabilities = read('webroot/js/capabilities.js');
const rustAlgorithms = quotedValues(rust, 'pub const ALL_ALGOS');
const uiAlgorithms = quotedValues(capabilities, 'export const ALL_ALGOS');
if (JSON.stringify(rustAlgorithms) !== JSON.stringify(uiAlgorithms)) fail('Rust and WebUI algorithm lists differ');
const rustQdiscs = quotedValues(rust, 'pub const KNOWN_QDISCS');
const uiQdiscs = quotedValues(capabilities, 'export const ALL_QDISCS');
if (JSON.stringify(rustQdiscs) !== JSON.stringify(uiQdiscs)) fail('Rust and WebUI qdisc lists differ');

// Production shell invariants. These checks prevent the former WebUI from
// silently regressing to a syntax-valid but unusable mobile-only shell.
for (const required of [
	'webroot/css/product.css',
	'webroot/css/product-layout.css',
	'webroot/js/product-ui.js',
	'webroot/js/settings-ui.js',
	'webroot/js/baseline-ui.js',
	'webroot/js/runtime-control-ui.js',
	'src/baseline_status.rs',
	'src/control.rs',
	'src/checkpoint.rs',
	'src/plan.rs',
]) {
	if (!exists(required)) fail(`missing production WebUI file ${required}`);
}
const productCss = read('webroot/css/product.css');
const productLayout = read('webroot/css/product-layout.css');
const productUi = read('webroot/js/product-ui.js');
const settingsUi = read('webroot/js/settings-ui.js');
const baselineUi = read('webroot/js/baseline-ui.js');
const runtimeControlUi = read('webroot/js/runtime-control-ui.js');
const baselineStatus = read('src/baseline_status.rs');
const runtimeControl = read('src/control.rs');
const checkpoint = read('src/checkpoint.rs');
const plan = read('src/plan.rs');
const daemon = read('src/daemon.rs');
const mainRust = read('src/main.rs');
const router = read('webroot/js/router.js');
const logs = read('webroot/js/logs.js');

requireText(productCss, '@media (min-width: 960px)', 'product CSS must define a real desktop breakpoint');
requireText(productCss, 'grid-template-columns: var(--ui-rail-width)', 'desktop layout must use a navigation rail');
requireText(productCss, '.log-toolbar', 'product CSS must style the functional log toolbar');
requireText(productCss, '.log-error-state', 'log read failures need a visible error state');
requireText(productCss, 'env(safe-area-inset-bottom', 'mobile navigation must respect display cutouts and gesture areas');
requireText(productLayout, '#pages > section[hidden]', 'final layout must preserve router-hidden pages');
requireText(productLayout, '#home-page:not([hidden])', 'desktop Home grid must only apply while Home is visible');
requireText(productLayout, 'display: grid', 'desktop Home must use a real two-column grid');
requireText(productLayout, '#baseline-health-panel', 'baseline health must occupy the desktop status column');

requireText(productUi, "link.href = 'css/product.css'", 'product-ui.js must load the production stylesheet after legacy CSS');
requireText(productUi, "layout.href = 'css/product-layout.css'", 'product-ui.js must load the final layout correction after product CSS');
requireText(productUi, 'initModalManagement', 'dialogs must have centralized keyboard and focus management');
requireText(productUi, "event.key === 'Escape'", 'dialogs must support Escape dismissal');
requireText(productUi, 'syncNavTabStops', 'navigation must implement roving keyboard focus');
requireText(productUi, "document.dispatchEvent(new CustomEvent('tcp:refresh'", 'the app shell must provide an explicit refresh action');
requireText(productUi, 'syncDebugFabVisibility', 'debug FAB visibility must be synchronized after legacy CSS and settings updates');
requireText(productUi, 'TCP_Optimiser_RS', 'the live repository link must point to the Rust project');

requireText(router, "from './product-ui.js'", 'router must initialize the production WebUI shell');
requireText(router, "from './settings-ui.js'", 'router must initialize settings enhancements');
requireText(router, "from './baseline-ui.js'", 'router must initialize baseline health UI');
requireText(router, 'pageFromLocation()', 'router must restore deep-linked pages');
requireText(router, "document.addEventListener('visibilitychange'", 'background WebViews must suspend polling');
requireText(router, "document.addEventListener('tcp:refresh'", 'router must service manual refresh requests');
requireText(router, 'rememberPageScroll(previousPage)', 'page navigation must preserve reading position');
rejectText(router, "history.replaceState({ page: 'home' }, '', '#home')", 'router must not force every startup to Home');

requireText(logs, `tail -n \${MAX_LINES_PER_SOURCE}`, 'log reads must be bounded instead of loading unbounded files');
requireText(logs, 'SOURCE_MARKER', 'logs must preserve source identity across service, debug and restore files');
requireText(logs, 'SOURCE_STATUS_MARKER', 'logs must expose missing and unreadable source files');
requireText(logs, 'log-filter-input', 'logs must provide filtering');
requireText(logs, 'pendingSource', 'log filtering must remove empty source headings');
requireText(logs, 'log-follow-btn', 'logs must provide controllable tail following');
requireText(logs, 'syncFollowButton', 'log follow state must remain synchronized with scrolling');
requireText(logs, 'fallbackCopy(text)', 'clipboard rejection must fall back to the legacy copy path');
requireText(logs, 'if (sources.length === 0)', 'log clearing must guard an unavailable module directory');
requireText(logs, 'router_state.logsError', 'log failures must not be represented as an empty list');
requireText(logs, 'window.confirm', 'destructive log clearing must require confirmation');

requireText(settingsUi, 'advanced-settings-search', 'advanced controls must provide a searchable index');
requireText(settingsUi, 'window.confirm', 'immediate policy application must require confirmation');
requireText(settingsUi, 'aria-pressed', 'selectable settings controls must expose semantic state');
requireText(settingsUi, 'position: sticky', 'mobile settings actions must remain reachable above bottom navigation');
requireText(settingsUi, 'scheduleSettingsSync', 'dynamic settings synchronization must be coalesced');
requireText(settingsUi, 'bindDelegatedSync', 'settings state must resynchronize from bounded user events');
rejectText(settingsUi, 'MutationObserver', 'settings enhancements must not install self-triggering DOM observers');

requireText(mainRust, 'mod baseline_status;', 'Rust CLI must include read-only baseline status support');
requireText(mainRust, 'BaselineStatus', 'Rust CLI must expose the baseline-status command');
requireText(baselineStatus, 'without creating or modifying', 'baseline status must remain read-only');
requireText(baselineUi, 'baseline-status', 'WebUI baseline card must call the read-only command');
rejectText(baselineUi, 'capture-baseline', 'opening the WebUI must never create rollback evidence');
requireText(baselineUi, 'captured_at_epoch', 'baseline UI must surface capture time');
requireText(baselineUi, 'sysctl_count', 'baseline UI must surface managed sysctl count');
requireText(baselineUi, "['http:', 'https:']", 'baseline preview data must be restricted to local HTTP preview');
rejectText(baselineUi, 'requested ||', 'a query parameter must not enable fake baseline data on production hosts');

requireText(baselineUi, "from './runtime-control-ui.js'", 'baseline bootstrap must initialize runtime control UI');
requireText(runtimeControlUi, "'control-status'", 'runtime UI must read the persistent control state');
requireText(runtimeControlUi, "'checkpoint-status'", 'runtime UI must expose the last-known-good checkpoint');
requireText(runtimeControlUi, "'restore-checkpoint'", 'runtime UI must expose guarded checkpoint restoration');
requireText(runtimeControlUi, "'safe-mode --disable'", 'runtime UI must provide an explicit safe-mode recovery path');
requireText(runtimeControlUi, 'Promise.allSettled', 'missing routes must not hide runtime control state');
requireText(runtimeControlUi, 'window.confirm', 'runtime write controls must require confirmation where appropriate');
requireText(runtimeControlUi, 'runtime-control-panel', 'runtime controls must be visible on the Home page');
requireText(runtimeControlUi, 'actionBusy = false', 'runtime actions must release their busy state before refreshing');
requireText(runtimeControlUi, 'STATUS_MARKER', 'nonzero runtime commands must preserve structured JSON reports');
requireText(runtimeControlUi, 'error.report = result.payload', 'restore failures must preserve rollback reports');
requireText(runtimeControlUi, 'await refreshRuntimeControl(true)', 'failed runtime actions must refresh the real control state');
requireText(runtimeControlUi, 'failure_state_error', 'failure journal corruption must remain visible in the WebUI');
requireText(runtimeControlUi, "['http:', 'https:']", 'runtime preview data must be restricted to local HTTP preview');
rejectText(runtimeControlUi, 'requested ||', 'a query parameter must not enable fake runtime data on production hosts');
rejectText(runtimeControlUi, 'findLastIndex', 'runtime command parsing must support older Android WebViews');

for (const command of [
	'Plan',
	'Diff',
	'Reload',
	'Pause',
	'Resume',
	'SafeMode',
	'ControlStatus',
	'CheckpointStatus',
	'RestoreCheckpoint',
]) {
	requireText(mainRust, command, `Rust CLI must expose ${command}`);
}
requireText(runtimeControl, 'runtime-control-v1.json', 'runtime mode must use a versioned persistent state file');
requireText(runtimeControl, 'runtime-control-ack-v1.json', 'daemon control acknowledgement must be versioned');
requireText(runtimeControl, 'pub fn safe_fallback', 'invalid runtime state must fail closed');
requireText(runtimeControl, 'recovery_state', 'explicit commands must recover from a corrupt control file');
requireText(runtimeControl, 'pub fn acknowledge', 'daemon must persist consumed control generations');
requireText(runtimeControl, 'pub fn ack_matches', 'restore must match acknowledgement generation, mode and PID');
requireText(checkpoint, 'last-good-policy-v2.json', 'last-known-good policy must use the complete v2 checkpoint');
requireText(checkpoint, 'advanced_sysctls', 'runtime checkpoints must include advanced sysctls verified by policy health');
requireText(checkpoint, 'CHECKPOINT_SYSCTLS', 'restored sysctl paths and ranges must be independently allowlisted');
requireText(checkpoint, 'failure_state_error', 'corrupt failure state must not be silently reset');
requireText(checkpoint, 'enter_automatic_safe_mode', 'failure-state corruption must request persistent safe mode');
requireText(checkpoint, 'wait_for_daemon_ack', 'runtime restoration must wait for exact daemon safe-mode acknowledgement');
requireText(checkpoint, 'AUTOMATIC_SAFE_MODE_THRESHOLD', 'checkpoint failures must have an explicit safe-mode threshold');
requireText(checkpoint, 'validate_checkpoint', 'checkpoint restoration must validate all stored kernel tokens');
requireText(plan, '&& policy_resolved', 'policy planning must not allow writes when policy resolution failed');
requireText(plan, 'network::root_qdisc', 'policy plans must compare the interface qdisc');
requireText(daemon, 'requested_action.requests_apply()', 'daemon must consume hot reload generations');
requireText(daemon, '!control_state.mode.allows_writes()', 'paused and safe modes must block daemon writes');
requireText(daemon, 'control::acknowledge', 'daemon must acknowledge a control state before acting on it');
requireText(daemon, 'pub fn running_pid()', 'checkpoint restoration must bind acknowledgement to a live daemon PID');
requireText(daemon, 'checkpoint::record_failure', 'daemon must journal failed policy verification');
requireText(daemon, 'checkpoint::persist', 'daemon must persist verified last-known-good policy');

if (!process.exitCode) {
	console.log(`webui validation: ${Object.keys(english).length} translations, ${rustAlgorithms.length} algorithms, ${rustQdiscs.length} qdiscs, runtime controls enforced`);
}
