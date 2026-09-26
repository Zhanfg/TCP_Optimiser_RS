# BBRv2 provider strategy

BBRv2 is treated differently from BBRv1 and the current BBRv3 compatibility module.

## Why a generic tcp_bbr2.ko is not shipped

Google's BBRv2 alpha implementation is not a self-contained congestion-control module.
The v2 patch series changes TCP core plumbing in addition to adding `tcp_bbr2.c`.
Important dependencies include rate-sample loss/ECN accounting, transmit-time in-flight
snapshots, a loss callback in `tcp_congestion_ops`, and TSO/ECN behavior.

Google BBR developer Neal Cardwell explicitly states that `tcp_bbr2.c` cannot be built
out-of-tree against an otherwise-unpatched kernel. It must be built inside the BBRv2
kernel tree or on a kernel carrying the corresponding core patch series.

Authoritative references:

- https://github.com/google/bbr/tree/v2alpha
- https://groups.google.com/g/bbr-dev/c/OCVS5U68dNY
- https://github.com/google/bbr/commit/c6ef88ba01cc47ac4c6a2cfe51e15eaa4d833476

## Provider order

1. **Native / patched kernel** — if `tcp_available_congestion_control` already contains
   `bbr2`, use it directly. This has zero extra payload.
2. **BPF struct_ops candidate** — on kernels exposing BTF plus the TCP congestion-control
   struct_ops types, a small prebuilt CO-RE BPF object may eventually provide a compact
   alternative. This is experimental until verifier compatibility and BBRv2 behavior are
   validated on each supported GKI family.
3. **Unavailable** — do not fake BBRv2 by renaming BBRv1/BBRv3.

## Why install-time kernel compilation is not the default

Compiling `tcp_bbr2.c` alone does not create the missing TCP-core semantics. Building a
patched kernel at module-install time would require a kernel source tree, generated headers,
Module.symvers, a matching LLVM toolchain and boot-image replacement logic. That is larger
and less reliable than the module itself.

The preferred minimal-footprint model is therefore **prebuild once, specialize at install**:

- choose native BBRv2 when the kernel already provides it;
- otherwise choose a verified CO-RE BPF provider where runtime capabilities permit;
- keep full patched-kernel builds outside the Magisk/KernelSU module.

### Install-time specialization instead of shipping Clang

For the BPF provider, "compile at flash time" should mean **CO-RE relocation +
kernel BPF JIT**, not compiling C source with an on-device LLVM toolchain.

The ZIP can carry a small precompiled BPF ELF. During installation the loader:

1. reads the running kernel's `/sys/kernel/btf/vmlinux`;
2. resolves CO-RE type/field relocations against that exact kernel;
3. asks the verifier to accept the TCP `struct_ops` programs;
4. registers the congestion-control provider;
5. lets the running kernel's BPF JIT translate accepted bytecode to native CPU
   instructions.

This gives per-device/per-kernel specialization at install time without bundling
Clang, kernel headers, a kernel source tree, or Module.symvers. The original BPF
ELF remains small and architecture-independent enough for the CO-RE workflow.
If any gate fails, installation must leave BBRv2 disabled and retain the normal
native/BBR/BBRv3 fallback rather than weakening verifier or kernel checks.

## Runtime probe

`tcp_optimiser bbr2-provider` reports only capabilities. A
`bpf-struct-ops-candidate` result does **not** automatically enable BPF BBRv2; it means
the basic syscall/BTF types are present and the device is eligible for a later verifier probe.
