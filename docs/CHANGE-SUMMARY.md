# Transactional baseline change summary

- Adds a versioned `baseline-v1.json` journal using atomic replacement and a process lock.
- Captures only kernel nodes that TCP Optimiser is capable of changing.
- Preserves the original journal during module upgrades.
- Refuses boot-time and daemon tuning when a valid journal is unavailable.
- Captures each physical interface root qdisc before its first replacement.
- Adds `capture-baseline` and `restore-baseline` Rust CLI commands with JSON output.
- Replaces the hard-coded uninstall fallback with exact restoration and readback verification.
- Restricts restoration to compiled sysctl, interface and qdisc allowlists.
- Documents rollback scope and real-device validation requirements.
- Corrects the documented algorithm name from unsupported `TLP` to `Vegas`.
