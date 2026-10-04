import { exec, moduleInfo, shellQuote } from '../kernelsu.js';
import { ALGORITHMS, QDISCS } from './catalog.js';

const MOD = '/data/adb/modules/tcp_optimiser';
const inflight = new Map();
let qdiscCache = null;
let activitySignalAt = 0;

function once(key, task) {
	if (inflight.has(key)) return inflight.get(key);
	const promise = Promise.resolve()
		.then(task)
		.finally(() => inflight.delete(key));
	inflight.set(key, promise);
	return promise;
}

function rust(subcommand) {
	return `moddir=${shellQuote(MOD)}
rust_bin="$moddir/bin/tcp_optimiser"
if [ ! -x "$rust_bin" ]; then
  case "$(getprop ro.product.cpu.abi 2>/dev/null)" in
    arm64-v8a) abi=arm64-v8a ;;
    armeabi-v7a|armeabi) abi=armeabi-v7a ;;
    x86_64) abi=x86_64 ;;
    *) exit 126 ;;
  esac
  rust_bin="$moddir/bin/$abi/tcp_optimiser"
fi
[ -x "$rust_bin" ] || exit 127
export TCP_OPTIMISER_MODULE_DIR="$moddir"
export PATH="/data/adb/ksu/bin:/system/bin:/system/xbin:$PATH"
"$rust_bin" ${subcommand}`;
}

function parseJson(text) {
	const value = JSON.parse(String(text || '').trim());
	if (!value || typeof value !== 'object') throw new Error('Invalid JSON payload');
	return value;
}

async function fetchPublished(name, maxAgeSeconds) {
	const response = await fetch(`runtime-data/${name}?t=${Date.now()}`, {
		cache: 'no-store',
	});
	if (!response.ok) throw new Error(`runtime snapshot HTTP ${response.status}`);
	const value = await response.json();
	if (!value || typeof value !== 'object' || !Number.isFinite(value.generated_epoch)) {
		throw new Error('invalid published runtime snapshot');
	}
	const age = Math.max(0, Date.now() / 1000 - value.generated_epoch);
	return { value, age, fresh: age <= maxAgeSeconds };
}

export async function signalActivity(force = false) {
	const now = Date.now();
	if (!force && now - activitySignalAt < 30000) return;
	activitySignalAt = now;
	try {
		await exec(`touch ${shellQuote(`${MOD}/webui.active`)} 2>/dev/null`, { timeoutMs: 900 });
	} catch (_) {}
}

export function getModuleInfo() {
	try {
		const raw = moduleInfo();
		const info = typeof raw === 'string' ? JSON.parse(raw) : (raw || {});
		return { ...info, moduleDir: MOD };
	} catch (_) {
		return { moduleDir: MOD };
	}
}

export async function runtime(force = false) {
	return once(force ? 'runtime-force' : 'runtime', async () => {
		if (!force) {
			try {
				const published = await fetchPublished('snapshot.json', 8);
				if (published.fresh) return published.value;
				// Return a still-useful stale snapshot immediately, while waking
				// the daemon asynchronously. Scrolling never waits on KSU bridge.
				if (published.age <= 120) {
					void signalActivity();
					return published.value;
				}
			} catch (_) {}
		}

		await signalActivity(force);
		const fallback = await exec(`# runtime-status-snapshot
${rust('status --runtime-only')}`, { timeoutMs: 2200 });
		return parseJson(fallback.stdout);
	});
}

function markerRead(prefix) {
	return `for f in "$moddir"/${prefix}_*; do
  [ -f "$f" ] || continue
  n=\${f##*/}; n=\${n#${prefix}_}
  case "$n" in bbr|bbr3|cubic|reno) printf '%s' "$n"; break ;; esac
done`;
}

export async function readSettings() {
	return once('settings', async () => {
		const { stdout } = await exec(`# v41-settings-read
moddir=${shellQuote(MOD)}
printf 'wifi='; ${markerRead('wlan')}; printf '\n'
printf 'cell='; ${markerRead('rmnet_data')}; printf '\n'
printf 'qdisc='; cat "$moddir/qdisc" 2>/dev/null; printf '\n'
[ -f "$moddir/kill_connections" ] && k=1 || k=0; printf 'kill=%s\n' "$k"
[ -f "$moddir/initcwnd_initrwnd" ] && i=1 || i=0; printf 'init=%s\n' "$i"
[ -f "$moddir/kill_connections_proxy" ] && p=1 || p=0; printf 'proxykill=%s\n' "$p"
[ -f "$moddir/disable_auto_tuning" ] && a=0 || a=1; printf 'auto=%s\n' "$a"`, { timeoutMs: 1400 });
		const result = { wifi: 'bbr', cell: 'bbr', qdisc: '', kill: false, init: false, proxyKill: false, auto: true };
		for (const line of stdout.split(/\r?\n/)) {
			const at = line.indexOf('=');
			if (at < 1) continue;
			const key = line.slice(0, at);
			const value = line.slice(at + 1).trim();
			if (key === 'wifi' && value) result.wifi = value;
			else if (key === 'cell' && value) result.cell = value;
			else if (key === 'qdisc') result.qdisc = value;
			else if (key === 'kill') result.kill = value === '1';
			else if (key === 'init') result.init = value === '1';
			else if (key === 'proxykill') result.proxyKill = value === '1';
			else if (key === 'auto') result.auto = value === '1';
		}
		return result;
	});
}

function markerWrite(prefix, algorithm) {
	const target = shellQuote(`${MOD}/${prefix}_${algorithm}`);
	const glob = shellQuote(`${MOD}/${prefix}_`) + '*';
	return `touch ${target}; for f in ${glob}; do [ "$f" = ${target} ] || rm -f "$f"; done`;
}

export async function applySettings(next, full = false) {
	const wifi = ALGORITHMS.includes(next.wifi) ? next.wifi : 'bbr';
	const cell = ALGORITHMS.includes(next.cell) ? next.cell : 'bbr';
	const qdisc = String(next.qdisc || '').trim();
	if (qdisc && !QDISCS.includes(qdisc)) throw new Error('Unsupported qdisc');
	const qdiscWrite = qdisc
		? `printf '%s\\n' ${shellQuote(qdisc)} > ${shellQuote(`${MOD}/qdisc`)}`
		: `rm -f ${shellQuote(`${MOD}/qdisc`)}`;
	const killWrite = next.kill ? `touch ${shellQuote(`${MOD}/kill_connections`)}` : `rm -f ${shellQuote(`${MOD}/kill_connections`)}`;
	const initWrite = next.init ? `touch ${shellQuote(`${MOD}/initcwnd_initrwnd`)}` : `rm -f ${shellQuote(`${MOD}/initcwnd_initrwnd`)}`;
	const proxyKillWrite = next.proxyKill ? `touch ${shellQuote(`${MOD}/kill_connections_proxy`)}` : `rm -f ${shellQuote(`${MOD}/kill_connections_proxy`)}`;
	const autoWrite = next.auto ? `rm -f ${shellQuote(`${MOD}/disable_auto_tuning`)}` : `touch ${shellQuote(`${MOD}/disable_auto_tuning`)}`;
	const mode = full ? 'apply-now --full' : 'apply-now';

	const { stdout } = await exec(`# v41-settings-apply
${markerWrite('wlan', wifi)}
${markerWrite('rmnet_data', cell)}
${qdiscWrite}
${killWrite}
${initWrite}
${proxyKillWrite}
${autoWrite}
${rust(mode)}`, { timeoutMs: full ? 5200 : 2400 });

	let result;
	try { result = parseJson(stdout); }
	catch (_) { result = { ok: true, mode: full ? 'full' : 'fast', elapsed_ms: null }; }
	if (result.ok === false) throw new Error(result.error || 'Apply failed');
	return result;
}

export async function refreshProfile() {
	const { stdout } = await exec(`# v41-profile-refresh
${rust('profile --refresh')} >/dev/null
${rust('apply-now --full')}`, { timeoutMs: 6500 });
	return parseJson(stdout);
}

export async function probeQdiscs() {
	if (qdiscCache) return qdiscCache;
	return once('qdisc-probe', async () => {
		const candidates = [...QDISCS];
		const { stdout } = await exec(`# qdisc-capability-probe
current=$(cat /proc/sys/net/core/default_qdisc 2>/dev/null)
for q in ${candidates.join(' ')}; do
  state=unknown
  if [ "$q" = "$current" ]; then state=supported
  elif command -v tc >/dev/null 2>&1; then
    probe=$(tc qdisc add dev lo root "$q" help 2>&1); rc=$?
    if printf '%s' "$probe" | grep -qiE 'unknown qdisc|not supported|operation not supported'; then state=unsupported
    elif [ -n "$probe" ] || [ "$rc" -eq 0 ]; then state=supported
    fi
  fi
  printf '%s=%s\n' "$q" "$state"
done`, { timeoutMs: 4200 });
		qdiscCache = stdout.split(/\r?\n/)
			.map(line => line.trim().split('=', 2))
			.filter(([name, status]) => candidates.includes(name) && status === 'supported')
			.map(([name]) => name);
		return qdiscCache;
	});
}

export async function runtimeDetails() {
	return once('runtime-details', async () => {
		try {
			const published = await fetchPublished('details.json', 35);
			if (published.fresh) return published.value;
		} catch (_) {}
		await signalActivity(true);
		const fallback = await exec(`# runtime-status-snapshot
${rust('status --runtime-only --details')}`, { timeoutMs: 3600 });
		return parseJson(fallback.stdout);
	});
}

export async function runtimeVerification() {
	return once('runtime-verify', async () => {
		const { stdout } = await exec(`# runtime-status-snapshot
${rust('status --runtime-only --verify')}`, { timeoutMs: 4500 });
		return parseJson(stdout);
	});
}

export async function sampleStats(details = false) {
	return once(details ? 'stats-detail' : 'stats', async () => {
		if (!details) {
			try {
				const published = await fetchPublished('snapshot.json', 7);
				if (published.fresh) return published.value;
				if (published.age <= 60) {
					void signalActivity();
					return published.value;
				}
			} catch (_) {}
		} else {
			try {
				const published = await fetchPublished('details.json', 35);
				if (published.fresh) return published.value;
			} catch (_) {}
		}

		await signalActivity(details);
		const { stdout } = await exec(`# runtime-stats-sample
${rust(`sample${details ? ' --details' : ''}`)}`, { timeoutMs: details ? 3200 : 2200 });
		return parseJson(stdout);
	});
}

export async function readLogs() {
	return once('logs', async () => {
		const { stdout } = await exec(`tail -n 160 ${shellQuote(`${MOD}/service.log`)} 2>/dev/null`, { timeoutMs: 1500 });
		return stdout || '';
	});
}

export async function clearLogs() {
	await exec(`rm -f ${shellQuote(`${MOD}/service.log`)}`, { timeoutMs: 1200 });
}

export async function verifyInstall() {
	await exec(`# module-integrity-check
${rust(`verify-module ${shellQuote(MOD)}`)}`, { timeoutMs: 5000 });
	return true;
}

export async function readAdvanced(fields) {
	return once('advanced-read', async () => {
		const safe = fields.filter(field =>
			/^[a-z0-9_]+$/.test(field.key)
			&& /^\/proc\/sys\/[a-z0-9_\/-]+$/.test(field.path)
		);
		const body = safe.map(field =>
			`if [ -r ${shellQuote(field.path)} ]; then printf '${field.key}='; cat ${shellQuote(field.path)}; fi`
		).join('\n');
		const { stdout } = await exec(`# advanced-sysctl-probe
${body}`, { timeoutMs: 2200 });
		const values = new Map();
		for (const line of stdout.split(/\r?\n/)) {
			const at = line.indexOf('=');
			if (at < 1) continue;
			const key = line.slice(0, at);
			const value = line.slice(at + 1).trim();
			if (/^-?\d+$/.test(value)) values.set(key, value);
		}
		return values;
	});
}

export async function applyAdvanced(fields, values) {
	const byKey = new Map(fields.map(field => [field.key, field]));
	const writes = [];
	const persisted = [];
	let ecn = null;
	let fastopen = null;

	for (const [key, raw] of values) {
		const field = byKey.get(key);
		if (!field) continue;
		if (!/^[a-z0-9_]+$/.test(field.key) || !/^\/proc\/sys\/[a-z0-9_\/-]+$/.test(field.path)) continue;
		const value = Number(raw);
		if (!Number.isInteger(value) || value < field.min || value > field.max) {
			throw new Error(`Invalid ${key}`);
		}
		writes.push(
			`[ -w ${shellQuote(field.path)} ] || exit 21; printf '%s\\n' ${value} > ${shellQuote(field.path)} || exit 22; actual=$(cat ${shellQuote(field.path)} 2>/dev/null); [ "$actual" = "${value}" ] || exit 23; printf '${field.key}=%s\\n' "$actual"`
		);
		if (key === 'tcp_ecn') ecn = value;
		else if (key === 'tcp_fastopen') fastopen = value;
		else persisted.push(`${field.key}=${value}`);
	}

	const configLines = persisted.map(line => `printf '%s\\n' ${shellQuote(line)}`).join('\n');
	const dedicated = [
		ecn == null ? '' : `printf '%s\\n' ${ecn} > ${shellQuote(`${MOD}/tcp_ecn`)}`,
		fastopen == null ? '' : `printf '%s\\n' ${fastopen} > ${shellQuote(`${MOD}/tcp_fastopen`)}`,
	].filter(Boolean).join('\n');

	const { stdout } = await exec(`# advanced-sysctl-apply
${writes.join('\n')}
tmp=${shellQuote(`${MOD}/advanced.conf.tmp`)}
cfg=${shellQuote(`${MOD}/advanced.conf`)}
${configLines ? `{
${configLines}
} > "$tmp"` : ': > "$tmp"'}
mv "$tmp" "$cfg"
${dedicated}`, { timeoutMs: 4200 });

	const verified = new Map();
	for (const line of stdout.split(/\r?\n/)) {
		const at = line.indexOf('=');
		if (at < 1) continue;
		verified.set(line.slice(0, at), line.slice(at + 1).trim());
	}
	for (const [key, value] of values) {
		if (verified.get(key) !== String(value)) throw new Error(`Readback failed for ${key}`);
	}
	return verified;
}
