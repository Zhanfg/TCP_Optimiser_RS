import { exec, toast } from './kernelsu.js';
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
let _prevBytes = { rx: 0, tx: 0, retrans: 0 };
let _prevTime = 0;

function pushHistory(arr, v) {
	arr.push(v);
	if (arr.length > HISTORY_MAX) arr.shift();
}

function svgPath(points, min, max, w, h) {
	if (points.length < 2) return '';
	const xScale = w / (points.length - 1);
	const yRange = max - min || 1;
	const pts = points.map((v, i) => `${(i * xScale).toFixed(1)},${(h - ((v - min) / yRange) * h).toFixed(1)}`);
	return `M${pts[0]} L${pts.join(' ')}`;
}

function svgArea(points, min, max, w, h) {
	if (points.length < 2) return '';
	const path = svgPath(points, min, max, w, h).replace('M', 'L');
	return `M0,${h} L${path} L${w},${h} Z`;
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
	setVal('stats-retrans-total', p.tcpCounters?.retrans || 0);

	// DNS
	const dnsEl = document.getElementById('dns-list');
	if (dnsEl && p.dnsServers) {
		if (p.dnsServers.length === 0) {
			dnsEl.innerHTML = '<span class="stat-dim">' + I18N.t('label_none') + '</span>';
		} else {
			dnsEl.innerHTML = p.dnsServers.map(s =>
				`<div class="dns-entry"><span class="dns-iface">${s.iface}</span><span class="dns-ip">${s.ip}</span></div>`
			).join('');
		}
	}

	// Charts
	renderChart('chart-tput-rx', _history.tputRx, I18N.t('stats_download'), 'kB/s', 'var(--md-sys-color-tertiary)', 90);
	renderChart('chart-tput-tx', _history.tputTx, I18N.t('stats_upload'), 'kB/s', 'var(--md-sys-color-primary)', 90);
	renderChart('chart-retrans', _history.retransPct, I18N.t('stats_retrans_rate'), '%', 'var(--md-sys-color-error)', 80);
	renderChart('chart-cwnd', _history.cwnd, I18N.t('stats_cwnd'), '', 'var(--md-sys-color-tertiary)', 70);
	renderChart('chart-rtt', _history.rtt, I18N.t('stats_rtt'), 'ms', 'var(--md-sys-color-primary)', 70);
}

function setVal(id, val) {
	const el = document.getElementById(id);
	if (el) el.textContent = val ?? '--';
}

async function sampleStats() {
	const [tcp, iface, sock, conns, dns, ssInfo] = await Promise.all([
		getTCPStatCounters(),
		router_state.homePageParams.active_iface ? getIfaceBytes(router_state.homePageParams.active_iface) : Promise.resolve({ rxBytes: 0, txBytes: 0 }),
		getSockStat(),
		getTCPConnsCount(),
		getDNSServers(),
		getSSInfo(),
	]);

	const now = Date.now();
	const dt = _prevTime ? (now - _prevTime) / 1000 : 1; // seconds
	const rxRate = dt > 0 ? ((iface.rxBytes - _prevBytes.rx) / dt / 1024) : 0;
	const txRate = dt > 0 ? ((iface.txBytes - _prevBytes.tx) / dt / 1024) : 0;
	const retransDelta = tcp.retrans - _prevBytes.retrans;
	const segsDelta = (tcp.outSegs - (_prevBytes.outSegs || tcp.outSegs)) || 1;
	const retransPct = (retransDelta / Math.max(segsDelta, 1)) * 100;

	pushHistory(_history.tputRx, Math.max(0, rxRate));
	pushHistory(_history.tputTx, Math.max(0, txRate));
	pushHistory(_history.retransPct, Math.min(retransPct, 100));
	if (ssInfo) {
		pushHistory(_history.cwnd, ssInfo.avgCWND);
		pushHistory(_history.rtt, ssInfo.avgRTT);
	}

	_prevBytes = { rx: iface.rxBytes, tx: iface.txBytes, retrans: tcp.retrans, outSegs: tcp.outSegs };
	_prevTime = now;

	router_state.statsParams = {
		tcpCounters: tcp,
		sockStat: sock,
		tcpConns: conns,
		dnsServers: dns,
		ssInfo,
	};
}

export async function updateStats() {
	await sampleStats();
	updateStatsUI();
}

export function initStatsUI() {
	router_state.isInitializing = false;
	updateStatsUI();
}

// Set up resize handler to re-render charts
let _resizeTimer = null;
window.addEventListener('resize', () => {
	if (router_state.current_active_page !== 'stats') return;
	clearTimeout(_resizeTimer);
	_resizeTimer = setTimeout(updateStatsUI, 200);
});
