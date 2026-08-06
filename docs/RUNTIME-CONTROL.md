# Runtime control and policy checkpoints

This document defines the first runtime-control protocol for TCP Optimiser. It is intentionally small and versioned so later network profiles, adaptive policy and experiments can use the same safety boundary.

## Goals

The runtime layer must support all of the following without restarting Android or replacing the daemon process:

- reload the configured policy;
- pause kernel writes while retaining monitoring;
- enter a persistent read-only safe mode;
- recover from a damaged control file through an explicit user command;
- preview planned kernel changes without writing them;
- preserve a last-known-good policy after complete verification;
- stop repeated unhealthy writes automatically.

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

Every accepted command increments `generation`. The daemon polls the small state file as part of its existing loop and consumes a generation once.

- `reload` keeps the current mode and requests one immediate reapplication only when the mode is active.
- `resume` changes the mode to active and requests one immediate reapplication.
- leaving safe mode changes the mode to active and requests one immediate reapplication.
- pause and entering safe mode never apply a policy.

The generation mechanism replaces ambiguous one-shot marker consumption for runtime control. Existing `force_apply` compatibility remains during migration.

## Failure behavior

### Missing control file

A missing file is treated as the initial active state. It does not indicate corruption.

### Malformed or unsupported control file

The daemon fails closed into an in-memory safe-mode state and performs no policy writes.

`control-status` returns a read-only safe-mode snapshot so the WebUI remains usable. An explicit `resume` or `safe-mode --disable` command rebuilds a valid version-1 state file. Passive reads never rewrite damaged evidence.

### File update

Control state is serialized to a process-specific temporary file and atomically renamed over the destination.

## Commands

```text
tcp_optimiser control-status
tcp_optimiser reload
tcp_optimiser pause
tcp_optimiser resume
tcp_optimiser safe-mode
tcp_optimiser safe-mode --disable
```

All commands return one JSON object on standard output. Errors are sent to standard error and use a nonzero exit code.

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

`diff` returns only values that would change. Neither command writes module files, sysctls or qdiscs.

No active physical route is a valid temporary state. The WebUI must still show runtime mode and checkpoint status when policy planning is unavailable.

## Last-known-good policy

The file `last-good-policy-v1.json` records:

```json
{
  "format_version": 1,
  "captured_at_epoch": 1786032000,
  "interface": "wlan0",
  "interface_mode": "Wi-Fi",
  "algorithm": "bbr",
  "qdisc": "fq",
  "pacing_ca": 200,
  "pacing_ss": 300
}
```

The daemon updates this file only when post-apply verification has:

- zero drifted values;
- zero unavailable values;
- zero parsing or policy errors.

Manual `repair` follows the same rule. A checkpoint persistence error does not change a successful kernel-repair result; it is reported separately as a warning.

## Runtime recovery

```text
tcp_optimiser checkpoint-status
tcp_optimiser restore-checkpoint
```

Before restoration, the command persists automatic safe mode. It then validates:

- checkpoint format version;
- interface-name syntax;
- algorithm allowlist membership;
- qdisc allowlist membership;
- pacing value ranges.

It restores the saved algorithm, global qdisc, interface qdisc and pacing values. The module remains in safe mode after restoration. The user must inspect the result and explicitly resume.

A checkpoint is tied to the recorded interface. Restoration fails visibly if that interface no longer exists or cannot accept the recorded qdisc.

## Automatic safe mode

`policy-failures-v1.json` stores consecutive failed post-apply verifications. A fully verified checkpoint clears the counter.

After three consecutive failures, the daemon persists automatic safe mode. This prevents a broken or incompatible policy from being written indefinitely.

The counter is for policy-health failures, not harmless UI errors or missing history data.

## WebUI contract

The Home runtime-control card must remain usable independently from network-route availability. It exposes:

- current mode and generation;
- last command and reason;
- checkpoint availability and timestamp;
- consecutive failure count and threshold;
- planned differences and warnings;
- reload, pause, resume, safe-mode and checkpoint-restore actions.

Pause, safe-mode transitions and checkpoint restoration require confirmation. The UI must release its busy state before refreshing command results.

## Device validation still required

Automated host tests cannot prove Android kernel behavior. Before this feature is merged into a stable release, validate:

1. pause while Wi-Fi is active;
2. resume and one-time immediate policy application;
3. reload without daemon PID change;
4. safe mode across reboot;
5. damaged control JSON and explicit recovery;
6. three injected verification failures entering automatic safe mode;
7. checkpoint restoration with the recorded interface present;
8. checkpoint restoration with the recorded interface absent;
9. Wi-Fi to cellular transition while paused;
10. Magisk, KernelSU and APatch WebUI command bridges.
