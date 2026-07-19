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
const javascript = fs.readdirSync(path.join(root, 'webroot/js'))
	.filter(file => file.endsWith('.js'))
	.map(file => read(`webroot/js/${file}`))
	.join('\n');
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

for (const file of fs.readdirSync(path.join(root, 'webroot/js')).filter(name => name.endsWith('.js'))) {
	const source = read(`webroot/js/${file}`);
	for (const match of source.matchAll(/(?:import|export)\s+(?:[\s\S]*?\s+from\s+)?['"](\.\.?\/[^'"]+)['"]/g)) {
		const imported = path.normalize(path.join('webroot/js', path.dirname(file), match[1]));
		if (!fs.existsSync(path.join(root, imported))) fail(`webroot/js/${file} imports missing ${match[1]}`);
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

if (!process.exitCode) {
	console.log(`webui validation: ${Object.keys(english).length} translations, ${rustAlgorithms.length} algorithms, ${rustQdiscs.length} qdiscs`);
}
