const reducedMotionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');

const HAPTIC_PATTERNS = Object.freeze({
	light: 8,
	selection: 10,
	success: [12, 32, 18],
	error: [18, 38, 18],
});

function motionAllowed() {
	return !reducedMotionQuery.matches;
}

export function haptic(kind = 'light') {
	if (typeof navigator.vibrate !== 'function' || document.hidden) return false;
	try {
		return navigator.vibrate(HAPTIC_PATTERNS[kind] || HAPTIC_PATTERNS.light);
	} catch (_) {
		return false;
	}
}

export function replayMotion(element, className) {
	if (!element || !motionAllowed()) return;
	element.classList.remove(className);
	requestAnimationFrame(() => element.classList.add(className));
}

export function setAnimatedText(element, value) {
	if (!element) return;
	const next = String(value);
	if (element.textContent === next) return;
	element.textContent = next;
	replayMotion(element, 'value-updated');
}

export function initMotion() {
	document.documentElement.dataset.haptics = typeof navigator.vibrate === 'function' ? 'supported' : 'unsupported';
	requestAnimationFrame(() => document.documentElement.classList.add('motion-ready'));

	document.addEventListener('click', event => {
		if (!event.isTrusted) return;
		const target = event.target.closest('button, [role="button"], summary, label.toggle');
		if (!target || target.matches(':disabled, [aria-disabled="true"]')) return;
		haptic(target.matches('.btn-primary, #adv-confirm-btn') ? 'selection' : 'light');
	}, { capture: true, passive: true });
}
