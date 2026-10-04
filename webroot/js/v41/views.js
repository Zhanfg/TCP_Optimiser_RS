import I18N from '../i18n.js';
import { state, patch } from './store.js';
import * as api from './api.js';

const $ = (selector, root = document) => root.querySelector(selector);
const all = (selector, root = document) => [...root.querySelectorAll(selector)];
const zh = () => I18N.currentLang === 'zh';
const t = (cn, en) => zh() ? cn : en;
const esc = (value) => String(value ?? '')
	.replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;')
	.replaceAll('"','&quot;').replaceAll("'","&#39;");

function ifaceLabel(iface = '') {
	if (/^(wlan|swlan|wifi)/i.test(iface)) return 'Wi‑Fi';
	if (/^(rmnet|ccmni|ccemni|wwan|pdp)/i.test(iface)) return t('蜂窝网络','Cellular');
	return iface || '—';
}

function statusDot(active) {
	return `<span class="dot ${active ? 'ok' : 'bad'}"></span>`;
}

export function homeTemplate() {
	return `
	<section class="page">
	  <div class="hero">
	    <div class="hero__status">
	      <span class="eyebrow">${t('运行状态','Runtime')}</span>
	      <div class="hero__line"><strong id="home-status">${t('读取中…','Loading…')}</strong><span id="home-status-dot"></span></div>
	      <small id="home-build">—</small>
	    </div>
	    <button class="icon-btn" data-action="refresh-home" aria-label="${t('刷新','Refresh')}">↻</button>
	  </div>

	  <div class="metric-grid">
	    <article class="metric"><span>${t('当前算法','Algorithm')}</span><strong id="home-algo">—</strong></article>
	    <article class="metric"><span>Qdisc</span><strong id="home-qdisc">—</strong></article>
	    <article class="metric"><span>${t('活动链路','Interface')}</span><strong id="home-iface">—</strong></article>
	    <article class="metric"><span>${t('路径类型','Path')}</span><strong id="home-mode">—</strong></article>
	  </div>

	  <article class="panel">
	    <div class="panel__title"><strong>${t('内核能力','Kernel capabilities')}</strong><small id="home-kernel">—</small></div>
	    <div id="home-algos" class="chips"></div>
	  </article>

	  <article class="panel compact">
	    <div class="row"><span>${t('匹配模块','Matched KOs')}</span><strong id="home-kos">—</strong></div>
	    <div class="row"><span>${t('自动调优','Managed tuning')}</span><strong id="home-auto">—</strong></div>
	    <div class="row"><span>${t('Qdisc 策略','Qdisc policy')}</span><strong id="home-qpolicy">—</strong></div>
	    <div class="row"><span>${t('路径健康','Path health')}</span><strong id="home-adaptive">—</strong></div>
	  </article>

	  <div class="inline-actions">
	    <button class="secondary" data-action="home-details">${t('详细信息','Details')}</button>
	    <button class="secondary" data-action="home-verify">${t('实际状态校验','Verify live state')}</button>
	  </div>
	  <article id="home-details-panel" class="panel compact" hidden></article>
	  <article id="home-verify-panel" class="panel compact" hidden></article>
	</section>`;
}

export function renderHome(snapshot = state.runtime) {
	if (!snapshot) return;
	const active = snapshot.module_active !== false;
	$('#home-status')?.replaceChildren(document.createTextNode(active ? t('已启用','Enabled') : t('未运行','Stopped')));
	if ($('#home-status-dot')) $('#home-status-dot').innerHTML = statusDot(active);
	const info = state.module || {};
	if ($('#home-build')) $('#home-build').textContent = info.version ? `v${info.version}` : 'TCP Optimiser';
	if ($('#home-algo')) $('#home-algo').textContent = snapshot.algorithm || '—';
	if ($('#home-qdisc')) $('#home-qdisc').textContent = snapshot.default_qdisc || '—';
	if ($('#home-iface')) $('#home-iface').textContent = snapshot.active_iface || '—';
	if ($('#home-mode')) $('#home-mode').textContent = ifaceLabel(snapshot.active_iface);
	if ($('#home-kernel')) $('#home-kernel').textContent = snapshot.kernel_bundle?.kernel_release || '—';
	if ($('#home-kos')) $('#home-kos').textContent = String(snapshot.kernel_bundle?.matched_modules ?? 0);
	if ($('#home-auto')) $('#home-auto').textContent = snapshot.auto_tuning_enabled === false ? t('关闭','Off') : t('开启','On');
	if ($('#home-qpolicy')) $('#home-qpolicy').textContent = snapshot.qdisc_policy || '—';
	const adaptive = snapshot.adaptive;
	const adaptiveState = adaptive?.stable_state || adaptive?.latest?.state || 'unknown';
	if ($('#home-adaptive')) $('#home-adaptive').textContent = adaptiveState === 'unknown'
		? t('学习中','Learning')
		: adaptiveState.replaceAll('_', ' ');
	const chips = $('#home-algos');
	if (chips) {
		const current = snapshot.algorithm;
		const available = snapshot.available_algorithms || [];
		chips.innerHTML = available.map(name => `<span class="chip ${name === current ? 'selected' : ''}">${esc(name)}</span>`).join('');
	}
}

export function renderHomeDetails(snapshot) {
	const panel = $('#home-details-panel');
	if (!panel) return;
	const proxy = snapshot?.proxy_state || snapshot?.proxy || null;
	const proxyText = typeof proxy === 'object'
		? [proxy.label || proxy.family, proxy.mode].filter(Boolean).join(' · ')
		: (proxy && proxy !== 'deferred' ? String(proxy) : '—');
	const dns = Array.isArray(snapshot?.dns) ? snapshot.dns.map(item => item.ip).filter(Boolean) : [];
	const conn = snapshot?.conn_info;
	panel.innerHTML = `
	  <div class="row"><span>${t('代理 / VPN','Proxy / VPN')}</span><strong>${esc(proxyText)}</strong></div>
	  <div class="row"><span>DNS</span><strong>${esc(dns.length ? dns.join(', ') : '—')}</strong></div>
	  <div class="row"><span>${t('平均 RTT','Average RTT')}</span><strong>${esc(Number.isFinite(conn?.avg_rtt_ms) ? conn.avg_rtt_ms.toFixed(1) + ' ms' : '—')}</strong></div>
	  <div class="row"><span>CWND</span><strong>${esc(Number.isFinite(conn?.avg_cwnd) ? conn.avg_cwnd.toFixed(1) : '—')}</strong></div>`;
	panel.hidden = false;
}

export function renderHomeVerification(snapshot) {
	const panel = $('#home-verify-panel');
	if (!panel) return;
	const verification = snapshot?.verification;
	const checks = Array.isArray(verification?.checks) ? verification.checks : [];
	const summary = verification?.summary;
	if (!summary || !checks.length) {
		panel.innerHTML = `<span class="hint">${t('当前没有可用的校验结果。','Verification is unavailable.')}</span>`;
		panel.hidden = false;
		return;
	}
	panel.innerHTML = `
	  <div class="row"><span>${t('匹配','Matched')}</span><strong>${summary.matched ?? 0} / ${summary.total ?? checks.length}</strong></div>
	  <div class="row"><span>${t('漂移','Drift')}</span><strong>${summary.drifted ?? 0}</strong></div>
	  <div class="row"><span>${t('不可用','Unavailable')}</span><strong>${summary.unavailable ?? 0}</strong></div>
	  <details class="verification-details">
	    <summary>${t('查看检查项','Show checks')}</summary>
	    <div class="verification-list">${checks.map(check => `
	      <div class="verify-row" data-state="${esc(check.state)}"><span>${esc(check.key)}</span><strong>${esc(check.actual ?? '—')}</strong></div>`
	    ).join('')}</div>
	  </details>`;
	panel.hidden = false;
}

function algoButtons(name, selected, available) {
	return ['bbr','bbr3','cubic','reno']
		.filter(item => available.includes(item))
		.map(item => `<button class="choice ${item === selected ? 'selected' : ''}" data-setting="${name}" data-value="${item}">${item.toUpperCase()}</button>`)
		.join('');
}

export function settingsTemplate() {
	const snap = state.runtime || {};
	const cfg = state.settings || { wifi:'bbr', cell:'bbr', qdisc:'', kill:false, init:false, auto:true };
	const available = snap.available_algorithms || ['bbr','cubic','reno'];
	const qdiscLabel = cfg.qdisc || t('自动','Auto');
	return `
	<section class="page">
	  <div class="page-head"><div><span class="eyebrow">${t('策略','Policy')}</span><h2>${t('网络策略','Network policy')}</h2></div><span id="settings-latency" class="latency"></span></div>

	  <article class="panel">
	    <div class="field"><label>Wi‑Fi</label><div class="choices" id="wifi-choices">${algoButtons('wifi', cfg.wifi, available)}</div></div>
	    <div class="field"><label>${t('蜂窝网络','Cellular')}</label><div class="choices" id="cell-choices">${algoButtons('cell', cfg.cell, available)}</div></div>
	  </article>

	  <article class="panel">
	    <div class="panel__title"><strong>Qdisc</strong><small id="qdisc-current">${esc(qdiscLabel)}</small></div>
	    <div id="qdisc-choices" class="choices">
	      <button class="choice ${!cfg.qdisc ? 'selected' : ''}" data-setting="qdisc" data-value="">${t('自动','Auto')}</button>
	      ${cfg.qdisc ? `<button class="choice selected" data-setting="qdisc" data-value="${esc(cfg.qdisc)}">${esc(cfg.qdisc)}</button>` : ''}
	    </div>
	    <button class="text-btn" data-action="load-qdiscs">${t('显示更多 Qdisc','Show more qdiscs')}</button>
	  </article>

	  <article class="panel compact">
	    <label class="switch-row"><span><strong>${t('自动调优','Managed tuning')}</strong><small>${t('自动应用安全网络参数','Apply safe network defaults automatically')}</small></span><input type="checkbox" data-toggle="auto" ${cfg.auto ? 'checked' : ''}></label>
	    <label class="switch-row"><span><strong>${t('切换时断开连接','Reset TCP sessions')}</strong><small>${t('仅完整应用时执行','Only on full apply')}</small></span><input type="checkbox" data-toggle="kill" ${cfg.kill ? 'checked' : ''}></label>
	    <label class="switch-row"><span><strong>initcwnd / initrwnd</strong><small>${t('仅完整应用时执行','Only on full apply')}</small></span><input type="checkbox" data-toggle="init" ${cfg.init ? 'checked' : ''}></label>
	  </article>

	  <div class="sticky-actions">
	    <button class="secondary" data-action="apply-full">${t('完整应用','Full apply')}</button>
	    <button class="primary" data-action="apply-fast">${t('立即应用','Apply now')}</button>
	  </div>

	  <article class="panel compact">
	    <div class="row"><span>${t('界面模式','Theme')}</span><div class="mini-actions"><button class="text-btn" data-theme="auto">${t('跟随系统','Auto')}</button><button class="text-btn" data-theme="dark">${t('深色','Dark')}</button><button class="text-btn" data-theme="light">${t('浅色','Light')}</button></div></div>
	    <div class="row"><span>${t('安装完整性','Integrity')}</span><button class="text-btn" data-action="verify">${t('检查','Check')}</button></div>
	    <div class="row"><span>${t('高级参数','Advanced controls')}</span><button class="text-btn" data-action="open-advanced">${t('打开','Open')}</button></div>
	  </article>

	  <article id="advanced-host" class="panel" hidden></article>
	</section>`;
}

function readDraft() {
	const current = state.settings || {};
	return {
		wifi: $('.choice.selected[data-setting="wifi"]')?.dataset.value || current.wifi || 'bbr',
		cell: $('.choice.selected[data-setting="cell"]')?.dataset.value || current.cell || 'bbr',
		qdisc: $('.choice.selected[data-setting="qdisc"]')?.dataset.value ?? current.qdisc ?? '',
		auto: $('[data-toggle="auto"]')?.checked ?? current.auto ?? true,
		kill: $('[data-toggle="kill"]')?.checked ?? current.kill ?? false,
		init: $('[data-toggle="init"]')?.checked ?? current.init ?? false,
	};
}

export async function handleSettingsAction(action) {
	if (action === 'load-qdiscs') {
		const button = $('[data-action="load-qdiscs"]');
		if (button) button.disabled = true;
		try {
			const items = await api.probeQdiscs();
			const root = $('#qdisc-choices');
			const selected = readDraft().qdisc;
			if (root) root.innerHTML = [
				`<button class="choice ${!selected ? 'selected' : ''}" data-setting="qdisc" data-value="">${t('自动','Auto')}</button>`,
				...items.map(q => `<button class="choice ${q === selected ? 'selected' : ''}" data-setting="qdisc" data-value="${q}">${q}</button>`)
			].join('');
			if (button) button.hidden = true;
		} finally {
			if (button) button.disabled = false;
		}
		return;
	}

	if (action === 'apply-fast' || action === 'apply-full') {
		const full = action === 'apply-full';
		const buttons = all('.sticky-actions button');
		buttons.forEach(btn => btn.disabled = true);
		try {
			const draft = readDraft();
			const result = await api.applySettings(draft, full);
			patch({ settings: draft });
			const latency = $('#settings-latency');
			if (latency) latency.textContent = Number.isFinite(result.elapsed_ms) ? `${result.elapsed_ms} ms` : t('完成','Done');
			const runtime = await api.runtime(true);
			patch({ runtime });
		} finally {
			buttons.forEach(btn => btn.disabled = false);
		}
		return;
	}

	if (action === 'verify') {
		const btn = $('[data-action="verify"]');
		if (btn) btn.disabled = true;
		try {
			await api.verifyInstall();
			if (btn) btn.textContent = t('通过','Passed');
		} catch (_) {
			if (btn) btn.textContent = t('失败','Failed');
		} finally {
			setTimeout(() => { if (btn) btn.disabled = false; }, 500);
		}
	}
}

export function statsTemplate() {
	return `
	<section class="page">
	  <div class="page-head"><div><span class="eyebrow">${t('实时采样','Live sample')}</span><h2>${t('网络统计','Network stats')}</h2></div><button class="icon-btn" data-action="refresh-stats">↻</button></div>
	  <div class="metric-grid">
	    <article class="metric"><span>TCP</span><strong id="stat-conns">—</strong><small>${t('活动连接','connections')}</small></article>
	    <article class="metric"><span>RTT</span><strong id="stat-rtt">—</strong><small>ms</small></article>
	    <article class="metric"><span>CWND</span><strong id="stat-cwnd">—</strong></article>
	    <article class="metric"><span>${t('重传','Retrans')}</span><strong id="stat-retrans">—</strong></article>
	  </div>
	  <article class="panel compact">
	    <div class="row"><span>RX</span><strong id="stat-rx">—</strong></div>
	    <div class="row"><span>TX</span><strong id="stat-tx">—</strong></div>
	    <div class="row"><span>TCP in-use</span><strong id="stat-inuse">—</strong></div>
	    <div class="row"><span>TIME_WAIT</span><strong id="stat-tw">—</strong></div>
	  </article>
	  <button class="secondary wide" data-action="stats-details">${t('读取 RTT / DNS 详细信息','Load RTT / DNS details')}</button>
	  <article id="stats-details" class="panel compact" hidden></article>
	</section>`;
}

export function renderStats(snapshot = state.stats) {
	if (!snapshot) return;
	if ($('#stat-conns')) $('#stat-conns').textContent = snapshot.established ?? '—';
	if ($('#stat-retrans')) $('#stat-retrans').textContent = snapshot.tcp?.retrans ?? '—';
	if ($('#stat-rx')) $('#stat-rx').textContent = formatBytes(snapshot.iface?.rx_bytes);
	if ($('#stat-tx')) $('#stat-tx').textContent = formatBytes(snapshot.iface?.tx_bytes);
	if ($('#stat-inuse')) $('#stat-inuse').textContent = snapshot.sock?.tcp_in_use ?? '—';
	if ($('#stat-tw')) $('#stat-tw').textContent = snapshot.sock?.tcp_tw ?? '—';
	if ($('#stat-rtt')) $('#stat-rtt').textContent = snapshot.conn_info?.avg_rtt_ms?.toFixed?.(1) ?? '—';
	if ($('#stat-cwnd')) $('#stat-cwnd').textContent = snapshot.conn_info?.avg_cwnd?.toFixed?.(1) ?? snapshot.conn_info?.avg_cwnd ?? '—';
}

export function renderStatsDetails(snapshot) {
	const panel = $('#stats-details');
	if (!panel) return;
	const dns = Array.isArray(snapshot.dns) ? snapshot.dns : [];
	panel.innerHTML = `
	  <div class="row"><span>${t('平均 RTT','Average RTT')}</span><strong>${esc(snapshot.conn_info?.avg_rtt_ms?.toFixed?.(1) ?? '—')} ms</strong></div>
	  <div class="row"><span>${t('最大 RTT','Maximum RTT')}</span><strong>${esc(snapshot.conn_info?.max_rtt_ms?.toFixed?.(1) ?? '—')} ms</strong></div>
	  <div class="row vertical"><span>DNS</span><code>${dns.length ? dns.map(item => esc(item.ip)).join('\n') : '—'}</code></div>`;
	panel.hidden = false;
}

function formatBytes(value) {
	if (!Number.isFinite(value)) return '—';
	if (value >= 1073741824) return `${(value / 1073741824).toFixed(2)} GiB`;
	if (value >= 1048576) return `${(value / 1048576).toFixed(1)} MiB`;
	if (value >= 1024) return `${(value / 1024).toFixed(1)} KiB`;
	return `${value} B`;
}

export function logsTemplate() {
	return `
	<section class="page">
	  <div class="page-head"><div><span class="eyebrow">${t('诊断','Diagnostics')}</span><h2>${t('日志','Logs')}</h2></div><div class="mini-actions"><button class="icon-btn" data-action="refresh-logs">↻</button><button class="icon-btn" data-action="clear-logs">⌫</button></div></div>
	  <pre id="log-output" class="log-output">${t('读取中…','Loading…')}</pre>
	</section>`;
}

export function renderLogs(text) {
	const output = $('#log-output');
	if (output) output.textContent = text || t('暂无日志','No logs');
}

export function selectChoice(button) {
	const setting = button.dataset.setting;
	if (!setting) return;
	all(`.choice[data-setting="${setting}"]`).forEach(item => item.classList.toggle('selected', item === button));
}

export function applyTheme(mode) {
	if (!['auto','dark','light'].includes(mode)) return;
	localStorage.setItem('tcp_themeMode', mode);
	const resolved = mode === 'auto'
		? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
		: mode;
	document.documentElement.dataset.theme = resolved;
	document.documentElement.style.colorScheme = resolved;
}
