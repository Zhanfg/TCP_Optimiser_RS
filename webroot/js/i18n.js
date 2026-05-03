const I18N = {
	source: {},
	translations: {},
	currentLang: 'en',

	async loadSource() {
		try {
			const resp = await fetch('lang/source/string.json');
			this.source = await resp.json();
		} catch (e) {
			console.error('Failed to load i18n source:', e);
			this.source = {};
		}
	},

	async loadLang(lang) {
		if (lang === 'en' || !lang) {
			this.translations = {};
			this.currentLang = 'en';
			return;
		}
		try {
			const resp = await fetch(`lang/${lang}.json`);
			this.translations = await resp.json();
			this.currentLang = lang;
		} catch (e) {
			console.warn(`Failed to load lang "${lang}", falling back to English`);
			this.translations = {};
			this.currentLang = 'en';
		}
	},

	t(key, params) {
		let text = this.translations[key] || this.source[key] || key;
		if (params) {
			Object.entries(params).forEach(([k, v]) => {
				text = text.replace(`{${k}}`, v);
			});
		}
		return text;
	},

	applyToDOM(root = document) {
		root.querySelectorAll('[data-i18n]').forEach(el => {
			const key = el.dataset.i18n;
			el.textContent = this.t(key);
		});
		root.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
			const key = el.dataset.i18nPlaceholder;
			el.placeholder = this.t(key);
		});
	},

	async init() {
		await this.loadSource();
		const saved = localStorage.getItem('tcp_lang') || 'en';
		await this.loadLang(saved);
		this.applyToDOM();
	},

	async switchTo(lang) {
		localStorage.setItem('tcp_lang', lang);
		await this.loadLang(lang);
		this.applyToDOM();
		document.dispatchEvent(new CustomEvent('i18n-changed'));
	}
};

export default I18N;
