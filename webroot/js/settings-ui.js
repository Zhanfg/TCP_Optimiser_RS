import I18N from './i18n.js';

let initialized = false;
let advancedSearch = null;
let forceGuardInstalled = false;
let syncQueued = false;

function localText(english, chinese) {
	const language = I18N.currentLang || localStorage.getItem('tcp_lang') || document.documentElement.lang;
	return String(language).toLowerCase().startsWith('zh') ? chinese : english;
}

function ensureStyles() {
	if (document.getElementById('settings-product-styles')) return;
	const style = document.createElement('style');
	style.id = 'settings-product-styles';
	style.textContent = `
		.settings-search {
			display: grid;
			grid-template-columns: minmax(0, 1fr) auto;
			gap: 10px;
			margin: 14px 0;
			padding: 10px;
			border: 1px solid var(--md-sys-color-outline);
			border-radius: 18px;
			background: var(--md-sys-color-surface-container-low);
		}
		.settings-search input {
			min-width: 0;
			min-height: 46px;
			padding: 0 14px;
			border: 1px solid var(--md-sys-color-outline);
			border-radius: 14px;
			background: var(--md-sys-color-surface-container-high);
			color: var(--md-sys-color-on-surface);
		}
		.settings-search__count {
			align-self: center;
			min-width: 72px;
			color: var(--md-sys-color-on-surface-variant);
			font-size: 0.76rem;
			text-align: center;
		}
		.advanced-control[hidden], #advanced-sysctl-groups > .settings-group[hidden] { display: none !important; }
		@media (max-width: 679px) {
			#settings-page .settings-action-row,
			#adv-page > #apply-advanced-btn {
				position: sticky;
				bottom: calc(var(--ui-nav-height, 72px) + env(safe-area-inset-bottom, 0px) + 16px);
				z-index: 50;
				padding: 10px;
				border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 55%, transparent);
				border-radius: 18px;
				background: color-mix(in srgb, var(--md-sys-color-surface-container) 94%, transparent);
				box-shadow: 0 12px 32px rgba(0, 0, 0, 0.28);
				-webkit-backdrop-filter: blur(18px);
				backdrop-filter: blur(18px);
			}
			.settings-search { grid-template-columns: 1fr; }
			.settings-search__count { text-align: start; padding-inline: 4px; }
		}
	`;
	document.head.appendChild(style);
}

function syncSelectableState(root = document) {
	for (const element of root.querySelectorAll('.algo-chip, .preset-chip, .theme-mode-btn, .lang-btn')) {
		const selected = element.classList.contains('selected');
		element.setAttribute('aria-pressed', String(selected));
		if (element.classList.contains('unsupported') || element.dataset.unavailable === 'true') {
			element.setAttribute('aria-disabled', 'true');
		} else {
			element.removeAttribute('aria-disabled');
		}
	}
}

function bindSingleOpenDetails(root) {
	for (const details of root.querySelectorAll(':scope > .settings-group, :scope .settings-dashboard > .settings-group')) {
		if (details.dataset.singleOpenBound === 'true') continue;
		details.dataset.singleOpenBound = 'true';
		details.addEventListener('toggle', () => {
			if (!details.open || window.matchMedia('(min-width: 680px)').matches) return;
			const parent = details.parentElement;
			for (const sibling of parent?.children || []) {
				if (sibling !== details && sibling instanceof HTMLDetailsElement && sibling.classList.contains('settings-group')) {
					sibling.open = false;
				}
			}
		});
	}
}

function filterAdvancedControls() {
	const groups = document.getElementById('advanced-sysctl-groups');
	if (!groups || !advancedSearch) return;
	const query = advancedSearch.value.trim().toLocaleLowerCase();
	let visible = 0;
	let total = 0;
	for (const group of groups.querySelectorAll(':scope > .settings-group')) {
		let groupVisible = 0;
		for (const control of group.querySelectorAll('.advanced-control')) {
			total += 1;
			const input = control.querySelector('[data-sysctl-key]');
			const searchable = `${control.textContent} ${input?.dataset.sysctlKey || ''}`.toLocaleLowerCase();
			const match = !query || searchable.includes(query);
			if (control.hidden === match) control.hidden = !match;
			if (match) {
				visible += 1;
				groupVisible += 1;
			}
		}
		const hideGroup = groupVisible === 0;
		if (group.hidden !== hideGroup) group.hidden = hideGroup;
		if (query && groupVisible > 0) group.open = true;
	}
	const count = document.getElementById('advanced-search-count');
	if (count) count.textContent = localText(`${visible} of ${total}`, `${visible} / ${total} 项`);
}

function ensureAdvancedSearch() {
	const groups = document.getElementById('advanced-sysctl-groups');
	if (!groups || document.getElementById('advanced-settings-search')) return;
	const wrapper = document.createElement('div');
	wrapper.id = 'advanced-settings-search';
	wrapper.className = 'settings-search';
	advancedSearch = document.createElement('input');
	advancedSearch.type = 'search';
	advancedSearch.autocomplete = 'off';
	advancedSearch.spellcheck = false;
	advancedSearch.placeholder = localText('Search kernel parameters', '搜索内核参数');
	advancedSearch.setAttribute('aria-label', advancedSearch.placeholder);
	const count = document.createElement('span');
	count.id = 'advanced-search-count';
	count.className = 'settings-search__count';
	wrapper.append(advancedSearch, count);
	groups.before(wrapper);
	advancedSearch.addEventListener('input', filterAdvancedControls);
	filterAdvancedControls();
}

function syncAdvancedSearchLanguage() {
	if (!advancedSearch) return;
	advancedSearch.placeholder = localText('Search kernel parameters', '搜索内核参数');
	advancedSearch.setAttribute('aria-label', advancedSearch.placeholder);
	filterAdvancedControls();
}

function installForceApplyGuard() {
	if (forceGuardInstalled) return;
	const button = document.getElementById('force-apply-btn');
	if (!button) return;
	forceGuardInstalled = true;
	button.addEventListener('click', event => {
		const confirmed = window.confirm(localText(
			'Apply the selected policy immediately? Active connections may be affected by the configured connection-reset option.',
			'立即应用所选策略吗？若已启用断开连接选项，当前连接可能受到影响。',
		));
		if (confirmed) return;
		event.preventDefault();
		event.stopImmediatePropagation();
	}, { capture: true });
}

function syncSettingsSemantics() {
	const settingsPage = document.getElementById('settings-page');
	const advancedPage = document.getElementById('adv-page');
	if (settingsPage) {
		syncSelectableState(settingsPage);
		bindSingleOpenDetails(settingsPage);
	}
	if (advancedPage) {
		syncSelectableState(advancedPage);
		bindSingleOpenDetails(document.getElementById('advanced-sysctl-groups') || advancedPage);
		ensureAdvancedSearch();
		filterAdvancedControls();
	}
	installForceApplyGuard();
}

function scheduleSettingsSync() {
	if (syncQueued) return;
	syncQueued = true;
	queueMicrotask(() => {
		syncQueued = false;
		syncSettingsSemantics();
	});
}

function bindDelegatedSync(root) {
	if (!root || root.dataset.settingsSyncBound === 'true') return;
	root.dataset.settingsSyncBound = 'true';
	root.addEventListener('click', scheduleSettingsSync);
	root.addEventListener('change', scheduleSettingsSync);
}

export function initSettingsEnhancements() {
	if (initialized) {
		scheduleSettingsSync();
		return;
	}
	initialized = true;
	ensureStyles();
	syncSettingsSemantics();
	bindDelegatedSync(document.getElementById('settings-page'));
	bindDelegatedSync(document.getElementById('adv-page'));
	document.addEventListener('i18n-changed', () => {
		syncSelectableState();
		syncAdvancedSearchLanguage();
	});
	document.addEventListener('tcp:page-change', event => {
		if (event.detail?.page === 'settings' || event.detail?.page === 'adv') scheduleSettingsSync();
	});
}
