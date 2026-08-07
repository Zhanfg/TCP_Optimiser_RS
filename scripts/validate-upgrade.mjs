import fs from 'node:fs';
import process from 'node:process';

const read = path => fs.readFileSync(path, 'utf8');
const fail = message => {
  console.error(`upgrade validation: ${message}`);
  process.exitCode = 1;
};
const requireText = (source, needle, message) => {
  if (!source.includes(needle)) fail(message);
};
const rejectText = (source, needle, message) => {
  if (source.includes(needle)) fail(message);
};

const moduleProp = read('module.prop');
const customize = read('customize.sh');
const install = read('src/install.rs');
const baselineStatus = read('src/baseline_status.rs');
const uninstall = read('uninstall.sh');
const rollbackDocs = read('docs/ROLLBACK-SAFETY.md');

requireText(moduleProp, 'id=tcp_optimiser\n', 'module ID must remain tcp_optimiser');
requireText(moduleProp, 'name=TCP Optimiser\n', 'module display name must remain TCP Optimiser');

requireText(customize, "grep -Fxq 'id=tcp_optimiser'", 'installer must verify stable module ID');
requireText(customize, "grep -Fxq 'name=TCP Optimiser'", 'installer must verify stable display name');
requireText(customize, 'preparing same-name in-place upgrade', 'installer must detect direct replacement');
requireText(customize, 'kill "$LIVE_PID"', 'old daemon must be stopped before migration');
rejectText(customize, 'Direct upgrade refused', 'shell installer must not reject same-name upgrades');

requireText(install, 'LegacyUpgradeSnapshot', 'Rust installer must classify legacy upgrades');
requireText(install, 'legacy_upgrade_snapshot', 'legacy compatibility provenance must be explicit');
requireText(install, 'exact_pre_module', 'exact baseline provenance must be explicit');
requireText(install, 'baseline-provenance-v1.json', 'baseline provenance sidecar must be preserved');
requireText(install, 'runtime-control-v1.json', 'runtime control state must survive upgrades');
requireText(install, 'last-good-policy-v2.json', 'last-known-good checkpoint must survive upgrades');
requireText(install, 'policy-failures-v1.json', 'failure journal must survive upgrades');
requireText(install, '"pacing_ca"', 'pacing CA override must use its actual filename');
rejectText(install, 'legacy TCP Optimiser installation has no exact kernel baseline', 'Rust installer must not hard-reject legacy upgrades');

requireText(baselineStatus, 'pub provenance: String', 'baseline status must expose provenance');
requireText(baselineStatus, 'pub exact_pre_module: bool', 'baseline status must expose exactness');
requireText(uninstall, 'Pre-upgrade compatibility snapshot restored', 'uninstall must report compatibility restores accurately');
requireText(rollbackDocs, 'Same-name in-place upgrade', 'rollback documentation must describe direct replacement');

if (!process.exitCode) {
  console.log('upgrade validation: stable identity, direct replacement, state preservation and baseline provenance enforced');
}
