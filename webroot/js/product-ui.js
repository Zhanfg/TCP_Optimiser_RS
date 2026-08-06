import I18N from './i18n.js';

const OVERLAY_IDS = ['about-overlay', 'detail-overlay', 'adv-warning-overlay', 'debug-overlay'];
const pageScroll = new Map();
let lastFocusedElement = null;
let activeOverlay = null;
let liveRegion = null;
let refreshButton = null;
let initialized = false;

function ensureProductStyles() {
	document.documentElement.classList.add('product-ui');
	if (document.getElementById('product-ui-styles')) return;
	const link = document.createElement('link');
	link.id = 'product-ui-styles';
	link.rel = 'stylesheet';
	link.href = 'css/product.css';
	document.head.appendChild(link);
}

ensureProductStyles();

function localText(english, chinese) {
	const language = I18N.currentLang || localStorage.getItem('tcp_lang') || document.documentElement.lang;
	return String(language).toLowerCase().startsWith('zh') ? chinese : english;
}

function ensureSkipLink() {
	if (document.querySelector('.skip-link')) return;
	const pages = document.getElementById('pages');
	if (!pages) return;
	pages.tabIndex = -1;
	const link = document.createElement('a');
	link.className = 'skip-link';
	link.href = '#pages';
	link.textContent = localText('Skip to content', '跳到主要内容');
	link.addEventListener('click', () => requestAnimationFrame(() => pages.focus({ preventScroll: true })));
	document.body.prepend(link);
}

function ensureLiveRegion() {
	liveRegion = document.getElementById('ui-live-region');
	if (liveRegion) return;
	liveRegion = document.createElement('div');
	liveRegion.id = 'ui-live-region';
	liveRegion.setAttribute('role', 'status');
	liveRegion.setAttribute('aria-live', 'polite');
	liveRegion.setAttribute('aria-atomic', 'true');
	Object.assign(liveRegion.style, {
		position: 'fixed',
		width: '1px',
		height: '1px',
		overflow: 'hidden',
		clip: 'rect(0 0 0 0)',
		clipPath: 'inset(50%)',
		whiteSpace: 'nowrap',
	});
	document.body.appendChild(liveRegion);
}

function updateRefreshLabel() {
	if (!refreshButton) return;
	const label = localText('Refresh current page', '刷新当前页面');
	refreshButton.title = label;
	refreshButton.setAttribute('aria-label', label);
}

function ensureGlobalRefresh() {
	if (document.getElementById('global-refresh-btn')) {
		refreshButton = document.getElementById('global-refresh-btn');
		updateRefreshLabel();
		return;
	}
	const statusChip = document.getElementById('status-chip');
	if (!statusChip?.parentElement) return;
	refreshButton = document.createElement('button');
	refreshButton.type = 'button';
	refreshButton.id = 'global-refresh-btn';
	refreshButton.className = 'global-refresh-btn';
	refreshButton.innerHTML = '<span class="ui-icon icon-refresh" aria-hidden="true"></span>';
	updateRefreshLabel();
	refreshButton.addEventListener('click', () => {
		if (refreshButton.getAttribute('aria-busy') === 'true') return;
		document.dispatchEvent(new CustomEvent('tcp:refresh', { detail: { source: 'toolbar' } }));
	});
	statusChip.before(refreshButton);
}

function visibleNavItems() {
	return [...document.querySelectorAll('.nav-item')].filter(item => {
		if (item.hidden || item.classList.contains('hidden')) return false;
		return getComputedStyle(item).display !== 'none';
	});
}

function syncNavTabStops() {
	const items = visibleNavItems();
	for (const item of items) item.tabIndex = item.classList.contains('active') ? 0 : -1;
	if (items.length > 0 && !items.some(item => item.tabIndex === 0)) items[0].tabIndex = 0;
}

function initNavigationKeyboard() {
	const nav = document.getElementById('nav-bar');
	if (!nav) return;
	nav.setAttribute('aria-label', localText('Main navigation', '主导航'));
	nav.addEventListener('keydown', event => {
		if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return;
		const items = visibleNavItems();
		const currentIndex = Math.max(0, items.indexOf(document.activeElement));
		let nextIndex = currentIndex;
		if (event.key === 'Home') nextIndex = 0;
		else if (event.key === 'End') nextIndex = items.length - 1;
		else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') nextIndex = (currentIndex - 1 + items.length) % items.length;
		else nextIndex = (currentIndex + 1) % items.length;
		event.preventDefault();
		items[nextIndex]?.focus();
	});
	syncNavTabStops();
	new MutationObserver(syncNavTabStops).observe(nav, {
		subtree: true,
		attributes: true,
		attributeFilter: ['class', 'hidden', 'aria-current'],
	});
}

function focusableElements(container) {
	if (!container) return [];
	return [...container.querySelectorAll(
		'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary, [tabindex]:not([tabindex="-1"])'
	)].filter(element => !element.hidden && getComputedStyle(element).visibility !== 'hidden' && getComputedStyle(element).display !== 'none');
}

function firstDialogElement(overlay) {
	return overlay?.querySelector('[role="dialog"]') || overlay?.firstElementChild || overlay;
}

function closeOverlay(overlay) {
	if (!overlay || overlay.hidden) return;
	const explicitClose = overlay.id === 'about-overlay'
		? document.getElementById('about-close-btn')
		: overlay.id === 'adv-warning-overlay'
			? document.getElementById('adv-cancel-btn')
			: overlay.id === 'debug-overlay'
				? document.getElementById('debug-close-btn')
				: null;
	if (explicitClose) explicitClose.click();
	else overlay.hidden = true;
}

function syncModalState() {
	const openOverlays = OVERLAY_IDS
		.map(id => document.getElementById(id))
		.filter(overlay => overlay && !overlay.hidden);
	const nextOverlay = openOverlays.at(-1) || null;
	if (nextOverlay && nextOverlay !== activeOverlay) {
		lastFocusedElement = document.activeElement instanceof HTMLElement ? document.activeElement : null;
		activeOverlay = nextOverlay;
		document.body.classList.add('modal-open');
		requestAnimationFrame(() => {
			const dialog = firstDialogElement(activeOverlay);
			const target = focusableElements(dialog)[0] || dialog;
			if (target instanceof HTMLElement) {
				if (!target.hasAttribute('tabindex') && target === dialog) target.tabIndex = -1;
				target.focus({ preventScroll: true });
			}
		});
		return;
	}
	if (!nextOverlay && activeOverlay) {
		activeOverlay = null;
		document.body.classList.remove('modal-open');
		if (lastFocusedElement?.isConnected) lastFocusedElement.focus({ preventScroll: true });
		lastFocusedElement = null;
	}
}

function initModalManagement() {
	for (const id of OVERLAY_IDS) {
		const overlay = document.getElementById(id);
		if (!overlay) continue;
		new MutationObserver(syncModalState).observe(overlay, { attributes: true, attributeFilter: ['hidden'] });
		overlay.addEventListener('click', event => {
			if (event.target === overlay && overlay.id !== 'debug-overlay') closeOverlay(overlay);
		});
	}
	document.addEventListener('keydown', event => {
		if (!activeOverlay) return;
		if (event.key === 'Escape') {
			event.preventDefault();
			closeOverlay(activeOverlay);
			return;
		}
		if (event.key !== 'Tab') return;
		const dialog = firstDialogElement(activeOverlay);
		const items = focusableElements(dialog);
		if (items.length === 0) {
			event.preventDefault();
			dialog?.focus();
			return;
		}
		const first = items[0];
		const last = items.at(-1);
		if (event.shiftKey && document.activeElement === first) {
			event.preventDefault();
			last.focus();
		} else if (!event.shiftKey && document.activeElement === last) {
			event.preventDefault();
			first.focus();
		}
	});
	syncModalState();
}

function persistDisclosureState() {
	const ids = [
		'verification-panel', 'home-capability-panel', 'network-policy-group', 'qdisc-policy-group',
		'preset-policy-group', 'advanced-policy-group', 'appearance-policy-group', 'stats-connection-panel',
		'stats-charts-panel', 'stats-dns-panel',
	];
	for (const id of ids) {
		const details = document.getElementById(id);
		if (!(details instanceof HTMLDetailsElement)) continue;
		const key = `tcp_details_${id}`;
		const saved = sessionStorage.getItem(key);
		if (saved !== null) details.open = saved === 'true';
		details.addEventListener('toggle', () => sessionStorage.setItem(key, String(details.open)));
	}
}

function improveInputs() {
	for (const input of document.querySelectorAll('input[type="text"], textarea')) {
		input.spellcheck = false;
		input.autocomplete = 'off';
	}
	const statusChip = document.getElementById('status-chip');
	statusChip?.setAttribute('role', 'status');
	statusChip?.setAttribute('aria-live', 'polite');
	statusChip?.setAttribute('aria-atomic', 'true');
}

export function rememberPageScroll(pageName) {
	if (pageName) pageScroll.set(pageName, window.scrollY);
}

export function restorePageScroll(pageName, reset = false) {
	requestAnimationFrame(() => {
		window.scrollTo({ top: reset ? 0 : (pageScroll.get(pageName) || 0), behavior: 'instant' });
	});
}

export function setGlobalBusy(busy) {
	if (!refreshButton) return;
	refreshButton.setAttribute('aria-busy', String(Boolean(busy)));
	refreshButton.disabled = Boolean(busy);
}

export function announce(message) {
	if (!liveRegion) return;
	liveRegion.textContent = '';
	requestAnimationFrame(() => { liveRegion.textContent = String(message || ''); });
}

export function syncProductUILanguage() {
	updateRefreshLabel();
	const skip = document.querySelector('.skip-link');
	if (skip) skip.textContent = localText('Skip to content', '跳到主要内容');
	const nav = document.getElementById('nav-bar');
	if (nav) nav.setAttribute('aria-label', localText('Main navigation', '主导航'));
}

export function initProductUI() {
	if (initialized) return;
	initialized = true;
	ensureProductStyles();
	ensureSkipLink();
	ensureLiveRegion();
	ensureGlobalRefresh();
	initNavigationKeyboard();
	initModalManagement();
	persistDisclosureState();
	improveInputs();
	document.addEventListener('i18n-changed', syncProductUILanguage);
	document.addEventListener('tcp:page-change', syncNavTabStops);
}
