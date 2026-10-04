let current = 'en';

function normalize(value) {
	return String(value || '').toLowerCase().startsWith('zh') ? 'zh' : 'en';
}

export function initLocale() {
	const saved = localStorage.getItem('tcp_lang');
	current = saved ? normalize(saved) : normalize(navigator.language);
	document.documentElement.lang = current === 'zh' ? 'zh-CN' : 'en';
	return current;
}

export function language() {
	return current;
}

export function isZh() {
	return current === 'zh';
}

export function t(zh, en) {
	return current === 'zh' ? zh : en;
}

export function setLanguage(value) {
	current = normalize(value);
	localStorage.setItem('tcp_lang', current);
	document.documentElement.lang = current === 'zh' ? 'zh-CN' : 'en';
	document.dispatchEvent(new CustomEvent('locale-changed', { detail: current }));
}
