import I18N from './i18n.js';
import router_state from './router.js';
import {
	getTCPStatCounters, getIfaceBytes, getSockStat, getTCPConnsCount,
	getDNSServers, getSSInfo,
} from './common.js';

// History buffers: 60 points × 5s = 5 minutes
const HISTORY_MAX = 60;
let _history = {
	tputRx: [],  // kbps
	tputTx: [],
	retransPct: [],
	cwnd: [],
	rtt: [],
};
let _prevBytes = null;
let _prevTime = 0;
let _sampling = false;
let _warmupTimer = null;

function pushHistory(arr, v) {
	arr.push(v);
	if (arr.length > HISTORY_MAX) arr.shift();
}

function svgPath(points, min, max, w, h) {
	if (points.length < 2) return '';
	const xScale = w / (points.length - 1);
	const yRange = max - min || 1;
	const pts = points.map((v, i) => `${(i * xScale).toFixed(1)},${(h - ((v - min) / yRange) * h).toFixed(1)}`);
	return `M${pts[0]} L${pts.slice(1).join(' ')}`;
}

function svgArea(points, min, max, w, h) {
	if (points.length < 2) return '';
	const path = svgPath(points, min, max, w, h).replace('M', 'L');
	return `M0,${h} ${path} L${w},${h} Z`;
}

function renderChart(canvasId, data, label, unit, color, height) {
	const container = document.getElementById(canvasId + '-container');
	if (!container) return;
	const w = container.clientWidth || 300;
	const h = height || 100;
	const min = Math.min(...data, 0);
	const max = Math.max(...data, 1) * 1.05;

	let html = `<svg viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" style="display:block">`;
	html += `<rect width="${w}" height="${h}" fill="none"/>`;
	// Grid lines
	for (let i = 0; i <= 3; i++) {
		const y = (h / 4) * i;
		html += `<line x1="0" y1="${y.toFixed(1)}" x2="${w}" y2="${y.toFixed(1)}" stroke="var(--md-sys-color-outline)" stroke-width="0.5" stroke-dasharray="3,3"/>`;
	}
	// Area
	html += `<path d="${svgArea(data, min, max, w, h)}" fill="${color}" fill-opacity="0.12"/>`;
	// Line
	html += `<path d="${svgPath(data, min, max, w, h)}" fill="none" stroke="${color}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>`;
	html += '</svg>';

	const labelEl = document.getElementById(canvasId + '-label');
	if (labelEl) labelEl.textContent = label;
	const valueEl = document.getElementById(canvasId + '-value');
	if (valueEl) valueEl.textContent = data.length > 0 ? `${data[data.length - 1].toFixed(1)} ${unit}` : '--';

	container.innerHTML = html;
}

function updateStatsUI() {
	if (router_state.current_active_page !== 'stats') return;
	const p = router_state.statsParams;

	// Info cards
	setVal('stats-tcp-conns', p.tcpConns);
	setVal('stats-tcp-inuse', p.sockStat?.tcpInUse);
	setVal('stats-rtt-avg', p.ssInfo ? `${p.ssInfo.avgRTT} ms` : '--');
	setVal('stats-cwnd-avg', p.ssInfo ? p.ssInfo.avgCWND : '--');
	setVal('stats-retrans-total', p.tcpCounters?.retrans);
	const rttSummary = p.ssInfo ? `${p.ssInfo.avgRTT} ms` : '--';
	const retransSummary = _history.retransPct.length > 0 ? `${_history.retransPct[_history.retransPct.length - 1].toFixed(1)}%` : '--';
	setText(document.getElementById('stats-connection-summary'), I18N.t('stats_connection_summary', {
		count: p.tcpConns ?? '--',
		rtt: rttSummary,
	}));
	setText(document.getElementById('stats-charts-summary'), I18N.t('stats_charts_summary', {
		retrans: retransSummary,
		rtt: rttSummary,
	}));
	setText(document.getElementById('stats-dns-summary'), p.dnsServers === null ? '--' : I18N.t('stats_dns_summary', {
		count: p.dnsServers?.length ?? '--',
	}));

	// DNS
	const dnsEl = document.getElementById('dns-list');
	if (dnsEl && Array.isArray(p.dnsServers)) {
		dnsEl.replaceChildren();
		if (p.dnsServers.length === 0) {
			const empty = document.createElement('span');
			empty.className = 'stat-dim';
			empty.textContent = I18N.t('label_none');
			dnsEl.appendChild(empty);
		} else {
			for (const server of p.dnsServers) {
				const row = document.createElement('div');
				row.className = 'dns-entry';
				for (const [className, value] of [['dns-iface', server.iface], ['dns-ip', server.ip]]) {
					const span = document.createElement('span');
					span.className = className;
					span.textContent = value;
					row.appendChild(span);
				}
				dnsEl.appendChild(row);
			}
		}
	}

	// Charts
	renderChart('chart-tput-rx', _history.tputRx, I18N.t('stats_download'), 'kB/s', 'var(--md-sys-color-tertiary)', 90);
	renderChart('chart-tput-tx', _history.tputTx, I18N.t('stats_upload'), 'kB/s', 'var(--md-sys-color-primary)', 90);
	if (document.getElementById('stats-charts-panel')?.open) renderDetailCharts();
}

function setVal(id, val) {
	const el = document.getElementById(id);
	if (el) el.textContent = val ?? '--';
}

function setText(element, value) {
	if (element) element.textContent = String(value);
}

function renderDetailCharts() {
	renderChart('chart-retrans', _history.retransPct, I18N.t('stats_retrans_rate'), '%', 'var(--md-sys-color-error)', 80);
	renderChart('chart-cwnd', _history.cwnd, I18N.t('stats_cwnd'), '', 'var(--md-sys-color-tertiary)', 70);
	renderChart('chart-rtt', _history.rtt, I18N.t('stats_rtt'), 'ms', 'var(--md-sys-color-primary)', 70);
}

async function sampleStats() {
	const activeIface = router_state.homePageParams.active_iface;
	const [tcp, iface, sock, conns, dns, ssInfo] = await Promise.all([
		getTCPStatCounters(),
		activeIface && !/^(unknown|none|error)$/i.test(activeIface) ? getIfaceBytes(activeIface) : Promise.resolve(null),
		getSockStat(), getTCPConnsCount(), getDNSServers(), getSSInfo(),
	]);

	const now = Date.now();
	const countersAvailable = iface && tcp;
	const sameCounterSeries = countersAvailable && _prevTime > 0 && _prevBytes && _prevBytes.iface === activeIface
		&& iface.rxBytes >= _prevBytes.rx && iface.txBytes >= _prevBytes.tx
		&& tcp.retrans >= _prevBytes.retrans && tcp.outSegs >= _prevBytes.outSegs;
	if (sameCounterSeries) {
		const dt = (now - _prevTime) / 1000;
		if (dt > 0) {
			pushHistory(_history.tputRx, (iface.rxBytes - _prevBytes.rx) / dt / 1024);
			pushHistory(_history.tputTx, (iface.txBytes - _prevBytes.tx) / dt / 1024);
			const retransDelta = tcp.retrans - _prevBytes.retrans;
			const segsDelta = tcp.outSegs - _prevBytes.outSegs;
			pushHistory(_history.retransPct, segsDelta > 0 ? Math.max(0, Math.min(100, retransDelta / segsDelta * 100)) : 0);
		}
	} else if (!countersAvailable || (_prevBytes?.iface && _prevBytes.iface !== activeIface)) {
		_history.tputRx = [];
		_history.tputTx = [];
		_history.retransPct = [];
	}
	if (ssInfo) {
		pushHistory(_history.cwnd, ssInfo.avgCWND);
		pushHistory(_history.rtt, ssInfo.avgRTT);
	}

	if (countersAvailable) {
		_prevBytes = { rx: iface.rxBytes, tx: iface.txBytes, retrans: tcp.retrans, outSegs: tcp.outSegs, iface: activeIface };
		_prevTime = now;
	} else {
		_prevBytes = null;
		_prevTime = 0;
	}

	router_state.statsParams = {
		tcpCounters: tcp,
		sockStat: sock,
		tcpConns: conns,
		dnsServers: dns,
		ssInfo,
	};
}

export async function updateStats() {
	if (_sampling) return;
	_sampling = true;
	try {
		await sampleStats();
		updateStatsUI();
		if (_prevBytes && _history.tputRx.length === 0 && !_warmupTimer && router_state.current_active_page === 'stats') {
			_warmupTimer = setTimeout(() => {
				_warmupTimer = null;
				if (router_state.current_active_page === 'stats') void updateStats();
			}, 1200);
		}
	} finally {
		_sampling = false;
	}
}

export function initStatsUI() {
	updateStatsUI();
	document.getElementById('stats-charts-panel')?.addEventListener('toggle', event => {
		if (event.currentTarget.open) renderDetailCharts();
	});
}

// Set up resize handler to re-render charts
let _resizeTimer = null;
window.addEventListener('resize', () => {
	if (router_state.current_active_page !== 'stats') return;
	clearTimeout(_resizeTimer);
	_resizeTimer = setTimeout(updateStatsUI, 200);
});
