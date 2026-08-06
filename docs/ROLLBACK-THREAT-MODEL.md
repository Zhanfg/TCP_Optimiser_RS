# Rollback threat model

The baseline is mutable runtime state. An attacker or damaged storage may alter its JSON values, paths or identifiers. Restoration therefore validates the schema, restricts sysctl paths to a compiled allowlist, validates interface and qdisc tokens, avoids shell interpolation, and verifies live readback after writes.
