import fs from 'node:fs';

const read = file => fs.readFileSync(file, 'utf8');
const fail = message => {
	console.error(`release validation: ${message}`);
	process.exitCode = 1;
};

const cargo = read('Cargo.toml');
const modulePropSource = read('module.prop');
const update = JSON.parse(read('update.json'));
const moduleProp = Object.fromEntries(modulePropSource
	.split(/\r?\n/)
	.filter(line => line && !line.startsWith('#') && line.includes('='))
	.map(line => {
		const separator = line.indexOf('=');
		return [line.slice(0, separator), line.slice(separator + 1)];
	}));
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const expectedZip = `TCP_Optimiser_RS-v${moduleProp.version}.zip`;

if (!cargoVersion) fail('Cargo.toml package version is missing');
if (cargoVersion !== moduleProp.version) fail(`Cargo.toml ${cargoVersion} != module.prop ${moduleProp.version}`);
if (update.version !== moduleProp.version) fail(`update.json ${update.version} != module.prop ${moduleProp.version}`);
if (String(update.versionCode) !== moduleProp.versionCode) fail('versionCode differs between update.json and module.prop');
if (!Number.isSafeInteger(update.versionCode) || update.versionCode < 1) fail('versionCode must be a positive integer');
if (typeof update.zipUrl !== 'string' || !update.zipUrl.endsWith(`/${expectedZip}`)) fail(`zipUrl must end with /${expectedZip}`);
if (/stealth|幽灵|隐身/i.test(modulePropSource)) fail('removed stealth metadata is still present');

if (!process.exitCode) console.log(`release validation: v${moduleProp.version} (${expectedZip})`);
