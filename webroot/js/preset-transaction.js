import { exec, shellQuote, toast } from './kernelsu.js';
import { addLog } from './logs.js';
import { haptic } from './motion.js';
import I18N from './i18n.js';
import router_state from './router.js';
import {
	ALL_ALGOS,
	ALL_QDISCS,
	getAlgorithmDescription,
	getQdiscDescription,
} from './capabilities.js';

const BUILTIN_PRESETS = [
	{ name: 'Balanced', wlanAlgo: 'cubic', cellAlgo: 'cubic', killConnections: false, initcwndInitrwnd: true, qdisc: 'fq_codel', pacing_ca: 150, pacing_ss: 200, tcp_fastopen: 3, tcp_ecn: 1, desc: 'Stable defaults — good for most users' },
	{ name: 'Gaming', wlanAlgo: 'bbr', cellAlgo: 'bbr', killConnections: true, initcwndInitrwnd: true, qdisc: 'fq', pacing_ca: 200, pacing_ss: 300, tcp_fastopen: 3, tcp_ecn: 1, desc: 'Low latency — aggressive BBR + FQ' },
	{ name: 'Streaming', wlanAlgo: 'bbr', cellAlgo: 'cubic', killConnections: false, initcwndInitrwnd: true, qdisc: 'fq_codel', pacing_ca: 180, pacing_ss: 250, tcp_fastopen: 3, tcp_ecn: 1, desc: 'High throughput — BBR for Wi-Fi, cubic for cell' },
	{ name: 'Battery Saver', wlanAlgo: 'vegas', cellAlgo: 'westwood', killConnections: false, initcwndInitrwnd: false, qdisc: 'fq_codel', pacing_ca: 120, pacing_ss: 180, tcp_fastopen: 1, tcp_ecn: 0, desc: 'Power efficient — delay-based, reduced pacing' },
	{ name: 'High-Speed', wlanAlgo: 'bbr3', cellAlgo: 'bbr3', killConnections: true, initcwndInitrwnd: true, qdisc: 'fq', pacing_ca: 220, pacing_ss: 320, tcp_fastopen: 3, tcp_ecn: 1, desc: 'Maximum throughput — BBR v3 experimental' },
];

function normalizePreset(value) {
	if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
	const name = typeof value.name === 'string' ? value.name.trim() : '';
	if (!name || name.length > 40 || /[\0\r\n]/.test(name)) return null;
	if (!ALL_ALGOS.includes(value.wlanAlgo) || !ALL_ALGOS.includes(value.cellAlgo)) return null;
	if (!ALL_QDISCS.includes(value.qdisc)) return null;
	const pacingCa = value.pacing_ca == null ? null : Number(value.pacing_ca);
	const pacingSs = value.pacing_ss == null ? null : Number(value.pacing_ss);
	const fastOpen = Number(value.tcp_fastopen);
	const ecn = Number(value.tcp_ecn);
	if ((pacingCa == null) !== (pacingSs == null)) return null;
	if (![pacingCa, pacingSs].filter(number => number != null)
		.every(number => Number.isInteger(number) && number >= 1 && number <= 1000)) return null;
	if (!Number.isInteger(fastOpen) || fastOpen < 0 || fastOpen > 3) return null;
	if (!Number.isInteger(ecn) || ecn < 0 || ecn > 2) return null;
	return {
		name,
		wlanAlgo: value.wlanAlgo,
		cellAlgo: value.cellAlgo,
		killConnections: value.killConnections === true,
		initcwndInitrwnd: value.initcwndInitrwnd === true,
		qdisc: value.qdisc,
		pacing_ca: pacingCa,
		pacing_ss: pacingSs,
		tcp_fastopen: fastOpen,
		tcp_ecn: ecn,
		desc: typeof value.desc === 'string' ? value.desc.slice(0, 160) : '',
	};
}

function savedPresets() {
	try {
		const parsed = JSON.parse(localStorage.getItem('tcp_presets') || '[]');
		return Array.isArray(parsed) ? parsed.map(normalizePreset).filter(Boolean) : [];
	} catch (error) {
		console.warn('Ignoring invalid saved presets:', error);
		return [];
	}
}

function resolvePreset(button, container) {
	const index = [...container.querySelectorAll('.preset-chip')].indexOf(button);
	if (index < 0) return null;
	return [...BUILTIN_PRESETS, ...savedPresets()][index] || null;
}

function algorithmMarkerCommand(dir, prefix, algorithm) {
	const selected = shellQuote(`${dir}/${prefix}_${algorithm}`);
	return `touch ${selected} && for marker in ${shellQuote(dir)}/${prefix}_*; do [ "$marker" = ${selected} ] || rm -f "$marker"; done`;
}

function setPresetControlsBusy(busy) {
	for (const button of document.querySelectorAll('#preset-list .preset-chip')) {
		button.disabled = busy || button.classList.contains('unsupported');
		button.setAttribute('aria-busy', String(busy));
	}
}

function syncPresetControls(preset) {
	for (const [containerId, selected] of [
		['wifi-algo-chips', preset.wlanAlgo],
		['cell-algo-chips', preset.cellAlgo],
		['qdisc-chips', preset.qdisc],
	]) {
		const container = document.getElementById(containerId);
		if (!container) continue;
		for (const chip of container.querySelectorAll('.algo-chip')) {
			const value = chip.dataset.algo || chip.dataset.qdisc;
			chip.classList.toggle('selected', value === selected);
			chip.setAttribute('aria-pressed', String(value === selected));
		}
	}
	const killConnections = document.getElementById('kill-connections');
	const initcwndInitrwnd = document.getElementById('initcwnd-initrwnd');
	if (killConnections) killConnections.checked = preset.killConnections;
	if (initcwndInitrwnd) initcwndInitrwnd.checked = preset.initcwndInitrwnd;
	const wifiDescription = document.getElementById('wifi-algo-description');
	const cellDescription = document.getElementById('cell-algo-description');
	const qdiscDescription = document.getElementById('qdisc-description');
	if (wifiDescription) wifiDescription.textContent = getAlgorithmDescription(preset.wlanAlgo, I18N.currentLang);
	if (cellDescription) cellDescription.textContent = getAlgorithmDescription(preset.cellAlgo, I18N.currentLang);
	if (qdiscDescription) qdiscDescription.textContent = getQdiscDescription(preset.qdisc, I18N.currentLang);
}

export function transactionalPresetCommand(dir, preset) {
	const quotedDir = shellQuote(dir);
	const managedFiles = [
		'kill_connections',
		'initcwnd_initrwnd',
		'pacing_ca',
		'pacing_ss',
		'qdisc',
		'tcp_fastopen',
		'tcp_ecn',
		'force_apply',
	];
	const backupFiles = managedFiles.map(name => {
		const source = shellQuote(`${dir}/${name}`);
		return `if [ -e ${source} ]; then cp -p ${source} "$backup/files/${name}"; else : > "$backup/missing/${name}"; fi`;
	}).join('\n');
	const restoreFiles = managedFiles.map(name => {
		const target = shellQuote(`${dir}/${name}`);
		return `if [ -e "$backup/missing/${name}" ]; then rm -f ${target}; elif [ -e "$backup/files/${name}" ]; then cp -p "$backup/files/${name}" ${target}; fi`;
	}).join('\n');
	const killCommand = preset.killConnections
		? `touch ${shellQuote(`${dir}/kill_connections`)}`
		: `rm -f ${shellQuote(`${dir}/kill_connections`)}`;
	const initCommand = preset.initcwndInitrwnd
		? `touch ${shellQuote(`${dir}/initcwnd_initrwnd`)}`
		: `rm -f ${shellQuote(`${dir}/initcwnd_initrwnd`)}`;
	const pacingCommand = preset.pacing_ca == null
		? `rm -f ${shellQuote(`${dir}/pacing_ca`)} ${shellQuote(`${dir}/pacing_ss`)}`
		: `write_config ${shellQuote(`${dir}/pacing_ca`)} ${shellQuote(String(preset.pacing_ca))}\nwrite_config ${shellQuote(`${dir}/pacing_ss`)} ${shellQuote(String(preset.pacing_ss))}`;

	return `# transactional-preset-apply
set -eu
module_dir=${quotedDir}
lock="$module_dir/.preset-transaction.lock"
if ! mkdir "$lock" 2>/dev/null; then
	lock_pid=$(cat "$lock/pid" 2>/dev/null || true)
	if [ -n "$lock_pid" ] && kill -0 "$lock_pid" 2>/dev/null; then exit 75; fi
	rm -rf "$lock" "$module_dir"/.preset-transaction.*
	mkdir "$lock"
fi
printf '%s\n' "$$" > "$lock/pid"
backup="$module_dir/.preset-transaction.$$"
mkdir -p "$backup/files" "$backup/missing" "$backup/markers"
armed=0
old_qdisc=
old_fastopen=
old_ecn=
write_config() {
	path=$1
	value=$2
	tmp="$path.tmp.$$"
	printf '%s\n' "$value" > "$tmp"
	mv "$tmp" "$path"
}
rollback() {
	set +e
	[ -n "\${old_qdisc:-}" ] && printf '%s\n' "$old_qdisc" > /proc/sys/net/core/default_qdisc
	[ -n "\${old_fastopen:-}" ] && printf '%s\n' "$old_fastopen" > /proc/sys/net/ipv4/tcp_fastopen
	[ -n "\${old_ecn:-}" ] && printf '%s\n' "$old_ecn" > /proc/sys/net/ipv4/tcp_ecn
	rm -f "$module_dir"/wlan_* "$module_dir"/rmnet_data_*
	for marker in "$backup"/markers/*; do [ -f "$marker" ] && cp -p "$marker" "$module_dir/"; done
${restoreFiles}
}
finish() {
	status=$?
	trap - EXIT HUP INT TERM
	if [ "$status" -ne 0 ] && [ "$armed" -eq 1 ]; then rollback; fi
	rm -f "$module_dir"/*.tmp.$$ 2>/dev/null || true
	rm -rf "$backup" "$lock"
	exit "$status"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
${backupFiles}
for marker in "$module_dir"/wlan_* "$module_dir"/rmnet_data_*; do
	[ -f "$marker" ] && cp -p "$marker" "$backup/markers/"
done
old_qdisc=$(cat /proc/sys/net/core/default_qdisc)
old_fastopen=$(cat /proc/sys/net/ipv4/tcp_fastopen)
old_ecn=$(cat /proc/sys/net/ipv4/tcp_ecn)
armed=1
${algorithmMarkerCommand(dir, 'wlan', preset.wlanAlgo)}
${algorithmMarkerCommand(dir, 'rmnet_data', preset.cellAlgo)}
${killCommand}
${initCommand}
${pacingCommand}
printf '%s\n' ${shellQuote(preset.qdisc)} > /proc/sys/net/core/default_qdisc
[ "$(cat /proc/sys/net/core/default_qdisc)" = ${shellQuote(preset.qdisc)} ]
write_config ${shellQuote(`${dir}/qdisc`)} ${shellQuote(preset.qdisc)}
printf '%s\n' ${preset.tcp_fastopen} > /proc/sys/net/ipv4/tcp_fastopen
[ "$(cat /proc/sys/net/ipv4/tcp_fastopen)" = ${shellQuote(String(preset.tcp_fastopen))} ]
write_config ${shellQuote(`${dir}/tcp_fastopen`)} ${shellQuote(String(preset.tcp_fastopen))}
printf '%s\n' ${preset.tcp_ecn} > /proc/sys/net/ipv4/tcp_ecn
[ "$(cat /proc/sys/net/ipv4/tcp_ecn)" = ${shellQuote(String(preset.tcp_ecn))} ]
write_config ${shellQuote(`${dir}/tcp_ecn`)} ${shellQuote(String(preset.tcp_ecn))}
touch ${shellQuote(`${dir}/force_apply`)}
trap - EXIT HUP INT TERM
rm -rf "$backup" "$lock"
printf 'preset-applied\n'`;
}

async function applyPresetTransaction(preset) {
	const qdiscSupported = router_state.qdiscCapabilities
		.some(item => item.name === preset.qdisc && item.state === 'supported');
	if (!qdiscSupported
		|| !router_state.available_algorithms.includes(preset.wlanAlgo)
		|| !router_state.available_algorithms.includes(preset.cellAlgo)) {
		toast(I18N.t('toast_invalid_algo'));
		return;
	}
	const dir = router_state.moduleInformation?.moduleDir;
	if (!dir) {
		toast(I18N.t('toast_error'));
		return;
	}
	setPresetControlsBusy(true);
	try {
		const { stdout } = await exec(transactionalPresetCommand(dir, preset));
		if (!stdout.includes('preset-applied')) throw new Error('preset transaction did not commit');
	} catch (error) {
		console.error('Failed to apply preset transaction:', error);
		toast(I18N.t('toast_error'));
		haptic('error');
		return;
	} finally {
		setPresetControlsBusy(false);
	}

	router_state.settingsPageParams.wlanAlgo = preset.wlanAlgo;
	router_state.settingsPageParams.rmnetAlgo = preset.cellAlgo;
	router_state.settingsPageParams.killConnections = preset.killConnections;
	router_state.settingsPageParams.initcwndInitrwnd = preset.initcwndInitrwnd;
	syncPresetControls(preset);
	localStorage.setItem('tcp_active_preset', preset.name);
	document.querySelectorAll('#preset-list .preset-chip').forEach(chip => chip.classList.remove('active'));
	const active = [...document.querySelectorAll('#preset-list .preset-chip')]
		.find(chip => chip.dataset.name === preset.name);
	if (active) active.classList.add('active');
	await addLog(`Preset applied transactionally: ${preset.name} (WiFi=${preset.wlanAlgo}, Cell=${preset.cellAlgo}, qdisc=${preset.qdisc})`);
	toast(I18N.t('toast_preset_applied', { name: preset.name }));
	haptic('success');
	document.dispatchEvent(new CustomEvent('tcp:preset-applied', { detail: { name: preset.name } }));
}

export function initPresetTransactions() {
	const container = document.getElementById('preset-list');
	if (!container || container.dataset.transactionBound === 'true') return;
	container.dataset.transactionBound = 'true';
	container.addEventListener('click', event => {
		const button = event.target.closest('.preset-chip');
		if (!button || !container.contains(button) || button.disabled || button.classList.contains('unsupported')) return;
		const preset = normalizePreset(resolvePreset(button, container));
		if (!preset) return;
		event.preventDefault();
		event.stopImmediatePropagation();
		void applyPresetTransaction(preset);
	}, { capture: true });
}
