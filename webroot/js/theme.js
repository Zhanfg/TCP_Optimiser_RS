import { exec } from './kernelsu.js';
import I18N from './i18n.js';

const DYNAMIC_COLOR_RESOURCES = [
	'system_accent1_0', 'system_accent1_100', 'system_accent1_200',
	'system_accent1_600', 'system_accent1_700', 'system_accent1_800', 'system_accent1_900',
	'system_accent2_100', 'system_accent2_700', 'system_accent2_900',
	'system_accent3_100', 'system_accent3_200', 'system_accent3_600',
	'system_accent3_700', 'system_accent3_900',
	'system_neutral1_10', 'system_neutral1_50', 'system_neutral1_100',
	'system_neutral1_800', 'system_neutral1_900',
	'system_neutral2_100', 'system_neutral2_200', 'system_neutral2_400',
	'system_neutral2_500', 'system_neutral2_700', 'system_neutral2_800', 'system_neutral2_900',
];

const TOKEN_MAP = {
	light: {
		'--md-sys-color-surface': 'system_neutral1_10',
		'--md-sys-color-on-surface': 'system_neutral1_900',
		'--md-sys-color-surface-variant': 'system_neutral2_100',
		'--md-sys-color-on-surface-variant': 'system_neutral2_700',
		'--md-sys-color-surface-container-low': 'system_neutral1_50',
		'--md-sys-color-surface-container': 'system_neutral1_100',
		'--md-sys-color-surface-container-high': 'system_neutral2_100',
		'--md-sys-color-surface-container-highest': 'system_neutral2_200',
		'--md-sys-color-primary': 'system_accent1_600',
		'--md-sys-color-on-primary': 'system_accent1_0',
		'--md-sys-color-primary-container': 'system_accent1_100',
		'--md-sys-color-on-primary-container': 'system_accent1_900',
		'--md-sys-color-secondary-container': 'system_accent2_100',
		'--md-sys-color-on-secondary-container': 'system_accent2_900',
		'--md-sys-color-tertiary': 'system_accent3_600',
		'--md-sys-color-tertiary-container': 'system_accent3_100',
		'--md-sys-color-on-tertiary-container': 'system_accent3_900',
		'--md-sys-color-outline': 'system_neutral2_500',
		'--md-sys-color-outline-variant': 'system_neutral2_200',
	},
	dark: {
		'--md-sys-color-surface': 'system_neutral1_900',
		'--md-sys-color-on-surface': 'system_neutral1_100',
		'--md-sys-color-surface-variant': 'system_neutral2_800',
		'--md-sys-color-on-surface-variant': 'system_neutral2_200',
		'--md-sys-color-surface-container-low': 'system_neutral1_900',
		'--md-sys-color-surface-container': 'system_neutral1_800',
		'--md-sys-color-surface-container-high': 'system_neutral2_800',
		'--md-sys-color-surface-container-highest': 'system_neutral2_700',
		'--md-sys-color-primary': 'system_accent1_200',
		'--md-sys-color-on-primary': 'system_accent1_800',
		'--md-sys-color-primary-container': 'system_accent1_700',
		'--md-sys-color-on-primary-container': 'system_accent1_100',
		'--md-sys-color-secondary-container': 'system_accent2_700',
		'--md-sys-color-on-secondary-container': 'system_accent2_100',
		'--md-sys-color-tertiary': 'system_accent3_200',
		'--md-sys-color-tertiary-container': 'system_accent3_700',
		'--md-sys-color-on-tertiary-container': 'system_accent3_100',
		'--md-sys-color-outline': 'system_neutral2_400',
		'--md-sys-color-outline-variant': 'system_neutral2_700',
	},
};

const DYNAMIC_TOKENS = [...new Set(Object.values(TOKEN_MAP).flatMap(map => Object.keys(map)))];

let dynamicPalette = null;
let dynamicColorEnabled = true;
let mediaListenerInstalled = false;

function resolvedMode(mode) {
	return mode === 'auto'
		? (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
		: mode;
}

function parsePalette(output) {
	const values = {};
	for (const line of output.split('\n')) {
		const separator = line.indexOf('=');
		if (separator < 1) continue;
		const name = line.slice(0, separator).trim();
		if (!DYNAMIC_COLOR_RESOURCES.includes(name)) continue;
		const matches = [...line.slice(separator + 1).matchAll(/(?:#|0x)([0-9a-f]{8}|[0-9a-f]{6})/gi)];
		if (matches.length === 0) continue;
		const raw = matches.at(-1)[1];
		values[name] = `#${raw.length === 8 ? raw.slice(2) : raw}`;
	}
	return DYNAMIC_COLOR_RESOURCES.every(name => values[name]) ? values : null;
}

async function readSystemPalette() {
	const names = DYNAMIC_COLOR_RESOURCES.join(' ');
	const command = `# dynamic-color-palette-probe
sdk=$(getprop ro.build.version.sdk 2>/dev/null); case "$sdk" in ''|*[!0-9]*) exit 1 ;; esac; [ "$sdk" -ge 31 ] || exit 1; user=$(am get-current-user 2>/dev/null); case "$user" in ''|*[!0-9]*) user=0 ;; esac; for name in ${names}; do value=$(cmd overlay lookup --user "$user" android "android:color/$name" 2>/dev/null) || exit 1; printf '%s=%s\n' "$name" "$value"; done`;
	try {
		const { stdout } = await exec(command);
		return parsePalette(stdout);
	} catch (error) {
		console.warn('Android dynamic palette unavailable; using fallback colors.');
		return null;
	}
}

function applyPalette(mode) {
	const resolved = resolvedMode(mode);
	const root = document.documentElement;
	root.setAttribute('data-theme', mode);
	root.setAttribute('data-theme-resolved', resolved);
	root.style.colorScheme = resolved;
	for (const token of DYNAMIC_TOKENS) root.style.removeProperty(token);
	if (!dynamicColorEnabled || !dynamicPalette) return;
	for (const [token, resource] of Object.entries(TOKEN_MAP[resolved])) {
		root.style.setProperty(token, dynamicPalette[resource]);
	}
}

function updateDynamicColorStatus() {
	const label = document.getElementById('dynamic-color-status-label');
	const state = !dynamicColorEnabled ? 'off' : dynamicPalette ? 'system' : 'fallback';
	if (label) label.textContent = I18N.t(`dynamic_color_${state}`);
	const card = document.getElementById('dynamic-color-status');
	if (card) card.dataset.state = state;
	const toggle = document.getElementById('dynamic-color-toggle');
	if (toggle) toggle.checked = dynamicColorEnabled;
}

export function setThemeMode(mode) {
	if (!['dark', 'light', 'auto'].includes(mode)) mode = 'auto';
	localStorage.setItem('tcp_themeMode', mode);
	applyPalette(mode);
	updateDynamicColorStatus();
}

export async function setDynamicColorEnabled(enabled) {
	dynamicColorEnabled = enabled === true;
	localStorage.setItem('tcp_dynamicColor', dynamicColorEnabled ? 'true' : 'false');
	if (dynamicColorEnabled && !dynamicPalette) dynamicPalette = await readSystemPalette();
	applyPalette(localStorage.getItem('tcp_themeMode') || 'auto');
	updateDynamicColorStatus();
}

export async function initDynamicColorTheme() {
	dynamicColorEnabled = localStorage.getItem('tcp_dynamicColor') !== 'false';
	if (dynamicColorEnabled) dynamicPalette = await readSystemPalette();
	setThemeMode(localStorage.getItem('tcp_themeMode') || 'auto');
	updateDynamicColorStatus();
	if (!mediaListenerInstalled) {
		window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
			if ((localStorage.getItem('tcp_themeMode') || 'auto') === 'auto') applyPalette('auto');
		});
		document.addEventListener('i18n-changed', updateDynamicColorStatus);
		mediaListenerInstalled = true;
	}
}
