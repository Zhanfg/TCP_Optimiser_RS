import * as api from './api.js';

export const FIELDS = [
	{ group:'lifecycle', key:'tcp_keepalive_time', path:'/proc/sys/net/ipv4/tcp_keepalive_time', min:10, max:7200, step:10 },
	{ group:'lifecycle', key:'tcp_keepalive_intvl', path:'/proc/sys/net/ipv4/tcp_keepalive_intvl', min:1, max:300, step:1 },
	{ group:'lifecycle', key:'tcp_keepalive_probes', path:'/proc/sys/net/ipv4/tcp_keepalive_probes', min:1, max:30, step:1 },
	{ group:'lifecycle', key:'tcp_fin_timeout', path:'/proc/sys/net/ipv4/tcp_fin_timeout', min:5, max:120, step:1 },
	{ group:'lifecycle', key:'tcp_syn_retries', path:'/proc/sys/net/ipv4/tcp_syn_retries', min:1, max:10, step:1 },
	{ group:'lifecycle', key:'tcp_synack_retries', path:'/proc/sys/net/ipv4/tcp_synack_retries', min:1, max:10, step:1 },
	{ group:'lifecycle', key:'tcp_retries2', path:'/proc/sys/net/ipv4/tcp_retries2', min:3, max:20, step:1 },
	{ group:'memory', key:'rmem_max', path:'/proc/sys/net/core/rmem_max', min:65536, max:134217728, step:65536 },
	{ group:'memory', key:'wmem_max', path:'/proc/sys/net/core/wmem_max', min:65536, max:134217728, step:65536 },
	{ group:'memory', key:'optmem_max', path:'/proc/sys/net/core/optmem_max', min:10240, max:4194304, step:1024 },
	{ group:'memory', key:'tcp_notsent_lowat', path:'/proc/sys/net/ipv4/tcp_notsent_lowat', min:0, max:4294967295, step:4096 },
	{ group:'queue', key:'somaxconn', path:'/proc/sys/net/core/somaxconn', min:128, max:65535, step:128 },
	{ group:'queue', key:'netdev_max_backlog', path:'/proc/sys/net/core/netdev_max_backlog', min:256, max:65535, step:256 },
	{ group:'queue', key:'tcp_max_syn_backlog', path:'/proc/sys/net/ipv4/tcp_max_syn_backlog', min:128, max:65535, step:128 },
	{ group:'queue', key:'netdev_budget', path:'/proc/sys/net/core/netdev_budget', min:64, max:4096, step:64 },
	{ group:'queue', key:'netdev_budget_usecs', path:'/proc/sys/net/core/netdev_budget_usecs', min:500, max:50000, step:500 },
	{ group:'recovery', key:'tcp_mtu_probing', path:'/proc/sys/net/ipv4/tcp_mtu_probing', min:0, max:2, step:1 },
	{ group:'recovery', key:'tcp_sack', path:'/proc/sys/net/ipv4/tcp_sack', min:0, max:1, step:1, boolean:true },
	{ group:'recovery', key:'tcp_dsack', path:'/proc/sys/net/ipv4/tcp_dsack', min:0, max:1, step:1, boolean:true },
	{ group:'recovery', key:'tcp_ecn', path:'/proc/sys/net/ipv4/tcp_ecn', min:0, max:2, step:1 },
	{ group:'recovery', key:'tcp_no_metrics_save', path:'/proc/sys/net/ipv4/tcp_no_metrics_save', min:0, max:1, step:1, boolean:true },
	{ group:'recovery', key:'tcp_slow_start_after_idle', path:'/proc/sys/net/ipv4/tcp_slow_start_after_idle', min:0, max:1, step:1, boolean:true },
	{ group:'recovery', key:'tcp_fastopen', path:'/proc/sys/net/ipv4/tcp_fastopen', min:0, max:3, step:1 },
	{ group:'recovery', key:'tcp_tw_reuse', path:'/proc/sys/net/ipv4/tcp_tw_reuse', min:0, max:2, step:1 },
	{ group:'recovery', key:'tcp_autocorking', path:'/proc/sys/net/ipv4/tcp_autocorking', min:0, max:1, step:1, boolean:true },
	{ group:'recovery', key:'tcp_early_retrans', path:'/proc/sys/net/ipv4/tcp_early_retrans', min:0, max:4, step:1 },
	{ group:'recovery', key:'tcp_thin_linear_timeouts', path:'/proc/sys/net/ipv4/tcp_thin_linear_timeouts', min:0, max:1, step:1, boolean:true },
	{ group:'recovery', key:'tcp_thin_dupack', path:'/proc/sys/net/ipv4/tcp_thin_dupack', min:0, max:1, step:1, boolean:true },
	{ group:'recovery', key:'tcp_rto_max_ms', path:'/proc/sys/net/ipv4/tcp_rto_max_ms', min:1000, max:120000, step:1000 },
	{ group:'plb', key:'tcp_plb_enabled', path:'/proc/sys/net/ipv4/tcp_plb_enabled', min:0, max:1, step:1, boolean:true },
	{ group:'plb', key:'tcp_plb_idle_rehash_rounds', path:'/proc/sys/net/ipv4/tcp_plb_idle_rehash_rounds', min:0, max:31, step:1 },
	{ group:'plb', key:'tcp_plb_rehash_rounds', path:'/proc/sys/net/ipv4/tcp_plb_rehash_rounds', min:0, max:31, step:1 },
	{ group:'plb', key:'tcp_plb_suspend_rto_sec', path:'/proc/sys/net/ipv4/tcp_plb_suspend_rto_sec', min:0, max:255, step:1 },
	{ group:'plb', key:'tcp_plb_cong_thresh', path:'/proc/sys/net/ipv4/tcp_plb_cong_thresh', min:0, max:256, step:1 },
	{ group:'latency', key:'busy_poll', path:'/proc/sys/net/core/busy_poll', min:0, max:100000, step:50 },
	{ group:'latency', key:'busy_read', path:'/proc/sys/net/core/busy_read', min:0, max:100000, step:50 },
	{ group:'conntrack', key:'nf_conntrack_max', path:'/proc/sys/net/netfilter/nf_conntrack_max', min:1024, max:1048576, step:1024 },
	{ group:'conntrack', key:'nf_conntrack_tcp_timeout_established', path:'/proc/sys/net/netfilter/nf_conntrack_tcp_timeout_established', min:60, max:432000, step:60 },
	{ group:'conntrack', key:'nf_conntrack_tcp_timeout_time_wait', path:'/proc/sys/net/netfilter/nf_conntrack_tcp_timeout_time_wait', min:1, max:600, step:1 },
];

const GROUPS = {
	lifecycle:['连接生命周期','Connection lifecycle'],
	memory:['缓冲与内存','Buffers & memory'],
	queue:['队列','Queues'],
	recovery:['恢复与 TCP 特性','Recovery & TCP features'],
	plb:['PLB','PLB'],
	latency:['低延迟轮询','Low-latency polling'],
	conntrack:['连接跟踪','Conntrack'],
};

const tr = pair => document.documentElement.lang.startsWith('zh') ? pair[0] : pair[1];
const esc = value => String(value ?? '').replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;');

export async function mountAdvanced(container) {
	container.innerHTML = '<div class="skeleton" style="min-height:180px"></div>';
	const values = await api.readAdvanced(FIELDS);
	const groups = Object.keys(GROUPS).map(group => {
		const fields = FIELDS.filter(field => field.group === group && values.has(field.key));
		if (!fields.length) return '';
		const rows = fields.map(field => {
			const value = values.get(field.key);
			if (field.boolean) {
				return `<label class="advanced-row"><span><strong>${esc(field.key)}</strong><small>${esc(field.path)}</small></span><input type="checkbox" data-adv="${field.key}" ${value === '1' ? 'checked' : ''}></label>`;
			}
			return `<label class="advanced-row"><span><strong>${esc(field.key)}</strong><small>${esc(field.path)}</small></span><input type="number" inputmode="numeric" data-adv="${field.key}" min="${field.min}" max="${field.max}" step="${field.step}" value="${esc(value)}"></label>`;
		}).join('');
		return `<details class="advanced-group"><summary>${esc(tr(GROUPS[group]))}<span>${fields.length}</span></summary><div class="advanced-group__body">${rows}</div></details>`;
	}).join('');

	container.innerHTML = `
	  <div class="panel__title"><strong>${tr(['高级内核参数','Advanced kernel controls'])}</strong><button class="text-btn" data-action="close-advanced">${tr(['收起','Close'])}</button></div>
	  <p class="hint">${tr(['仅在展开时读取；保存时一次写入并回读验证。','Loaded only on demand; save is one batched write with readback verification.'])}</p>
	  <div class="advanced-list">${groups}</div>
	  <button class="primary wide" data-action="save-advanced">${tr(['保存高级参数','Save advanced controls'])}</button>`;
	container.dataset.loaded = 'true';
}

export async function saveAdvanced(container) {
	const values = new Map();
	for (const field of FIELDS) {
		const input = container.querySelector(`[data-adv="${field.key}"]`);
		if (!input) continue;
		const value = field.boolean ? (input.checked ? 1 : 0) : Number.parseInt(input.value, 10);
		if (!Number.isInteger(value) || value < field.min || value > field.max) {
			throw new Error(`Invalid ${field.key}`);
		}
		values.set(field.key, value);
	}
	await api.applyAdvanced(FIELDS, values);
}
