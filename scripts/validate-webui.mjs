import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const root = process.cwd();
const read = relative => fs.readFileSync(path.join(root, relative), 'utf8');
const fail = message => {
	console.error(`webui validation: ${message}`);
	process.exitCode = 1;
};

function parseFlatLanguageFile(relative) {
	const source = read(relative);
	const seen = new Map();
	for (const match of source.matchAll(/^\s*"((?:\\.|[^"])*)"\s*:/gm)) {
		const key = JSON.parse(`"${match[1]}"`);
		const line = source.slice(0, match.index).split(/\r?\n/).length;
		if (seen.has(key)) fail(`${relative}:${line} duplicates ${key} (first at ${seen.get(key)})`);
		else seen.set(key, line);
	}
	try {
		return JSON.parse(source);
	} catch (error) {
		fail(`${relative} is invalid JSON: ${error.message}`);
		return {};
	}
}

function walk(dir) {
	return fs.readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
		const full = path.join(dir, entry.name);
		return entry.isDirectory() ? walk(full) : [full];
	});
}

function quotedValues(source, declaration) {
	const block = source.match(new RegExp(`${declaration}[^=]*=\\s*&?\\[([\\s\\S]*?)\\];`));
	if (!block) {
		fail(`cannot find ${declaration}`);
		return [];
	}
	return [...block[1].matchAll(/["']([a-z0-9_]+)["']/g)].map(match => match[1]);
}

const english = parseFlatLanguageFile('webroot/lang/source/string.json');
const chinese = parseFlatLanguageFile('webroot/lang/zh.json');
for (const key of Object.keys(english)) if (!(key in chinese)) fail(`zh.json is missing ${key}`);
for (const key of Object.keys(chinese)) if (!(key in english)) fail(`zh.json has unknown key ${key}`);

const html = read('webroot/index.html');
const jsFiles = walk(path.join(root, 'webroot/js'))
	.filter(file => file.endsWith('.js'))
	.map(file => path.relative(root, file));
const javascript = jsFiles.map(read).join('\n');
const referencedKeys = new Set([
	...[...html.matchAll(/data-i18n(?:-placeholder|-aria-label)?="([^"]+)"/g)].map(match => match[1]),
	...[...javascript.matchAll(/I18N\.t\(\s*['"]([A-Za-z0-9_]+)['"]\s*(?:[,\)])/g)].map(match => match[1]),
]);
for (const key of referencedKeys) if (!(key in english)) fail(`translation key ${key} is referenced but undefined`);

const ids = new Map();
for (const match of html.matchAll(/\sid="([^"]+)"/g)) {
	const id = match[1];
	const line = html.slice(0, match.index).split(/\r?\n/).length;
	if (ids.has(id)) fail(`webroot/index.html:${line} duplicates id ${id} (first at ${ids.get(id)})`);
	else ids.set(id, line);
}

for (const match of html.matchAll(/<script[^>]+src="([^"]+)"/g)) {
	const relative = path.join('webroot', match[1]);
	if (!fs.existsSync(path.join(root, relative))) fail(`missing script ${relative}`);
}

for (const file of jsFiles) {
	const source = read(file);
	const base = path.dirname(file);
	for (const match of source.matchAll(/(?:import|export)\s+(?:[\s\S]*?\s+from\s+)?['"](\.\.?\/[^'"]+)['"]/g)) {
		const imported = path.normalize(path.join(base, match[1]));
		const target = path.join(root, imported);
		const resolved = fs.existsSync(target) ? target : `${target}.js`;
		if (!fs.existsSync(resolved)) fail(`${file} imports missing ${match[1]}`);
	}
}

const rust = read('src/config.rs');
const capabilities = read('webroot/js/capabilities.js');
const rustAlgorithms = quotedValues(rust, 'pub const ALL_ALGOS');
const uiAlgorithms = quotedValues(capabilities, 'export const ALL_ALGOS');
if (JSON.stringify(rustAlgorithms) !== JSON.stringify(uiAlgorithms)) fail('Rust and WebUI algorithm lists differ');
const rustQdiscs = quotedValues(rust, 'pub const KNOWN_QDISCS');
const uiQdiscs = quotedValues(capabilities, 'export const ALL_QDISCS');
if (JSON.stringify(rustQdiscs) !== JSON.stringify(uiQdiscs)) fail('Rust and WebUI qdisc lists differ');

const index = read('webroot/index.html');
const app = read('webroot/js/v41/app.js');
const api = read('webroot/js/v41/api.js');
const views = read('webroot/js/v41/views.js');
const css = read('webroot/css/v41.css');
const ksuBridge = read('webroot/js/kernelsu.js');

const sizeChecks = {
	'webroot/index.html': 4096,
	'webroot/css/v41.css': 16384,
	'webroot/js/kernelsu.js': 6144,
	'webroot/js/v41/app.js': 10240,
	'webroot/js/v41/api.js': 16384,
	'webroot/js/v41/views.js': 24576,
};
for (const [file, maxBytes] of Object.entries(sizeChecks)) {
	const bytes = Buffer.byteLength(read(file), 'utf8');
	if (bytes > maxBytes) fail(`${file} is ${bytes} bytes; v4.1 budget is ${maxBytes}`);
}

const architectureChecks = {
	lightweight_shell: !index.includes('id="home-page"') && index.includes('id="app-outlet"'),
	lazy_page_mounting: ['homeTemplate','settingsTemplate','statsTemplate','logsTemplate'].every(name => app.includes(name)),
	single_scheduler: app.includes('let timer = null') && !app.includes('setInterval('),
	deduped_bridge: api.includes('const inflight = new Map()') && api.includes('function once('),
	single_runtime_read: api.includes('runtime_snapshot.json'),
	lazy_qdisc_probe: views.includes("action === 'load-qdiscs'"),
	log_tail_not_full_cat: api.includes('tail -n 160'),
	no_dynamic_palette_boot_probe: !app.includes('initDynamicColorTheme'),
	no_legacy_router_entry: !index.includes('js/router.js'),
	no_heavy_motion_entry: !index.includes('js/motion.js'),
	no_backdrop_filter: !css.includes('backdrop-filter'),
	preview_mock_split: !ksuBridge.includes('function mockExec') && ksuBridge.includes("import('./mock-ksu.js')"),
	production_bridge_small: Buffer.byteLength(ksuBridge, 'utf8') <= 6144,
	no_legacy_frontend_files: [
		'webroot/js/common.js','webroot/js/router.js','webroot/js/home.js',
		'webroot/js/settings.js','webroot/js/stats.js','webroot/js/logs.js',
		'webroot/js/motion.js','webroot/js/theme.js','webroot/js/debug.js',
		'webroot/css/main.css',
	].every(file => !fs.existsSync(path.join(root, file))),
};
for (const [name, ok] of Object.entries(architectureChecks)) if (!ok) fail(`v4.1 architecture invariant failed: ${name}`);

const mainRust = read('src/main.rs');
const coreCoverage = [
	['Daemon', 'service.sh', '"$RUST_BIN" daemon'],
	['Once', 'post-fs-data.sh', '"$RUST_BIN" once'],
	['Install', 'customize.sh', '"$RUST_BIN" install'],
	['VerifyModule', 'webroot/js/v41/api.js', 'verify-module'],
	['Status', 'webroot/js/v41/api.js', 'status --runtime-only'],
	['Sample', 'webroot/js/v41/api.js', 'sample'],
	['ApplyNow', 'webroot/js/v41/api.js', 'apply-now'],
];
for (const [command, file, needle] of coreCoverage) {
	if (!mainRust.includes(`Command::${command}`)) fail(`core coverage references missing Rust command ${command}`);
	if (!read(file).includes(needle)) fail(`Rust command ${command} has no v4.1 consumer in ${file}`);
}

for (const id of ['app-outlet','shell-status','shell-title']) {
	if (!ids.has(id)) fail(`v4.1 shell is missing #${id}`);
}

if (!process.exitCode) {
	console.log(`webui validation: v4.1 lightweight architecture OK; ${jsFiles.length} JS files checked; ${rustAlgorithms.length} algorithms; ${rustQdiscs.length} qdiscs`);
}
