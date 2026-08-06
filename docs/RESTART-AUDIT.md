# Restart audit — first pass

Date: 2026-08-06

## Scope reviewed

- Rust daemon policy application and qdisc reconciliation
- module installation and upgrade preservation
- early-boot and long-running service entrypoints
- uninstall behavior
- CI validation and signed packaging path
- Chinese and English user documentation

## P0 finding fixed in this change

The previous uninstaller always wrote `cubic` and `fq_codel`. Those are common defaults but are not necessarily the original values supplied by an Android vendor or custom kernel. The module also changes pacing, ECN, TCP Fast Open, memory limits and optional advanced nodes, none of which were restored.

The fix introduces a versioned, atomic baseline journal, fail-closed startup guards, per-interface qdisc capture and readback-verified restoration.

## Additional inconsistency fixed

The Chinese README named `TLP` as a supported congestion-control algorithm and in the battery preset, while the Rust allowlist and configuration table use `vegas`. Both language documents now match the executable allowlist.

## Deferred follow-up

- Real-device validation on Magisk, KernelSU and APatch environments
- Android vendor kernels where `tc qdisc del` recreates a device-specific implicit qdisc
- Route-level `initcwnd`/`initrwnd` lifecycle evidence across interface teardown and reboot
- WebUI surface for baseline health, capture timestamp and last uninstall/repair evidence
- Release version bump only after device validation succeeds
