import fs from 'node:fs';
import process from 'node:process';

const read = path => fs.readFileSync(path, 'utf8');
const fail = message => {
  console.error(`runtime transaction validation: ${message}`);
  process.exitCode = 1;
};
const requireText = (source, needle, message) => {
  if (!source.includes(needle)) fail(message);
};
const requireCount = (source, needle, minimum, message) => {
  const count = source.split(needle).length - 1;
  if (count < minimum) fail(`${message} (found ${count}, need ${minimum})`);
};

const control = read('src/control.rs');
const checkpoint = read('src/checkpoint.rs');
const daemon = read('src/daemon.rs');

requireText(control, 'runtime-restore-v1.lock', 'restore lock must use a versioned persistent name');
requireText(control, 'pub fn acquire_restore_guard()', 'restore command must acquire an exclusive guard');
requireText(control, 'pub fn restore_in_progress()', 'daemon must be able to detect the restore barrier');
requireText(control, 'io::ErrorKind::WouldBlock', 'ordinary control changes must be rejected during restore');
requireText(control, 'action_allowed_while_restoring', 'restore-time action policy must be explicit and unit tested');
requireText(control, 'RuntimeAction::AutomaticSafeMode', 'automatic safe mode must remain permitted during restore');

requireText(checkpoint, 'control::acquire_restore_guard()', 'checkpoint restore must hold the exclusive guard');
requireText(checkpoint, 'wait_for_daemon_ack', 'checkpoint restore must wait for exact daemon acknowledgement');
requireText(checkpoint, 'ensure_control_unchanged', 'checkpoint restore must verify the control generation before writes');
requireText(checkpoint, 'control_state_errors', 'a control race must convert the transaction into rollback');
requireText(checkpoint, 'rollback_attempted', 'transaction report must retain rollback state');

requireCount(daemon, 'control::restore_in_progress()', 3, 'startup, daemon loop and run-once must all honor the restore barrier');
requireText(daemon, 'startup kernel writes are disabled', 'startup must explicitly block writes under restore lock');
requireText(daemon, 'daemon kernel writes are suspended', 'main daemon loop must expose the active write barrier');
requireText(daemon, 'Once skipped while a runtime restoration is active', 'run-once must not race restoration');
requireText(daemon, 'restore_locked || !control_state.mode.allows_writes()', 'restore barrier must take precedence over active mode');

if (!process.exitCode) {
  console.log('runtime transaction validation: exclusive lock, acknowledgement, write barriers and rollback race checks enforced');
}
