// Minimal production KernelSU WebUI bridge.
// Browser preview data lives in mock-ksu.js and is imported only on demand.

const realBridge = window.ksu || null;
const previewRequested = new URLSearchParams(window.location.search).get('preview') === '1';
const previewHost = ['localhost', '127.0.0.1', '[::1]'].includes(window.location.hostname);
const previewAllowed = previewRequested
	|| (['http:', 'https:'].includes(window.location.protocol) && previewHost);

let callbackCounter = 0;
let previewModulePromise = null;

function callbackName() {
	return `tcp_exec_${Date.now()}_${callbackCounter++}`;
}

function loadPreview() {
	if (!previewAllowed) return Promise.reject(new Error('KernelSU WebUI bridge is unavailable'));
	previewModulePromise ||= import('./mock-ksu.js');
	return previewModulePromise;
}

export function shellQuote(value) {
	const text = String(value);
	if (text.includes('\0')) throw new TypeError('Shell values cannot contain NUL bytes');
	return `'${text.replace(/'/g, `'"'"'`)}'`;
}

export function exec(command, options = {}) {
	if (!realBridge) {
		return loadPreview().then(({ mockExec }) => {
			const result = mockExec(command);
			if (result.errno === 0) return result;
			throw new Error(result.stderr || `Command failed with errno ${result.errno}`);
		});
	}

	return new Promise((resolve, reject) => {
		const name = callbackName();
		const timeoutMs = Math.max(500, Math.min(60000, Number(options.timeoutMs) || 15000));
		let timer = null;
		const cleanup = () => {
			if (timer) clearTimeout(timer);
			delete window[name];
		};
		timer = setTimeout(() => {
			cleanup();
			reject(new Error(`KernelSU command timed out after ${timeoutMs} ms`));
		}, timeoutMs);

		window[name] = (errno, stdout, stderr) => {
			cleanup();
			if (errno === 0) resolve({ errno, stdout, stderr });
			else reject(new Error(stderr || `Command failed with errno ${errno}`));
		};

		try {
			realBridge.exec(command, JSON.stringify(options), name);
		} catch (error) {
			cleanup();
			reject(error);
		}
	});
}

export function toast(message) {
	if (realBridge?.toast) {
		realBridge.toast(String(message));
		return;
	}
	if (!previewAllowed) {
		console.warn('[toast]', message);
		return;
	}
	const node = document.createElement('div');
	node.className = 'preview-toast';
	node.textContent = String(message);
	document.body.appendChild(node);
	setTimeout(() => node.remove(), 1800);
}

export function moduleInfo() {
	if (realBridge?.moduleInfo) return realBridge.moduleInfo();
	if (!previewAllowed) return null;
	return JSON.stringify({
		id: 'tcp_optimiser',
		name: 'TCP Optimiser',
		version: '4.1.0',
		versionCode: '41',
		author: 'fatalcoder524 & axymorrsen',
		description: 'TCP Optimiser v4.1.0 — Lightweight WebUI',
	});
}

export function isBridgeAvailable() {
	return Boolean(realBridge || previewAllowed);
}

export function fullScreen(enabled) {
	try { realBridge?.fullScreen?.(Boolean(enabled)); } catch (_) {}
}

window._ksuExec = exec;
window._ksuToast = toast;
