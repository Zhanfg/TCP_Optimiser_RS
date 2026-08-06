# Rollback implementation invariants

1. The original baseline is immutable across upgrades.
2. Kernel tuning cannot start without a valid baseline.
3. Restoration cannot write outside the compiled allowlist.
4. Every restored sysctl must pass exact readback.
5. Every interface qdisc is captured before its first replacement.
6. Missing restoration evidence never triggers guessed defaults.
