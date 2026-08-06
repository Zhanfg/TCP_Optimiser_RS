# Transactional rollback validation

This checklist must be completed before the feature branch is tagged as a stable release.

## Static and hosted CI

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --all-targets --locked`
- [ ] `cargo clippy --all-targets --locked -- -D warnings`
- [ ] Shell syntax and ShellCheck validation
- [ ] WebUI and release metadata validation
- [ ] arm64-v8a build
- [ ] armeabi-v7a build
- [ ] x86_64 build

## Fresh installation

- [ ] `baseline-v1.json` is created before `post-fs-data` applies tuning
- [ ] capture timestamp remains unchanged after reboot
- [ ] all readable managed sysctls are present in the snapshot
- [ ] current physical interface root qdisc is journaled
- [ ] daemon starts only after the snapshot validates

## Upgrade

- [ ] install an older module build and record the baseline hash
- [ ] apply a non-default algorithm, qdisc and advanced settings
- [ ] flash the candidate build without uninstalling
- [ ] confirm the original baseline hash and timestamp are unchanged
- [ ] confirm user-selected configuration files are preserved

## Interface lifecycle

- [ ] boot on Wi-Fi and later enable cellular data
- [ ] boot on cellular and later connect Wi-Fi
- [ ] test dual-SIM interface renumbering where available
- [ ] test VPN/TUN operation while the physical route is selected
- [ ] verify each newly modified physical interface is added once

## Uninstall

- [ ] record all managed sysctls and root qdiscs before installation
- [ ] change algorithms, qdiscs, pacing and at least three advanced nodes
- [ ] uninstall without rebooting first
- [ ] compare every restored sysctl with the pre-install value
- [ ] compare every present journaled interface qdisc with the pre-install state
- [ ] confirm no generic `cubic/fq_codel` fallback occurs after baseline corruption
- [ ] confirm restoration errors appear in logcat and the temporary uninstall report

## Recovery behavior

- [ ] remove the baseline and confirm early tuning is skipped
- [ ] corrupt the JSON and confirm daemon startup is refused
- [ ] change the schema version and confirm startup is refused
- [ ] inject an unmanaged sysctl path and confirm restore rejects it
- [ ] inject unsafe interface/qdisc identifiers and confirm restore rejects them
