export const ALL_ALGOS = [
	'bbr', 'bbr2', 'bbr3', 'cubic', 'westwood', 'westwood_plus', 'reno',
	'htcp', 'vegas', 'yeah', 'illinois', 'dctcp', 'cdg', 'bic', 'highspeed',
	'hybla', 'nv', 'scalable', 'lp',
];

export const ALL_QDISCS = [
	'fq', 'fq_codel', 'cake', 'pfifo_fast', 'codel', 'fq_pie', 'pfifo',
];

export const ALGO_DESC = {
	bbr: 'Google BBR — high throughput, low latency',
	bbr2: 'BBR v2 — improved fairness',
	bbr3: 'BBR v3 — experimental',
	cubic: 'Default Linux — stable and reliable',
	westwood: 'Bandwidth estimation — good for wireless',
	westwood_plus: 'Westwood+ — improved wireless variant',
	reno: 'Classic TCP — widely compatible',
	htcp: 'Hamilton TCP — high-speed long-distance',
	vegas: 'Delay-based — low latency',
	yeah: 'YeAH — high-speed with fairness',
	illinois: 'Illinois — hybrid for high BDP',
	dctcp: 'Data Center TCP — low queuing',
	cdg: 'CAIA Delay Gradient',
	bic: 'Binary Increase',
	highspeed: 'RFC 3649 for fast links',
	hybla: 'Satellite / high-latency links',
	nv: 'New Vegas — modern delay-based',
	scalable: 'Scalable — simple high-speed',
	lp: 'Low Priority — background transfers',
};

const ALGO_DESC_ZH = {
	bbr: 'Google BBR — 高吞吐、低延迟，适合多数高速网络',
	bbr2: 'BBR v2 — 改善多连接公平性',
	bbr3: 'BBR v3 — 实验性新版本',
	cubic: 'Linux 默认算法 — 稳定且兼容性好',
	westwood: '带宽估算算法 — 适合无线网络',
	westwood_plus: 'Westwood+ — 改进的无线网络版本',
	reno: '经典 TCP — 兼容范围广',
	htcp: 'Hamilton TCP — 适合高速长距离链路',
	vegas: '基于延迟 — 偏向低排队延迟',
	yeah: 'YeAH — 兼顾高速与公平性',
	illinois: 'Illinois — 适合高带宽时延积链路',
	dctcp: '数据中心 TCP — 偏向低排队延迟',
	cdg: 'CAIA 延迟梯度算法',
	bic: '二进制增长算法',
	highspeed: 'RFC 3649 — 面向高速链路',
	hybla: '适合卫星和高延迟链路',
	nv: 'New Vegas — 现代延迟型算法',
	scalable: 'Scalable TCP — 简洁的高速算法',
	lp: '低优先级算法 — 适合后台传输',
};

export const QDISC_DESC = {
	fq: 'Fair Queue — low latency and a strong BBR pairing',
	fq_codel: 'Fair Queuing + CoDel — balanced latency control',
	cake: 'CAKE — advanced fairness and traffic shaping',
	pfifo_fast: 'Classic priority FIFO — broad compatibility',
	codel: 'CoDel — controls persistent queue delay',
	fq_pie: 'Fair Queue + PIE — active queue management',
	pfifo: 'Simple packet FIFO queue',
};

const QDISC_DESC_ZH = {
	fq: '公平队列 — 低延迟，适合与 BBR 搭配',
	fq_codel: '公平队列 + CoDel — 均衡控制排队延迟',
	cake: 'CAKE — 更强的公平性与流量整形',
	pfifo_fast: '经典优先级 FIFO — 兼容性广',
	codel: 'CoDel — 控制持续排队延迟',
	fq_pie: '公平队列 + PIE — 主动队列管理',
	pfifo: '简单的先进先出数据包队列',
};

export function getAlgorithmDescription(name, language = 'en') {
	return (language === 'zh' ? ALGO_DESC_ZH : ALGO_DESC)[name] || name;
}

export function getQdiscDescription(name, language = 'en') {
	return (language === 'zh' ? QDISC_DESC_ZH : QDISC_DESC)[name] || name;
}
