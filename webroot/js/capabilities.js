export const ALL_ALGOS = ['bbr', 'bbr3', 'cubic', 'reno'];

export const ALL_QDISCS = ['fq', 'fq_codel', 'codel', 'cake', 'pie', 'fq_pie'];

export const ALGO_DESC = {
	bbr: 'BBRv1 · kernel native',
	bbr3: 'BBRv3 · bundled kernel module',
	cubic: 'CUBIC · kernel native',
	reno: 'Reno · kernel native',
};

const ALGO_DESC_ZH = {
	bbr: 'BBRv1 · 内核原生',
	bbr3: 'BBRv3 · 随模块提供',
	cubic: 'CUBIC · 内核原生',
	reno: 'Reno · 内核原生',
};

export const QDISC_DESC = {
	fq: 'FQ · kernel native fair queue',
	fq_codel: 'FQ-CoDel · kernel native fair queue + CoDel',
	codel: 'CoDel · kernel native active queue management',
	cake: 'CAKE · bundled queue discipline',
	pie: 'PIE · bundled active queue management',
	fq_pie: 'FQ-PIE · bundled fair queue + PIE',
};

const QDISC_DESC_ZH = {
	fq: 'FQ · 内核原生公平队列',
	fq_codel: 'FQ-CoDel · 内核原生公平队列 + CoDel',
	codel: 'CoDel · 内核原生主动队列管理',
	cake: 'CAKE · 随模块提供',
	pie: 'PIE · 随模块提供主动队列管理',
	fq_pie: 'FQ-PIE · 随模块提供公平队列 + PIE',
};

export function getAlgorithmDisplayName(name) {
	if (name === 'bbr') return 'BBRv1';
	if (name === 'bbr3') return 'BBRv3';
	if (name === 'cubic') return 'CUBIC';
	if (name === 'reno') return 'Reno';
	return name;
}

export function getAlgorithmDescription(name, language = 'en') {
	return (language === 'zh' ? ALGO_DESC_ZH : ALGO_DESC)[name] || name;
}

export function getQdiscDescription(name, language = 'en') {
	return (language === 'zh' ? QDISC_DESC_ZH : QDISC_DESC)[name] || name;
}
