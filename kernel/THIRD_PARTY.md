# Kernel module source provenance

The Android GKI module build uses two pinned source families:

1. Android common kernel: `android.googlesource.com/kernel/common`, branch
   `android15-6.6`. qdisc modules and the kernel-tree BBR module are built
   directly from that target kernel tree.
2. BBRv3 compatibility source: `hrimfaxi/tcp_bbr_modules`, pinned to
   commit `c5c557584175b5fed8939bf91ec249aed158597d`.

The BBRv3 source repository documents the BBRv3 backport as Dual BSD/GPL code
and its repository README states GPL-2.0-only for the project. No third-party
binary is downloaded or trusted: CI builds the modules from source against the
same prepared Android kernel tree and records the exact revisions in the
generated manifest.
