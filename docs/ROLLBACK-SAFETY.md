# Transactional kernel rollback

TCP Optimiser changes global TCP sysctls and the root qdisc of active physical interfaces. A fixed fallback such as `cubic` plus `fq_codel` is not a valid uninstall strategy because Android vendors and custom kernels ship different defaults.

## Baseline lifecycle

1. During first installation, the Rust installer reads every kernel node that TCP Optimiser is allowed to modify and writes `baseline-v1.json` in the module directory.
2. Existing baselines are validated and preserved during upgrades. An upgrade never replaces the original snapshot with values already written by an older TCP Optimiser build.
3. Wi-Fi and cellular interfaces can appear after installation. Immediately before the first qdisc replacement on each interface, its original root qdisc is added to the baseline journal.
4. Both early boot and the long-running daemon refuse to tune the kernel when the baseline is missing, malformed or unsupported.
5. Uninstall invokes `tcp_optimiser restore-baseline`, restores the recorded values, reads each sysctl and qdisc back, and records all mismatches. It does not force generic congestion-control or qdisc defaults when an exact restore is unavailable.

## Integrity boundary

The baseline is runtime state and therefore is not part of the signed immutable module payload. It is treated as untrusted input during restoration:

- the JSON schema and version must parse correctly;
- sysctl writes are restricted to a compiled allowlist of nodes the module itself can modify;
- interface and qdisc identifiers are syntax checked before they reach `tc`;
- values are written through direct file I/O or fixed command arguments, not through a shell;
- every successful sysctl restore requires readback equality;
- interface qdisc restoration is verified after `tc` returns.

A damaged baseline causes restoration to fail closed. The uninstaller reports the failure but does not guess replacement values.

## Scope

The snapshot covers global sysctls and root qdiscs modified by the module. Optional `initcwnd` and `initrwnd` route attributes are tied to transient routes and are naturally discarded when Android rebuilds those routes or the device reboots; they are not persisted as part of the global baseline.
