# PJZ110 / ColorOS 17 BBRv3 KO ABI audit

Date: 2026-10-04

## Scope

This audit covers the **existing** `tcp_bbr3.ko` from the
`device/oneplus13-pjz110-ko` line. It does **not** approve the experimental
KPM implementation; that KPM branch remains unsafe and closed.

No congestion-control switch was performed during this audit.

## Runtime identity

- Build display: `PJZ110_17.0.0.101(SP02CN01)`
- Fingerprint:
  `OnePlus/PJZ110/OP5D0DL1:17/CP2A.260605.016/V.56c7be9-30d0b48-30d0b45:user/release-keys`
- `uname -r`: `6.6.147-android15-9-g39338465`
- `/proc/sys/kernel/osrelease`:
  `6.6.147-android15-8-gd4c13fc2e857-abogki500782043-4k`
- Running `vmlinux` BTF SHA-256:
  `6129a25e3908557498bc5fab2ced419f9a3751837b858efbaca5ff12263bf2a2`

The module loader/runtime path intentionally keys off procfs kernel identity,
not the virtualized/public `uname` string.

## Live BBRv3 module

- module: `tcp_bbr3`
- state: `live`
- refcount during audit: `0`
- version: `3`
- srcversion: `42AD107FAC3095653297A61`
- KO SHA-256:
  `2d46336c0e2957b145f670a9b9ebe394660fb3cf735210cd9abc42364563b27e`
- vermagic:
  `6.6.118-android15-8-o-4k+ SMP preempt mod_unload modversions aarch64`
- runtime bridge parameters present:
  - `bbr3_register_addr`
  - `bbr3_unregister_addr`
- `ecn_low=N`

At audit time:

- available CC: `reno bbr cubic bbr3`
- current/default CC: `bbr`

So the KO was registered but not selected as the default, and no live module
references were held.

## Structure ABI comparison

The KO retains DWARF debug information. Its compile-time layouts were compared
directly against the running ColorOS 17 kernel's `/sys/kernel/btf/vmlinux`.

| Type | Result | Critical details |
| --- | --- | --- |
| `tcp_congestion_ops` | exact match | size 192; callback/name/owner/list/key/flags/init/release offsets identical |
| `rate_sample` | exact match | size 72; delivered/loss/acked/inflight/app-limited fields identical |
| `inet_connection_sock` | layout match | size 1536; `icsk_ca_ops=1320`, `icsk_ca_state=1368`, `icsk_ca_priv=1432`, private area 104 bytes |
| `tcp_sock` | relevant layout match | size 2384; all BBRv3-touched fields checked at identical offsets |
| `sock` | relevant layout match | size 856; pacing/GSO fields checked at identical offsets |
| `tcp_plb_state` | exact match | identical |
| `ack_sample` | exact match | identical |
| `tcp_bbr_info` | exact match | identical |
| `tcp_ca_event` | exact match | enum values 0..5 identical |
| `tcp_ca_state` | exact match | Open=0, Disorder=1, CWR=2, Recovery=3, Loss=4 |

The only textual `pahole` differences for `tcp_sock`,
`inet_connection_sock`, and `sock` were duplicate anonymous
struct/union presentation and an alignment annotation; the actual named
member offsets and overall sizes used by BBRv3 were unchanged.

Selected `tcp_sock` offsets verified identical:

- `rcv_nxt=1560`
- `mss_cache=1676`
- `is_cwnd_limited=1743:7`
- `tcp_wstamp_ns=1752`
- `tcp_mstamp=1768`
- `srtt_us=1776`
- `packets_out=1820`
- `ecn_flags=1838`
- `snd_ssthresh=1876`
- `snd_cwnd=1880`
- `snd_cwnd_clamp=1888`
- `delivered=1912`
- `delivered_ce=1916`
- `lost=1920`
- `app_limited=1924`
- `delivered_mstamp=1936`
- `lost_out=1968`

Selected `sock` offsets verified identical:

- `sk_pacing_status=396`
- `sk_pacing_rate=472`
- `sk_max_pacing_rate=480`
- `sk_gso_max_size=516`
- `sk_pacing_shift=529`

## KCFI validation

The running kernel has:

- `CONFIG_CFI_CLANG=y`
- `CONFIG_CFI_PERMISSIVE` disabled
- `CONFIG_SHADOW_CALL_STACK=y`
- `CONFIG_ARM64_PTR_AUTH_KERNEL=y`

The KO's runtime-address registration bridge is compiled with KCFI checks.

Its `init_module` indirect call checks type hash:

`0x4b52cb19`

which matches:

`int (struct tcp_congestion_ops *)`

Its unload path checks:

`0x317ce0d9`

which matches:

`void (struct tcp_congestion_ops *)`

These are the correct prototypes for
`tcp_register_congestion_control()` and
`tcp_unregister_congestion_control()`.

This is materially different from the unsafe diagnostic KPM, whose added
`printk` pointer used a wrong `void (...)` prototype and triggered an
immediate KCFI trap.

## MODVERSIONS / imported symbols

The KO has `__versions` and was accepted by the running module loader without
`insmod -f`.

Imported symbols include:

- `__stack_chk_fail`
- `__warn_printk`
- `alt_cb_patch_nops`
- `get_random_u32`
- `get_random_u8`
- `jiffies`
- `kfree`
- `kmalloc_caches`
- `kmalloc_trace`
- `param_ops_bool`
- `param_ops_ulong`

Their embedded MODVERSIONS CRCs match the PJZ110 profile used to build the KO.
Because the module is currently `Live`, the running kernel has already
accepted those versioned imports.

## Safety decision

The existing KO is considered **ABI-compatible with this exact audited
ColorOS 17 kernel/BTF pair**.

This does not create a blanket approval for future ColorOS 17 builds. A future
kernel update must be treated as unverified until its live BTF fingerprint and
module identity are audited again.

The runtime now uses an exact safety gate for BBRv3 selection:

- exact procfs kernel release
- exact live `vmlinux` BTF SHA-256
- exact `tcp_bbr3` version + srcversion
- presence of PJZ110 runtime bridge parameters
- exact installed KO SHA-256

If any check fails, `bbr3` is hidden from selectable algorithms and the final
sysctl write is refused.
