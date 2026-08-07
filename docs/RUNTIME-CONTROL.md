# Runtime control and policy checkpoints

This document defines the versioned runtime-control protocol for TCP Optimiser. Network profiles, adaptive policy and experimental features must use this safety boundary rather than inventing separate one-shot marker files.

## Goals

The runtime layer must support all of the following without restarting Android or replacing the daemon process:

- reload the configured policy;
- pause kernel writes while retaining monitoring;
- enter a persistent read-only safe mode;
- recover from a damaged control file through an explicit user command;
- preview planned kernel changes without writing them;
- preserve a complete last-known-good policy after verification;
- stop repeated unhealthy writes automatically;
- prevent the daemon from racing a runtime restoration.

The runtime checkpoint is not a replacement for `baseline-v1.json`:

- the **baseline** records the original system state for uninstall and full rollback;
- the **last-known-good checkpoint** records a recently verified module policy for runtime recovery.

## Persistent control state

The file `runtime-control-v1.json` is stored in the module directory.

```json
{
  "format_version": 1,
  "generation": 4,
  "mode": "active",
  "requested_action": "reload",
  "updated_at_epoch": 1786032000,
  "reason": "cli-reload"
}
```

### Modes

| Mode | Monitoring | Policy/sysctl/qdisc writes | Reload allowed |
|---|---:|---:|---:|
| `active` | yes | yes | yes |
| `paused` | yes | no | no |
| `safe_mode` | yes | no | no |

`paused` is a normal operator choice. `safe_mode` is a persistent safety state and may also be entered automatically.

### Generation semantics

Every accepted command increments `generation`. The daemon polls the state file as part of its existing loop and consumes a generation once.

- `reload` keeps the current mode and requests one immediate reapplication only when the mode is active.
- `resume` changes the mode to active and requests one immediate reapplication.
- leaving safe mode changes the mode to active and requests one immediate reapplication.
- pause and entering safe mode never apply a policy.

The generation mechanism replaces ambiguous one-shot marker consumption for runtime control. Existing `force_apply` compatibility remains during migration.

## Daemon acknowledgement

The daemon writes `runtime-control-ack-v1.json` after it has consumed a control generation and before it performs any write allowed by that generation.

```json
{
  "format_version": 1,
  "generation": 9,
  "mode": "safe_mode",
  "daemon_pid": 1234,
  "acknowledged_at_epoch": 1786032010
}
```

A runtime restoration is bound to all three values:

- the exact control generation;
- `safe_mode`;
- the PID currently recorded in `daemon.pid` and verified through `/proc/<pid>/cmdline`.

Restoration waits up to 15 seconds for that exact acknowledgement. A stale acknowledgement, a different daemon PID or a missing acknowledgement causes a visible timeout before any checkpoint value is written. This replaces the former fixed-delay quiescence guess.

If acknowledgement persistence fails, the daemon remains fail-closed and does not continue kernel writes.

## Exclusive restoration transaction

`restore-checkpoint` creates `runtime-restore-v1.lock` before reading or applying the checkpoint. The lock is held by an RAII guard until the command returns, including all error and rollback paths.

While this lock exists:

- daemon startup does not apply base sysctls;
- the main daemon loop does not process `force_apply`, qdisc reconciliation or interface policy writes;
- `run_once` returns without writing;
- ordinary `reload`, `pause`, `resume` and user safe-mode transitions return `WouldBlock`;
- automatic safe mode remains permitted.

A second restore command is rejected when the recorded restore owner is still live. A stale lock from a dead restore process is removed only after checking `/proc/<pid>/cmdline`. Malformed or unreadable lock evidence fails closed rather than being silently discarded.

The restore flow checks the safe-mode generation before capturing the old state and checks it again after application and readback. Any unexpected control-state change converts the operation into a failed transaction and triggers rollback. These checks are additional to the daemon write barrier; neither mechanism is treated as a substitute for the other.

## Failure behavior

### Missing control file

A missing file is treated as the initial active state. It does not indicate corruption.

### Malformed or unsupported control file

The daemon fails closed into an in-memory safe-mode state and performs no policy writes.

`control-status` returns a read-only safe-mode snapshot so the WebUI remains usable. An explicit `resume` or `safe-mode --disable` command rebuilds a valid version-1 state file. Passive reads never rewrite damaged evidence.

### File update

Control state, acknowledgement state, checkpoints and failure records are serialized to process-specific temporary files and atomically renamed over their destinations.

## Commands

```text
tcp_optimiser control-status
tcp_optimiser reload
tcp_optimiser pause
tcp_optimiser resume
tcp_optimiser safe-mode
tcp_optimiser safe-mode --disable
```

All commands return one JSON object on standard output. Errors use a nonzero exit code. The WebUI shell wrapper preserves both the JSON report and the real exit status so a failed transaction can still expose rollback evidence.

## Read-only policy planning

```text
tcp_optimiser plan [--iface IFACE]
tcp_optimiser diff [--iface IFACE]
```

`plan` reports current and expected values for:

- congestion-control algorithm;
- default qdisc;
- active-interface root qdisc;
- TCP pacing CA ratio;
- TCP pacing slow-start ratio.

It also reports interface type, MTU, Wi-Fi frequency, runtime mode, write eligibility and warnings.

`write_allowed` is false when the runtime mode blocks writes, the interface is unsupported or the configured policy cannot be resolved. `diff` returns only values that would change. Neither command writes module files, sysctls or qdiscs.

No active physical route is a valid temporary state. The WebUI must still show runtime mode and checkpoint status when policy planning is unavailable, and must not describe an unavailable diff as a verified match.

## Last-known-good policy

The file `last-good-policy-v2.json` records the full policy covered by post-apply health verification:

```json
{
  "format_version": 2,
  "captured_at_epoch": 1786032000,
  "interface": "wlan0",
  "interface_mode": "Wi-Fi",
  "algorithm": "bbr",
  "qdisc": "fq",
  "pacing_ca": 200,
  "pacing_ss": 300,
  "advanced_sysctls": [
    {
      "key": "tcp_fin_timeout",
      "path": "/proc/sys/net/ipv4/tcp_fin_timeout",
      "value": 30
    }
  ]
}
```

The daemon updates this file only when post-apply verification has:

- zero drifted values;
- zero unavailable values;
- zero parsing or policy errors.

Manual `repair` follows the same rule. A checkpoint persistence error does not change a successful kernel-repair result; it is reported separately as a warning.

Advanced sysctl entries are independently validated against a fixed key, `/proc/sys` path and numeric-range allowlist before restoration. The saved policy can still be restored after `advanced.conf` changes; the module remains in safe mode afterward so the operator can reconcile the configuration before resuming.

The former v1 checkpoint omitted advanced sysctls even though health verification included them. It is intentionally not accepted as a complete runtime checkpoint.

## Runtime recovery

```text
tcp_optimiser checkpoint-status
tcp_optimiser restore-checkpoint
```

Before restoration, the command acquires the exclusive transaction guard, persists automatic safe mode and waits for the matching daemon acknowledgement. It then validates:

- checkpoint format version;
- interface-name syntax and recorded interface type;
- algorithm allowlist membership and current availability;
- qdisc allowlist membership;
- pacing value ranges;
- every advanced sysctl key, path and numeric range;
- current readability of every target value required for transactional rollback.

It captures the complete pre-restore runtime state, restores the saved algorithm, global qdisc, interface qdisc, pacing values and advanced sysctls, then reads every value back.

If application, readback or transaction-state verification fails, the captured pre-restore state is reapplied and read back again. The report distinguishes:

- `rollback_attempted`;
- `rollback_succeeded`;
- original application/readback errors;
- rollback errors.

The module remains in safe mode after restoration. The user must inspect the result and explicitly resume after the transaction lock is released.

A checkpoint is tied to the recorded interface and interface type. Restoration fails visibly before writing if the interface no longer exists, changed class or cannot provide a readable root qdisc.

## Automatic safe mode

`policy-failures-v1.json` stores consecutive failed post-apply verifications. A fully verified checkpoint clears the counter.

After three consecutive failures, the daemon persists automatic safe mode. This prevents a broken or incompatible policy from being written indefinitely.

Malformed or unsupported failure-state data is never reset to zero. When a policy failure occurs with an unusable journal, the module requests persistent automatic safe mode and returns an explicit error. `checkpoint-status` exposes the journal error to the WebUI.

The counter is for policy-health failures, not harmless UI errors or missing history data.

## WebUI contract

The Home runtime-control card must remain usable independently from network-route availability. It exposes:

- current mode and generation;
- last command and reason;
- checkpoint availability, timestamp and advanced-sysctl count;
- consecutive failure count, threshold and journal errors;
- planned differences and warnings;
- reload, pause, resume, safe-mode and checkpoint-restore actions.

Pause, safe-mode transitions and checkpoint restoration require confirmation. After every action, including a failed one, the UI refreshes the actual control state. Failed restoration retains and displays the structured rollback report instead of replacing it with a generic command error.

Fake preview data is restricted to localhost over HTTP or HTTPS. A query parameter alone cannot enable preview data on a production WebView host.

## Device validation still required

Automated host tests cannot prove Android kernel behavior. Before this feature is merged into a stable release, validate:

1. pause while Wi-Fi is active;
2. resume and one-time immediate policy application;
3. reload without daemon PID change;
4. safe mode across reboot;
5. damaged control JSON and explicit recovery;
6. damaged failure journal entering fail-closed behavior on the next policy failure;
7. three injected verification failures entering automatic safe mode;
8. acknowledgement timeout with a deliberately stalled daemon;
9. checkpoint restoration with the recorded interface present;
10. checkpoint restoration with the recorded interface absent or changed type;
11. checkpoint application failure followed by successful rollback;
12. rollback failure being retained in the WebUI report;
13. advanced sysctl drift and last-known-good restoration;
14. concurrent `resume` and `reload` being rejected during restoration;
15. daemon, `run_once` and legacy `force_apply` remaining write-blocked while the restore lock exists;
16. stale restore-lock recovery after a killed restore process;
17. Wi-Fi to cellular transition while paused;
18. Magisk, KernelSU and APatch WebUI command bridges.
