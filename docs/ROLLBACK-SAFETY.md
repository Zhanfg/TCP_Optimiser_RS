# Transactional kernel rollback

TCP Optimiser changes global TCP sysctls and the root qdisc of active physical interfaces. A fixed fallback such as `cubic` plus `fq_codel` is not a valid uninstall strategy because Android vendors and custom kernels ship different defaults.

## Baseline lifecycle

1. During a clean first installation, the Rust installer reads every kernel node that TCP Optimiser is allowed to modify and writes `baseline-v1.json` in the module directory.
2. `baseline-provenance-v1.json` records whether that snapshot is an exact pre-module baseline or a compatibility snapshot taken immediately before replacing a legacy installation.
3. Existing baselines and provenance are validated and preserved during upgrades. An upgrade never replaces an earlier transactional baseline.
4. Wi-Fi and cellular interfaces can appear after installation. Immediately before the first qdisc replacement on each interface, its original root qdisc is added to the baseline journal.
5. Both early boot and the long-running daemon refuse to tune the kernel when the baseline is missing, malformed or unsupported.
6. Uninstall invokes `tcp_optimiser restore-baseline`, restores the recorded values, reads each sysctl and qdisc back, and records all mismatches. It does not force generic congestion-control or qdisc defaults when restoration evidence is unavailable.

## Same-name in-place upgrade

The module identity is stable across upgrades:

```text
id=tcp_optimiser
name=TCP Optimiser
```

A new ZIP may therefore be installed directly over the existing module. The installer uses `/data/adb/modules/tcp_optimiser` as the live source and the module manager's staging directory as the destination.

Before migration it stops the old daemon. It then preserves:

- `baseline-v1.json` and `baseline-provenance-v1.json`;
- selected Wi-Fi and cellular congestion algorithms;
- qdisc and pacing overrides;
- advanced sysctl configuration;
- runtime control mode;
- Last Known Good policy checkpoint;
- policy failure journal;
- optional connection-kill, route-window and debug settings.

Transient files such as `daemon.pid`, runtime acknowledgement and restoration locks are deliberately not copied.

## Migration from releases without a transactional baseline

Some legacy releases have no `baseline-v1.json`. Direct replacement is still supported, but the installer must not describe the current tuned kernel state as an Android vendor default.

For such an upgrade it:

1. stops the old daemon before capture;
2. deletes any stale staged `baseline-v1.json` and orphan provenance left by an interrupted installation;
3. captures the complete managed state immediately before replacement;
4. writes `baseline-provenance-v1.json` with:

```json
{
  "format_version": 1,
  "provenance": "legacy_upgrade_snapshot",
  "exact_pre_module": false
}
```

5. continues installation under the same module ID and display name.

This snapshot makes the new upgrade transaction reversible to the state that existed immediately before the upgrade. It is **not** guaranteed to represent the vendor or clean-boot defaults. `baseline-status` and uninstall logs expose this distinction explicitly.

A clean uninstall, reboot and reinstall remains the only way to establish a verified exact pre-module baseline after a legacy release has already modified the live kernel. It is no longer a mandatory prerequisite for installing an update.

## Integrity boundary

The baseline and provenance sidecar are runtime state and therefore are not part of the signed immutable module payload. They are treated as untrusted input during restoration and status inspection:

- the JSON schema and version must parse correctly;
- provenance values and their exactness flag must agree;
- sysctl writes are restricted to a compiled allowlist of nodes the module itself can modify;
- interface and qdisc identifiers are syntax checked before they reach `tc`;
- values are written through direct file I/O or fixed command arguments, not through a shell;
- every successful sysctl restore requires readback equality;
- interface qdisc restoration is verified after `tc` returns.

Malformed, unsupported or unsafe entries are rejected and reported. Restoration never substitutes guessed default values for rejected or unavailable evidence.

## Scope

The snapshot covers global sysctls and root qdiscs modified by the module. Optional `initcwnd` and `initrwnd` route attributes are tied to transient routes and are naturally discarded when Android rebuilds those routes or the device reboots; they are not persisted as part of the global baseline.
