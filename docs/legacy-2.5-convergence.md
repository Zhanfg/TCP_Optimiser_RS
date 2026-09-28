# TCP Optimiser 2.5 → Rust 3.x convergence

This document records the final feature audit against the legacy
`ZhanfgBuild/TCP_Optimiser` 2.5 line. The Rust repository is the sole
long-term implementation.

| Area | Legacy 2.5 | Rust 3.x disposition |
|---|---|---|
| WebUI | Home / Stats / Settings / Logs / Advanced, MD3 | Superseded by the current MD3 UI, capability states, policy verification, dynamic colour and richer diagnostics. |
| Congestion control | 18 algorithms | Superseded: 19 known algorithms, including Westwood+, with runtime kernel capability checks. |
| qdisc | 7 named qdiscs, installer probe | Superseded: 9 known qdiscs with runtime support probing, readback and automatic reconciliation. |
| Proxy detection | Process-based Clash/V2Ray/sing-box/Shadowsocks detection | Superseded: process, package, root-module, core version, VPN and TPROXY evidence are reported separately. |
| Hosts / DNS | Hosts heuristics and Android DNS properties with resolv.conf fallback | Hosts detection is superseded; the useful resolv.conf fallback is retained in the Rust stats path for devices without net.dns properties. |
| Baseband backup | Partition discovery, dd backup, generated restore script | Retained in the current Advanced UI with partition sizing and Qualcomm path fallback. |
| Stealth | Rewrote module identity and suppressed normal logging | Intentionally dropped. Runtime identity rewriting conflicts with signed package integrity, provenance and reliable diagnostics. |
| Presets | Five built-ins plus JSON import/export | Retained and reimplemented in the current WebUI. |
| Uninstall / recovery | Forced generic values such as cubic/fq_codel and disabled selected TCP features | Reimplemented: Rust captures the real allowlisted pre-tuning sysctl baseline before first apply and restores it on uninstall. Unknown defaults are no longer guessed. |
| Installer | Shell capability probes and config preservation | Superseded by signed-payload verification, Rust install logic, ABI checks, free-space checks and config preservation. |
| i18n | English / Simplified Chinese | Superseded by the current validated translation set and CI parity checks. |
| CI / release | Shell ZIP/release workflows | Superseded by Rust fmt/test/clippy, WebUI validation, shellcheck, three Android ABI builds, reproducible packaging, signed manifests and attestations. |

## Migration rules

- New feature work belongs only in `Zhanfg/TCP_Optimiser_RS`.
- Legacy shell runtime code is reference material, not a second implementation.
- Historical tags, releases, license and attribution remain preserved.
- Device validation is separate from build/CI validation; a successful build is not
  recorded as device validation.
