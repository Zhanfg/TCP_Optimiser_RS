import { exec, moduleInfo, shellQuote } from '../kernelsu.js';

const MOD = '/data/adb/modules/tcp_optimiser';
const inflight = new Map();
let qdiscCache = null;

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
		const { stdout } = await exec(`# runtime-status-snapshot
touch ${shellQuote(`${MOD}/webui.active`)} 2>/dev/null || true
cat ${shellQuote(`${MOD}/runtime_snapshot.json`)} 2>/dev/null`, { timeoutMs: 1200 });
		try {
			const snap = parseJson(stdout);
			const age = Number.isFinite(snap.generated_epoch)
				? Math.max(0, Date.now() / 1000 - snap.generated_epoch)
				: 999;
			if (!force && age <= 12) return snap;
		} catch (_) {}

		const fallback = await exec(`# runtime-status-snapshot
${rust('status --runtime-only')}`, { timeoutMs: 2200 });
		return parseJson(fallback.stdout);
	});
}

function markerRead(prefix) {
	return `for f in "$moddir"/${prefix}_*; do
  [ -f "$f" ] || continue
  n=${f##*/}; n=${n#${prefix}_}
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
[ -f "$moddir/disable_auto_tuning" ] && a=0 || a=1; printf 'auto=%s\n' "$a"`, { timeoutMs: 1400 });
		const result = { wifi: 'bbr', cell: 'bbr', qdisc: '', kill: false, init: false, auto: true };
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
	const wifi = ['bbr','bbr3','cubic','reno'].includes(next.wifi) ? next.wifi : 'bbr';
	const cell = ['bbr','bbr3','cubic','reno'].includes(next.cell) ? next.cell : 'bbr';
	const qdisc = String(next.qdisc || '').trim();
	const qdiscWrite = qdisc
		? `printf '%s\\n' ${shellQuote(qdisc)} > ${shellQuote(`${MOD}/qdisc`)}`
		: `rm -f ${shellQuote(`${MOD}/qdisc`)}`;
	const killWrite = next.kill ? `touch ${shellQuote(`${MOD}/kill_connections`)}` : `rm -f ${shellQuote(`${MOD}/kill_connections`)}`;
	const initWrite = next.init ? `touch ${shellQuote(`${MOD}/initcwnd_initrwnd`)}` : `rm -f ${shellQuote(`${MOD}/initcwnd_initrwnd`)}`;
	const autoWrite = next.auto ? `rm -f ${shellQuote(`${MOD}/disable_auto_tuning`)}` : `touch ${shellQuote(`${MOD}/disable_auto_tuning`)}`;
	const mode = full ? 'apply-now --full' : 'apply-now';

	const { stdout } = await exec(`# v41-settings-apply
${markerWrite('wlan', wifi)}
${markerWrite('rmnet_data', cell)}
${qdiscWrite}
${killWrite}
${initWrite}
${autoWrite}
${rust(mode)}`, { timeoutMs: full ? 5200 : 2400 });

	let result;
	try { result = parseJson(stdout); }
	catch (_) { result = { ok: true, mode: full ? 'full' : 'fast', elapsed_ms: null }; }
	if (result.ok === false) throw new Error(result.error || 'Apply failed');
	return result;
}

export async function probeQdiscs() {
	if (qdiscCache) return qdiscCache;
	return once('qdisc-probe', async () => {
		const candidates = ['fq','fq_codel','codel','cake','pie','fq_pie','pfifo_fast'];
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

export async function sampleStats(details = false) {
	return once(details ? 'stats-detail' : 'stats', async () => {
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
