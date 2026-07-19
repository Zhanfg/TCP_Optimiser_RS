import { shellQuote } from './kernelsu.js';

function isSafeExportPath(value) {
	const path = String(value).trim();
	return path.length >= 2 && path.length <= 220 && !/[\0\r\n]/.test(path)
		&& ['/sdcard/', '/storage/emulated/0/', '/data/media/0/'].some(prefix => path.startsWith(prefix))
		&& !path.split('/').includes('..');
}

// Debug overlay — intercept ksu.exec and capture console output.
const _debug = {
	enabled: false,
	visible: false,
	entries: [],
	maxEntries: 500,
	_overlay: null,
	_panel: null,
	_startTime: Date.now(),

	init() {
		this._overlay = document.getElementById('debug-overlay');
		this._panel = document.getElementById('debug-panel');
		this._toggleBtn = document.getElementById('debug-toggle-btn');
		this._closeBtn = document.getElementById('debug-close-btn');
		this._saveBtn = document.getElementById('debug-save-btn');
		this._clearBtn = document.getElementById('debug-clear-btn');

		if (this._toggleBtn) {
			this._toggleBtn.addEventListener('click', () => this.toggle());
		}
		if (this._closeBtn) {
			this._closeBtn.addEventListener('click', () => this.toggle());
		}
		if (this._saveBtn) {
			this._saveBtn.addEventListener('click', () => this.saveLog());
		}
		if (this._clearBtn) {
			this._clearBtn.addEventListener('click', () => this.clearLog());
		}
	},

	toggle() {
		if (!this.enabled) return;
		this.visible = !this.visible;
		if (this.visible) {
			this._overlay.hidden = false;
			this.refresh();
		} else {
			this._overlay.hidden = true;
		}
	},

	log(type, msg, detail) {
		if (!this.enabled) return;
		const entry = {
			ts: Date.now() - this._startTime,
			type,
			msg: String(msg).substring(0, 200),
			detail: detail ? String(detail).substring(0, 500) : '',
		};
		this.entries.push(entry);
		if (this.entries.length > this.maxEntries) this.entries.shift();
		if (this.enabled && this._panel) this.refresh();
	},

	refresh() {
		if (!this._panel) return;
		this._panel.replaceChildren(...this.entries.map(entry => {
			const row = document.createElement('div');
			row.className = `dl dl-${/^[a-z]+$/.test(entry.type) ? entry.type : 'log'}`;
			for (const [className, value] of [
				['dl-ts', `${(entry.ts / 1000).toFixed(1)}s`],
				['dl-type', entry.type],
				['dl-msg', entry.msg],
				...(entry.detail ? [['dl-detail', entry.detail]] : []),
			]) {
				const span = document.createElement('span');
				span.className = className;
				span.textContent = value;
				row.appendChild(span);
			}
			return row;
		}));
		this._panel.scrollTop = this._panel.scrollHeight;
	},

	async saveLog() {
		const lines = this.entries.map(e =>
			`${(e.ts/1000).toFixed(3)}s [${e.type}] ${e.msg} ${e.detail}`
		).join('\n');
		const dest = document.getElementById('debug-path-input')?.value?.trim() || '/sdcard/Download/tcp_debug.log';
		if (!isSafeExportPath(dest)) {
			window._ksuToast('Invalid export path');
			return;
		}
		try {
			await window._ksuExec(`printf '%s' ${shellQuote(lines)} > ${shellQuote(dest)}`);
			window._ksuToast('Debug log saved: ' + dest);
		} catch (e) {
			window._ksuToast('Save failed');
		}
	},

	clearLog() {
		this.entries = [];
		if (this._panel) {
			const empty = document.createElement('div');
			empty.className = 'dl';
			empty.textContent = '— cleared —';
			this._panel.replaceChildren(empty);
		}
	},
};

// Expose globally
window._debug = _debug;

// Intercept console
['log', 'warn', 'error', 'info'].forEach(level => {
	const orig = console[level];
	console[level] = function(...args) {
		orig.apply(console, args);
		_debug.log(level, args.join(' '));
	};
});
